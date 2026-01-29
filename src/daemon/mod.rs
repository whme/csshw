//! Daemon imlementation

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use std::cmp::max;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{thread, time};

use crate::get_console_window_handle;
use crate::utils::config::{Cluster, DaemonConfig};
use crate::utils::debug::StringRepr;
use crate::utils::windows::{clear_screen, set_console_color, WindowsApi};
use crate::{
    serde::{
        serialization::serialize_input_record_0, CONTROL_SEQ_STATE_DISABLED,
        CONTROL_SEQ_STATE_ENABLED, CONTROL_SEQ_STATE_SELECTED, SERIALIZED_INPUT_RECORD_0_LENGTH,
    },
    spawn_console_process,
    utils::{
        constants::{PIPE_NAME, PKG_NAME},
        windows::{
            arrange_console, get_console_input_buffer, read_keyboard_input,
            set_console_border_color,
        },
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
use windows::Win32::System::Console::{
    CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD_0, LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_C, VK_DOWN, VK_E, VK_ESCAPE, VK_H, VK_LEFT, VK_N, VK_R, VK_RIGHT, VK_T,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_RESTORE, SW_SHOWMINIMIZED};
use windows::Win32::{
    Foundation::{COLORREF, HANDLE, HWND, STILL_ACTIVE},
    System::{Console::ENABLE_PROCESSED_INPUT, Threading::PROCESS_QUERY_INFORMATION},
};

use self::workspace::WorkspaceArea;

mod workspace;

/// The capacity of the broadcast channel used
/// to send the input records read from the console input buffer
/// to the named pipe servers connected to each client in parallel.
const SENDER_CAPACITY: usize = 1024 * 1024;

/// Client window visual state
#[derive(Clone, Copy, Debug, PartialEq)]
enum ClientState {
    /// Client window is enabled for input
    /// Visual change for client: No highlight
    Enabled,
    /// Client window is disabled for input
    /// Visual change for client: Grey-ish background
    Disabled,
    /// Client window is currently selected (for toggling enabled/disabled)
    /// Visual change for client: Powder blue background
    Selected,
}

/// Representation of a client
#[derive(Clone)]
struct Client {
    /// Hostname the client is connect to (or supposed to connect to).
    hostname: String,
    /// Window handle to the clients console window.
    window_handle: HWND,
    /// Process handle to the client process.
    process_handle: HANDLE,
    /// Current state of the client window (enabled, disabled, or selected)
    state: ClientState,
    /// State before the window was selected (used to restore when navigating away)
    state_before_selection: Option<ClientState>,
    /// Pending state update control sequence to send to this client
    pending_state_update: Option<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    /// Grid row position (0-based)
    grid_row: i32,
    /// Grid column position (0-based, leftmost column this window occupies)
    grid_column: i32,
    /// Number of columns this window spans (for incomplete rows)
    grid_column_span: i32,
    /// Number of rows this window spans (for incomplete columns)
    grid_row_span: i32,
}

unsafe impl Send for Client {}

impl Client {
    /// Get the geometric center of the client window.
    ///
    /// # Arguments
    ///
    /// * `windows_api` - The Windows API implementation to use
    ///
    /// # Returns
    ///
    /// A tuple containing the (x, y) coordinates of the window center.
    /// Returns (0, 0) if the window placement cannot be retrieved.
    fn get_center<W: WindowsApi>(&self, windows_api: &W) -> (i32, i32) {
        match windows_api.get_window_placement(self.window_handle) {
            Ok(placement) => {
                let rect = placement.rcNormalPosition;
                let center_x = (rect.left + rect.right) / 2;
                let center_y = (rect.top + rect.bottom) / 2;
                return (center_x, center_y);
            }
            Err(_) => return (0, 0),
        }
    }

    /// Queue a state update control sequence for this client.
    fn queue_state_update(&mut self) {
        let control_seq = match self.state {
            ClientState::Enabled => CONTROL_SEQ_STATE_ENABLED,
            ClientState::Disabled => CONTROL_SEQ_STATE_DISABLED,
            ClientState::Selected => CONTROL_SEQ_STATE_SELECTED,
        };
        debug!(
            "Queuing state update for client '{}': {:?}",
            self.hostname, self.state
        );
        self.pending_state_update = Some(control_seq);
    }
}

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
fn get_console_window_wrapper(api: &dyn WindowsApi) -> HWNDWrapper {
    return HWNDWrapper {
        hwdn: api.get_console_window(),
    };
}

/// Returns a window handle to the foreground window.
///
/// The [HWND] is wrapped in a `HWNDWrapper` so that
/// we can pass it inbetween threads.
fn get_foreground_window_wrapper(api: &dyn WindowsApi) -> HWNDWrapper {
    return HWNDWrapper {
        hwdn: api.get_foreground_window(),
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
    /// Enable/disable input control mode is active.
    /// Arrow keys navigate between windows, T toggles selected window.
    EnableDisableMode,
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
    /// Index of the currently selected client in enable/disable mode.
    /// None if no client is selected.
    selected_client_index: Option<usize>,
    /// If debug mode is enabled on the daemon it will also be enabled on all
    /// clients.
    debug: bool,
}

impl<'a> Daemon<'a> {
    /// Launches all client windows and blocks on the main run loop.
    ///
    /// Sets up the daemon console by disabling processed input mode and applying
    /// the configured colors and dimensions.
    /// Once all client windows have successfully started the daemon console window
    /// is moved to the foreground and receives focus.
    async fn launch<W: WindowsApi + Clone + 'static>(mut self, windows_api: &W) {
        windows_api
            .set_console_title(format!("{PKG_NAME} daemon").as_str())
            .unwrap();
        set_console_color(
            windows_api,
            CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color),
        );
        let _ = set_console_border_color(windows_api, COLORREF(0x000000FF));

        toggle_processed_input_mode(windows_api); // Disable processed input mode

        // Initialize the COM library so we can use UI automation
        windows_api
            .initialize_com_library(windows::Win32::System::Com::COINIT_MULTITHREADED)
            .unwrap();

        let workspace_area = workspace::get_workspace_area(windows_api, self.config.height);

        self.arrange_daemon_console(windows_api, &workspace_area);

        // Looks like on windows 10 re-arranging the console resets the console output buffer
        set_console_color(
            windows_api,
            CONSOLE_CHARACTER_ATTRIBUTES(self.config.console_color),
        );

        let mut clients = Arc::new(Mutex::new(
            launch_clients(
                windows_api,
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
        let daemon_console = windows_api.get_console_window();
        let _ = windows_api.set_foreground_window(daemon_console);
        let _ = windows_api.focus_window_with_automation(daemon_console);

        self.print_instructions(windows_api);
        self.run(windows_api, &mut clients, &workspace_area).await;
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
    /// * `windows_api`                     - The Windows API implementation to use
    /// * `clients`                         - A thread safe mapping from the number
    ///                                       a client console window was launched at
    ///                                       in relation to the other client windows
    ///                                       and the clients console window handle.
    /// * `workspace_area`                  - The available workspace area on the
    ///                                       primary monitor minus the space occupied
    ///                                       by the daemon console window.
    async fn run<W: WindowsApi + Clone + 'static>(
        &mut self,
        windows_api: &W,
        clients: &mut Arc<Mutex<Vec<Client>>>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = Arc::new(Mutex::new(self.launch_named_pipe_servers(&sender, clients)));

        // Monitor client processes
        let clients_clone = Arc::clone(clients);
        let windows_api_clone = windows_api.clone();
        tokio::spawn(async move {
            loop {
                clients_clone.lock().unwrap().retain(|client| {
                    match windows_api_clone.get_exit_code(client.process_handle) {
                        Ok(exit_code) => return exit_code == STILL_ACTIVE.0 as u32,
                        Err(_) => return false, // Process handle is invalid, remove client
                    }
                });
                if clients_clone.lock().unwrap().is_empty() {
                    // All clients have exited, exit the daemon as well
                    std::process::exit(0);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        ensure_client_z_order_in_sync_with_daemon(
            Arc::new(windows_api.clone()),
            clients.to_owned(),
        );

        loop {
            self.handle_input_record(
                windows_api,
                &sender,
                read_keyboard_input(windows_api),
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
    /// * `clients` - Thread-safe list of clients to check state
    ///
    /// # Returns
    ///
    /// Returns a list of [JoinHandle]s, one handle for each thread.
    fn launch_named_pipe_servers(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        clients: &Arc<Mutex<Vec<Client>>>,
    ) -> Vec<JoinHandle<()>> {
        let mut servers: Vec<JoinHandle<()>> = Vec::new();
        let clients_lock = clients.lock().unwrap();
        for client in clients_lock.iter() {
            self.launch_named_pipe_server(
                &mut servers,
                sender,
                client.window_handle.0 as isize,
                clients,
            );
        }
        drop(clients_lock);
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
    /// * `client_window_handle_raw` - Raw window handle value of the client this server corresponds to
    /// * `clients` - Thread-safe list of clients to check state
    fn launch_named_pipe_server(
        &self,
        servers: &mut Vec<JoinHandle<()>>,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        client_window_handle_raw: isize,
        clients: &Arc<Mutex<Vec<Client>>>,
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
        let clients_clone = Arc::clone(clients);
        servers.push(tokio::spawn(async move {
            named_pipe_server_routine(
                named_pipe_server,
                &mut receiver,
                client_window_handle_raw,
                clients_clone,
            )
            .await;
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
    async fn handle_input_record<W: WindowsApi + Clone + 'static>(
        &mut self,
        windows_api: &W,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        input_record: INPUT_RECORD_0,
        clients: &mut Arc<Mutex<Vec<Client>>>,
        workspace_area: &workspace::WorkspaceArea,
        servers: &mut Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) {
        if self.control_mode_is_active(windows_api, input_record) {
            if self.control_mode_state == ControlModeState::Initiated {
                clear_screen(windows_api);
                println!("Control Mode (Esc to exit)");
                println!("[c]reate window(s), [e]nable/disable input, e[n]able all, [r]etile, copy active [h]ostname(s)");
                self.control_mode_state = ControlModeState::Active;
                return;
            }
            let key_event = unsafe { input_record.KeyEvent };
            if !key_event.bKeyDown.as_bool() {
                return;
            }

            // Handle enable/disable mode separately
            if self.control_mode_state == ControlModeState::EnableDisableMode {
                return self.handle_enable_disable_mode(
                    windows_api,
                    key_event,
                    clients,
                    workspace_area,
                );
            }

            match (
                VIRTUAL_KEY(key_event.wVirtualKeyCode),
                key_event.dwControlKeyState,
            ) {
                (VK_R, 0) => {
                    self.rearrange_client_windows(
                        windows_api,
                        &clients.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(windows_api, workspace_area);
                }
                (VK_E, 0) => {
                    self.enter_enable_disable_mode(windows_api, clients);
                }
                (VK_N, 0) => {
                    self.enable_all_clients(clients);
                    self.quit_control_mode(windows_api);
                }
                (VK_C, 0) => {
                    clear_screen(windows_api);
                    // TODO: make ESC abort
                    println!("Hostname(s) or cluster tag(s): (leave empty to abort)");
                    toggle_processed_input_mode(windows_api); // As it was disabled before, this enables it again
                    let mut hostnames = String::new();
                    match io::stdin().read_line(&mut hostnames) {
                        Ok(2) => {
                            // Empty input (only newline '\n')
                        }
                        Ok(_) => {
                            let number_of_existing_clients = clients.lock().unwrap().len();
                            let new_clients = launch_clients(
                                windows_api,
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
                                let client_hwnd_raw = client.window_handle.0 as isize;
                                clients.lock().unwrap().push(client);
                                self.launch_named_pipe_server(
                                    &mut servers.lock().unwrap(),
                                    sender,
                                    client_hwnd_raw,
                                    clients,
                                );
                            }
                        }
                        Err(error) => {
                            error!("{error}");
                        }
                    }
                    toggle_processed_input_mode(windows_api); // Re-disable processed input mode.
                    self.rearrange_client_windows(
                        windows_api,
                        &clients.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(windows_api, workspace_area);
                    // Focus the daemon console again.
                    let daemon_window = windows_api.get_console_window();
                    let _ = windows_api.set_foreground_window(daemon_window);
                    let _ = windows_api.focus_window_with_automation(daemon_window);
                    self.quit_control_mode(windows_api);
                }
                (VK_H, 0) => {
                    let mut active_hostnames: Vec<String> = vec![];
                    for client in clients.lock().unwrap().iter() {
                        if windows_api.is_window(client.window_handle) {
                            active_hostnames.push(client.hostname.clone());
                        }
                    }
                    cli_clipboard::set_contents(active_hostnames.join(" ")).unwrap();
                    self.quit_control_mode(windows_api);
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
    /// * `windows_api` - The Windows API implementation to use
    /// * `input_record` -  A KeyEvent input record.
    ///
    /// # Returns
    ///
    /// Whether or not control mode is active.
    fn control_mode_is_active<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        input_record: INPUT_RECORD_0,
    ) -> bool {
        let key_event = unsafe { input_record.KeyEvent };

        // EnableDisableMode needs special handling for Escape - don't intercept it here
        if self.control_mode_state == ControlModeState::EnableDisableMode {
            return true;
        }

        if self.control_mode_state == ControlModeState::Active {
            if key_event.wVirtualKeyCode == VK_ESCAPE.0 {
                self.quit_control_mode(windows_api);
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
    fn quit_control_mode<W: WindowsApi>(&mut self, windows_api: &W) {
        self.print_instructions(windows_api);
        self.control_mode_state = ControlModeState::Inactive;
        self.selected_client_index = None;
    }

    /// Enters enable/disable mode and selects the top-left-most window.
    ///
    /// # Arguments
    ///
    /// * `windows_api` - The Windows API implementation to use
    /// * `clients` - Thread-safe list of clients
    fn enter_enable_disable_mode<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        clients: &Arc<Mutex<Vec<Client>>>,
    ) {
        clear_screen(windows_api);
        println!("Enable/Disable Mode (Esc to exit)");
        println!("Arrow keys to navigate, [t]oggle selected, e[n]able all");

        self.control_mode_state = ControlModeState::EnableDisableMode;

        // Find the top-left-most window and select it
        let mut clients_lock = clients.lock().unwrap();
        if clients_lock.is_empty() {
            return;
        }

        let mut top_left_index = 0;
        let mut min_distance = i32::MAX;

        debug!("Window positions at mode entry:");
        for (index, client) in clients_lock.iter().enumerate() {
            let (x, y) = client.get_center(windows_api);
            let distance = x * x + y * y; // Distance from origin (0,0)
            debug!(
                "  [{}] {}: center=({}, {}), distance={}",
                index, client.hostname, x, y, distance
            );
            if distance < min_distance {
                min_distance = distance;
                top_left_index = index;
            }
        }

        self.selected_client_index = Some(top_left_index);
        debug!(
            "Selecting top-left client at index {}: {}",
            top_left_index, clients_lock[top_left_index].hostname
        );
        clients_lock[top_left_index].state_before_selection =
            Some(clients_lock[top_left_index].state);
        clients_lock[top_left_index].state = ClientState::Selected;
        clients_lock[top_left_index].queue_state_update();
    }

    /// Handles input in enable/disable mode (arrow keys and toggle).
    ///
    /// # Arguments
    ///
    /// * `windows_api` - The Windows API implementation to use
    /// * `key_event` - The key event to handle
    /// * `clients` - Thread-safe list of clients
    fn handle_enable_disable_mode<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        key_event: windows::Win32::System::Console::KEY_EVENT_RECORD,
        clients: &Arc<Mutex<Vec<Client>>>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        debug!(
            "handle_enable_disable_mode: key={}, bKeyDown={:?}",
            key_event.wVirtualKeyCode, key_event.bKeyDown
        );

        match VIRTUAL_KEY(key_event.wVirtualKeyCode) {
            VK_ESCAPE => {
                debug!("Matched VK_ESCAPE - exiting enable/disable mode");
                // Exit enable/disable mode
                {
                    let mut clients_lock = clients.lock().unwrap();
                    if let Some(index) = self.selected_client_index {
                        if index < clients_lock.len() {
                            // Restore to state before selection, or Enabled if no state was saved
                            let mut restore_state = clients_lock[index]
                                .state_before_selection
                                .unwrap_or(ClientState::Enabled);

                            // Safety: ensure we never restore to Selected
                            if restore_state == ClientState::Selected {
                                restore_state = ClientState::Enabled;
                            }

                            debug!(
                                "Exiting mode: restoring client {} from {:?} to {:?}",
                                clients_lock[index].hostname,
                                clients_lock[index].state,
                                restore_state
                            );

                            clients_lock[index].state = restore_state;
                            clients_lock[index].state_before_selection = None;
                            clients_lock[index].queue_state_update();
                        }
                    }
                } // Drop the lock before calling quit_control_mode
                self.quit_control_mode(windows_api);
            }
            VK_T => {
                debug!("Matched VK_T - toggling selected client");
                // Toggle the selected client
                self.toggle_selected_client(clients);
            }
            VK_N => {
                debug!("Matched VK_N - enabling all clients");
                // Enable all clients
                self.enable_all_clients(clients);
                self.quit_control_mode(windows_api);
            }
            VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT => {
                debug!("Matched arrow key - navigating");
                // Navigate to another window
                self.navigate_selection(
                    windows_api,
                    key_event.wVirtualKeyCode,
                    clients,
                    workspace_area,
                );
            }
            _ => {
                debug!(
                    "Unhandled key in enable/disable mode: {}",
                    key_event.wVirtualKeyCode
                );
            }
        }
    }

    /// Toggles the selected client between enabled and disabled.
    ///
    /// # Arguments
    ///
    /// * `clients` - Thread-safe list of clients
    fn toggle_selected_client(&mut self, clients: &Arc<Mutex<Vec<Client>>>) {
        let mut clients_lock = clients.lock().unwrap();
        if let Some(index) = self.selected_client_index {
            if index < clients_lock.len() {
                // Toggle the base state (what it will be when not selected)
                let new_base_state = match clients_lock[index].state_before_selection {
                    Some(ClientState::Enabled) | None => ClientState::Disabled,
                    Some(ClientState::Disabled) => ClientState::Enabled,
                    Some(ClientState::Selected) => ClientState::Disabled, // shouldn't happen
                };

                debug!(
                    "Toggling client {} base state from {:?} to {:?}",
                    clients_lock[index].hostname,
                    clients_lock[index].state_before_selection,
                    new_base_state
                );

                // Update the base state and keep window selected
                clients_lock[index].state_before_selection = Some(new_base_state);
                clients_lock[index].state = ClientState::Selected;
                clients_lock[index].queue_state_update();
            }
        }
    }

    /// Enables all clients.
    ///
    /// # Arguments
    ///
    /// * `clients` - Thread-safe list of clients
    fn enable_all_clients(&mut self, clients: &Arc<Mutex<Vec<Client>>>) {
        let mut clients_lock = clients.lock().unwrap();
        for client in clients_lock.iter_mut() {
            client.state = ClientState::Enabled;
            client.state_before_selection = None;
            client.queue_state_update();
        }
    }

    /// Navigates the selection to the nearest window in the specified direction using grid logic.
    ///
    /// # Arguments
    ///
    /// * `direction` - The arrow key code indicating direction
    /// * `clients` - Thread-safe list of clients
    /// * `windows_api` - The Windows API implementation (unused but kept for consistency)
    /// * `workspace_area` - The workspace area (unused but kept for consistency)
    fn navigate_selection<W: WindowsApi>(
        &mut self,
        _windows_api: &W,
        direction: u16,
        clients: &Arc<Mutex<Vec<Client>>>,
        _workspace_area: &workspace::WorkspaceArea,
    ) {
        let mut clients_lock = clients.lock().unwrap();

        let current_index = match self.selected_client_index {
            Some(index) if index < clients_lock.len() => index,
            _ => return,
        };

        let current_row = clients_lock[current_index].grid_row;
        let current_col = clients_lock[current_index].grid_column;
        let current_span = clients_lock[current_index].grid_column_span;

        let direction_name = match VIRTUAL_KEY(direction) {
            VK_UP => "UP",
            VK_DOWN => "DOWN",
            VK_LEFT => "LEFT",
            VK_RIGHT => "RIGHT",
            _ => "UNKNOWN",
        };
        debug!(
            "Navigating {} from [{}] {} (row={}, col={}, span={})",
            direction_name,
            current_index,
            clients_lock[current_index].hostname,
            current_row,
            current_col,
            current_span
        );

        // Helper function to check if a window occupies a specific grid cell
        let window_occupies_cell = |client: &Client, row: i32, col: i32| -> bool {
            let row_in_range =
                row >= client.grid_row && row < client.grid_row + client.grid_row_span;
            let col_in_range =
                col >= client.grid_column && col < client.grid_column + client.grid_column_span;
            return row_in_range && col_in_range;
        };

        // Build list of all cells occupied by current window
        let mut current_cells = Vec::new();
        for r in current_row..(current_row + clients_lock[current_index].grid_row_span) {
            for c in current_col..(current_col + current_span) {
                current_cells.push((r, c));
            }
        }

        // Find windows in current cells
        let mut current_cell_windows: Vec<usize> = Vec::new();
        for (row, col) in &current_cells {
            for (index, client) in clients_lock.iter().enumerate() {
                if window_occupies_cell(client, *row, *col)
                    && !current_cell_windows.contains(&index)
                {
                    current_cell_windows.push(index);
                }
            }
        }

        // Determine target cells based on direction
        let target_cells: Vec<(i32, i32)> = match VIRTUAL_KEY(direction) {
            VK_RIGHT => {
                // All cells one column to the right
                let mut cells = Vec::new();
                for r in current_row..(current_row + clients_lock[current_index].grid_row_span) {
                    cells.push((r, current_col + current_span));
                }
                cells
            }
            VK_LEFT => {
                // All cells one column to the left
                let mut cells = Vec::new();
                for r in current_row..(current_row + clients_lock[current_index].grid_row_span) {
                    cells.push((r, current_col - 1));
                }
                cells
            }
            VK_DOWN => {
                // All cells one row down
                let mut cells = Vec::new();
                for c in current_col..(current_col + current_span) {
                    cells.push((current_row + clients_lock[current_index].grid_row_span, c));
                }
                cells
            }
            VK_UP => {
                // All cells one row up
                let mut cells = Vec::new();
                for c in current_col..(current_col + current_span) {
                    cells.push((current_row - 1, c));
                }
                cells
            }
            _ => Vec::new(),
        };

        // Find windows in target cells
        let mut target_cell_windows: Vec<usize> = Vec::new();
        for (row, col) in &target_cells {
            for (index, client) in clients_lock.iter().enumerate() {
                if window_occupies_cell(client, *row, *col) && !target_cell_windows.contains(&index)
                {
                    target_cell_windows.push(index);
                }
            }
        }

        // Find windows in target cells that are different from current window
        // For overlapping windows, we check the window's primary position (leftmost column for LEFT/RIGHT)
        let mut best_index: Option<usize> = None;
        match VIRTUAL_KEY(direction) {
            VK_LEFT => {
                let mut max_col = i32::MIN;
                for &index in &target_cell_windows {
                    if index == current_index {
                        continue;
                    }
                    let col = clients_lock[index].grid_column;
                    if col < current_col && col > max_col {
                        max_col = col;
                        best_index = Some(index);
                    }
                }
            }
            VK_RIGHT => {
                let mut min_col = i32::MAX;
                for &index in &target_cell_windows {
                    if index == current_index {
                        continue;
                    }
                    let col = clients_lock[index].grid_column;
                    if col > current_col && col < min_col {
                        min_col = col;
                        best_index = Some(index);
                    }
                }
            }
            VK_UP => {
                let mut max_row = i32::MIN;
                for &index in &target_cell_windows {
                    if index == current_index {
                        continue;
                    }
                    let row = clients_lock[index].grid_row;
                    if row < current_row && row > max_row {
                        max_row = row;
                        best_index = Some(index);
                    }
                }
            }
            VK_DOWN => {
                let mut min_row = i32::MAX;
                for &index in &target_cell_windows {
                    if index == current_index {
                        continue;
                    }
                    let row = clients_lock[index].grid_row;
                    if row > current_row && row < min_row {
                        min_row = row;
                        best_index = Some(index);
                    }
                }
            }
            _ => {}
        }

        if let Some(index) = best_index {
            debug!(
                "  Found neighbor: [{}] {} (row={}, col={}, row_span={}, col_span={})",
                index,
                clients_lock[index].hostname,
                clients_lock[index].grid_row,
                clients_lock[index].grid_column,
                clients_lock[index].grid_row_span,
                clients_lock[index].grid_column_span
            );
        } else {
            debug!("  No neighbor found in that direction");
        }

        // Update selection if we found a better window
        if let Some(best_index) = best_index {
            debug!(
                "Navigating from client {} to client {}",
                clients_lock[current_index].hostname, clients_lock[best_index].hostname
            );
            // Restore previous window's state to what it was before selection
            if let Some(previous_state) = clients_lock[current_index].state_before_selection {
                clients_lock[current_index].state = previous_state;
                clients_lock[current_index].state_before_selection = None;
                clients_lock[current_index].queue_state_update();
            }

            // Save current state (only if it's not Selected, otherwise keep the saved state)
            if clients_lock[best_index].state != ClientState::Selected {
                clients_lock[best_index].state_before_selection =
                    Some(clients_lock[best_index].state);
            }
            clients_lock[best_index].state = ClientState::Selected;
            clients_lock[best_index].queue_state_update();

            self.selected_client_index = Some(best_index);
        }
    }

    /// Clears the console screen and prints the default daemon instructions.
    fn print_instructions<W: WindowsApi>(&self, windows_api: &W) {
        clear_screen(windows_api);
        println!("Input to terminal: (Ctrl-A to enter control mode)");
    }

    /// Iterates over all still open client windows and re-arranges them
    /// on the screen based on the aspect ration adjustment daemon configuration.
    ///
    /// Client windows will be re-sized and re-positioned.
    ///
    /// # Arguments
    ///
    /// * `windows_api`                     - The Windows API implementation to use
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
    fn rearrange_client_windows<W: WindowsApi>(
        &self,
        windows_api: &W,
        clients: &[Client],
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let mut valid_clients = Vec::new();
        for client in clients.iter() {
            let exit_code = match windows_api.get_exit_code(client.process_handle) {
                Ok(code) => code,
                Err(_) => continue, // Process handle is invalid, skip client
            };
            if exit_code == STILL_ACTIVE.0 as u32 && windows_api.is_window(client.window_handle) {
                valid_clients.push(client);
            }
        }
        for (index, client) in valid_clients.iter().enumerate() {
            arrange_client_window(
                windows_api,
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
    /// * `windows_api` - The Windows API implementation to use
    /// * `workspace_area` - The available workspace area on the
    ///                      primary monitor minus the space occupied
    ///                      by the daemon console window.
    fn arrange_daemon_console<W: WindowsApi>(
        &self,
        windows_api: &W,
        workspace_area: &WorkspaceArea,
    ) {
        let (x, y, width, height) = get_console_rect(
            0,
            workspace_area.height,
            workspace_area.width - (workspace_area.x_fixed_frame + workspace_area.x_size_frame),
            self.config.height,
            workspace_area,
        );
        arrange_console(windows_api, x, y, width, height);
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
///
/// # Arguments
///
/// * `windows_api` - The Windows API implementation to use
fn toggle_processed_input_mode<W: WindowsApi>(windows_api: &W) {
    let handle = get_console_input_buffer();
    let mode = windows_api.get_console_mode(handle).unwrap();
    let new_mode = windows::Win32::System::Console::CONSOLE_MODE(mode.0 ^ ENABLE_PROCESSED_INPUT.0);
    let _ = windows_api.set_console_mode(handle, new_mode);
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
/// * `windows_api`             - The Windows API implementation to use
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
async fn launch_clients<W: WindowsApi + 'static + Clone>(
    windows_api: &W,
    hosts: Vec<String>,
    username: &Option<String>,
    port: Option<u16>,
    debug: bool,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
    index_offset: usize,
) -> Vec<Client> {
    let len_hosts = hosts.len();
    let _guard = WindowsSettingsDefaultTerminalApplicationGuard::new();

    // Create an Arc to share the windows_api across parallel tasks
    let windows_api_arc = Arc::new(windows_api.clone());

    // Create tasks for each client launch using spawn_blocking to handle the synchronous operations
    let mut tasks = Vec::new();

    for (index, host) in hosts.into_iter().enumerate() {
        let username_client = username.clone();
        let workspace_area_client = *workspace_area;
        let windows_api_clone = Arc::clone(&windows_api_arc);

        // Use spawn_blocking to run the synchronous launch_client_console in parallel
        let task = tokio::task::spawn_blocking(move || {
            let (window_handle, process_handle) = launch_client_console(
                windows_api_clone.as_ref(),
                &host,
                username_client,
                port,
                debug,
                index + index_offset,
                &workspace_area_client,
                len_hosts + index_offset,
                aspect_ratio_adjustment,
            );
            let (grid_row, grid_column, grid_column_span, grid_row_span) = calculate_grid_position(
                (index + index_offset) as i32,
                (len_hosts + index_offset) as i32,
                &workspace_area_client,
                aspect_ratio_adjustment,
            );
            return (
                index,
                Client {
                    hostname: host,
                    window_handle,
                    process_handle,
                    state: ClientState::Enabled,
                    state_before_selection: None,
                    pending_state_update: None,
                    grid_row,
                    grid_column,
                    grid_column_span,
                    grid_row_span,
                },
            );
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete in parallel
    let mut results = Vec::new();
    for task in tasks {
        match task.await {
            Ok(result) => results.push(result),
            Err(e) => panic!("Failed to launch client: {e}"),
        }
    }

    // Sort results by index to maintain order
    results.sort_by_key(|(index, _)| return *index);

    return results
        .into_iter()
        .map(|(_, client)| return client)
        .collect();
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
/// * `windows_api`             - The Windows API implementation to use
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
fn launch_client_console<W: WindowsApi>(
    windows_api: &W,
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

    let process_info = spawn_console_process(windows_api, &format!("{PKG_NAME}.exe"), client_args)
        .expect("Failed to create process");
    let client_window_handle = get_console_window_handle(windows_api, process_info.dwProcessId);
    let process_handle = windows_api
        .open_process(PROCESS_QUERY_INFORMATION.0, false, process_info.dwProcessId)
        .unwrap_or_else(|err| {
            panic!(
                "Failed to open process handle for process {}: {}",
                process_info.dwProcessId, err
            );
        });

    arrange_client_window(
        windows_api,
        &client_window_handle,
        workspace_area,
        index,
        number_of_consoles,
        aspect_ratio_adjustment,
    );
    return (client_window_handle, process_handle);
}

/// Wait for the named pipe server to connect, then forward serialized
/// input records read from the broadcast channel to the named pipe server.
///
/// If writing to the pipe fails the pipe is closed and the routine ends.
/// To detect if a client is still alive even if we are currently
/// not sending data, we send a "keep alive packet",
/// [`SERIALIZED_INPUT_RECORD_0_LENGTH`] bytes of `1`s. If that fails, the routine ends.
///
/// Disabled clients (ClientState::Disabled) will not receive input records.
///
/// # Arguments
///
/// * `server`   - The named pipe server over which we send data to the
///                client.
/// * `receiver` - The receiving end of the broadcast channel through
///                which we get the serialize input records from the main
///                thread that are to be sent to the client via the named
///                pipe.
/// * `client_window_handle_raw` - Raw window handle value of the client this server corresponds to
/// * `clients` - Thread-safe list of clients to check state
async fn named_pipe_server_routine(
    server: NamedPipeServer,
    receiver: &mut Receiver<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    client_window_handle_raw: isize,
    clients: Arc<Mutex<Vec<Client>>>,
) {
    // wait for a client to connect
    server.connect().await.unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Timeded out waiting for clients to connect to named pipe server",)
    });

    // First, receive the client's window handle identification
    let actual_client_window_handle_raw: isize = {
        let mut id_buffer = [0u8; 8];
        server.readable().await.unwrap();
        match server.try_read(&mut id_buffer) {
            Ok(8) => isize::from_le_bytes(id_buffer),
            Ok(n) => {
                error!("Received incomplete client ID: {} bytes", n);
                return;
            }
            Err(e) => {
                error!("Failed to receive client ID: {}", e);
                return;
            }
        }
    };

    debug!(
        "Pipe server (expected client HWND 0x{:X}) connected to actual client HWND 0x{:X}",
        client_window_handle_raw, actual_client_window_handle_raw
    );

    // Use the actual client window handle for all operations
    let client_window_handle_raw = actual_client_window_handle_raw;

    loop {
        // Check for and send any pending state updates first
        let (pending_update, hostname) = {
            let mut clients_lock = clients.lock().unwrap();
            clients_lock
                .iter_mut()
                .find(|c| return c.window_handle.0 as isize == client_window_handle_raw)
                .map(|client| return (client.pending_state_update.take(), client.hostname.clone()))
                .unwrap_or((None, String::new()))
        };

        if let Some(control_seq) = pending_update {
            debug!(
                "Sending control sequence to client '{}' (HWND 0x{:X}): {:?}",
                hostname, client_window_handle_raw, control_seq
            );
            // Send the control sequence
            loop {
                server.writable().await.unwrap_or_else(|err| {
                    error!("{}", err);
                    panic!("Timed out waiting for named pipe server to become writable",)
                });
                match server.try_write(&control_seq) {
                    Ok(SERIALIZED_INPUT_RECORD_0_LENGTH) => {
                        debug!("Successfully sent control sequence");
                        break;
                    }
                    Ok(n) => {
                        warn!(
                            "Partially written control sequence, expected {} but only wrote {}",
                            SERIALIZED_INPUT_RECORD_0_LENGTH, n
                        );
                        continue;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(_) => {
                        debug!(
                            "Named pipe server ({:?}) is closed, stopping named pipe server routine",
                            server
                        );
                        return;
                    }
                }
            }
        }

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

        // Check if this client is disabled - if so, skip sending input
        {
            let clients_lock = clients.lock().unwrap();
            if let Some(client) = clients_lock
                .iter()
                .find(|c| return c.window_handle.0 as isize == client_window_handle_raw)
            {
                if client.state == ClientState::Disabled {
                    continue; // Skip sending to disabled clients
                }
            }
        }

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
/// * `windows_api`              - The Windows API implementation to use
/// * `handle`                   - Reference the windows handle of a client console window.
/// * `workspace_area`           - The available workspace area on the primary monitor
///                                minus the space occupied by the daemon console window.
/// * `index`                    - The index of the client in the list of all clients.
/// * `number_of_consoles`       - The total number of active client console windows.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
fn arrange_client_window<W: WindowsApi>(
    windows_api: &W,
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
    // Since windows update 10.0.19041.5072 it can happen that a client windows rendering is broken
    // after a move+resize. Why is unclear, but resizing again does solve the issue.
    // We first make the window 1 pixel in each dimension too small and imediately fix it.
    // To reduce overhead we do not repaint the window the first time.
    windows_api
        .move_window(*handle, x, y, width - 1, height - 1, false)
        .unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to move window",)
        });
    windows_api
        .move_window(*handle, x, y, width, height, true)
        .unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to move window",)
        });
}

/// Calculates the grid position (row, column, and column span) for a client window.
///
/// # Arguments
///
/// * `index`                    - The index of the client in the list of all clients.
/// * `number_of_consoles`       - The total number of active client console windows.
/// * `workspace_area`           - The available workspace area on the primary monitor
///                                minus the space occupied by the daemon console window.
/// * `aspect_ratio_adjustment` - The `aspect_ratio_adjustment` daemon configuration.
///
/// # Returns
///
/// A tuple containing (grid_row, grid_column, grid_column_span)
fn calculate_grid_position(
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

    // Calculate column span for incomplete rows
    let is_last_row = grid_row_index == grid_rows - 1;
    let last_row_console_count = number_of_consoles % grid_columns;

    let (final_column_index, column_span) = if is_last_row && last_row_console_count != 0 {
        // This is the last row and it's incomplete
        // Each window spans multiple columns proportionally
        let position_in_row = grid_column_index;

        // Calculate which columns this window occupies
        // For example, with 3 columns and 2 windows:
        // Window 0: columns 0-1 (span 2)
        // Window 1: columns 1-2 (span 2)
        let left_col = (position_in_row * grid_columns) / last_row_console_count;
        let right_col = ((position_in_row + 1) * grid_columns - 1) / last_row_console_count;
        let span = right_col - left_col + 1;

        (left_col, span)
    } else {
        // Complete row, each window occupies exactly one column
        (grid_column_index, 1)
    };

    // Row span is always 1 since we fill by rows, not columns
    // (Incomplete columns don't exist in our row-first layout)
    let row_span = 1;

    return (grid_row_index, final_column_index, column_span, row_span);
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
/// * `windows_api` - Arc-wrapped Windows API implementation for thread-safe access
/// * `clients`     - A thread safe mapping from the number
///                   a client console window was launched at
///                   in relation to the other client windows
///                   and the clients console window handle.
///                   The mapping must be thread safe to allow
///                   it to be modified by the main thread
///                   while we periodically read from it in the
///                   background thread.
fn ensure_client_z_order_in_sync_with_daemon<W: WindowsApi + Send + Sync + 'static>(
    windows_api: Arc<W>,
    clients: Arc<Mutex<Vec<Client>>>,
) {
    tokio::spawn(async move {
        let daemon_handle = get_console_window_wrapper(windows_api.as_ref());
        let mut previous_foreground_window = get_foreground_window_wrapper(windows_api.as_ref());
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let foreground_window = get_foreground_window_wrapper(windows_api.as_ref());
            if previous_foreground_window == foreground_window {
                continue;
            }
            if foreground_window == daemon_handle
                && !clients.lock().unwrap().iter().any(|client| {
                    return client.window_handle == previous_foreground_window.hwdn
                        || client.window_handle == daemon_handle.hwdn;
                })
            {
                defer_windows(
                    windows_api.as_ref(),
                    &clients.lock().unwrap(),
                    &daemon_handle.hwdn,
                );
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
/// * `windows_api`                   - The Windows API implementation to use
/// * `clients`                       - A thread safe mapping from the number
///                                     a client console window was launched at
///                                     in relation to the other client windows
///                                     and the clients console window handle.
/// * `daemon_handle`                 - Handle to the daemon console window.
fn defer_windows<W: WindowsApi>(windows_api: &W, clients: &[Client], daemon_handle: &HWND) {
    for client in clients.iter().chain([&Client {
        hostname: "root".to_owned(),
        window_handle: *daemon_handle,
        process_handle: HANDLE::default(),
        state: ClientState::Enabled,
        state_before_selection: None,
        pending_state_update: None,
        grid_row: 0,
        grid_column: 0,
        grid_column_span: 1,
        grid_row_span: 1,
    }]) {
        let placement = match windows_api.get_window_placement(client.window_handle) {
            Ok(placement) => placement,
            Err(_) => {
                continue;
            }
        };
        // First restore if window is minimized
        if placement.showCmd == SW_SHOWMINIMIZED.0.try_into().unwrap() {
            let _ = windows_api.show_window(client.window_handle, SW_RESTORE);
        }
        // Then bring it to front using UI automation
        let _ = windows_api.focus_window_with_automation(client.window_handle);
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
/// * `windows_api` - The Windows API implementation to use
/// * `hosts`    - List of hostnames for which to launch clients.
/// * `username` - Username used to connect to the hosts.
///                If none, each client will use the SSH config to determine
///                a suitable username for their respective host.
/// * `port`     - Optional port used for all SSH connections.
/// * `config`   - The `DaemonConfig`.
/// * `debug`    - Enables debug logging
pub async fn main<W: WindowsApi + Clone + 'static>(
    windows_api: &W,
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
        selected_client_index: None,
        debug,
    };
    daemon.launch(windows_api).await;
    debug!("Actually exiting");
}

#[cfg(test)]
#[path = "../tests/daemon/test_mod.rs"]
mod test_mod;
