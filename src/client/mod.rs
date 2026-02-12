//! Client implementation

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use log::{error, info, warn};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use windows::Win32::System::Console::CONSOLE_CHARACTER_ATTRIBUTES;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;

use crate::utils::config::ClientConfig;
use crate::utils::windows::{
    get_console_title, set_console_border_color, set_console_color, WindowsApi,
};

/// Stores the original console text attributes to restore later
static ORIGINAL_CONSOLE_ATTRIBUTES: OnceLock<CONSOLE_CHARACTER_ATTRIBUTES> = OnceLock::new();
use ssh2_config::{ParseRule, SshConfig};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::process::{Child, Command};
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Console::{
    INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, LEFT_ALT_PRESSED, RIGHT_ALT_PRESSED,
    SHIFT_PRESSED,
};

use crate::{
    serde::{
        deserialization::deserialize_input_record_0, is_control_sequence,
        CONTROL_SEQ_STATE_DISABLED, CONTROL_SEQ_STATE_ENABLED, CONTROL_SEQ_STATE_SELECTED,
        SERIALIZED_INPUT_RECORD_0_LENGTH,
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

/// Handle a control sequence by updating the client's visual state.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `control_seq` - The control sequence bytes.
fn handle_control_sequence(api: &dyn WindowsApi, control_seq: &[u8]) {
    // ENABLED state: restore default console appearance (no special color)
    if control_seq == CONTROL_SEQ_STATE_ENABLED {
        log::debug!("Restoring original console appearance (ENABLED)");
        // Restore default border color on Windows 11+
        let _ = set_console_border_color(api, COLORREF(0x00000000)); // Black = default
                                                                     // Restore original console colors that were saved at startup
        if let Some(&original_attrs) = ORIGINAL_CONSOLE_ATTRIBUTES.get() {
            log::debug!("Restoring original attributes: {:?}", original_attrs);
            set_console_color(api, original_attrs);
        } else {
            log::warn!("Original console attributes not saved, using default");
            use windows::Win32::System::Console::*;
            let default_color = CONSOLE_CHARACTER_ATTRIBUTES(
                FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_BLUE.0,
            );
            set_console_color(api, default_color);
        }
        return;
    }

    let (color, state_name) = if control_seq == CONTROL_SEQ_STATE_DISABLED {
        (COLORREF(0x00808080), "DISABLED") // Grey border
    } else if control_seq == CONTROL_SEQ_STATE_SELECTED {
        (COLORREF(0x00B0E0E6), "SELECTED") // Powder blue border
    } else {
        log::debug!("Unknown control sequence: {:?}", control_seq);
        return; // Unknown control sequence
    };

    log::debug!("Setting border color for state {}: {:?}", state_name, color);

    // Try to set border color (Windows 11+), fallback to background color (Windows 10)
    match set_console_border_color(api, color) {
        Ok(_) => {
            log::debug!("Border color set successfully");
        }
        Err(_) => {
            // Fallback to background colors on Windows 10
            use windows::Win32::System::Console::*;

            let color_attributes = if color.0 == 0x00808080 {
                // Grey (DISABLED) -> Grey background with white text
                CONSOLE_CHARACTER_ATTRIBUTES(
                    BACKGROUND_INTENSITY.0
                        | FOREGROUND_RED.0
                        | FOREGROUND_GREEN.0
                        | FOREGROUND_BLUE.0,
                )
            } else if color.0 == 0x00B0E0E6 {
                // Powder blue (SELECTED) -> Blue background with bright white text
                CONSOLE_CHARACTER_ATTRIBUTES(
                    BACKGROUND_BLUE.0
                        | BACKGROUND_INTENSITY.0
                        | FOREGROUND_RED.0
                        | FOREGROUND_GREEN.0
                        | FOREGROUND_BLUE.0
                        | FOREGROUND_INTENSITY.0,
                )
            } else {
                // Default
                CONSOLE_CHARACTER_ATTRIBUTES(
                    FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_BLUE.0,
                )
            };

            log::debug!(
                "Using background color fallback (Windows 10): {:?}",
                color_attributes
            );
            set_console_color(api, color_attributes);
            log::debug!("Background color set successfully");
        }
    }
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

/// Read all available [INPUT_RECORD_0] from the named pipe and write them to the console input buffer using the provided API.
///
/// This function also extracts the [KEY_EVENT_RECORD]s, making them available to the caller via
/// `ReadWriteResult::Success` and handles incomple reads from the named pipe via the internal buffer.
///
/// The daemon might send a "keep alive packet", which is just [`SERIALIZED_INPUT_RECORD_0_LENGTH`] bytes of `1`s,
/// we ignore this.
///
/// # Arguments
///
/// * `api`                 - The Windows API implementation to use.
/// * `named_pipe_client`   - The [Windows named pipe][1] client that has successfully connected to
///                           the named pipe created by the daemon.
/// * `internal_buffer`     - Vector containing incomplete `SERIALIZED_INPUT_RECORD_0` sequences
///                           that were read in a previous call.
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
            internal_buffer.extend(&mut buf[0..n].iter());
            let iter = internal_buffer.chunks_exact(SERIALIZED_INPUT_RECORD_0_LENGTH);
            let mut key_event_records: Vec<KEY_EVENT_RECORD> = Vec::new();
            for serialzied_input_record in iter.clone() {
                if is_keep_alive_packet(serialzied_input_record) {
                    continue;
                }

                // Check if this is a control sequence
                if is_control_sequence(serialzied_input_record) {
                    log::debug!("Received control sequence: {:?}", serialzied_input_record);
                    handle_control_sequence(api, serialzied_input_record);
                    continue;
                }

                let input_record = deserialize_input_record_0(serialzied_input_record);
                write_console_input(api, input_record);
                key_event_records.push(unsafe { input_record.KeyEvent });
            }
            return ReadWriteResult::Success {
                remainder: iter.remainder().to_vec(),
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

/// Checks if a byte sequence represents a keep-alive packet.
///
/// # Arguments
///
/// * `packet` - The byte sequence to check.
///
/// # Returns
///
/// `true` if the packet is a keep-alive packet, `false` otherwise.
fn is_keep_alive_packet(packet: &[u8]) -> bool {
    return packet == [u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH];
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

/// The main run loop of the client.
///
/// Connects to the named pipe opened by the daemon, reads all input records from it
/// and replays them to the console input buffer of the given child process.
/// Handles the `Alt + Shift + C` key combination used to close the console window
/// after the child process encountered an unexpected error.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `child` - Handle to the running SSH process.
async fn run(api: &dyn WindowsApi, child: &mut Child) {
    // Get our own window handle to send as identification
    let own_window_handle_raw = api.get_console_window().0 as isize;

    // Many clients trying to open the pipe at the same time can cause
    // a file not found error, so keep trying until we managed to open it
    let named_pipe_client: NamedPipeClient = loop {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(named_pipe_client) => {
                break named_pipe_client;
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
        }
    };

    // Send our window handle as identification
    let id_bytes = own_window_handle_raw.to_le_bytes();
    loop {
        named_pipe_client.writable().await.unwrap();
        match named_pipe_client.try_write(&id_bytes) {
            Ok(8) => {
                log::debug!(
                    "Sent client identification: HWND 0x{:X}",
                    own_window_handle_raw
                );
                break;
            }
            Ok(n) => {
                log::warn!("Partially sent identification: {} bytes", n);
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                log::error!("Failed to send identification: {}", e);
                return;
            }
        }
    }

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

        match read_write_loop(api, &named_pipe_client, &mut internal_buffer).await {
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
    // Save original console attributes at startup so we can restore them later
    if ORIGINAL_CONSOLE_ATTRIBUTES.get().is_none() {
        if let Ok(buffer_info) = api.get_console_screen_buffer_info() {
            let _ = ORIGINAL_CONSOLE_ATTRIBUTES.set(buffer_info.wAttributes);
            log::debug!(
                "Saved original console attributes: {:?}",
                buffer_info.wAttributes
            );
        }
    }

    let (host, inline_port) =
        host.rsplit_once(':')
            .map_or((host.as_str(), None), |(host, port)| {
                return (host, Some(port));
            });
    let inline_port = inline_port.and_then(|p| {
        return p
            .parse::<u16>()
            .map_err(|e| {
                warn!("Invalid port '{}': {}. Using default SSH port.", p, e);
            })
            .ok();
    });
    // Inline port takes precedence over CLI port
    let port = inline_port.or(cli_port);

    // Resolve username using SSH config if needed
    let resolved_username = resolve_username(username, host, config);

    // Create title for console window
    let title_host = if let Some(port) = port {
        format!("{host}:{port}")
    } else {
        host.to_string()
    };
    let username_host_title = format!("{resolved_username}@{title_host}");
    let console_title = format!("{PKG_NAME} - {username_host_title}");
    let title_task = {
        let console_title = console_title.clone();
        async move {
            loop {
                // Set the console title (child might overwrite it, so we have to keep checking it)
                if console_title != get_console_title(api) {
                    api.set_console_title(console_title.as_str())
                        .unwrap_or_else(|err| {
                            error!("Failed to set console title: {}", err);
                        });
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    };
    let child_task = async {
        let mut child = launch_ssh_process(&resolved_username, host, port, config).await;
        run(api, &mut child).await;
        return child;
    };

    // Use tokio::select to run both tasks concurrently
    let child = tokio::select! {
        child = child_task => child,
        _ = title_task => {
            panic!("Title task should never complete");
        }
    };

    // Make sure the client and all its subprocesses
    // are aware they need to shutdown.
    api.generate_console_ctrl_event(0, 0).unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Failed to send `ctrl + c` to remaining client windows",)
    });
    drop(child);
}

#[cfg(test)]
#[path = "../tests/client/test_mod.rs"]
mod test_mod;
