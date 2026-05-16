//! Daemon implementation

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
    serialization::{serialize_client_state, serialize_highlight, serialize_input_record_0},
    ClientState, FRAMED_HIGHLIGHT_LENGTH, FRAMED_INPUT_RECORD_LENGTH, FRAMED_STATE_CHANGE_LENGTH,
    SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH, TAG_HIGHLIGHT, TAG_INPUT_RECORD,
    TAG_KEEP_ALIVE, TAG_STATE_CHANGE,
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
    CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD_0, KEY_EVENT_RECORD, LEFT_ALT_PRESSED,
    LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED, RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_C, VK_D, VK_DOWN, VK_E, VK_ESCAPE, VK_H, VK_J, VK_K, VK_L, VK_LEFT, VK_N,
    VK_R, VK_RIGHT, VK_T, VK_UP,
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

/// Bits in `KEY_EVENT_RECORD::dwControlKeyState` that represent
/// "real" modifier keys (Ctrl / Alt / Shift) as opposed to lock
/// toggles (`CAPSLOCK_ON`, `NUMLOCK_ON`, `SCROLLLOCK_ON`) or the
/// `ENHANCED_KEY` flag.
///
/// Control-mode key classification ANDs `dwControlKeyState` with
/// this mask before matching; otherwise an enabled CapsLock or
/// NumLock would make `dwControlKeyState` non-zero and silently
/// skip every `(VK_*, 0)` arm.
const MODIFIER_MASK: u32 =
    LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED | LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED | SHIFT_PRESSED;

/// Top-level control-mode action a keystroke classifies into.
///
/// Extracted from [`Daemon::handle_input_record`]'s dispatch match
/// so the classification - including the [`MODIFIER_MASK`] step -
/// can be regression tested without instantiating a full
/// [`Daemon`].
#[derive(Debug, PartialEq, Eq)]
enum ControlModeAction {
    /// `[r]` - rearrange every client window.
    Retile,
    /// `[e]` - open the enable/disable input submenu.
    OpenEnableDisableSubmenu,
    /// `[t]` - flip each client's [`ClientState`].
    ToggleEnabled,
    /// `[n]` - force every client back to [`ClientState::Active`].
    EnableAll,
    /// `[c]` - prompt for new hostnames and launch additional clients.
    CreateWindows,
    /// `[h]` - copy the active clients' hostnames to the clipboard.
    CopyHostnames,
    /// Any other key in the active control-mode prompt.
    NoOp,
}

/// Enable/disable-submenu action a keystroke classifies into.
///
/// Extracted from [`Daemon::handle_enable_disable_submenu_key`]'s
/// dispatch match for the same reason as [`ControlModeAction`].
#[derive(Debug, PartialEq, Eq)]
enum EnableDisableSubmenuAction {
    /// `[e]` - force the targeted client(s) to [`ClientState::Active`].
    Enable,
    /// `[d]` - force the targeted client(s) to [`ClientState::Disabled`].
    Disable,
    /// `[t]` - flip the targeted client(s)' [`ClientState`].
    Toggle,
    /// Arrow key or vim motion - move the submenu's selection cursor.
    Navigate(NavigationDirection),
    /// Any other key while the submenu is open.
    NoOp,
}

/// Direction of a navigation keystroke inside the enable/disable
/// submenu.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum NavigationDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Classifies a top-level control-mode keystroke.
///
/// `control_key_state` is ANDed with [`MODIFIER_MASK`] so lock
/// toggles (`CAPSLOCK_ON`, `NUMLOCK_ON`, `SCROLLLOCK_ON`) and the
/// `ENHANCED_KEY` flag never bleed into the match - the
/// `(VK_*, 0)` arms must still fire while any of those bits are
/// set. Any "real" modifier bit (Ctrl / Alt / Shift) survives the
/// mask and falls through to [`ControlModeAction::NoOp`].
///
/// # Arguments
///
/// * `virtual_key`       - The pressed key's [`VIRTUAL_KEY`].
/// * `control_key_state` - The raw `dwControlKeyState` field from
///                         the [`KEY_EVENT_RECORD`].
///
/// # Returns
///
/// The [`ControlModeAction`] the dispatch should execute.
fn classify_control_mode_key(
    virtual_key: VIRTUAL_KEY,
    control_key_state: u32,
) -> ControlModeAction {
    return match (virtual_key, control_key_state & MODIFIER_MASK) {
        (VK_R, 0) => ControlModeAction::Retile,
        (VK_E, 0) => ControlModeAction::OpenEnableDisableSubmenu,
        (VK_T, 0) => ControlModeAction::ToggleEnabled,
        (VK_N, 0) => ControlModeAction::EnableAll,
        (VK_C, 0) => ControlModeAction::CreateWindows,
        (VK_H, 0) => ControlModeAction::CopyHostnames,
        _ => ControlModeAction::NoOp,
    };
}

/// Classifies an enable/disable-submenu keystroke.
///
/// See [`classify_control_mode_key`] for the [`MODIFIER_MASK`]
/// rationale; the same lock-state / `ENHANCED_KEY` masking applies
/// to the submenu so its `[e]`, `[d]`, `[t]` bindings keep working
/// regardless of lock state.
///
/// # Arguments
///
/// * `virtual_key`       - The pressed key's [`VIRTUAL_KEY`].
/// * `control_key_state` - The raw `dwControlKeyState` field from
///                         the [`KEY_EVENT_RECORD`].
///
/// # Returns
///
/// The [`EnableDisableSubmenuAction`] the dispatch should execute.
fn classify_enable_disable_submenu_key(
    virtual_key: VIRTUAL_KEY,
    control_key_state: u32,
) -> EnableDisableSubmenuAction {
    return match (virtual_key, control_key_state & MODIFIER_MASK) {
        (VK_E, 0) => EnableDisableSubmenuAction::Enable,
        (VK_D, 0) => EnableDisableSubmenuAction::Disable,
        (VK_T, 0) => EnableDisableSubmenuAction::Toggle,
        (VK_UP, 0) | (VK_K, 0) => EnableDisableSubmenuAction::Navigate(NavigationDirection::Up),
        (VK_DOWN, 0) | (VK_J, 0) => EnableDisableSubmenuAction::Navigate(NavigationDirection::Down),
        (VK_LEFT, 0) | (VK_H, 0) => EnableDisableSubmenuAction::Navigate(NavigationDirection::Left),
        (VK_RIGHT, 0) | (VK_L, 0) => {
            EnableDisableSubmenuAction::Navigate(NavigationDirection::Right)
        }
        _ => EnableDisableSubmenuAction::NoOp,
    };
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
    /// Authoritative source for this client's highlight flag. Set to
    /// `true` while the client is the daemon's currently selected
    /// submenu client. Visual only; input gating uses
    /// [`Client::state_tx`]. The pipe-server task subscribes alongside
    /// `state_tx` and forwards every change as a
    /// [`crate::protocol::TAG_HIGHLIGHT`] frame.
    highlight_tx: watch::Sender<bool>,
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
    /// The user opened the `[e]nable/disable input` submenu from
    /// [`ControlModeState::Active`]. Each non-`Esc` key is
    /// interpreted as a submenu action (`[e]`, `[d]`, `[t]`, or a
    /// navigation key) applied to the currently selected client;
    /// unrecognised keys are ignored. The submenu remains open after
    /// every key press and is left only via `Esc`, which exits
    /// control mode entirely. Like [`ControlModeState::Active`],
    /// this state suppresses input forwarding to clients.
    EnableDisableSubmenu,
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
    /// Index into [`Clients::list`] of the client currently selected
    /// in the [`ControlModeState::EnableDisableSubmenu`] prompt.
    /// `None` outside the submenu and whenever the cluster is empty.
    submenu_selected_index: Option<usize>,
    /// PID of the client whose `highlight_tx` is currently `true`,
    /// or `None` if no client is highlighted. Tracked separately
    /// from [`Daemon::submenu_selected_index`] so the clear half of
    /// [`Daemon::apply_submenu_highlight`] survives a `retain` shift
    /// of [`Clients::list`].
    submenu_highlighted_pid: Option<u32>,
    /// If debug mode is enabled on the daemon it will also be enabled on all
    /// clients.
    debug: bool,
}

impl<'a> Daemon<'a> {
    /// Builds a minimal [`Daemon`] suitable for unit tests.
    ///
    /// Populates every field with defaults that do not touch the
    /// Windows API or the network. Tests pick the
    /// [`ControlModeState`] they need to exercise; everything else
    /// stays inert.
    #[cfg(test)]
    fn for_test(
        config: &'a DaemonConfig,
        clusters: &'a [Cluster],
        control_mode_state: ControlModeState,
    ) -> Self {
        return Self {
            hosts: Vec::new(),
            username: None,
            port: None,
            config,
            clusters,
            control_mode_state,
            submenu_selected_index: None,
            submenu_highlighted_pid: None,
            debug: false,
        };
    }

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
        if self.control_mode_is_active(windows_api, clients, input_record) {
            if self.control_mode_state == ControlModeState::Initiated {
                clear_screen(windows_api);
                println!("Control Mode (Esc to exit)");
                println!(
                    "[c]reate window(s), [r]etile, [e]nable/disable input, [t]oggle enabled, e[n]able all, copy active [h]ostname(s)"
                );
                self.control_mode_state = ControlModeState::Active;
                return;
            }
            let key_event = unsafe { input_record.KeyEvent };
            if !key_event.bKeyDown.as_bool() {
                return;
            }
            if self.control_mode_state == ControlModeState::EnableDisableSubmenu {
                self.handle_enable_disable_submenu_key(windows_api, clients, key_event);
                return;
            }
            match classify_control_mode_key(
                VIRTUAL_KEY(key_event.wVirtualKeyCode),
                key_event.dwControlKeyState,
            ) {
                ControlModeAction::Retile => {
                    self.rearrange_client_windows(
                        windows_api,
                        &clients.lock().unwrap(),
                        workspace_area,
                    );
                    self.arrange_daemon_console(windows_api, workspace_area);
                }
                ControlModeAction::OpenEnableDisableSubmenu => {
                    let clients_guard = clients.lock().unwrap();
                    let next_selected = if clients_guard.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    self.apply_submenu_highlight(&clients_guard, next_selected);
                    self.submenu_selected_index = next_selected;
                    self.control_mode_state = ControlModeState::EnableDisableSubmenu;
                    self.render_enable_disable_submenu(windows_api);
                }
                ControlModeAction::ToggleEnabled => {
                    // Snapshot before flipping so each client toggles relative
                    // to its own pre-loop state, not to writes this loop has
                    // already made.
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
                ControlModeAction::EnableAll => {
                    self.update_client_states(clients, |clients_guard| {
                        return clients_guard
                            .iter()
                            .map(|client| return (client.process_id, ClientState::Active))
                            .collect();
                    });
                    self.quit_control_mode(windows_api);
                }
                ControlModeAction::CreateWindows => {
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
                                expand_hosts(
                                    hostnames.split(' ').map(|x| return x.trim()).collect(),
                                    self.clusters,
                                ),
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
                ControlModeAction::CopyHostnames => {
                    let mut active_hostnames: Vec<String> = vec![];
                    for client in clients.lock().unwrap().iter() {
                        if windows_api.is_window(client.window_handle) {
                            active_hostnames.push(client.hostname.clone());
                        }
                    }
                    cli_clipboard::set_contents(active_hostnames.join(" ")).unwrap();
                    self.quit_control_mode(windows_api);
                }
                ControlModeAction::NoOp => {}
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

    /// Updates `self.control_mode_state` for the given input record and
    /// reports whether control mode owned the keystroke.
    ///
    /// Entering control mode requires this function to be called twice
    /// because the activating chord `Ctrl + A` produces two input
    /// records (the modifier press and the `A` key). Once active, every
    /// subsequent key - including the `Esc` that exits control mode -
    /// is reported as consumed so callers do not forward it to clients.
    ///
    /// # Arguments
    ///
    /// * `windows_api`  - The Windows API implementation to use.
    /// * `clients`      - Currently tracked clients. Used to clear the
    ///                    submenu highlight on the previously-selected
    ///                    client when `Esc` exits the enable/disable
    ///                    submenu.
    /// * `input_record` - A KeyEvent input record.
    ///
    /// # Returns
    ///
    /// Whether the input record was consumed by control mode. Returns
    /// `true` while control mode is active (including the `Esc`
    /// keystroke that exits it), so callers must not forward such
    /// records to clients.
    fn control_mode_is_active<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        clients: &Mutex<Clients>,
        input_record: INPUT_RECORD_0,
    ) -> bool {
        let key_event = unsafe { input_record.KeyEvent };
        if self.control_mode_state == ControlModeState::Active
            || self.control_mode_state == ControlModeState::EnableDisableSubmenu
        {
            if key_event.wVirtualKeyCode == VK_ESCAPE.0 {
                if self.control_mode_state == ControlModeState::EnableDisableSubmenu {
                    let clients_guard = clients.lock().unwrap();
                    self.apply_submenu_highlight(&clients_guard, None);
                }
                self.quit_control_mode(windows_api);
                return true;
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

    /// Prints the default daemon instructions to the daemon console
    /// and resets the submenu selection/highlight tracking.
    ///
    /// # Arguments
    ///
    /// * `windows_api` - Windows API used to clear and redraw the
    ///                   daemon console.
    fn quit_control_mode<W: WindowsApi>(&mut self, windows_api: &W) {
        self.print_instructions(windows_api);
        self.control_mode_state = ControlModeState::Inactive;
        self.submenu_selected_index = None;
        self.submenu_highlighted_pid = None;
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

    /// Dispatches a key press received while the daemon is in the
    /// [`ControlModeState::EnableDisableSubmenu`] state. `[e]/[d]/[t]`
    /// act on the currently selected client; `Navigate` moves the
    /// selection and redraws the prompt. The submenu is left via
    /// `ESC`, which is handled by the caller.
    ///
    /// # Arguments
    ///
    /// * `windows_api` - Windows API implementation used by the
    ///                   render helper when redrawing after navigation.
    /// * `clients`     - Shared client collection. Empty lists are a
    ///                   no-op for every action.
    /// * `key_event`   - The key-down [`KEY_EVENT_RECORD`] dispatched
    ///                   from `handle_input_record`.
    fn handle_enable_disable_submenu_key<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        clients: &Mutex<Clients>,
        key_event: KEY_EVENT_RECORD,
    ) {
        match classify_enable_disable_submenu_key(
            VIRTUAL_KEY(key_event.wVirtualKeyCode),
            key_event.dwControlKeyState,
        ) {
            EnableDisableSubmenuAction::Enable => {
                let selected = self.submenu_selected_index;
                self.update_client_states(clients, |clients_guard| {
                    return selected
                        .and_then(|idx| return clients_guard.get(idx))
                        .map(|client| return vec![(client.process_id, ClientState::Active)])
                        .unwrap_or_default();
                });
            }
            EnableDisableSubmenuAction::Disable => {
                let selected = self.submenu_selected_index;
                self.update_client_states(clients, |clients_guard| {
                    return selected
                        .and_then(|idx| return clients_guard.get(idx))
                        .map(|client| return vec![(client.process_id, ClientState::Disabled)])
                        .unwrap_or_default();
                });
            }
            EnableDisableSubmenuAction::Toggle => {
                let selected = self.submenu_selected_index;
                self.update_client_states(clients, |clients_guard| {
                    return selected
                        .and_then(|idx| return clients_guard.get(idx))
                        .map(|client| {
                            let flipped = match *client.state_tx.borrow() {
                                ClientState::Active => ClientState::Disabled,
                                ClientState::Disabled => ClientState::Active,
                            };
                            return vec![(client.process_id, flipped)];
                        })
                        .unwrap_or_default();
                });
            }
            EnableDisableSubmenuAction::Navigate(direction) => {
                let clients_guard = clients.lock().unwrap();
                self.move_submenu_selection(direction, clients_guard.len());
                self.apply_submenu_highlight(&clients_guard, self.submenu_selected_index);
                self.render_enable_disable_submenu(windows_api);
            }
            EnableDisableSubmenuAction::NoOp => {}
        }
    }

    /// Redraws the enable/disable submenu prompt. The currently
    /// selected window is identified by its highlight color, so the
    /// daemon console only needs to show the header and keymap.
    ///
    /// # Arguments
    ///
    /// * `windows_api` - Windows API used to clear the console.
    fn render_enable_disable_submenu<W: WindowsApi>(&self, windows_api: &W) {
        clear_screen(windows_api);
        println!("Enable/Disable input (Esc to exit)");
        println!("[e]nable, [d]isable, [t]oggle, arrows/hjkl to move");
    }

    /// Moves [`Daemon::submenu_selected_index`] one step back
    /// (Up/Left) or forward (Down/Right), clamped to `[0, len - 1]`.
    /// Clears the selection when `len` is `0`.
    ///
    /// # Arguments
    ///
    /// * `direction` - Direction the navigation keystroke encoded.
    /// * `len`       - Current number of tracked clients.
    fn move_submenu_selection(&mut self, direction: NavigationDirection, len: usize) {
        if len == 0 {
            self.submenu_selected_index = None;
            return;
        }
        let Some(current) = self.submenu_selected_index else {
            return;
        };
        // Clamp a stale index back into range so an Up/Left after the
        // background monitor retain-ed exited clients still lands on a
        // valid survivor instead of stepping from a phantom slot.
        let current = current.min(len - 1);
        let next = match direction {
            NavigationDirection::Up | NavigationDirection::Left => current.saturating_sub(1),
            NavigationDirection::Down | NavigationDirection::Right => (current + 1).min(len - 1),
        };
        self.submenu_selected_index = Some(next);
    }

    /// Move the per-client highlight to the client at `next`.
    ///
    /// Clears the highlight on whichever client is currently tracked
    /// via [`Daemon::submenu_highlighted_pid`] and sets it on the
    /// client at `next` (if any). PID-based clearing tolerates the
    /// background monitor's `retain` shifting indices while the
    /// submenu is open.
    ///
    /// # Arguments
    ///
    /// * `clients` - Currently tracked clients, indexed in their
    ///               submenu order.
    /// * `next`    - Index of the client to highlight now, or `None`
    ///               to clear the highlight entirely.
    fn apply_submenu_highlight(&mut self, clients: &Clients, next: Option<usize>) {
        let next_client = next.and_then(|idx| return clients.get(idx));
        let next_pid = next_client.map(|c| return c.process_id);
        if let Some(prev_pid) = self.submenu_highlighted_pid {
            if Some(prev_pid) != next_pid {
                if let Some(prev_client) = clients.get_by_pid(prev_pid) {
                    prev_client.highlight_tx.send_replace(false);
                }
            }
        }
        if let Some(client) = next_client {
            client.highlight_tx.send_replace(true);
        }
        self.submenu_highlighted_pid = next_pid;
    }

    /// Apply a batch of [`ClientState`] updates while holding the
    /// [`Clients`] mutex exactly once.
    ///
    /// `f` is called with the locked guard and returns the list of
    /// `(pid, new_state)` updates to apply. The guard is held across both
    /// the build and the apply phase so callers see a stable snapshot.
    ///
    /// # Arguments
    ///
    /// * `clients` - Shared client collection.
    /// * `f`       - Builds the updates from a `&Clients` snapshot.
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

    /// Push a new [`ClientState`] for the client identified by `pid`.
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

/// Resolve cluster tags in `hosts` and expand brace expressions
/// (e.g. `host{1..3}.local`) in each resulting hostname.
///
/// Used by the control-mode `[c]reate window(s)` path so hostname
/// input behaves the same as on the CLI. Each cluster-resolved
/// hostname is passed through [`bracoxide::explode`] individually;
/// hostnames that do not contain a brace expression are kept as-is.
///
/// # Arguments
///
/// * `hosts`    - User-supplied hostnames and/or cluster tags.
/// * `clusters` - Available cluster definitions.
///
/// # Returns
///
/// The fully resolved, brace-expanded list of hostnames.
pub fn expand_hosts(hosts: Vec<&str>, clusters: &[Cluster]) -> Vec<String> {
    return resolve_cluster_tags(hosts, clusters)
        .into_iter()
        .flat_map(|host| return explode(host).unwrap_or_else(|_| return vec![host.to_owned()]))
        .collect();
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
            // The receivers are dropped immediately; pipe-server tasks
            // acquire their own receivers via `subscribe()` after PID
            // correlation. Holding the senders on the [`Client`] keeps both
            // channels alive for the lifetime of the client.
            let (state_tx, _state_rx) = watch::channel(ClientState::Active);
            let (highlight_tx, _highlight_rx) = watch::channel(false);
            return (
                index,
                Client {
                    hostname: host,
                    window_handle,
                    process_handle,
                    process_id,
                    state_tx,
                    highlight_tx,
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

/// Correlate the connecting client by PID, then multiplex input records,
/// [`ClientState`] updates, and keep-alives onto the named pipe.
///
/// The post-subscribe initial-state push is intentional: `state_rx.changed`
/// only fires on transitions observed *after* `subscribe`, so a state set
/// in the brief window between [`Client`] construction and `subscribe`
/// would otherwise leave the client on its default until the next
/// transition.
///
/// The `select!` is biased toward `recv` so the keep-alive tick never
/// preempts active input traffic; the [`ClientState::Disabled`] arm
/// therefore probes the pipe itself, otherwise sustained input would
/// hide a disconnect.
///
/// # Errors and termination
///
/// An unknown PID exits the process (production) or panics (tests) -
/// the daemon's bookkeeping is broken and recovery is not possible.
/// A failed pipe write or a dropped [`watch::Sender`] ends the routine
/// cleanly.
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
    let (mut state_rx, mut highlight_rx) = match clients.lock().unwrap().get_by_pid(pid) {
        Some(client) => (client.state_tx.subscribe(), client.highlight_tx.subscribe()),
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

    // Initial state push - see fn docs.
    let initial_state = *state_rx.borrow_and_update();
    let initial_state_frame: [u8; FRAMED_STATE_CHANGE_LENGTH] =
        [TAG_STATE_CHANGE, serialize_client_state(initial_state)];
    if !write_framed_message(&server, &initial_state_frame).await {
        return;
    }

    // Initial highlight push - same rationale as the state push above.
    let initial_highlight = *highlight_rx.borrow_and_update();
    let initial_highlight_frame: [u8; FRAMED_HIGHLIGHT_LENGTH] =
        [TAG_HIGHLIGHT, serialize_highlight(initial_highlight)];
    if !write_framed_message(&server, &initial_highlight_frame).await {
        return;
    }

    loop {
        tokio::select! {
            biased;
            recv_result = receiver.recv() => {
                let ser_input_record = match recv_result {
                    Ok(val) => val,
                    Err(RecvError::Lagged(skipped)) => {
                        // Slow consumers (typically disabled clients) drop
                        // records rather than kill the routine; debug-level
                        // because this can fire repeatedly under load.
                        debug!(
                            "Named pipe server routine lagged behind broadcast channel - dropping {} record(s)",
                            skipped
                        );
                        // Probe and yield so sustained lag cannot starve
                        // the keep-alive tick (the `select!` is `biased`
                        // toward `recv`) and so a closed pipe is still
                        // detected promptly under load.
                        if !probe_pipe_alive(&server) {
                            return;
                        }
                        tokio::task::yield_now().await;
                        continue;
                    }
                    Err(RecvError::Closed) => {
                        error!("Broadcast channel closed");
                        panic!("Failed to receive data from the Receiver");
                    }
                };
                // Copy out before any `.await` - `watch::Ref` is not `Send`.
                let current_state = *state_rx.borrow();
                match current_state {
                    ClientState::Active => {}
                    ClientState::Disabled => {
                        // Probe the pipe so a disabled client cannot hide a
                        // disconnect under sustained input - the keep-alive
                        // tick is starved while recv keeps yielding records.
                        if !probe_pipe_alive(&server) {
                            return;
                        }
                        tokio::task::yield_now().await;
                        continue;
                    }
                }
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
            changed_result = highlight_rx.changed() => {
                // Sender dropped - same rationale as the `state_rx` arm.
                if changed_result.is_err() {
                    debug!(
                        "Client highlight sender dropped, stopping named pipe server routine ({:?})",
                        server
                    );
                    return;
                }
                let highlighted = *highlight_rx.borrow_and_update();
                let frame: [u8; FRAMED_HIGHLIGHT_LENGTH] =
                    [TAG_HIGHLIGHT, serialize_highlight(highlighted)];
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

/// Best-effort, non-blocking probe of the named pipe.
///
/// Returns `true` if a single `TAG_KEEP_ALIVE` byte either wrote
/// successfully or returned `WouldBlock` (the pipe is still open but
/// the OS buffer is full); `false` if any other error indicates the
/// pipe is closed.
fn probe_pipe_alive(server: &NamedPipeServer) -> bool {
    match server.try_write(&[TAG_KEEP_ALIVE]) {
        Ok(_) => return true,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return true,
        Err(_) => {
            debug!(
                "Named pipe server ({:?}) is closed, stopping named pipe server routine",
                server
            );
            return false;
        }
    }
}

/// Write all of `frame` to the named pipe server, retrying partial
/// writes and `WouldBlock` results until the buffer is fully drained.
///
/// Returns `true` on full write, `false` if the pipe is closed.
///
/// # Panics
///
/// Panics if waiting for the pipe to become writable returns an error.
async fn write_framed_message(server: &NamedPipeServer, frame: &[u8]) -> bool {
    let mut written = 0usize;
    while written < frame.len() {
        server.writable().await.unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Timed out waiting for named pipe server to become writable",)
        });
        match server.try_write(&frame[written..]) {
            Ok(n) => {
                written += n;
                if written < frame.len() {
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
        highlight_tx: watch::channel(false).0,
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
        submenu_selected_index: None,
        submenu_highlighted_pid: None,
        debug,
    };
    daemon.launch(windows_api).await;
    debug!("Actually exiting");
}

#[cfg(test)]
#[path = "../tests/daemon/test_mod.rs"]
mod test_mod;
