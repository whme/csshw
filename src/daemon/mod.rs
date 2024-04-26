#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::cmp::max;
use std::collections::BTreeMap;
use std::{
    ffi::c_void,
    io, mem,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{thread, time};

use crate::utils::config::DaemonConfig;
use crate::utils::debug::StringRepr;
use crate::utils::{clear_screen, get_window_title, set_console_color};
use crate::{
    serde::{serialization::Serialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        arrange_console,
        constants::{DEFAULT_SSH_USERNAME_KEY, PIPE_NAME, PKG_NAME},
        get_console_input_buffer, read_keyboard_input, set_console_border_color, set_console_title,
    },
};
use log::{debug, error, warn};
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
    sync::broadcast::{self, Receiver, Sender},
    task::JoinHandle,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Console::{CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD_0};

use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_CONTROL, VK_E, VK_ESCAPE, VK_R, VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, IsWindow, MoveWindow, SetForegroundWindow, ShowWindow,
    SW_RESTORE, SW_SHOWMINIMIZED, WINDOWPLACEMENT,
};
use windows::Win32::{
    Foundation::{BOOL, COLORREF, FALSE, HWND, LPARAM, TRUE},
    System::Console::{
        GetConsoleMode, GetConsoleWindow, SetConsoleMode, CONSOLE_MODE, ENABLE_PROCESSED_INPUT,
    },
    UI::WindowsAndMessaging::EnumWindows,
};
use windows::Win32::{
    System::Threading::PROCESS_INFORMATION, UI::WindowsAndMessaging::GetWindowThreadProcessId,
};

use self::workspace::WorkspaceArea;

mod workspace;

const SENDER_CAPACITY: usize = 4096;

struct Daemon<'a> {
    hosts: Vec<String>,
    username: Option<String>,
    config: &'a DaemonConfig,
    control_mode_state: ControlModeState,
    debug: bool,
}

#[derive(PartialEq, Debug)]
enum ControlModeState {
    Inactive,
    Initiated,
    Active,
}

impl Daemon<'_> {
    async fn launch(mut self) {
        set_console_title(format!("{} daemon", PKG_NAME).as_str());
        set_console_color(CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color));
        set_console_border_color(COLORREF(0x000000FF));

        // Makes sure ctrl+c is reported as a keyboard input rather than as signal
        // https://learn.microsoft.com/en-us/windows/console/ctrl-c-and-ctrl-break-signals
        disable_processed_input_mode();

        let workspace_area =
            workspace::get_workspace_area(workspace::Scaling::Logical, self.config.height);

        self.arrange_daemon_console(&workspace_area);

        // Looks like on windows 10 re-arranging the console resets the console output buffer
        set_console_color(CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color));

        let client_console_window_handles =
            launch_clients(self.hosts.to_vec(), &self.username, self.debug).await;

        self.rearrange_client_windows(&client_console_window_handles, &workspace_area);

        // TODO: set some hook (CBTProc or SetWinEventHook) to detect
        // window focus changes and when the daemon console get's focus
        // iterate through all client windows + daemon and use
        // SetForegroundWindow.

        // Now that all clients started, focus the daemon console again.
        unsafe { SetForegroundWindow(GetConsoleWindow()) };

        self.print_instructions();
        self.run(&client_console_window_handles, &workspace_area);
    }

    fn run(
        &mut self,
        client_console_window_handles: &BTreeMap<usize, HWND>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = self.launch_named_pipe_servers(&sender);

        // FIXME: somehow we can't detect if the client consoles are being
        // closed from the outside ...
        tokio::spawn(async move {
            loop {
                servers.retain(|server| {
                    return !server.is_finished();
                });
                if servers.is_empty() {
                    // All clients have exited, exit the daemon as well
                    std::process::exit(0);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        ensure_client_z_order_in_sync_with_daemon(client_console_window_handles.clone());

        loop {
            self.handle_input_record(
                &sender,
                read_keyboard_input(),
                client_console_window_handles,
                workspace_area,
            );
        }
    }

    fn launch_named_pipe_servers(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    ) -> Vec<JoinHandle<()>> {
        let mut servers: Vec<JoinHandle<()>> = Vec::new();
        for _ in &self.hosts {
            let named_pipe_server = ServerOptions::new()
                .access_outbound(true)
                .pipe_mode(PipeMode::Message)
                .create(PIPE_NAME)
                .unwrap_or_else(|err| {
                    error!("{}", err);
                    panic!("Failed to create named pipe server",)
                });
            let mut receiver = sender.subscribe();
            servers.push(tokio::spawn(async move {
                named_pipe_server_routine(named_pipe_server, &mut receiver).await;
            }));
        }
        return servers;
    }

    fn handle_input_record(
        &mut self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        input_record: INPUT_RECORD_0,
        client_console_window_handles: &BTreeMap<usize, HWND>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        if self.control_mode_is_active(input_record) {
            if self.control_mode_state == ControlModeState::Initiated {
                clear_screen();
                println!("Control Mode (Esc to exit)");
                println!("[r]etile");
                self.control_mode_state = ControlModeState::Active;
                return;
            }
            let key_event = unsafe { input_record.KeyEvent };
            if !key_event.bKeyDown.as_bool() {
                return;
            }
            match VIRTUAL_KEY(key_event.wVirtualKeyCode) {
                VK_R => {
                    self.rearrange_client_windows(client_console_window_handles, workspace_area);
                    self.arrange_daemon_console(workspace_area);
                }
                VK_E => {
                    // TODO: Select windows
                }
                VK_T => {
                    // TODO: trigger input on selected windows
                }
                _ => {}
            }
            return;
        }
        let _error_handler = |err| {
            error!("{}", err);
            panic!(
                "Failed to serialize input recored `{}`",
                input_record.string_repr()
            )
        };
        match sender.send(
            input_record.serialize().as_mut_vec()[..]
                .try_into()
                .unwrap_or_else(_error_handler),
        ) {
            Ok(_) => {}
            Err(_) => {
                thread::sleep(time::Duration::from_millis(1));
            }
        }
    }

    fn control_mode_is_active(&mut self, input_record: INPUT_RECORD_0) -> bool {
        let key_event = unsafe { input_record.KeyEvent };
        if self.control_mode_state == ControlModeState::Active {
            if key_event.wVirtualKeyCode == VK_ESCAPE.0 {
                self.print_instructions();
                self.control_mode_state = ControlModeState::Inactive;
                return false;
            }
            return true;
        }
        if key_event.wVirtualKeyCode == VK_CONTROL.0 {
            if key_event.bKeyDown.as_bool() {
                self.control_mode_state = ControlModeState::Initiated
            } else {
                self.control_mode_state = ControlModeState::Inactive
            }
        } else if key_event.wVirtualKeyCode == VK_A.0
            && self.control_mode_state == ControlModeState::Initiated
        {
            return true;
        }
        return false;
    }

    fn print_instructions(&self) {
        clear_screen();
        println!("Input to terminal: (Ctrl-A to enter control mode)");
    }

    fn rearrange_client_windows(
        &self,
        client_console_window_handles: &BTreeMap<usize, HWND>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let mut valid_handles: Vec<HWND> = Vec::new();
        for handle in client_console_window_handles.values() {
            if unsafe { IsWindow(*handle).as_bool() } {
                valid_handles.push(*handle);
            }
        }
        for (index, handle) in valid_handles.iter().enumerate() {
            let (x, y, width, height) = determine_client_spatial_attributes(
                index as i32,
                valid_handles.len() as i32,
                workspace_area,
                self.config.aspect_ratio_adjustement,
            );
            unsafe {
                MoveWindow(*handle, x, y, width, height, true).unwrap_or_else(|err| {
                    error!("{}", err);
                    panic!("Failed to move window",)
                });
            }
        }
    }

    fn arrange_daemon_console(&self, workspace_area: &WorkspaceArea) {
        let (x, y, width, height) = get_console_rect(
            0,
            workspace_area.height,
            workspace_area.width,
            self.config.height,
            workspace_area,
        );
        arrange_console(x, y, width, height);
    }
}

fn ensure_client_z_order_in_sync_with_daemon(client_console_window_handles: BTreeMap<usize, HWND>) {
    tokio::spawn(async move {
        let daemon_handle = unsafe { GetConsoleWindow() };
        let mut previous_foreground_window = unsafe { GetForegroundWindow() };
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let foreground_window = unsafe { GetForegroundWindow() };
            if previous_foreground_window == foreground_window {
                continue;
            }
            if foreground_window == daemon_handle
                && !client_console_window_handles.values().any(|client_handle| {
                    return *client_handle == previous_foreground_window
                        || *client_handle == daemon_handle;
                })
            {
                defer_windows(&client_console_window_handles, &daemon_handle);
            }
            previous_foreground_window = foreground_window;
        }
    });
}

fn defer_windows(client_console_window_handles: &BTreeMap<usize, HWND>, daemon_handle: &HWND) {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).unwrap() };
    for handle in client_console_window_handles
        .values()
        .chain([daemon_handle])
    {
        // First restore if window is minimized
        let mut placement: WINDOWPLACEMENT = WINDOWPLACEMENT {
            length: mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        match unsafe { GetWindowPlacement(*handle, &mut placement) } {
            Ok(_) => {}
            Err(_) => {
                continue;
            }
        }
        if placement.showCmd == SW_SHOWMINIMIZED.0.try_into().unwrap() {
            unsafe { ShowWindow(*handle, SW_RESTORE) };
        }
        // Then bring it to front using UI automation
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.unwrap();
        if let Ok(window) = unsafe { automation.ElementFromHandle(*handle) } {
            unsafe { window.SetFocus() }.unwrap();
        }
    }
}

fn determine_client_spatial_attributes(
    index: i32,
    number_of_consoles: i32,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
) -> (i32, i32, i32, i32) {
    let aspect_ratio = workspace_area.width as f64 / workspace_area.height as f64;

    let grid_columns = max(
        ((number_of_consoles as f64).sqrt() * (aspect_ratio + aspect_ratio_adjustment)) as i32,
        1,
    );
    let grid_rows = max(
        (number_of_consoles as f64 / grid_columns as f64).ceil() as i32,
        1,
    );

    let grid_column_index = index % grid_columns;
    let grid_row_index = index / grid_columns;

    let is_last_row = grid_row_index == grid_rows - 1;
    let last_row_console_count = number_of_consoles % grid_columns;

    let console_width = if is_last_row && last_row_console_count != 0 {
        workspace_area.width / last_row_console_count
    } else {
        workspace_area.width / grid_columns
    };

    let console_height = workspace_area.height / grid_rows;

    let x = grid_column_index * console_width;
    let y = grid_row_index * console_height;

    return get_console_rect(x, y, console_width, console_height, workspace_area);
}

fn get_console_rect(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    workspace_area: &workspace::WorkspaceArea,
) -> (i32, i32, i32, i32) {
    return (
        workspace_area.x + x,
        workspace_area.y + y,
        width + workspace_area.x_fixed_frame + workspace_area.x_size_frame * 2,
        height + workspace_area.y_size_frame * 2,
    );
}

fn launch_client_console(host: &str, username: Option<String>, debug: bool) -> PROCESS_INFORMATION {
    // The first argument must be `--` to ensure all following arguments are treated
    // as positional arguments and not as options if they start with `-`.
    let mut client_args: Vec<&str> = Vec::new();
    if debug {
        client_args.push("-d");
    }
    let default_username = DEFAULT_SSH_USERNAME_KEY.to_string();
    client_args.extend(vec![
        "client",
        "--",
        host,
        username.as_ref().unwrap_or(&default_username),
    ]);
    return spawn_console_process(&format!("{PKG_NAME}.exe"), client_args);
}

async fn named_pipe_server_routine(
    server: NamedPipeServer,
    receiver: &mut Receiver<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
) {
    // wait for a client to connect
    server.connect().await.unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Timeded out waiting for clients to connect to named pipe server",)
    });
    loop {
        let ser_input_record = match receiver.recv().await {
            Ok(val) => val,
            Err(_) => return,
        };
        loop {
            server.writable().await.unwrap_or_else(|err| {
                error!("{}", err);
                panic!("Timed out waiting for named pipe server to become writable",)
            });
            match server.try_write(&ser_input_record) {
                Ok(SERIALIZED_INPUT_RECORD_0_LENGTH) => {
                    debug!("Successfully written all data");
                    break;
                }
                Ok(n) => {
                    // The data was only written partially, try again
                    warn!(
                        "Partially written data, expected {} but only wrote {}",
                        SERIALIZED_INPUT_RECORD_0_LENGTH, n
                    );
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Try again
                    debug!("Writing to named pipe server would have blocked");
                    continue;
                }
                Err(_) => {
                    // Can happen if the pipe is closed because the
                    // client exited
                    debug!(
                        "Named pipe server ({:?}) is closed, stopping named pipe server routine",
                        server
                    );
                    return;
                }
            }
        }
    }
}

async fn _launch_client_processes_and_wait(
    hosts: Vec<String>,
    username: &Option<String>,
    debug: bool,
) -> Arc<Mutex<Vec<u32>>> {
    let mut handles = vec![];
    let process_ids = Arc::new(Mutex::new(Vec::<u32>::new()));
    for host in hosts.into_iter() {
        let _username = username.clone();
        let process_ids_arc = Arc::clone(&process_ids);
        let future = tokio::spawn(async move {
            process_ids_arc
                .lock()
                .unwrap()
                .push(launch_client_console(&host, _username, debug).dwProcessId);
        });
        handles.push(future);
    }
    // Wait for each client process to actually have started
    for handle in handles {
        handle.await.unwrap();
    }
    return process_ids;
}

async fn _launch_clients(hosts: Vec<String>, username: &Option<String>, debug: bool) -> Vec<HWND> {
    let number_of_hosts = hosts.len();
    let process_ids = _launch_client_processes_and_wait(hosts, username, debug).await;
    let client_handles: Vec<HWND>;
    // Wait for each client process to have opened its console window
    loop {
        // FIXME: doesn't have to be ArcMutex
        let client_handles_arc_mutex = Arc::new(Mutex::new(Vec::<HWND>::new()));
        let client_handles_arc_mutex_clone = Arc::clone(&client_handles_arc_mutex);
        enumerate_windows(|handle| {
            let mut window_process_id: u32 = 0;
            unsafe { GetWindowThreadProcessId(handle, Some(&mut window_process_id)) };
            if process_ids.lock().unwrap().contains(&window_process_id) {
                client_handles_arc_mutex_clone.lock().unwrap().push(handle);
            }
            return true;
        });
        let _client_handles = client_handles_arc_mutex.lock().unwrap();
        if _client_handles.len() == number_of_hosts {
            client_handles = _client_handles.to_vec();
            break;
        }
    }
    return client_handles;
}

/// Launches a client console for each given host and
/// waits for the client windows to exist before
/// returning their handles.
async fn launch_clients(
    hosts: Vec<String>,
    username: &Option<String>,
    debug: bool,
) -> BTreeMap<usize, HWND> {
    let client_handles = _launch_clients(hosts.clone(), username, debug).await;
    let mut result = BTreeMap::new();
    // Map window handle to host based on window title
    loop {
        // Wait for all window titles to have been set before mapping anything
        if client_handles.iter().all(|handle| {
            return get_window_title(handle) != format!("{}.exe", PKG_NAME);
        }) {
            break;
        }
    }
    for client_handle in client_handles.iter() {
        let mut _index = 0;
        // Account for duplicate hosts
        loop {
            if let Some(index) = hosts.iter().enumerate().position(|(position, host)| {
                if get_window_title(client_handle).contains(host) && position >= _index {
                    return true;
                }
                return false;
            }) {
                if result.contains_key(&index) {
                    _index = index + 1;
                    continue;
                }
                result.insert(index, *client_handle);
            }
            break;
        }
    }
    return result;
}

fn enumerate_windows<F>(mut callback: F)
where
    F: FnMut(HWND) -> bool,
{
    let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
    let closure_pointer_pointer: *mut c_void = unsafe { mem::transmute(&mut trait_obj) };

    let lparam = LPARAM(closure_pointer_pointer as isize);
    unsafe { EnumWindows(Some(enumerate_callback), lparam).unwrap() };
}

unsafe extern "system" fn enumerate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let closure: &mut &mut dyn FnMut(HWND) -> bool = &mut *(lparam.0 as *mut c_void
        as *mut &mut dyn std::ops::FnMut(windows::Win32::Foundation::HWND) -> bool);
    if closure(hwnd) {
        return TRUE;
    } else {
        return FALSE;
    }
}

fn disable_processed_input_mode() {
    let handle = get_console_input_buffer();
    let mut mode = CONSOLE_MODE(0u32);
    unsafe {
        GetConsoleMode(handle, &mut mode).unwrap();
    }
    unsafe {
        SetConsoleMode(handle, CONSOLE_MODE(mode.0 ^ ENABLE_PROCESSED_INPUT.0)).unwrap();
    }
}

pub async fn main(
    hosts: Vec<String>,
    username: Option<String>,
    config: &DaemonConfig,
    debug: bool,
) {
    let daemon: Daemon = Daemon {
        hosts,
        username,
        config,
        control_mode_state: ControlModeState::Inactive,
        debug,
    };
    daemon.launch().await;
}
