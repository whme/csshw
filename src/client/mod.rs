#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]

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

enum ReadWriteResult {
    Success {
        remainder: Vec<u8>,
        key_event_records: Vec<KEY_EVENT_RECORD>,
    },
    WouldBlock,
    Err,
    Disconnect,
}

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
/// Returns `<username>@<host>`.
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

    return format!("{}@{}", username, host);
}

/// Launch the SSH process.
/// It might overwrite the console title once it launches, so we wait for that
/// to happen and set the title again.
async fn launch_ssh_process(username_host: &str, config: &ClientConfig) -> Child {
    let arguments = config.arguments.clone().into_iter().map(|arg| {
        return arg.replace(config.username_host_placeholder.as_str(), username_host);
    });
    let child = Command::new(&config.program)
        .args(arguments.clone())
        .spawn()
        .unwrap_or_else(|err| {
            let args: String =
                itertools::Itertools::intersperse(arguments, " ".to_owned()).collect();
            error!("{}", err);
            panic!(
                "Failed to launch process `{}` with arguments `{}`",
                config.program, args
            )
        });
    return child;
}

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
                if serialzied_input_record == [u8::MAX; 18] {
                    // Just a keep alive packet from the daemon, ignore it
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
                        if (key_event.dwControlKeyState & LEFT_ALT_PRESSED >= 1
                            || key_event.dwControlKeyState & RIGHT_ALT_PRESSED == 1)
                            && key_event.dwControlKeyState & SHIFT_PRESSED >= 1
                            && key_event.wVirtualKeyCode == VK_C.0
                        {
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

pub async fn main(host: String, username: Option<String>, config: &ClientConfig) {
    let username_host = get_username_and_host(username, &host, config);
    let _username_host = username_host.clone();
    tokio::spawn(async move {
        loop {
            // Set the console title (child might overwrite it, so we have to keep checking it)
            let console_title = format!("{} - {}", PKG_NAME, _username_host);
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
