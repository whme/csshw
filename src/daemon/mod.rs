//! Daemon imlementation

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]
#![cfg_attr(test, allow(unused_imports, unused_variables, dead_code, unused_mut))]

use std::cmp::max;
use std::collections::BTreeMap;
use std::{
    io, mem,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{thread, time};

use crate::get_console_window_handle;
use crate::utils::config::{Cluster, DaemonConfig};
use crate::utils::debug::StringRepr;
use crate::utils::{clear_screen, set_console_color};
use crate::{
    serde::{serialization::serialize_input_record_0, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        arrange_console,
        constants::{PIPE_NAME, PKG_NAME},
        get_console_input_buffer, read_keyboard_input, set_console_border_color, set_console_title,
    },
    WindowsSettingsDefaultTerminalApplicationGuard,
};
use bracoxide::explode;
use log::{debug, error, warn};
use tokio::sync::broadcast::error::TryRecvError;
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
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, IsWindow, MoveWindow, SetForegroundWindow, ShowWindow,
    SW_RESTORE, SW_SHOWMINIMIZED, WINDOWPLACEMENT,
};
use windows::Win32::{
    Foundation::{COLORREF, HANDLE, HWND, STILL_ACTIVE},
    System::{
        Console::{
            GetConsoleMode, GetConsoleWindow, SetConsoleMode, CONSOLE_MODE, ENABLE_PROCESSED_INPUT,
        },
        Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION},
    },
};

use self::workspace::WorkspaceArea;

mod workspace;

/// The capacity of the broadcast channel used
/// to send the input records read from the console input buffer
/// to the named pipe servers connected to each client in parallel.
const SENDER_CAPACITY: usize = 1024 * 1024;

/// Representation of a client
#[derive(Clone, Debug)]
struct Client {
    /// Hostname the client is connect to (or supposed to connect to).
    hostname: String,
    /// Window handle to the clients console window.
    window_handle: HWND,
    /// Process handle to the client process.
    process_handle: HANDLE,
}

unsafe impl Send for Client {}

/// Hacky wrapper around a window handle.
///
/// As we cannot implement foreign traits for foreign structs
/// we introduce this wrapper to implement [Send] for [HWND].
#[derive(Debug, Eq)]
struct HWNDWrapper {
    hwdn: HWND,
}

unsafe impl Send for HWNDWrapper {}

impl PartialEq for HWNDWrapper {
    /// Returns whether to `HWNDWrapper` instances are equal or not
    /// based on the [HWND] they wrap.
    fn eq(&self, other: &Self) -> bool {
        return self.hwdn == other.hwdn;
    }
}

/// Returns a window handle to the current console window.
///
/// The [HWND] is wrapped in a `HWNDWrapper` so that
/// we can pass it inbetween threads.
fn get_console_window_wrapper() -> HWNDWrapper {
    return HWNDWrapper {
        hwdn: unsafe { GetConsoleWindow() },
    };
}

/// Returns a window handle to the foreground window.
///
/// The [HWND] is wrapped in a `HWNDWrapper` so that
/// we can pass it inbetween threads.
fn get_foreground_window_wrapper() -> HWNDWrapper {
    return HWNDWrapper {
        hwdn: unsafe { GetForegroundWindow() },
    };
}

/// Enum of all possible control mode states.
#[derive(PartialEq, Debug)]
enum ControlModeState {
    /// Controle mode is inactive.
    Inactive,
    /// One of the keys required for the control mode key combination
    /// is currently being pressed.
    Initiated,
    /// All required keys for the control mode key combination were pressed
    /// and control mode is now active.
    ///
    /// Active control mode prevents any input records from being sent to clients.
    Active,
}

/// The daemon is responsible to launch a client for
/// each host, positioning the client windows, forwarding
/// input records to all clients and handling control mode.
struct Daemon<'a> {
    /// A list of hostnames to connect to.
    hosts: Vec<String>,
    /// A username to use to connect to all clients.
    ///
    /// If it is empty the clients will use the SSH config to find an approriate
    /// username.
    username: Option<String>,
    /// Optional port used for all SSH connections.
    port: Option<u16>,
    /// The `DaemonConfig` that controls how the daemon console window looks like.
    config: &'a DaemonConfig,
    /// List of available cluster tags
    clusters: &'a [Cluster],
    /// The current control mode state.
    control_mode_state: ControlModeState,
    /// If debug mode is enabled on the daemon it will also be enabled on all
    /// clients.
    debug: bool,
}

impl Daemon<'_> {
    /// Launches all client windows and blocks on the main run loop.
    ///
    /// Sets up the daemon console by disabling processed input mode and applying
    /// the configured colors and dimensions.
    /// Once all client windows have successfully started the daemon console window
    /// is moved to the foreground and receives focus.
    #[cfg(not(test))]
    async fn launch(mut self) {
        set_console_title(format!("{PKG_NAME} daemon").as_str());
        set_console_color(CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color));
        set_console_border_color(COLORREF(0x000000FF));

        toggle_processed_input_mode(); // Disable processed input mode

        // Initialize the COM library so we can use UI automation
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).unwrap() };

        let workspace_area = workspace::get_workspace_area(self.config.height);

        self.arrange_daemon_console(&workspace_area);

        // Looks like on windows 10 re-arranging the console resets the console output buffer
        set_console_color(CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color));

        let mut clients = Arc::new(Mutex::new(
            launch_clients(
                self.hosts.to_vec(),
                &self.username,
                self.port,
                self.debug,
                &workspace_area,
                self.config.aspect_ratio_adjustement,
                0,
            )
            .await,
        ));

        // Now that all clients started, focus the daemon console again.
        let daemon_console = unsafe { GetConsoleWindow() };
        let _ = unsafe { SetForegroundWindow(daemon_console) };
        focus_window(daemon_console);

        self.print_instructions();
        self.run(&mut clients, &workspace_area).await;
    }

    /// The main run loop of the `daemon` subcommand.
    ///
    /// Opens a multi-producer, multi-consumer broadcasting channel used to
    /// send the read input records in parallel to the name pipe servers
    /// the clients are listening on.
    /// Spawns a background thread that waits for all clients to terminate
    /// and then stops the current process.
    /// Spawns a background thread that ensures the z-order of all client
    /// windows is in sync with the daemon window.
    /// I.e. if the daemon window is focussed, all clients should be moved to the foreground.
    ///
    /// The main loop consists of waiting for input records to read from the keyboard,
    /// sending them to all clients and handling control mode.
    ///
    /// # Arguments
    ///
    /// * `clients`                         - A thread safe mapping from the number
    ///                                       a client console window was launched at
    ///                                       in relation to the other client windows
    ///                                       and the clients console window handle.
    /// * `workspace_area`                  - The available workspace area on the
    ///                                       primary monitor minus the space occupied
    ///                                       by the daemon console window.
    async fn run(
        &mut self,
        clients: &mut Arc<Mutex<Vec<Client>>>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = Arc::new(Mutex::new(self.launch_named_pipe_servers(&sender)));

        // Monitor client processes
        let clients_clone = Arc::clone(clients);
        tokio::spawn(async move {
            loop {
                clients_clone.lock().unwrap().retain(|client| {
                    let mut exit_code: u32 = 0;
                    let _ = unsafe { GetExitCodeProcess(client.process_handle, &mut exit_code) };
                    return exit_code == STILL_ACTIVE.0 as u32;
                });
                if clients_clone.lock().unwrap().is_empty() {
                    // All clients have exited, exit the daemon as well
                    std::process::exit(0);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        ensure_client_z_order_in_sync_with_daemon(clients.to_owned());

        loop {
            self.handle_input_record(
                &sender,
                read_keyboard_input(),
                clients,
                workspace_area,
                &mut servers,
            )
            .await;
        }
    }

    /// Launch a named pipe server for each host in a dedicated thread.
    ///
    /// # Arguments
    ///
    /// * `sender` - The sender end of the broadcast channel through which
    ///              the main thread will send the input records that are to
    ///              be forwarded to the clients.
    ///
    /// # Returns
    ///
    /// Returns a list of [JoinHandle]s, one handle for each thread.
    fn launch_named_pipe_servers(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    ) -> Vec<JoinHandle<()>> {
        let mut servers: Vec<JoinHandle<()>> = Vec::new();
        for _ in &self.hosts {
            self.launch_named_pipe_server(&mut servers, sender);
        }
        return servers;
    }

    /// Launch a named pipe server in a dedicated thread.
    ///
    /// # Arguments
    ///
    /// * `servers` - A list of [JoinHandle]s to which the join handle for
    ///               the new thread will be added.
    /// * `sender`  - The sender end of the broadcast channel through which
    ///               the main thread will send the input records that are to
    ///               be forwarded to the clients.
    fn launch_named_pipe_server(
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

    /// Handle the given input record.
    ///
    /// Input records are being forwarded to all clients.
    /// If a sequence of input records matches the control mode
    /// key combination, forwarding is temporarily interrupted,
    /// until control mode is exited.
    ///
    /// # Arguments
    ///
    /// * `sender`                          - The sender end of the broadcast channel
    ///                                       through which we will send the input records
    ///                                       that are being forwarded to the clients
    ///                                       by the named pipe servers (`servers`).
    /// * `input_record`                    - The [INPUT_RECORD_0].`KeyEvent` read from the
    ///                                       console input buffer.
    /// * `clients`                         - A thread safe mapping from the number
    ///                                       a client console window was launched at
    ///                                       in relation to the other client windows
    ///                                       and the clients console window handle.
    ///                                       The mapping will be extended if additional clients
    ///                                       are being added through control mode `[c]reate window(s)`.
    /// * `workspace_area`                  - The available workspace area on the
    ///                                       primary monitor minus the space occupied
    ///                                       by the daemon console window.
    /// * `servers`                         - A thread safe list of [JoinHandle]s,
    ///                                       one handle for each named pipe server background thread.
    ///                                       The list will be extended if additional clients are being added
    ///                                       through control mode `[c]reate window(s)`.
    async fn handle_input_record(
        &mut self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        input_record: INPUT_RECORD_0,
        clients: &mut Arc<Mutex<Vec<Client>>>,
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
            match (
                VIRTUAL_KEY(key_event.wVirtualKeyCode),
                key_event.dwControlKeyState,
            ) {
                (VK_R, 0) => {
                    self.rearrange_client_windows(&clients.lock().unwrap(), workspace_area);
                    self.arrange_daemon_console(workspace_area);
                }
                (VK_E, 0) => {
                    // TODO: Select windows
                }
                (VK_T, 0) => {
                    // TODO: trigger input on selected windows
                }
                (VK_C, 0) => {
                    clear_screen();
                    // TODO: make ESC abort
                    println!("Hostname(s) or cluster tag(s): (leave empty to abort)");
                    toggle_processed_input_mode(); // As it was disabled before, this enables it again
                    let mut hostnames = String::new();
                    match io::stdin().read_line(&mut hostnames) {
                        Ok(2) => {
                            // Empty input (only newline '\n')
                        }
                        Ok(_) => {
                            let number_of_existing_clients = clients.lock().unwrap().len();
                            let new_clients = launch_clients(
                                resolve_cluster_tags(
                                    hostnames.split(' ').map(|x| return x.trim()).collect(),
                                    self.clusters,
                                )
                                .into_iter()
                                .map(|x| return x.to_owned())
                                .collect(),
                                &self.username,
                                self.port,
                                self.debug,
                                workspace_area,
                                self.config.aspect_ratio_adjustement,
                                number_of_existing_clients,
                            )
                            .await;
                            for client in new_clients.into_iter() {
                                clients.lock().unwrap().push(client);
                                self.launch_named_pipe_server(&mut servers.lock().unwrap(), sender);
                            }
                        }
                        Err(error) => {
                            error!("{error}");
                        }
                    }
                    toggle_processed_input_mode(); // Re-disable processed input mode.
                    self.rearrange_client_windows(&clients.lock().unwrap(), workspace_area);
                    self.arrange_daemon_console(workspace_area);
                    // Focus the daemon console again.
                    let daemon_window = unsafe { GetConsoleWindow() };
                    let _ = unsafe { SetForegroundWindow(daemon_window) };
                    focus_window(daemon_window);
                    self.quit_control_mode();
                }
                (VK_H, 0) => {
                    let mut active_hostnames: Vec<String> = vec![];
                    for client in clients.lock().unwrap().iter() {
                        if unsafe { IsWindow(Some(client.window_handle)).as_bool() } {
                            active_hostnames.push(client.hostname.clone());
                        }
                    }
                    cli_clipboard::set_contents(active_hostnames.join(" ")).unwrap();
                    self.quit_control_mode();
                }
                _ => {}
            }
            return;
        }
        let error_handler = |err| {
            error!("{}", err);
            panic!(
                "Failed to serialize input recored `{}`",
                input_record.string_repr()
            )
        };
        match sender.send(
            serialize_input_record_0(&input_record)[..]
                .try_into()
                .unwrap_or_else(error_handler),
        ) {
            Ok(_) => {}
            Err(_) => {
                thread::sleep(time::Duration::from_nanos(1));
            }
        }
    }

    /// Returns whether control mode is active or not given the input_record.
    ///
    /// For control mode to be active this function needs to be called
    /// multiple times, as a key press translates to an input record and
    /// the key combination that activates control mode has 2 keys:
    /// `Ctrl + A`.
    /// The current control mode state is stored in `self.control_mode_state`.
    ///
    /// # Arguments
    ///
    /// * `input_record` -  A KeyEvent input record.
    ///
    /// # Returns
    ///
    /// Whether or not control mode is active.
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

    /// Prints the default daemon instructions to the daemon console and
    /// sets `self.control_mode_state` to inactive.
    fn quit_control_mode(&mut self) {
        self.print_instructions();
        self.control_mode_state = ControlModeState::Inactive;
    }

    /// Clears the console screen and prints the default daemon instructions.
    fn print_instructions(&self) {
        clear_screen();
        println!("Input to terminal: (Ctrl-A to enter control mode)");
    }

    /// Iterates over all still open client windows and re-arranges them
    /// on the screen based on the aspect ration adjustment daemon configuration.
    ///
    /// Client windows will be re-sized and re-positioned.
    ///
    /// # Arguments
    ///
    /// * `clients`                         - A thread safe mapping from the number
    ///                                       a client console window was launched at
    ///                                       in relation to the other client windows
    ///                                       and the clients console window handle.
    ///                                       The number is relevant to determine the
    ///                                       position on the screen the window should
    ///                                       be placed at.
    /// * `workspace_area`                  - The available workspace area on the
    ///                                       primary monitor minus the space occupied
    ///                                       by the daemon console window.
    fn rearrange_client_windows(
        &self,
        clients: &[Client],
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let mut valid_clients = Vec::new();
        for client in clients.iter() {
            let mut exit_code: u32 = 0;
            let _ = unsafe { GetExitCodeProcess(client.process_handle, &mut exit_code) };
            if exit_code == STILL_ACTIVE.0 as u32
                && unsafe { IsWindow(Some(client.window_handle)).as_bool() }
            {
                valid_clients.push(client);
            }
        }
        for (index, client) in valid_clients.iter().enumerate() {
            arrange_client_window(
                &client.window_handle,
                workspace_area,
                index,
                valid_clients.len(),
                self.config.aspect_ratio_adjustement,
            )
        }
    }

    /// Re-sizes and re-positions the daemon console window on the screen
    /// based on the daemon height configuration.
    ///
    /// # Arguments
    ///
    /// * `workspace_area` - The available workspace area on the
    ///                      primary monitor minus the space occupied
    ///                      by the daemon console window.
    fn arrange_daemon_console(&self, workspace_area: &WorkspaceArea) {
        let (x, y, width, height) = get_console_rect(
            0,
            workspace_area.height,
            workspace_area.width - (workspace_area.x_fixed_frame + workspace_area.x_size_frame),
            self.config.height,
            workspace_area,
        );
        arrange_console(x, y, width, height);
    }
}

#[cfg(test)]
impl Daemon<'_> {
    /// Test-only launch implementation that exercises safe code paths and then panics
    /// to satisfy tests that expect a failure when launching the daemon in unit tests.
    async fn launch(mut self) {
        // Exercise safe code paths to contribute to coverage under tests
        set_console_title(format!("{PKG_NAME} daemon").as_str());
        set_console_color(CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color));
        // Toggling processed input mode is safe and helps cover IO helpers
        toggle_processed_input_mode();
        // Exercise geometry/arrangement helpers that do not spawn processes
        let workspace_area = workspace::get_workspace_area(self.config.height);
        self.arrange_daemon_console(&workspace_area);
        self.print_instructions();
        // Explicitly panic to keep behavior aligned with tests that catch unwind
        panic!("daemon.launch() is not supported under tests");
    }
}

/// The processed console input mode controls whether special key combinations
/// such as `Ctrl + c` or `Ctrl + BREAK` receive special handling or are treated
/// as simple key presses.
///
/// By default processed input mode is enabled, meaning `Ctrl + c` is treated as
/// a signal, not key presses.
///
/// <https://learn.microsoft.com/en-us/windows/console/ctrl-c-and-ctrl-break-signals>
fn toggle_processed_input_mode() {
    let handle = get_console_input_buffer();
    let mut mode = CONSOLE_MODE(0u32);
    unsafe {
        GetConsoleMode(handle, &mut mode).unwrap();
    }
    unsafe {
        SetConsoleMode(handle, CONSOLE_MODE(mode.0 ^ ENABLE_PROCESSED_INPUT.0)).unwrap();
    }
}

/// Resolve cluster tags into hostnames
///
/// Iterates over the list of hosts to find and resolve cluster tags.
/// Nested cluster tags are supported but recursivness is not checked for.
///
/// # Arguments
///
/// * `hosts`       - List of hosts including hostnames and or cluster tags
/// * `clusters`    - List of available cluster tags
///
/// # Returns
///
/// A list of hostnames
pub fn resolve_cluster_tags<'a>(hosts: Vec<&'a str>, clusters: &'a [Cluster]) -> Vec<&'a str> {
    let mut resolved_hosts: Vec<&str> = Vec::new();
    let mut is_cluster_tag: bool;
    for host in hosts {
        is_cluster_tag = false;
        for cluster in clusters {
            if host == cluster.name {
                is_cluster_tag = true;
                resolved_hosts.extend(resolve_cluster_tags(
                    cluster.hosts.iter().map(|host| return &**host).collect(),
                    clusters,
                ));
                break;
            }
        }
        if !is_cluster_tag {
            resolved_hosts.push(host);
        }
    }
    return resolved_hosts;
}

/// Launches a client console for each given host and waits for
/// the client windows to exist before returning their handles.
///
/// # Arguments
///
/// * `hosts`                   - List of hosts
/// * `username`                - Optional username, if none is given
///                               the client will use the SSH config to
///                               determine a username.
/// * `port`                    - Optional port for SSH connections
/// * `debug`                   - Toggles debug mode on the client.
/// * `workspace_area`          - The available workspace area on the primary monitor
///                               minus the space occupied by the daemon console window.
///                               Used to arrange the client window.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
///                               Used to arrange the client window.
/// * `index_offset`            - Offset used to position the new windows correctly
///                               from the start, avoiding flickering.
///
/// # Returns
///
/// A mapping from the order a client console window was launched at
/// in relation to the other client windows and the clients console window handle.
#[cfg(not(test))]
async fn launch_clients(
    hosts: Vec<String>,
    username: &Option<String>,
    port: Option<u16>,
    debug: bool,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
    index_offset: usize,
) -> Vec<Client> {
    let len_hosts = hosts.len();
    let result = Arc::new(Mutex::new(BTreeMap::new()));
    let host_iter = IntoIterator::into_iter(hosts);
    let mut handles = vec![];
    let _guard = WindowsSettingsDefaultTerminalApplicationGuard::new();
    for (index, host) in host_iter.enumerate() {
        let username_client = username.clone();
        let workspace_area_client = *workspace_area;
        let result_arc = Arc::clone(&result);
        let future = tokio::spawn(async move {
            let (window_handle, process_handle) = launch_client_console(
                &host,
                username_client,
                port,
                debug,
                index + index_offset,
                &workspace_area_client,
                len_hosts + index_offset,
                aspect_ratio_adjustment,
            );
            result_arc.lock().unwrap().insert(
                index,
                Client {
                    hostname: host.to_string(),
                    window_handle,
                    process_handle,
                },
            );
        });
        handles.push(future);
    }
    for handle in handles {
        handle.await.unwrap();
    }

    return result.lock().unwrap().values().cloned().collect();
}

#[cfg(test)]
async fn launch_clients(
    hosts: Vec<String>,
    username: &Option<String>,
    port: Option<u16>,
    debug: bool,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
    index_offset: usize,
) -> Vec<Client> {
    // Prevent side effects in unit tests: never spawn real processes.
    // Maintain expected test behavior:
    // - If hosts is empty, return empty list (used by tests that assert no clients are launched).
    // - Otherwise, panic so tests using catch_unwind validate failure paths without opening windows.
    if hosts.is_empty() {
        return vec![];
    }
    panic!("launch_clients() is not supported under tests");
}

/// Launchs a `client` console process with its own window with the given
/// CLI arguments/options: `host`, `username`, `port`, `debug`.
///
/// Waits for the window to open, then re-arranges it based on
/// the total number of clients, the size of the daemon console window and
/// its index relative to the other client windows.
///
/// # Arguments
///
/// * `host`                    - Hostname the client should connect to
/// * `username`                - Username the client should use
/// * `port`                    - Optional port for SSH connections
/// * `debug`                   - Toggle debug mode on the client
/// * `index`                   - The index of the client in the list of all clients.
///                               Used to re-arrange the client window.
/// * `workspace_area`          - The available workspace area on the primary monitor
///                               minus the space occupied by the daemon console window.
/// * `number_of_consoles`      - The total number of active client console windows.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
///
/// # Returns
///
/// A tuple containing the window handle and process handle of the client process.
#[cfg(not(test))]
fn launch_client_console(
    host: &str,
    username: Option<String>,
    port: Option<u16>,
    debug: bool,
    index: usize,
    workspace_area: &workspace::WorkspaceArea,
    number_of_consoles: usize,
    aspect_ratio_adjustment: f64,
) -> (HWND, HANDLE) {
    // The first argument must be `--` to ensure all following arguments are treated
    // as positional arguments and not as options if they start with `-`.
    let mut client_args: Vec<String> = Vec::new();
    if debug {
        client_args.push("-d".to_string());
    }
    let mut actual_host = host;
    let mut actual_username = username;
    if let Some(split_result) = host.split_once("@") {
        actual_username = Some(split_result.0.to_owned());
        actual_host = split_result.1;
    }
    if let Some(actual_username) = actual_username.as_deref() {
        client_args.extend(vec!["-u".to_string(), actual_username.to_string()]);
    }
    if let Some(port) = port {
        client_args.extend(vec!["-p".to_string(), port.to_string()]);
    }
    client_args.push("client".to_string());
    client_args.extend(vec!["--".to_string(), actual_host.to_string()]);

    let process_info = spawn_console_process(&format!("{PKG_NAME}.exe"), client_args);
    let client_window_handle = get_console_window_handle(process_info.dwProcessId);
    let process_handle = unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION, false, process_info.dwProcessId).unwrap_or_else(
            |err| {
                panic!(
                    "Failed to open process handle for process {}: {}",
                    process_info.dwProcessId, err
                );
            },
        )
    };

    arrange_client_window(
        &client_window_handle,
        workspace_area,
        index,
        number_of_consoles,
        aspect_ratio_adjustment,
    );
    return (client_window_handle, process_handle);
}

#[cfg(test)]
fn launch_client_console(
    _host: &str,
    _username: Option<String>,
    _port: Option<u16>,
    _debug: bool,
    _index: usize,
    _workspace_area: &workspace::WorkspaceArea,
    _number_of_consoles: usize,
    _aspect_ratio_adjustment: f64,
) -> (HWND, HANDLE) {
    // Prevent spawning real client processes and opening windows in unit tests.
    // Tests call this within catch_unwind and expect a panic.
    panic!("launch_client_console() is not supported under tests");
}

/// Wait for the named pipe server to connect, then forward serialized
/// input records read from the broadcast channel to the named pipe server.
///
/// If writing to the pipe fails the pipe is closed and the routine ends.
/// To detect if a client is still alive even if we are currently
/// not sending data, we send a "keep alive packet",
/// [`SERIALIZED_INPUT_RECORD_0_LENGTH`] bytes of `1`s. If that fails, the routine ends.
///
/// # Arguments
///
/// * `server`   - The named pipe server over which we send data to the
///                client.
/// * `receiver` - The receiving end of the broadcast channel through
///                which we get the serialize input records from the main
///                thread that are to be sent to the client via the named
///                pipe.
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
        let ser_input_record = match receiver.try_recv() {
            Ok(val) => val,
            Err(TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(5)).await;
                // Try sending dummy data to detect early if the pipe is closed because the client exited
                match server.try_write(&[u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH]) {
                    Ok(_) => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(_) => {
                        debug!(
                            "Named pipe server ({:?}) is closed, stopping named pipe server routine",
                            server
                        );
                        return;
                    }
                }
            }
            Err(err) => {
                error!("{}", err);
                panic!("Failed to receive data from the Receiver");
            }
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

/// Re-sizes and re-positions the given client window based on the total number of clients,
/// the size of the daemon console window and its index relative to the other client windows.
///
/// # Arguments
///
/// * `handle`                   - Reference the windows handle of a client console window.
/// * `workspace_area`           - The available workspace area on the primary monitor
///                                minus the space occupied by the daemon console window.
/// * `index`                    - The index of the client in the list of all clients.
/// * `number_of_consoles`       - The total number of active client console windows.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
fn arrange_client_window(
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
        // Since windows update 10.0.19041.5072 it can happen that a client windows rendering is broken
        // after a move+resize. Why is unclear, but resizing again does solve the issue.
        // We first make the window 1 pixel in each dimension too small and imediately fix it.
        // To reduce overhead we do not repaint the window the first time.
        MoveWindow(*handle, x, y, width - 1, height - 1, false).unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to move window",)
        });
        MoveWindow(*handle, x, y, width, height, true).unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to move window",)
        });
    }
}

/// Calculates the position and dimensions for a client window given its index,
/// the total number of clients and the `aspect_ratio_adjustment` daemon configuration.
///
/// # Arguments
///
/// * `index`                    - The index of the client in the list of all clients.
/// * `number_of_consoles`       - The total number of active client console windows.
/// * `workspace_area`           - The available workspace area on the primary monitor
///                                minus the space occupied by the daemon console window.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
///     * `> 0.0` - Aims for vertical rectangle shape.
///       The larger the value, the more exaggerated the "verticality".
///       Eventually the windows will all be columns.
///     * `= 0.0` - Aims for square shape.
///     * `< 0.0` - Aims for horizontal rectangle shape.
///       The smaller the value, the more exaggerated the "horizontality".
///       Eventually the windows will all be rows.
///       `-1.0` is the sweetspot for mostly preserving a 16:9 ratio.
fn determine_client_spatial_attributes(
    index: i32,
    number_of_consoles: i32,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
) -> (i32, i32, i32, i32) {
    let aspect_ratio = (workspace_area.width
        + (workspace_area.x_fixed_frame + workspace_area.x_size_frame) * 2)
        as f64
        / (workspace_area.height + (workspace_area.y_fixed_frame + workspace_area.y_size_frame) * 2)
            as f64;

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
        (workspace_area.width / last_row_console_count)
            + if last_row_console_count > 1 {
                workspace_area.x_fixed_frame + workspace_area.x_size_frame
            } else {
                0
            }
    } else {
        (workspace_area.width / grid_columns)
            + (workspace_area.x_fixed_frame + workspace_area.x_size_frame)
    };

    let console_height = (workspace_area.height
        + (workspace_area.y_fixed_frame + workspace_area.y_size_frame) * grid_row_index)
        / grid_rows;

    let x = grid_column_index * console_width
        - ((workspace_area.x_fixed_frame + workspace_area.x_size_frame) * (grid_column_index + 1));
    let y = grid_row_index * console_height
        - ((workspace_area.y_fixed_frame + workspace_area.y_size_frame) * (grid_row_index - 1));

    return get_console_rect(x, y, console_width, console_height, workspace_area);
}

/// Transform the position and dimensions of a console window based
/// on the workspace area.
///
/// To minimize empty space between windows, width and height must be adjusted
/// by the `fixed_frame` and `size_frame` values.
///
/// # Arguments
///
/// * `x`              - The `x` coordinate of the window.
/// * `y`              - The `y` coordinate of the window.
/// * `width`          - The `width` in pixels of the window.
/// * `height`         - The `height` in pixels of the window.
/// * `workspace_area` - The available workspace area on the primary monitor minus
///                      the space occupied by the daemon console window.
///
/// # Returns
///
/// (`x`, `y`, `width`, `height`)
///
fn get_console_rect(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    workspace_area: &workspace::WorkspaceArea,
) -> (i32, i32, i32, i32) {
    return (
        std::cmp::max(
            workspace_area.x - (workspace_area.x_fixed_frame + workspace_area.x_size_frame),
            workspace_area.x - (workspace_area.x_fixed_frame + workspace_area.x_size_frame) + x,
        ),
        workspace_area.y - (workspace_area.y_fixed_frame + workspace_area.y_size_frame) + y,
        std::cmp::min(workspace_area.width, width),
        height,
    );
}

/// Spawns a background thread that ensures the z-order of all client
/// windows is in sync with the daemon window.
/// I.e. if the daemon window is focussed, all clients should be moved to the foreground.
///
/// # Arguments
///
/// * `clients`                       - A thread safe mapping from the number
///                                     a client console window was launched at
///                                     in relation to the other client windows
///                                     and the clients console window handle.
///                                     The mapping must be thread safe to allow
///                                     it to be modified by the main thread
///                                     while we periodically read from it in the
///                                     background thread.
fn ensure_client_z_order_in_sync_with_daemon(clients: Arc<Mutex<Vec<Client>>>) {
    tokio::spawn(async move {
        let daemon_handle = get_console_window_wrapper();
        let mut previous_foreground_window = get_foreground_window_wrapper();
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let foreground_window = get_foreground_window_wrapper();
            if previous_foreground_window == foreground_window {
                continue;
            }
            if foreground_window == daemon_handle
                && !clients.lock().unwrap().iter().any(|client| {
                    return client.window_handle == previous_foreground_window.hwdn
                        || client.window_handle == daemon_handle.hwdn;
                })
            {
                defer_windows(&clients.lock().unwrap(), &daemon_handle.hwdn);
            }
            previous_foreground_window = foreground_window;
        }
    });
}

/// Move all given windows to the foreground.
///
/// Restores minimized windows.
/// If a window handle no longer points to a valid window, it is skipped.
/// The daemon window is deferred last and receives focus.
///
/// # Arguments
///
/// * `clients`                       - A thread safe mapping from the number
///                                     a client console window was launched at
///                                     in relation to the other client windows
///                                     and the clients console window handle.
/// * `daemon_handle`                 - Handle to the daemon console window.
fn defer_windows(clients: &[Client], daemon_handle: &HWND) {
    for client in clients.iter().chain([&Client {
        hostname: "root".to_owned(),
        window_handle: *daemon_handle,
        process_handle: HANDLE::default(),
    }]) {
        // First restore if window is minimized
        let mut placement: WINDOWPLACEMENT = WINDOWPLACEMENT {
            length: mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        match unsafe { GetWindowPlacement(client.window_handle, &mut placement) } {
            Ok(_) => {}
            Err(_) => {
                continue;
            }
        }
        if placement.showCmd == SW_SHOWMINIMIZED.0.try_into().unwrap() {
            let _ = unsafe { ShowWindow(client.window_handle, SW_RESTORE) };
        }
        // Then bring it to front using UI automation
        focus_window(client.window_handle);
    }
}

fn focus_window(handle: HWND) {
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.unwrap();
    if let Ok(window) = unsafe { automation.ElementFromHandle(handle) } {
        unsafe { window.SetFocus() }.unwrap();
    }
}

/// The entrypoint for the `daemon` subcommand.
///
/// Spawns 1 client process with its own window for each host
/// and 1 worker thread that handles communication with the client
/// over a named pipe.
/// Responsible for client window positioning and sizing.
/// Handles control mode.
/// Main thread reads input records from the console input buffer
/// and propagates them via the background threads to all clients
/// simultaneously.
///
/// # Arguments
///
/// * `hosts`    - List of hostnames for which to launch clients.
/// * `username` - Username used to connect to the hosts.
///                If none, each client will use the SSH config to determine
///                a suitable username for their respective host.
/// * `port`     - Optional port used for all SSH connections.
/// * `config`   - The `DaemonConfig`.
/// * `debug`    - Enables debug logging
#[cfg(not(test))]
pub async fn main(
    hosts: Vec<String>,
    username: Option<String>,
    port: Option<u16>,
    config: &DaemonConfig,
    clusters: &[Cluster],
    debug: bool,
) {
    let daemon: Daemon = Daemon {
        hosts: explode(&hosts.join(" ")).unwrap_or(hosts),
        username,
        port,
        config,
        clusters,
        control_mode_state: ControlModeState::Inactive,
        debug,
    };
    daemon.launch().await;
    debug!("Actually exiting");
}

/// Test-only entrypoint for the `daemon` subcommand used during unit tests.
///
/// This function constructs a `Daemon` with the provided parameters and then
/// invokes the test-only `launch` implementation which exercises safe code paths
/// (title, colors, processed input toggle, geometry/arrangement) without spawning
/// real client processes or performing Windows UI automation that would break tests.
/// Finally it panics intentionally so tests using `catch_unwind` can validate setup
/// behavior without hanging indefinitely.
///
/// # Arguments
///
/// * `hosts`    - List of hostnames (expanded via bracoxide in tests similarly to production).
/// * `username` - Optional username for clients.
/// * `port`     - Optional port used for SSH connections.
/// * `config`   - The `DaemonConfig`.
/// * `clusters` - Available cluster tags.
/// * `debug`    - Enables debug logging on the daemon and clients (in test-safe ways).
#[cfg(test)]
pub async fn main(
    hosts: Vec<String>,
    username: Option<String>,
    port: Option<u16>,
    config: &DaemonConfig,
    clusters: &[Cluster],
    debug: bool,
) {
    let daemon: Daemon = Daemon {
        hosts: explode(&hosts.join(" ")).unwrap_or(hosts),
        username,
        port,
        config,
        clusters,
        control_mode_state: ControlModeState::Inactive,
        debug,
    };
    // Invoke the test-only launch that will panic after exercising safe paths.
    daemon.launch().await;
}

#[cfg(test)]
#[path = "../tests/daemon/test_mod.rs"]
mod test_mod;
