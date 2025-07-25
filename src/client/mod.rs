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
use crate::utils::{get_console_input_buffer, get_console_title, set_console_title};
use ssh2_config::{ParseRule, SshConfig};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::process::{Child, Command};
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Console::{
    GenerateConsoleCtrlEvent, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, LEFT_ALT_PRESSED, RIGHT_ALT_PRESSED, SHIFT_PRESSED,
};

use crate::{
    serde::{deserialization::Deserialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
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

/// Write the given [INPUT_RECORD_0] to the console input buffer.
///
/// # Arguments
///
/// * `input_record` - The [INPUT_RECORD_0].`KeyEvent` input record to write.
fn write_console_input(input_record: INPUT_RECORD_0) {
    let buffer: [INPUT_RECORD; 1] = [INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: input_record,
    }];
    let mut nb_of_events_written: u32 = 0;
    match unsafe {
        WriteConsoleInputW(
            get_console_input_buffer(),
            &buffer,
            &mut nb_of_events_written,
        )
    } {
        Ok(_) => {
            if nb_of_events_written == 0 {
                error!("Failed to write console input");
                error!("{:?}", unsafe { GetLastError() });
            }
        }
        Err(_) => {
            error!("Failed to write console input");
            error!("{:?}", unsafe { GetLastError() });
        }
    };
}

/// Use `username` or load the adequate one from SSH config.
///
/// # Returns
///
/// The username and hostname in the format expected by SSH `<username>@<host>`.
fn get_username_and_host(username: Option<String>, host: &str, config: &ClientConfig) -> String {
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

    let host_specific_params = ssh_config.query(<&str>::clone(&host));

    let username: String = if let Some(val) = username {
        val
    } else {
        host_specific_params.user.unwrap_or_default()
    };

    return format!("{username}@{host}");
}

/// Launch the SSH process.
///
/// The process might overwrite the console title once it launched, so we wait for that
/// to happen and set the title again.
///
/// # Arguments
///
/// * `username_host`   - `<username>@<hostname>` string.
/// * `config`          - The client config indicating how to call the SSH program.
///
/// # Returns
///
/// The handle to created [Child] process.
async fn launch_ssh_process(username_host: &str, config: &ClientConfig) -> Child {
    let arguments = replace_argument_placeholders(
        &config.arguments,
        &config.username_host_placeholder,
        username_host,
    );
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

/// Read all available [INPUT_RECORD_0] from the named pipe and write them to the console input buffer.
///
/// This function also extracts the [KEY_EVENT_RECORD]s, making them available to the caller via
/// `ReadWriteResult::Success` and handles incomple reads from the named pipe via the internal buffer.
///
/// The daemon might send a "keep alive packet", which is just [`SERIALIZED_INPUT_RECORD_0_LENGTH`] bytes of `1`s,
/// we ignore this.
///
/// # Arguments
///
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
                };
                let input_record =
                    INPUT_RECORD_0::deserialize(&mut serialzied_input_record.to_owned());
                write_console_input(input_record);
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
/// * `child` - Handle to the running SSH process.
async fn run(child: &mut Child) {
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

        match read_write_loop(&named_pipe_client, &mut internal_buffer).await {
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

/// The entrypoint for the `client` subcommand.
///
/// Spawns a tokio background thread to ensure the console window title is not replaced
/// by the name of the child process once its launched.
/// Starts the SSH process as child process.
/// Executes the main run loop.
///
/// # Arguments
///
/// * `host`        - The name of the host to connect tol
/// * `username`    - The username to be used.
///                   Will try to resolve the correct username from the ssh config
///                   if none is given.
/// * `config`      - A reference to the `ClientConfig`.
pub async fn main(host: String, username: Option<String>, config: &ClientConfig) {
    let username_host = get_username_and_host(username, &host, config);
    let username_host_title = username_host.clone();
    tokio::spawn(async move {
        loop {
            // Set the console title (child might overwrite it, so we have to keep checking it)
            let console_title = format!("{PKG_NAME} - {username_host_title}");
            if console_title != get_console_title() {
                set_console_title(console_title.as_str());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    let mut child = launch_ssh_process(&username_host, config).await;

    run(&mut child).await;

    // Make sure the client and all its subprocesses
    // are aware they need to shutdown.
    unsafe {
        GenerateConsoleCtrlEvent(0, 0).unwrap_or_else(|err| {
            error!("{}", err);
            panic!("Failed to send `ctrl + c` to remaining client windows",)
        });
    }
    drop(child);
}

#[cfg(test)]
#[path = "../tests/test_client.rs"]
mod test_client;
