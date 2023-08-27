#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::cmp::max;
use std::{
    ffi::c_void,
    io, mem,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{thread, time};

use crate::utils::config::DaemonConfig;
use crate::utils::{clear_screen, set_console_color};
use crate::{
    serde::{serialization::Serialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        arrange_console as arrange_daemon_console,
        constants::{DEFAULT_SSH_USERNAME_KEY, PIPE_NAME, PKG_NAME},
        get_console_input_buffer, read_keyboard_input, set_console_border_color, set_console_title,
    },
};
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
    sync::broadcast::{self, Receiver, Sender},
    task::JoinHandle,
};
use windows::Win32::System::Console::{
    BACKGROUND_INTENSITY, BACKGROUND_RED, FOREGROUND_INTENSITY, INPUT_RECORD_0,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_CONTROL, VK_E, VK_ESCAPE, VK_R, VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, MoveWindow, SetForegroundWindow};
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

mod workspace;

const SENDER_CAPACITY: usize = 4096;

struct Daemon<'a> {
    hosts: Vec<String>,
    username: Option<String>,
    config: &'a DaemonConfig,
    control_mode_state: ControlModeState,
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
        set_console_color(FOREGROUND_INTENSITY | BACKGROUND_INTENSITY | BACKGROUND_RED);
        set_console_border_color(COLORREF(0x000000FF));

        // Makes sure ctrl+c is reported as a keyboard input rather than as signal
        // https://learn.microsoft.com/en-us/windows/console/ctrl-c-and-ctrl-break-signals
        disable_processed_input_mode();

        let workspace_area =
            workspace::get_workspace_area(workspace::Scaling::Logical, self.config.height);
        let number_of_consoles = self.hosts.len() as i32;

        let (x, y, width, height) = get_console_rect(
            0,
            workspace_area.height,
            workspace_area.width,
            self.config.height,
            &workspace_area,
        );
        arrange_daemon_console(x, y, width, height);

        let client_console_window_handles = launch_clients(
            self.hosts.to_vec(),
            &self.username,
            workspace_area,
            number_of_consoles,
            self.config.aspect_ratio_adjustement,
        )
        .await;

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
        client_console_window_handles: &[HWND],
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = self.launch_named_pipe_servers(&sender);

        // FIXME: somehow we can't detect if the client consoles are being
        // closes from the outside ...
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

        let mut transmitted_records = 0;
        loop {
            if transmitted_records == SENDER_CAPACITY - 1 {
                thread::sleep(time::Duration::from_millis(1));
                transmitted_records = 0;
            }
            let input_record = read_keyboard_input();
            self.handle_input_record(
                &sender,
                input_record,
                client_console_window_handles,
                workspace_area,
            );
            transmitted_records += 1;
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
                .unwrap();
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
        client_console_window_handles: &[HWND],
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
                    // FIXME: the order in which the windows are present in our vector
                    // is non-deterministic, so retiling will result in a different order ...
                    let mut valid_handles: Vec<HWND> = Vec::new();
                    for handle in client_console_window_handles.iter() {
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
                            MoveWindow(*handle, x, y, width, height, true);
                        }
                    }
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
        match sender.send(
            input_record.serialize().as_mut_vec()[..]
                .try_into()
                .unwrap(),
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
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> PROCESS_INFORMATION {
    // The first argument must be `--` to ensure all following arguments are treated
    // as positional arguments and not as options if they start with `-`.
    return spawn_console_process(
        &format!("{PKG_NAME}.exe"),
        vec![
            "client",
            "--",
            host,
            username
                .as_ref()
                .unwrap_or(&DEFAULT_SSH_USERNAME_KEY.to_string()),
            &x.to_string(),
            &y.to_string(),
            &width.to_string(),
            &height.to_string(),
        ],
    );
}

async fn named_pipe_server_routine(
    server: NamedPipeServer,
    receiver: &mut Receiver<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
) {
    // wait for a client to connect
    server.connect().await.unwrap();
    loop {
        let ser_input_record = match receiver.recv().await {
            Ok(val) => val,
            Err(_) => return,
        };
        loop {
            server.writable().await.unwrap();
            match server.try_write(&ser_input_record) {
                Ok(_) => {
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Try again
                    continue;
                }
                Err(_) => {
                    // Can happen if the pipe is closed because the
                    // client exited
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
    workspace_area: workspace::WorkspaceArea,
    number_of_consoles: i32,
    aspect_ratio_adjustment: f64,
) -> Vec<HWND> {
    let mut handles = vec![];
    let process_ids = Arc::new(Mutex::new(Vec::<u32>::new()));
    for (index, host) in hosts.iter().cloned().enumerate() {
        let _username = username.clone();
        let process_ids_arc = Arc::clone(&process_ids);
        let future = tokio::spawn(async move {
            let (x, y, width, height) = determine_client_spatial_attributes(
                index as i32,
                number_of_consoles,
                &workspace_area,
                aspect_ratio_adjustment,
            );
            process_ids_arc
                .lock()
                .unwrap()
                .push(launch_client_console(&host, _username, x, y, width, height).dwProcessId);
        });
        handles.push(future);
    }
    for handle in handles {
        handle.await.unwrap();
    }

    loop {
        // FIXME: doesn't have to be ArcMutex
        let client_handles = Arc::new(Mutex::new(Vec::<HWND>::new()));
        let client_handles_arc = Arc::clone(&client_handles);
        enumerate_windows(|handle| {
            let mut window_process_id: u32 = 0;
            unsafe { GetWindowThreadProcessId(handle, Some(&mut window_process_id)) };
            if process_ids.lock().unwrap().contains(&window_process_id) {
                client_handles_arc.lock().unwrap().push(handle);
            }
            return true;
        });
        let result = client_handles.lock().unwrap();
        if result.len() == hosts.len() {
            return result.to_vec();
        }
    }
}

fn enumerate_windows<F>(mut callback: F)
where
    F: FnMut(HWND) -> bool,
{
    let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
    let closure_pointer_pointer: *mut c_void = unsafe { mem::transmute(&mut trait_obj) };

    let lparam = LPARAM(closure_pointer_pointer as isize);
    unsafe { EnumWindows(Some(enumerate_callback), lparam) };
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
        GetConsoleMode(handle, &mut mode);
    }
    unsafe {
        SetConsoleMode(handle, CONSOLE_MODE(mode.0 ^ ENABLE_PROCESSED_INPUT.0));
    }
}

pub async fn main(hosts: Vec<String>, username: Option<String>, config: &DaemonConfig) {
    let daemon: Daemon = Daemon {
        hosts,
        username,
        config,
        control_mode_state: ControlModeState::Inactive,
    };
    daemon.launch().await;
}
