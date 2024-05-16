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
use crate::utils::{clear_screen, set_console_color};
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
use windows::Win32::System::Console::{
    CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD_0, LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED,
};

use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_C, VK_E, VK_ESCAPE, VK_H, VK_R, VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
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

use self::workspace::WorkspaceArea;

mod workspace;

const SENDER_CAPACITY: usize = 4096;

#[derive(Clone)]
struct ClientWindow {
    hostname: String,
    hwnd: HWND,
}

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

        let mut client_console_window_handles = Arc::new(Mutex::new(
            launch_clients(
                self.hosts.to_vec(),
                &self.username,
                self.debug,
                &workspace_area,
                self.config.aspect_ratio_adjustement,
            )
            .await,
        ));

        // TODO: set some hook (CBTProc or SetWinEventHook) to detect
        // window focus changes and when the daemon console get's focus
        // iterate through all client windows + daemon and use
        // SetForegroundWindow.

        // Now that all clients started, focus the daemon console again.
        unsafe { SetForegroundWindow(GetConsoleWindow()) };

        self.print_instructions();
        self.run(&mut client_console_window_handles, &workspace_area)
            .await;
    }

    async fn run(
        &mut self,
        client_console_window_handles: &mut Arc<Mutex<BTreeMap<usize, ClientWindow>>>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = Arc::new(Mutex::new(self.launch_named_pipe_servers(&sender)));
        let mut _server_clone = Arc::clone(&servers);

        // FIXME: somehow we can't detect if the client consoles are being
        // closed from the outside ...
        tokio::spawn(async move {
            loop {
                _server_clone.lock().unwrap().retain(|server| {
                    return !server.is_finished();
                });
                if _server_clone.lock().unwrap().is_empty() {
                    // All clients have exited, exit the daemon as well
                    std::process::exit(0);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        ensure_client_z_order_in_sync_with_daemon(client_console_window_handles.to_owned());

        loop {
            self.handle_input_record(
                &sender,
                read_keyboard_input(),
                client_console_window_handles,
                workspace_area,
                &mut servers,
            )
            .await;
        }
    }

    fn launch_named_pipe_servers(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    ) -> Vec<JoinHandle<()>> {
        let mut servers: Vec<JoinHandle<()>> = Vec::new();
        for _ in &self.hosts {
            self._launch_named_pipe_server(&mut servers, sender);
        }
        return servers;
    }

    fn _launch_named_pipe_server(
        &self,
        servers: &mut Vec<JoinHandle<()>>,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    ) {
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

    async fn handle_input_record(
        &mut self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        input_record: INPUT_RECORD_0,
        client_console_window_handles: &mut Arc<Mutex<BTreeMap<usize, ClientWindow>>>,
        workspace_area: &workspace::WorkspaceArea,
        servers: &mut Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) {
        if self.control_mode_is_active(input_record) {
            if self.control_mode_state == ControlModeState::Initiated {
                clear_screen();
                println!("Control Mode (Esc to exit)");
                println!("[c]reate window(s), [r]etile, copy active [h]ostname(s)");
                self.control_mode_state = ControlModeState::Active;
                return;
            }
            let key_event = unsafe { input_record.KeyEvent };
            if !key_event.bKeyDown.as_bool() {
                return;
            }
            match VIRTUAL_KEY(key_event.wVirtualKeyCode) {
                VK_R => {
                    self.rearrange_client_windows(
                        &client_console_window_handles.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(workspace_area);
                }
                VK_E => {
                    // TODO: Select windows
                }
                VK_T => {
                    // TODO: trigger input on selected windows
                }
                VK_C => {
                    clear_screen();
                    // TODO: make ESC abort
                    println!("Hostname(s): (leave empty to abort)");
                    disable_processed_input_mode(); // As it was disabled before, this enables it again
                    let mut hostnames = String::new();
                    match io::stdin().read_line(&mut hostnames) {
                        Ok(2) => {
                            // Empty input (only newline '\n')
                        }
                        Ok(_) => {
                            let new_clients = launch_clients(
                                hostnames
                                    .split(' ')
                                    .map(|x| return x.trim().to_owned())
                                    .collect(),
                                &self.username,
                                self.debug,
                                workspace_area,
                                self.config.aspect_ratio_adjustement,
                            )
                            .await;
                            let number_of_existing_client_console_window_handles =
                                client_console_window_handles.lock().unwrap().len();
                            for (index, client_window) in new_clients {
                                client_console_window_handles.lock().unwrap().insert(
                                    number_of_existing_client_console_window_handles + index + 1,
                                    client_window,
                                );
                                self._launch_named_pipe_server(
                                    &mut servers.lock().unwrap(),
                                    sender,
                                );
                            }
                        }
                        Err(error) => {
                            error!("{error}");
                        }
                    }
                    disable_processed_input_mode();
                    self.rearrange_client_windows(
                        &client_console_window_handles.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(workspace_area);
                    // Focus the daemon console again.
                    unsafe { SetForegroundWindow(GetConsoleWindow()) };
                    self.quit_control_mode();
                }
                VK_H => {
                    let mut active_hostnames: Vec<String> = vec![];
                    for handle in client_console_window_handles.lock().unwrap().values() {
                        if unsafe { IsWindow(handle.hwnd).as_bool() } {
                            active_hostnames.push(handle.hostname.clone());
                        }
                    }
                    cli_clipboard::set_contents(active_hostnames.join(" ")).unwrap();
                    self.quit_control_mode();
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
                self.quit_control_mode();
                return false;
            }
            return true;
        }
        if (key_event.dwControlKeyState & LEFT_CTRL_PRESSED >= 1
            || key_event.dwControlKeyState & RIGHT_CTRL_PRESSED >= 1)
            && key_event.wVirtualKeyCode == VK_A.0
        {
            self.control_mode_state = ControlModeState::Initiated;
            return true;
        }
        return false;
    }

    fn quit_control_mode(&mut self) {
        self.print_instructions();
        self.control_mode_state = ControlModeState::Inactive;
    }

    fn print_instructions(&self) {
        clear_screen();
        println!("Input to terminal: (Ctrl-A to enter control mode)");
    }

    fn rearrange_client_windows(
        &self,
        client_console_window_handles: &BTreeMap<usize, ClientWindow>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let mut valid_handles: Vec<HWND> = Vec::new();
        for handle in client_console_window_handles.values() {
            if unsafe { IsWindow(handle.hwnd).as_bool() } {
                valid_handles.push(handle.hwnd);
            }
        }
        for (index, handle) in valid_handles.iter().enumerate() {
            arrage_client_window(
                handle,
                workspace_area,
                index,
                valid_handles.len(),
                self.config.aspect_ratio_adjustement,
            )
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

fn arrage_client_window(
    handle: &HWND,
    workspace_area: &workspace::WorkspaceArea,
    index: usize,
    number_of_consoles: usize,
    aspect_ratio_adjustment: f64,
) {
    let (x, y, width, height) = determine_client_spatial_attributes(
        index as i32,
        number_of_consoles as i32,
        workspace_area,
        aspect_ratio_adjustment,
    );
    unsafe {
        MoveWindow(*handle, x, y, width, height, true).unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to move window",)
        });
    }
}

fn ensure_client_z_order_in_sync_with_daemon(
    client_console_window_handles: Arc<Mutex<BTreeMap<usize, ClientWindow>>>,
) {
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
                && !client_console_window_handles
                    .lock()
                    .unwrap()
                    .values()
                    .any(|client_handle| {
                        return client_handle.hwnd == previous_foreground_window
                            || client_handle.hwnd == daemon_handle;
                    })
            {
                defer_windows(
                    &client_console_window_handles.lock().unwrap(),
                    &daemon_handle,
                );
            }
            previous_foreground_window = foreground_window;
        }
    });
}

fn defer_windows(
    client_console_window_handles: &BTreeMap<usize, ClientWindow>,
    daemon_handle: &HWND,
) {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).unwrap() };
    for handle in client_console_window_handles
        .values()
        .chain([&ClientWindow {
            hostname: "root".to_owned(),
            hwnd: *daemon_handle,
        }])
    {
        // First restore if window is minimized
        let mut placement: WINDOWPLACEMENT = WINDOWPLACEMENT {
            length: mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        match unsafe { GetWindowPlacement(handle.hwnd, &mut placement) } {
            Ok(_) => {}
            Err(_) => {
                continue;
            }
        }
        if placement.showCmd == SW_SHOWMINIMIZED.0.try_into().unwrap() {
            unsafe { ShowWindow(handle.hwnd, SW_RESTORE) };
        }
        // Then bring it to front using UI automation
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.unwrap();
        if let Ok(window) = unsafe { automation.ElementFromHandle(handle.hwnd) } {
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

fn launch_client_console(
    host: &str,
    username: Option<String>,
    debug: bool,
    index: usize,
    workspace_area: &workspace::WorkspaceArea,
    number_of_consoles: usize,
    aspect_ratio_adjustment: f64,
) -> HWND {
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
    let process_id = spawn_console_process(&format!("{PKG_NAME}.exe"), client_args).dwProcessId;
    let mut client_window_handle: Option<HWND> = None;
    loop {
        enumerate_windows(|handle| {
            let mut window_process_id: u32 = 0;
            unsafe { GetWindowThreadProcessId(handle, Some(&mut window_process_id)) };
            if process_id == window_process_id {
                client_window_handle = Some(handle);
            }
            return true;
        });
        if client_window_handle.is_some() {
            break;
        }
    }
    arrage_client_window(
        &client_window_handle.unwrap(),
        workspace_area,
        index,
        number_of_consoles,
        aspect_ratio_adjustment,
    );
    return client_window_handle.unwrap();
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

/// Launches a client console for each given host and
/// waits for the client windows to exist before
/// returning their handles.
async fn launch_clients(
    hosts: Vec<String>,
    username: &Option<String>,
    debug: bool,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
) -> BTreeMap<usize, ClientWindow> {
    let result = Arc::new(Mutex::new(BTreeMap::new()));
    let len_hosts = hosts.len();
    let host_iter = IntoIterator::into_iter(hosts);
    let mut handles = vec![];
    for (index, host) in host_iter.enumerate() {
        let _username = username.clone();
        let _workspace = *workspace_area;
        let result_arc = Arc::clone(&result);
        let future = tokio::spawn(async move {
            let handle = launch_client_console(
                &host,
                _username,
                debug,
                index,
                &_workspace,
                len_hosts,
                aspect_ratio_adjustment,
            );
            result_arc.lock().unwrap().insert(
                index,
                ClientWindow {
                    hostname: host.to_string(),
                    hwnd: handle,
                },
            );
        });
        handles.push(future);
    }
    for handle in handles {
        handle.await.unwrap();
    }
    return result.lock().unwrap().clone();
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
