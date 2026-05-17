//! Client implementation

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use log::{error, info, warn};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;

use crate::utils::config::ClientConfig;
use crate::utils::windows::{get_console_title, set_console_color, WindowsApi};
use ssh2_config::{ParseRule, SshConfig};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use windows::Win32::System::Console::{
    CONSOLE_CHARACTER_ATTRIBUTES, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD,
    LEFT_ALT_PRESSED, RIGHT_ALT_PRESSED, SHIFT_PRESSED,
};

use crate::{
    protocol::{
        deserialization::parse_daemon_to_client_messages, serialization::serialize_pid,
        ClientState, DaemonToClientMessage, SERIALIZED_INPUT_RECORD_0_LENGTH,
        SERIALIZED_PID_LENGTH,
    },
    utils::constants::{PIPE_NAME, PKG_NAME},
};

/// Possible results when reading from the named pipe and writing to the
/// current process's stdinput.
enum ReadWriteResult {
    /// We wrote all complete [INPUT_RECORD_0] sequences we read from
    /// the named pipe to stdin.
    Success {
        /// Incomplete [INPUT_RECORD_0] sequence.
        ///
        /// What we read from the named pipe is a serialized [INPUT_RECORD_0].`KeyEvent`.
        /// As this is simply a [`SERIALIZED_INPUT_RECORD_0_LENGTH`] byte long sequence and we try to read from the pipe until we
        /// have some of the data it can happen that during any one read/write iteration we don't
        /// read the full sequence so we must keep track of what we read for next iterations
        /// where we will be able to read the remainder of the sequence.
        remainder: Vec<u8>,
        /// List of [KEY_EVENT_RECORD]s we have read from the named pipe.
        ///
        /// Used to detect the `Alt + Shift + C` key combination used
        /// to close the console window after the client process encountered an unexpected error.
        key_event_records: Vec<KEY_EVENT_RECORD>,
    },
    /// Trying to read from the pipe would require us to wait for data.
    WouldBlock,
    /// Something went wrong.
    Err,
    /// The pipe was closed.
    Disconnect,
}

/// Duration of the action-feedback flash painted on a highlighted client
/// when the user toggles the state.
const HIGHLIGHT_FLASH_DURATION: Duration = Duration::from_millis(250);

/// Resolve the console color to paint for a given
/// `(state, highlighted)` combination. Highlight wins over the
/// disabled color. Returns `None` for every combination when
/// `original_console_color` is `None`, so a failed startup capture
/// degrades to a no-op rather than stranding the window in the
/// disabled/highlight color with no restore path.
///
/// # Arguments
///
/// * `state`                    - The client's current [`ClientState`].
/// * `highlighted`              - `true` while the client is the selected
///                                window in the daemon's enable/disable
///                                submenu.
/// * `original_console_color`   - Console color captured at startup.
/// * `disabled_console_color`   - Color applied while the client is
///                                [`ClientState::Disabled`].
/// * `highlighted_console_color`- Color applied while the client is
///                                highlighted.
///
/// # Returns
///
/// The color to paint, or `None` if no repaint is possible because the
/// original color was never captured.
fn get_effective_color(
    state: ClientState,
    highlighted: bool,
    original_console_color: Option<CONSOLE_CHARACTER_ATTRIBUTES>,
    disabled_console_color: CONSOLE_CHARACTER_ATTRIBUTES,
    highlighted_console_color: CONSOLE_CHARACTER_ATTRIBUTES,
) -> Option<CONSOLE_CHARACTER_ATTRIBUTES> {
    let original = original_console_color?;
    if highlighted {
        return Some(highlighted_console_color);
    }
    match state {
        ClientState::Active => return Some(original),
        ClientState::Disabled => return Some(disabled_console_color),
    }
}

/// Color painted during the action-feedback flash: the underlying
/// state color, with the highlight overlay bypassed so the user can
/// see what they just did. Degrades to `None` when
/// `original_console_color` is `None` for the same reason as
/// [`get_effective_color`].
///
/// # Arguments
///
/// * `state`                    - The just-applied [`ClientState`].
/// * `original_console_color`   - Console color captured at startup.
/// * `disabled_console_color`   - Color applied while the client is
///                                [`ClientState::Disabled`].
///
/// # Returns
///
/// The color to paint for the flash, or `None` if no repaint is
/// possible.
fn get_flash_color(
    state: ClientState,
    original_console_color: Option<CONSOLE_CHARACTER_ATTRIBUTES>,
    disabled_console_color: CONSOLE_CHARACTER_ATTRIBUTES,
) -> Option<CONSOLE_CHARACTER_ATTRIBUTES> {
    let original = original_console_color?;
    match state {
        ClientState::Active => return Some(original),
        ClientState::Disabled => return Some(disabled_console_color),
    }
}

/// Paint `target` if it differs from `last`, then update `last`.
/// Skipping unchanged repaints keeps the slow per-row
/// `fill_console_output_attribute` calls off the hot path; a `None`
/// target leaves the console untouched.
///
/// # Arguments
///
/// * `api`    - The Windows API implementation to use.
/// * `target` - The color to paint, or `None` to skip.
/// * `last`   - The most recently painted color. Updated in-place
///              after a successful repaint so the next call can
///              short-circuit on an unchanged target.
fn paint_console_color(
    api: &dyn WindowsApi,
    target: Option<CONSOLE_CHARACTER_ATTRIBUTES>,
    last: &mut Option<CONSOLE_CHARACTER_ATTRIBUTES>,
) {
    let Some(color) = target else {
        return;
    };
    if last.map(|c| return c.0) == Some(color.0) {
        return;
    }
    set_console_color(api, color);
    *last = Some(color);
}

/// Write the given [INPUT_RECORD_0] to the console input buffer using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `input_record` - The [INPUT_RECORD_0].`KeyEvent` input record to write.
fn write_console_input(api: &dyn WindowsApi, input_record: INPUT_RECORD_0) {
    let buffer: [INPUT_RECORD; 1] = [INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: input_record,
    }];
    let mut nb_of_events_written = 0u32;
    match api.write_console_input(&buffer, &mut nb_of_events_written) {
        Ok(_) => {
            if nb_of_events_written == 0 {
                error!("Failed to write console input");
                error!("{:?}", api.get_last_error());
            }
        }
        Err(_) => {
            error!("Failed to write console input");
            error!("{:?}", api.get_last_error());
        }
    };
}

/// Resolve the username from the provided value or SSH config.
///
/// # Arguments
///
/// * `username` - Optional username to use. If None, will try to resolve from SSH config.
/// * `host` - The hostname (without port) to connect to.
/// * `config` - The client configuration containing SSH config path.
///
/// # Returns
///
/// The resolved username.
fn resolve_username(username: Option<String>, host: &str, config: &ClientConfig) -> String {
    if let Some(val) = username {
        return val;
    }

    let mut ssh_config = SshConfig::default();
    let ssh_config_path = Path::new(config.ssh_config_path.as_str());
    if ssh_config_path.exists() {
        let mut reader = BufReader::new(
            File::open(ssh_config_path).expect("Could not open SSH configuration file."),
        );
        ssh_config = SshConfig::default()
            .parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)
            .expect("Failed to parse SSH configuration file");
    }
    return ssh_config
        .query(<&str>::clone(&host))
        .user
        .unwrap_or_default();
}

/// Build the SSH arguments from the username, host, port, and config.
///
/// # Arguments
///
/// * `username`    - The username to connect with.
/// * `host`        - The hostname to connect to.
/// * `port`        - Optional port number (0-65535).
/// * `config`      - The client config indicating how to call the SSH program.
///
/// # Returns
///
/// A vector of arguments ready to be passed to the SSH command.
fn build_ssh_arguments(
    username: &str,
    host: &str,
    port: Option<u16>,
    config: &ClientConfig,
) -> Vec<String> {
    let username_host = format!("{username}@{host}");

    let mut arguments = replace_argument_placeholders(
        &config.arguments,
        &config.username_host_placeholder,
        &username_host,
    );

    // Add port arguments if port was specified
    if let Some(port) = port {
        arguments.push("-p".to_string());
        arguments.push(port.to_string());
    }

    return arguments;
}

/// Launch the SSH process.
///
/// The process might overwrite the console title once it launched, so we wait for that
/// to happen and set the title again.
///
/// # Arguments
///
/// * `username`    - The username to connect with.
/// * `host`        - The hostname to connect to.
/// * `port`        - Optional port number (0-65535).
/// * `config`      - The client config indicating how to call the SSH program.
///
/// # Returns
///
/// The handle to created [Child] process.
async fn launch_ssh_process(
    username: &str,
    host: &str,
    port: Option<u16>,
    config: &ClientConfig,
) -> Child {
    let arguments = build_ssh_arguments(username, host, port, config);
    let child = Command::new(&config.program)
        .args(arguments.clone())
        .spawn()
        .unwrap_or_else(|err| {
            let args: String = arguments.join(" ");
            error!("{}", err);
            panic!(
                "Failed to launch process `{}` with arguments `{}`",
                config.program, args
            )
        });
    return child;
}

/// Read all available daemon-to-client messages from the named pipe and apply them.
///
/// Input records are written to the console input buffer using the provided API
/// and their key-event payloads are returned via `ReadWriteResult::Success` so
/// the caller can detect the Alt+Shift+C close combination. State-change frames
/// are forwarded via [`watch::Sender::send_replace`] on `state_tx`, making the
/// authoritative [`ClientState`] visible to every watch subscriber (currently
/// the visuals task in [`main`]) without coupling this loop to any
/// state-dependent rendering. Keep-alive frames are ignored. Partial trailing
/// frames are returned as `remainder` for the next call to prepend.
///
/// # Arguments
///
/// * `api`                 - The Windows API implementation to use.
/// * `named_pipe_client`   - The [Windows named pipe][1] client that has successfully connected to
///                           the named pipe created by the daemon.
/// * `internal_buffer`     - Vector containing the unconsumed bytes (possibly an
///                           incomplete trailing frame) from a previous call.
/// * `state_tx`            - Watch sender used to broadcast every
///                           [`DaemonToClientMessage::StateChange`] payload as
///                           the client's authoritative [`ClientState`].
/// * `highlight_tx`        - Watch sender used to broadcast every
///                           [`DaemonToClientMessage::Highlight`] payload as
///                           the client's current highlight flag.
/// # Returns
///
/// A `ReadWriteResult` indicating whether we were able to read from the named pipe and write the available INPUT_RECORDs
/// to the console input buffer or not.
///
/// [1]: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipes
async fn read_write_loop(
    api: &dyn WindowsApi,
    named_pipe_client: &NamedPipeClient,
    internal_buffer: &mut Vec<u8>,
    state_tx: &watch::Sender<ClientState>,
    highlight_tx: &watch::Sender<bool>,
) -> ReadWriteResult {
    let mut buf: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH * 10] =
        [0; SERIALIZED_INPUT_RECORD_0_LENGTH * 10];
    match named_pipe_client.try_read(&mut buf) {
        Ok(0) => {
            // Seems to only happen if the pipe is closed/server disconnects
            // indicating that the daemon has been closed.
            // Exit the client too in that case.
            return ReadWriteResult::Disconnect;
        }
        Ok(n) => {
            internal_buffer.extend_from_slice(&buf[..n]);
            let (messages, remainder) = parse_daemon_to_client_messages(internal_buffer);
            let mut key_event_records: Vec<KEY_EVENT_RECORD> = Vec::new();
            for message in messages {
                match message {
                    DaemonToClientMessage::InputRecord(input_record) => {
                        write_console_input(api, input_record);
                        key_event_records.push(unsafe { input_record.KeyEvent });
                    }
                    DaemonToClientMessage::StateChange(state) => {
                        state_tx.send_replace(state);
                    }
                    DaemonToClientMessage::Highlight(highlighted) => {
                        highlight_tx.send_replace(highlighted);
                    }
                    DaemonToClientMessage::KeepAlive => {}
                }
            }
            return ReadWriteResult::Success {
                remainder,
                key_event_records,
            };
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            return ReadWriteResult::WouldBlock;
        }
        Err(e) => {
            error!("{}", e);
            return ReadWriteResult::Err;
        }
    }
}

/// Checks if a key event represents the Alt+Shift+C combination.
///
/// # Arguments
///
/// * `key_event` - The key event record to check.
///
/// # Returns
///
/// `true` if the key event represents Alt+Shift+C, `false` otherwise.
fn is_alt_shift_c_combination(key_event: &KEY_EVENT_RECORD) -> bool {
    return (key_event.dwControlKeyState & LEFT_ALT_PRESSED >= 1
        || key_event.dwControlKeyState & RIGHT_ALT_PRESSED == 1)
        && key_event.dwControlKeyState & SHIFT_PRESSED >= 1
        && key_event.wVirtualKeyCode == VK_C.0;
}

/// Replaces placeholders in SSH command arguments.
///
/// # Arguments
///
/// * `arguments` - The argument templates.
/// * `placeholder` - The placeholder string to replace.
/// * `replacement` - The value to replace the placeholder with.
///
/// # Returns
///
/// A vector of arguments with placeholders replaced.
fn replace_argument_placeholders(
    arguments: &[String],
    placeholder: &str,
    replacement: &str,
) -> Vec<String> {
    return arguments
        .iter()
        .map(|arg| return arg.replace(placeholder, replacement))
        .collect();
}

/// Send this process's id over the pipe to the daemon as a 4 byte
/// little-endian sequence.
///
/// The daemon uses the PID to match the pipe connection to the correct
/// [`crate::daemon`] `Client` entry. Without this handshake the daemon will
/// not forward any input records.
///
/// # Arguments
///
/// * `named_pipe_client` - The connected pipe client to write the PID to.
///
/// # Panics
///
/// Panics if the pipe write fails in a way that cannot be retried.
async fn send_pid_handshake(named_pipe_client: &NamedPipeClient) {
    let pid_bytes = serialize_pid(std::process::id());
    let mut written = 0usize;
    while written < SERIALIZED_PID_LENGTH {
        named_pipe_client.writable().await.unwrap_or_else(|err| {
            panic!("Named pipe client is not writable for PID handshake: {err}")
        });
        match named_pipe_client.try_write(&pid_bytes[written..]) {
            Ok(0) => {
                panic!("Named pipe closed before PID handshake could complete");
            }
            Ok(n) => {
                written += n;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                panic!("Failed to send PID handshake to daemon: {e}");
            }
        }
    }
    return;
}

/// The main run loop of the client.
///
/// Connects to the named pipe opened by the daemon, reads all input records from it
/// and replays them to the console input buffer of the given child process.
/// Handles the `Alt + Shift + C` key combination used to close the console window
/// after the child process encountered an unexpected error.
///
/// # Arguments
///
/// * `api`         - The Windows API implementation to use.
/// * `child`       - Handle to the running SSH process.
/// * `state_tx`    - Watch sender used by [`read_write_loop`] to broadcast the
///                   client's authoritative [`ClientState`] to subscribers
///                   such as the visuals task in [`main`].
/// * `highlight_tx`- Watch sender used by [`read_write_loop`] to broadcast the
///                   client's current highlight flag.
async fn run(
    api: &dyn WindowsApi,
    child: &mut Child,
    state_tx: &watch::Sender<ClientState>,
    highlight_tx: &watch::Sender<bool>,
) {
    // Many clients trying to open the pipe at the same time can cause
    // a file not found error, so keep trying until we managed to open it
    let named_pipe_client: NamedPipeClient = loop {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(named_pipe_client) => {
                break named_pipe_client;
            }
            Err(_) => {
                continue;
            }
        }
    };
    // Identify ourselves to the daemon's pipe server by sending our PID.
    // The daemon uses this to correlate this pipe connection to the corresponding
    // client in its internal bookkeeping.
    send_pid_handshake(&named_pipe_client).await;
    let mut child_error = false;
    let mut internal_buffer: Vec<u8> = Vec::new();
    loop {
        named_pipe_client
            .ready(Interest::READABLE)
            .await
            .unwrap_or_else(|err| {
                error!("{}", err);
                panic!("Named client pipe is not ready to be read",)
            });

        match read_write_loop(
            api,
            &named_pipe_client,
            &mut internal_buffer,
            state_tx,
            highlight_tx,
        )
        .await
        {
            ReadWriteResult::Success {
                remainder,
                key_event_records,
            } => {
                internal_buffer = remainder;
                if child_error {
                    for key_event in key_event_records.into_iter() {
                        if is_alt_shift_c_combination(&key_event) {
                            return;
                        }
                    }
                }
            }
            ReadWriteResult::WouldBlock | ReadWriteResult::Err => {
                // Sleep some time to avoid hogging 100% CPU usage.
                tokio::time::sleep(Duration::from_nanos(5)).await;
            }
            ReadWriteResult::Disconnect => {
                warn!("Encountered disconnect when trying to read from named pipe");
                break;
            }
        }
        match child.try_wait() {
            Ok(Some(exit_status)) => match exit_status.code().unwrap() {
                0 | 1 | 130 => {
                    // 0 -> last command successful
                    // 1 -> last command unsuccessful
                    // 130 -> last command cancelled (Ctrl + C)
                    info!(
                        "Application terminated, last exit code: {}",
                        exit_status.code().unwrap()
                    );
                    break;
                }
                _ => {
                    if !child_error {
                        println!("Failed to establish SSH connection: {exit_status}");
                        println!("Shift-Alt-C to exit");
                        child_error = true;
                    }
                }
            },
            Ok(None) => (
                // child is still running
            ),
            Err(e) => panic!("{}", e),
        }
    }
}

/// Snapshot the current console color.
/// Returns `None` (with a warning) if the Windows API
/// rejects the query; in that case the visuals task degrades to a
/// no-op for every `(state, highlight)` combination.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Returns
///
/// `Some(original)` on success, `None` if the buffer info could
/// not be read.
fn capture_original_console_color(api: &dyn WindowsApi) -> Option<CONSOLE_CHARACTER_ATTRIBUTES> {
    match api.get_console_screen_buffer_info() {
        Ok(info) => return Some(info.wAttributes),
        Err(err) => {
            warn!(
                "Failed to capture original console color; state visuals will be skipped: {}",
                err
            );
            return None;
        }
    }
}

/// Splits `host` on its trailing `:port` suffix (if any) and parses
/// the port. An invalid `:port` is logged and treated as absent so
/// the CLI port can still apply.
///
/// # Arguments
///
/// * `host` - Raw host argument, optionally with `:port` suffix.
///
/// # Returns
///
/// `(host_without_port, inline_port)`.
fn split_host_and_inline_port(host: &str) -> (&str, Option<u16>) {
    let (bare_host, port_str) = host
        .rsplit_once(':')
        .map_or((host, None), |(h, p)| return (h, Some(p)));
    let inline_port = port_str.and_then(|p| {
        return p
            .parse::<u16>()
            .map_err(|e| {
                warn!("Invalid port '{}': {}. Using default SSH port.", p, e);
            })
            .ok();
    });
    return (bare_host, inline_port);
}

/// Builds the console window title shown to the user.
///
/// # Arguments
///
/// * `resolved_username` - Username after SSH config resolution.
/// * `host`              - Bare hostname.
/// * `port`              - Effective port (inline or CLI), if any.
///
/// # Returns
///
/// The console title string in `csshw - user@host[:port]` form.
fn build_console_title(resolved_username: &str, host: &str, port: Option<u16>) -> String {
    let title_host = if let Some(port) = port {
        format!("{host}:{port}")
    } else {
        host.to_string()
    };
    return format!("{PKG_NAME} - {resolved_username}@{title_host}");
}

/// Keeps the console window title pinned to `console_title`, since
/// the SSH child can overwrite it on connect.
///
/// # Arguments
///
/// * `api`           - The Windows API implementation to use.
/// * `console_title` - The title to (re)apply.
async fn run_title_loop(api: &dyn WindowsApi, console_title: String) {
    loop {
        if console_title != get_console_title(api) {
            api.set_console_title(console_title.as_str())
                .unwrap_or_else(|err| {
                    error!("Failed to set console title: {}", err);
                });
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Drives the per-client console color: tracks `state_rx` and
/// `highlight_rx`, paints the steady-state combination, and flashes
/// the underlying state color for [`HIGHLIGHT_FLASH_DURATION`].
///
/// # Arguments
///
/// * `api`                       - The Windows API implementation to use.
/// * `state_rx`                  - Receiver for daemon-driven state changes.
/// * `highlight_rx`              - Receiver for submenu highlight transitions.
/// * `original_console_color`    - Pristine attributes; `None`
///                                 degrades all painting to a no-op.
/// * `disabled_console_color`    - Color for [`ClientState::Disabled`].
/// * `highlighted_console_color` - Color while highlighted; overrides
///                                 the disabled color.
async fn run_visuals_loop(
    api: &dyn WindowsApi,
    mut state_rx: watch::Receiver<ClientState>,
    mut highlight_rx: watch::Receiver<bool>,
    original_console_color: Option<CONSOLE_CHARACTER_ATTRIBUTES>,
    disabled_console_color: CONSOLE_CHARACTER_ATTRIBUTES,
    highlighted_console_color: CONSOLE_CHARACTER_ATTRIBUTES,
) {
    let mut prev_state = *state_rx.borrow_and_update();
    let mut prev_highlight = *highlight_rx.borrow_and_update();
    let mut last_painted: Option<CONSOLE_CHARACTER_ATTRIBUTES> = None;

    paint_console_color(
        api,
        get_effective_color(
            prev_state,
            prev_highlight,
            original_console_color,
            disabled_console_color,
            highlighted_console_color,
        ),
        &mut last_painted,
    );

    let mut flash_until: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            state_changed = state_rx.changed() => {
                if state_changed.is_err() {
                    return;
                }
                let next_state = *state_rx.borrow_and_update();
                prev_state = next_state;
                if prev_highlight {
                    // State pushes come from `[e]`/`[d]`/`[t]` keypresses;
                    // flash even on no-op updates so the user always gets
                    // visual confirmation.
                    paint_console_color(
                        api,
                        get_flash_color(
                            next_state,
                            original_console_color,
                            disabled_console_color,
                        ),
                        &mut last_painted,
                    );
                    flash_until =
                        Some(tokio::time::Instant::now() + HIGHLIGHT_FLASH_DURATION);
                } else {
                    paint_console_color(
                        api,
                        get_effective_color(
                            next_state,
                            prev_highlight,
                            original_console_color,
                            disabled_console_color,
                            highlighted_console_color,
                        ),
                        &mut last_painted,
                    );
                    flash_until = None;
                }
            }
            highlight_changed = highlight_rx.changed() => {
                if highlight_changed.is_err() {
                    return;
                }
                let next_highlight = *highlight_rx.borrow_and_update();
                if next_highlight == prev_highlight {
                    continue;
                }
                prev_highlight = next_highlight;
                flash_until = None;
                paint_console_color(
                    api,
                    get_effective_color(
                        prev_state,
                        prev_highlight,
                        original_console_color,
                        disabled_console_color,
                        highlighted_console_color,
                    ),
                    &mut last_painted,
                );
            }
            _ = async {
                match flash_until {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                flash_until = None;
                paint_console_color(
                    api,
                    get_effective_color(
                        prev_state,
                        prev_highlight,
                        original_console_color,
                        disabled_console_color,
                        highlighted_console_color,
                    ),
                    &mut last_painted,
                );
            }
        }
    }
}

/// The entrypoint for the `client` subcommand with API dependency injection.
///
/// Spawns a tokio background thread to ensure the console window title is not replaced
/// by the name of the child process once its launched.
/// Starts the SSH process as child process.
/// Executes the main run loop.
///
/// # Arguments
///
/// * `api`         - The Windows API implementation to use.
/// * `host`        - The name of the host to connect to, optionally with `:port` suffix.
/// * `username`    - The username to be used.
///                   Will try to resolve the correct username from the ssh config
///                   if none is given.
/// * `cli_port`    - Optional port from CLI option. Inline port takes precedence.
/// * `config`      - A reference to the `ClientConfig`.
pub async fn main(
    api: &dyn WindowsApi,
    host: String,
    username: Option<String>,
    cli_port: Option<u16>,
    config: &ClientConfig,
) {
    let original_console_color = capture_original_console_color(api);

    let (state_tx, state_rx) = watch::channel(ClientState::Active);
    let (highlight_tx, highlight_rx) = watch::channel(false);

    let (host, inline_port) = split_host_and_inline_port(&host);
    // Inline port takes precedence over CLI port.
    let port = inline_port.or(cli_port);

    let resolved_username = resolve_username(username, host, config);
    let console_title = build_console_title(&resolved_username, host, port);

    let title_task = run_title_loop(api, console_title);
    let child_task = async {
        let mut child = launch_ssh_process(&resolved_username, host, port, config).await;
        run(api, &mut child, &state_tx, &highlight_tx).await;
        return child;
    };
    let visuals_task = run_visuals_loop(
        api,
        state_rx,
        highlight_rx,
        original_console_color,
        CONSOLE_CHARACTER_ATTRIBUTES(config.disabled_console_color),
        CONSOLE_CHARACTER_ATTRIBUTES(config.highlighted_console_color),
    );

    // The title and visuals tasks are infinite by construction; if either
    // ever completes, that is a logic bug, not a shutdown path.
    let child = tokio::select! {
        child = child_task => child,
        _ = title_task => {
            panic!("Title task should never complete");
        }
        _ = visuals_task => {
            panic!("Visuals task should never complete");
        }
    };

    api.generate_console_ctrl_event(0, 0).unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Failed to send `ctrl + c` to remaining client windows",)
    });
    drop(child);
}

#[cfg(test)]
#[path = "../tests/client/test_mod.rs"]
mod test_mod;
