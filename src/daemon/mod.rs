//! Daemon imlementation

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use std::cmp::max;
use std::collections::HashMap;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{thread, time};

use crate::get_console_window_handle;
use crate::protocol::{
    deserialization::deserialize_pid,
    serialization::{serialize_client_state, serialize_input_record_0},
    ClientState, FRAMED_INPUT_RECORD_LENGTH, FRAMED_STATE_CHANGE_LENGTH,
    SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH, TAG_INPUT_RECORD, TAG_KEEP_ALIVE,
    TAG_STATE_CHANGE,
};
use crate::utils::config::{Cluster, DaemonConfig};
use crate::utils::debug::StringRepr;
use crate::utils::windows::{clear_screen, set_console_color, WindowsApi};
use crate::{
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
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
    sync::{
        broadcast::{self, error::RecvError, Receiver, Sender},
        watch,
    },
    task::JoinHandle,
};
use windows::Win32::System::Console::{
    CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD_0, LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_C, VK_E, VK_ESCAPE, VK_H, VK_N, VK_R, VK_T,
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

/// Representation of a client
#[derive(Clone)]
struct Client {
    /// Hostname the client is connect to (or supposed to connect to).
    hostname: String,
    /// Window handle to the clients console window.
    window_handle: HWND,
    /// Process handle to the client process.
    process_handle: HANDLE,
    /// Process id of the client process.
    ///
    /// Used by the pipe server task to correlate which client has connected
    /// to it, via a handshake over the named pipe.
    process_id: u32,
    /// Authoritative source for this client's [`ClientState`].
    ///
    /// The daemon broadcasts new state values through the [`watch::Sender`];
    /// the assigned pipe-server task subscribes upon successful PID
    /// correlation and forwards every change to the client over the named
    /// pipe. [`watch::Sender`] is itself [`Clone`], so cloning a [`Client`]
    /// produces another sender that drives the same channel.
    state_tx: watch::Sender<ClientState>,
}

unsafe impl Send for Client {}

/// Collection of [`Client`]s maintaining insertion order and a PID-indexed
/// lookup table.
///
/// The ordered list preserves client window placement semantics, while the
/// index enables O(1) lookup by process id - required by the pipe server task
/// during PID correlation and future per-client pipe server control.
struct Clients {
    /// Ordered list of clients; order matches launch order and is used for
    /// window arrangement and z-order synchronization.
    list: Vec<Client>,
    /// Maps a client's process id to its index in [`list`](Clients::list).
    pid_index: HashMap<u32, usize>,
}

impl Clients {
    /// Creates a new empty collection.
    fn new() -> Self {
        return Clients {
            list: Vec::new(),
            pid_index: HashMap::new(),
        };
    }

    /// Appends a client to the collection and records its position in the
    /// PID index.
    ///
    /// # Arguments
    ///
    /// * `client` - The [`Client`] to add.
    ///
    /// # Panics
    ///
    /// Panics if a client with the same process id is already present, as
    /// duplicate PIDs indicate broken daemon bookkeeping.
    fn push(&mut self, client: Client) {
        let index = self.list.len();
        assert!(
            !self.pid_index.contains_key(&client.process_id),
            "Duplicate client PID {} - daemon bookkeeping broken",
            client.process_id,
        );
        self.pid_index.insert(client.process_id, index);
        self.list.push(client);
    }

    /// Returns a reference to the client with the given process id, if any.
    ///
    /// # Arguments
    ///
    /// * `pid` - The process id of the client to look up.
    ///
    /// # Returns
    ///
    /// `Some(&Client)` if a client with the given PID exists, `None` otherwise.
    fn get_by_pid(&self, pid: u32) -> Option<&Client> {
        return self
            .pid_index
            .get(&pid)
            .map(|&index| return &self.list[index]);
    }

    /// Retains only the clients for which the predicate returns `true`,
    /// rebuilding the PID index to reflect the new positions.
    ///
    /// # Arguments
    ///
    /// * `f` - Predicate applied to each [`Client`]; kept when it returns `true`.
    fn retain<F: FnMut(&Client) -> bool>(&mut self, mut f: F) {
        self.list.retain(|client| return f(client));
        self.pid_index.clear();
        for (index, client) in self.list.iter().enumerate() {
            self.pid_index.insert(client.process_id, index);
        }
    }
}

/// Allows treating a [`Clients`] collection as a `&[Client]`, so callers can
/// use `&clients` where a slice is expected and get slice methods
/// (`iter`, `len`, `is_empty`, ...) via deref coercion.
impl std::ops::Deref for Clients {
    type Target = [Client];

    fn deref(&self) -> &[Client] {
        return &self.list;
    }
}

/// Consumes the collection and yields its clients in insertion order.
///
/// Used when merging a freshly launched [`Clients`] batch into an existing
/// collection while also spawning per-client pipe servers.
impl IntoIterator for Clients {
    type Item = Client;
    type IntoIter = std::vec::IntoIter<Client>;

    fn into_iter(self) -> Self::IntoIter {
        return self.list.into_iter();
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
        set_console_border_color(windows_api, COLORREF(0x000000FF));

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
        clients: &mut Arc<Mutex<Clients>>,
        workspace_area: &workspace::WorkspaceArea,
    ) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(SENDER_CAPACITY);

        let mut servers = Arc::new(Mutex::new(
            self.launch_named_pipe_servers(&sender, Arc::clone(clients)),
        ));

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
    ///
    /// # Returns
    ///
    /// Returns a list of [JoinHandle]s, one handle for each thread.
    fn launch_named_pipe_servers(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
        clients: Arc<Mutex<Clients>>,
    ) -> Vec<JoinHandle<()>> {
        let mut servers: Vec<JoinHandle<()>> = Vec::new();
        for _ in &self.hosts {
            self.launch_named_pipe_server(&mut servers, sender, Arc::clone(&clients));
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
        clients: Arc<Mutex<Clients>>,
    ) {
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(PIPE_NAME)
            .unwrap_or_else(|err| {
                error!("{}", err);
                panic!("Failed to create named pipe server",)
            });
        let mut receiver = sender.subscribe();
        servers.push(tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
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
        clients: &mut Arc<Mutex<Clients>>,
        workspace_area: &workspace::WorkspaceArea,
        servers: &mut Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) {
        if self.control_mode_is_active(windows_api, input_record) {
            if self.control_mode_state == ControlModeState::Initiated {
                clear_screen(windows_api);
                println!("Control Mode (Esc to exit)");
                println!(
                    "[c]reate window(s), [r]etile, [t]oggle enabled, e[n]able all, copy active [h]ostname(s)"
                );
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
                    self.rearrange_client_windows(
                        windows_api,
                        &clients.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(windows_api, workspace_area);
                }
                (VK_E, 0) => {
                    // TODO: Select windows
                }
                (VK_T, 0) => {
                    // Snapshot each client's current state before flipping so
                    // every client toggles independently and the loop does not
                    // observe its own writes.
                    self.update_client_states(clients, |clients_guard| {
                        return clients_guard
                            .iter()
                            .map(|client| {
                                let flipped = match *client.state_tx.borrow() {
                                    ClientState::Active => ClientState::Disabled,
                                    ClientState::Disabled => ClientState::Active,
                                };
                                return (client.process_id, flipped);
                            })
                            .collect();
                    });
                    self.quit_control_mode(windows_api);
                }
                (VK_N, 0) => {
                    self.update_client_states(clients, |clients_guard| {
                        return clients_guard
                            .iter()
                            .map(|client| return (client.process_id, ClientState::Active))
                            .collect();
                    });
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
                                clients.lock().unwrap().push(client);
                                self.launch_named_pipe_server(
                                    &mut servers.lock().unwrap(),
                                    sender,
                                    Arc::clone(clients),
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

    /// Apply a batch of [`ClientState`] updates while holding the
    /// [`Clients`] mutex exactly once.
    ///
    /// Locks `clients`, invokes `f` with the guard so the caller can build
    /// the list of `(pid, new_state)` pairs while observing a stable
    /// snapshot, applies each update via [`Daemon::set_client_state`], and
    /// releases the guard. Centralises the lock-once / snapshot / apply /
    /// release pattern shared by the `[t]oggle enabled` and `e[n]able all`
    /// control-mode handlers.
    ///
    /// # Arguments
    ///
    /// * `clients` - Shared client collection.
    /// * `f`       - Closure that receives a `&Clients` guard and returns
    ///               the `(pid, new_state)` updates to broadcast.
    fn update_client_states<F>(&self, clients: &Mutex<Clients>, f: F)
    where
        F: FnOnce(&Clients) -> Vec<(u32, ClientState)>,
    {
        let clients_guard = clients.lock().unwrap();
        let updates = f(&clients_guard);
        for (pid, state) in updates {
            self.set_client_state(&clients_guard, pid, state);
        }
    }

    /// Push a new [`ClientState`] to the client identified by `pid`.
    ///
    /// Looks the client up by PID and broadcasts the new state through its
    /// [`watch::Sender`]. The pipe-server task subscribed to that sender
    /// observes the change and forwards a [`crate::protocol::TAG_STATE_CHANGE`]
    /// frame to the client over the named pipe. Called from the
    /// control-mode handlers for `[t]oggle enabled` and `e[n]able all` via
    /// [`Daemon::update_client_states`].
    ///
    /// # Arguments
    ///
    /// * `clients` - The daemon's tracked clients.
    /// * `pid`     - Process id of the client whose state should change.
    /// * `state`   - The new state to broadcast.
    fn set_client_state(&self, clients: &Clients, pid: u32, state: ClientState) {
        if let Some(client) = clients.get_by_pid(pid) {
            // `send_replace` always updates the stored value (unlike `send`,
            // which returns `Err` and leaves the value untouched when there
            // are no active receivers). This matters during the brief window
            // between [`Client`] construction and the pipe-server task's
            // `subscribe()`: any state change pushed in that window must
            // still be visible to the next subscriber via `borrow`.
            client.state_tx.send_replace(state);
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
/// A [`Clients`] collection preserving the launch order and indexed by
/// process id for pipe-server correlation.
async fn launch_clients<W: WindowsApi + 'static + Clone>(
    windows_api: &W,
    hosts: Vec<String>,
    username: &Option<String>,
    port: Option<u16>,
    debug: bool,
    workspace_area: &workspace::WorkspaceArea,
    aspect_ratio_adjustment: f64,
    index_offset: usize,
) -> Clients {
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
            let (window_handle, process_handle, process_id) = launch_client_console(
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
            // The receiver is dropped immediately; pipe-server tasks acquire
            // their own receivers via `state_tx.subscribe()` after PID
            // correlation. Holding the sender on the [`Client`] keeps the
            // channel alive for the lifetime of the client.
            let (state_tx, _state_rx) = watch::channel(ClientState::Active);
            return (
                index,
                Client {
                    hostname: host,
                    window_handle,
                    process_handle,
                    process_id,
                    state_tx,
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

    let mut clients = Clients::new();
    for (_, client) in results.into_iter() {
        clients.push(client);
    }
    return clients;
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
/// A tuple containing the window handle, process handle, and process id of the
/// client process.
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
) -> (HWND, HANDLE, u32) {
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
    return (
        client_window_handle,
        process_handle,
        process_info.dwProcessId,
    );
}

/// Wait for the named pipe server to connect, correlate the client by
/// its process id, then multiplex broadcast input records, [`ClientState`]
/// updates, and idle keep-alives onto the named pipe.
///
/// Correlation: after [`NamedPipeServer::connect`] resolves, the client is
/// expected to write its 4 byte little-endian process id into the pipe. The
/// routine looks up the [`Client`] with that PID in the daemon's `clients`
/// collection; if it is not found, the routine logs an error and terminates
/// the daemon - an unknown PID indicates broken daemon bookkeeping and is
/// unrecoverable.
///
/// Multiplexing: a biased [`tokio::select`] polls three branches per
/// iteration in order - (a) the broadcast `receiver` for input records,
/// (b) `state_rx.changed` for [`ClientState`] updates pushed by the
/// daemon, (c) a 5 ms timer that emits a keep-alive frame so dead pipes
/// are detected even when the daemon has nothing to send. The biased
/// ordering ensures the keep-alive branch only fires when neither input
/// nor state-change is ready, so it cannot interrupt active traffic.
/// Input records are gated on the current [`ClientState`] read via
/// `*state_rx.borrow()`: [`ClientState::Active`] forwards the record,
/// [`ClientState::Disabled`] drops it and yields so the daemon does not
/// tight-loop while the client is suppressed.
///
/// If any write to the pipe fails the pipe is considered closed and the
/// routine ends. If the [`watch::Sender`] is dropped (i.e. the daemon
/// removed the client from its bookkeeping) the routine likewise ends.
///
/// # Arguments
///
/// * `server`   - The named pipe server over which we send data to the
///                client.
/// * `receiver` - The receiving end of the broadcast channel through
///                which we get the serialized input records from the main
///                thread that are to be sent to the client via the named
///                pipe.
/// * `clients`  - The daemon's collection of tracked clients, used to
///                correlate the connecting client by PID and to obtain
///                the shared [`watch::Sender`] reference for this server.
///
/// # Panics
///
/// Panics if the connecting client sends a PID that is not present in
/// `clients`.
async fn named_pipe_server_routine(
    server: NamedPipeServer,
    receiver: &mut Receiver<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    clients: Arc<Mutex<Clients>>,
) {
    // wait for a client to connect
    server.connect().await.unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Timed out waiting for clients to connect to named pipe server",)
    });

    // Correlate the connecting client by reading its 4 byte PID.
    let pid = read_client_pid(&server).await;
    let mut state_rx = match clients.lock().unwrap().get_by_pid(pid) {
        Some(client) => client.state_tx.subscribe(),
        None => {
            error!(
                "Named pipe server received unknown PID {} - daemon bookkeeping broken",
                pid
            );
            // In production this exits the daemon; in tests process::exit would kill
            // the test runner, so we panic instead so tokio::spawn can catch it.
            #[cfg(not(test))]
            std::process::exit(1);
            #[cfg(test)]
            panic!("Unknown client PID {} - daemon bookkeeping broken", pid);
        }
    };

    loop {
        tokio::select! {
            biased;
            recv_result = receiver.recv() => {
                let ser_input_record = match recv_result {
                    Ok(val) => val,
                    Err(RecvError::Lagged(skipped)) => {
                        // A slow consumer (typically a disabled client throttling
                        // its read loop) can fall behind the bounded broadcast
                        // buffer. Drop the skipped records and continue rather
                        // than killing the routine - the missed keystrokes are
                        // unrecoverable, but the pipe is still useful. Logged at
                        // `debug!` because lagged drops can fire repeatedly and
                        // are not actionable per occurrence.
                        debug!(
                            "Named pipe server routine lagged behind broadcast channel - dropping {} record(s)",
                            skipped
                        );
                        continue;
                    }
                    Err(RecvError::Closed) => {
                        error!("Broadcast channel closed");
                        panic!("Failed to receive data from the Receiver");
                    }
                };
                // Gate forwarding on the current state. The match is exhaustive
                // so the compiler will flag this site when new variants are
                // added. Copy the value out before any `.await` so the
                // `watch::Ref` (not `Send`) does not span the await.
                let current_state = *state_rx.borrow();
                match current_state {
                    ClientState::Active => {}
                    ClientState::Disabled => {
                        // Yield so we don't tight-loop when the broadcast
                        // channel is busy. The keep-alive branch will detect
                        // pipe death even while the client is suppressed.
                        tokio::task::yield_now().await;
                        continue;
                    }
                }
                // Build the tagged input-record frame: [TAG_INPUT_RECORD][13-byte payload].
                let mut frame = [0u8; FRAMED_INPUT_RECORD_LENGTH];
                frame[0] = TAG_INPUT_RECORD;
                frame[1..].copy_from_slice(&ser_input_record);
                if !write_framed_message(&server, &frame).await {
                    return;
                }
            }
            changed_result = state_rx.changed() => {
                // Sender dropped - the daemon has removed this client from its
                // bookkeeping, so there is nothing left to forward.
                if changed_result.is_err() {
                    debug!(
                        "Client state sender dropped, stopping named pipe server routine ({:?})",
                        server
                    );
                    return;
                }
                let state = *state_rx.borrow_and_update();
                let frame: [u8; FRAMED_STATE_CHANGE_LENGTH] =
                    [TAG_STATE_CHANGE, serialize_client_state(state)];
                if !write_framed_message(&server, &frame).await {
                    return;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(5)) => {
                if !write_framed_message(&server, &[TAG_KEEP_ALIVE]).await {
                    return;
                }
            }
        }
    }
}

/// Write all of `frame` to the named pipe server, retrying partial writes
/// and `WouldBlock` results until the buffer is fully drained.
///
/// Shared by every write path inside [`named_pipe_server_routine`] so the
/// partial-write retry loop and pipe-closed detection live in one place.
///
/// # Arguments
///
/// * `server` - The connected named pipe server to write to.
/// * `frame`  - The complete framed wire bytes to push to the client.
///
/// # Returns
///
/// `true` once the entire frame has been written. `false` if the pipe is
/// closed (typically because the client process exited); the caller treats
/// this as a signal to terminate the routine.
///
/// # Panics
///
/// Panics if waiting for the pipe to become writable returns an error, as
/// the daemon cannot recover from a broken pipe handle.
async fn write_framed_message(server: &NamedPipeServer, frame: &[u8]) -> bool {
    let mut written = 0usize;
    while written < frame.len() {
        server.writable().await.unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Timed out waiting for named pipe server to become writable",)
        });
        match server.try_write(&frame[written..]) {
            Ok(0) => {
                // A zero-byte successful write means the pipe is closed
                // (typically because the client exited).
                debug!(
                    "Named pipe server ({:?}) is closed, stopping named pipe server routine",
                    server
                );
                return false;
            }
            Ok(n) => {
                written += n;
                if written < frame.len() {
                    // The data was only written partially, retry the
                    // remaining suffix on the next iteration.
                    warn!(
                        "Partially written data, expected {} but only wrote {} so far",
                        frame.len(),
                        written
                    );
                }
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
                return false;
            }
        }
    }
    debug!("Successfully written all data");
    return true;
}

/// Read the connecting client's 4 byte little-endian process id from the pipe.
///
/// Reads exactly 4 bytes from `server`, retrying on `WouldBlock`, and decodes
/// them as a `u32`. Any non-recoverable I/O error panics, as a client that
/// cannot send its PID cannot be correlated and forwarding would be
/// impossible.
///
/// # Arguments
///
/// * `server` - The connected named pipe server to read from.
///
/// # Returns
///
/// The process id sent by the client.
///
/// # Panics
///
/// Panics if the pipe is closed before 4 bytes can be read, or if any
/// non-`WouldBlock` I/O error occurs.
async fn read_client_pid(server: &NamedPipeServer) -> u32 {
    let mut buf = [0u8; SERIALIZED_PID_LENGTH];
    let mut read = 0usize;
    while read < SERIALIZED_PID_LENGTH {
        server.readable().await.unwrap_or_else(|err| {
            panic!("Named pipe server is not readable for PID handshake: {err}")
        });
        match server.try_read(&mut buf[read..]) {
            Ok(0) => {
                panic!("Named pipe server closed before PID handshake completed");
            }
            Ok(n) => {
                read += n;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                panic!("Failed to read PID from named pipe client: {e}");
            }
        }
    }
    return deserialize_pid(&buf);
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
    clients: Arc<Mutex<Clients>>,
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
        process_id: 0,
        state_tx: watch::channel(ClientState::Active).0,
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
        debug,
    };
    daemon.launch(windows_api).await;
    debug!("Actually exiting");
}

#[cfg(test)]
#[path = "../tests/daemon/test_mod.rs"]
mod test_mod;
