use std::{io, time::Duration};

use clap::Parser;
use dissh::{
    serde::{serialization::Serialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        constants::{DEFAULT_SSH_USERNAME_KEY, PIPE_NAME, PKG_NAME},
        get_console_input_buffer, set_console_title,
    },
};
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
    sync::broadcast::{self, Receiver, Sender},
    task::JoinHandle,
};
use windows::Win32::System::Console::{
    GetConsoleMode, GetConsoleWindow, ReadConsoleInputW, SetConsoleMode, CONSOLE_MODE,
    ENABLE_PROCESSED_INPUT, INPUT_RECORD, INPUT_RECORD_0,
};
use windows::Win32::System::Threading::PROCESS_INFORMATION;
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

mod workspace;

const KEY_EVENT: u16 = 1;

/// Daemon CLI. Manages client consoles and user input
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Username used to connect to the hosts
    #[clap(short, long)]
    username: Option<String>,

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

struct Daemon {
    hosts: Vec<String>,
    username: Option<String>,
}

impl Daemon {
    async fn launch(self) {
        set_console_title(format!("{} daemon", PKG_NAME).as_str());

        // Makes sure ctrl+c is reported as a keyboard input rather than as signal
        // https://learn.microsoft.com/en-us/windows/console/ctrl-c-and-ctrl-break-signals
        disable_processed_input_mode();

        let workspace_area = workspace::get_workspace_area(workspace::Scaling::LOGICAL);
        // +1 to account for the daemon console
        let number_of_consoles = (self.hosts.len() + 1) as i32;

        // The daemon console can be treated as a client console when it comes
        // to figuring out where to put it on the screen.
        // TODO: the daemon console should always be on the bottom left
        let (x, y, width, height) = determine_client_spacial_attributes(
            number_of_consoles - 1, // -1 because the index starts at 0
            number_of_consoles,
            &workspace_area,
        );
        arrange_daemon_console(x, y, width, height);

        launch_clients(
            self.hosts.to_vec(),
            &self.username,
            workspace_area,
            number_of_consoles,
        )
        .await;

        self.run();
    }

    fn run(&self) {
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(self.hosts.len());

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

        loop {
            let input_record = read_keyboard_input();
            sender
                .send(
                    input_record.serialize().as_mut_vec()[..]
                        .try_into()
                        .unwrap(),
                )
                .unwrap();
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
}

fn arrange_daemon_console(x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        MoveWindow(GetConsoleWindow(), x, y, width, height, true);
    }
}

fn determine_client_spacial_attributes(
    index: i32,
    number_of_consoles: i32,
    workspace_area: &workspace::WorkspaceArea,
) -> (i32, i32, i32, i32) {
    let height_width_ratio = workspace_area.height as f64 / workspace_area.width as f64;
    let number_of_columns = (number_of_consoles as f64 / height_width_ratio).sqrt() as i32;
    let console_width = workspace_area.width / number_of_columns;
    let console_height = (console_width as f64 * height_width_ratio) as i32;
    let x = workspace_area.width / number_of_columns * (index % number_of_columns);
    let y = index / number_of_columns * console_height;
    return (
        workspace_area.x + x,
        workspace_area.y + y,
        console_width + workspace_area.x_fixed_frame + workspace_area.x_size_frame * 2,
        console_height + workspace_area.y_size_frame * 2,
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
        &format!("{PKG_NAME}-client.exe"),
        vec![
            "--",
            host,
            &username
                .as_ref()
                .unwrap_or(&DEFAULT_SSH_USERNAME_KEY.to_string()),
            &x.to_string(),
            &y.to_string(),
            &width.to_string(),
            &height.to_string(),
        ],
    );
}

fn read_keyboard_input() -> INPUT_RECORD_0 {
    loop {
        let input_record = read_console_input();
        match input_record.EventType {
            KEY_EVENT => {
                return input_record.Event;
            }
            _ => {
                continue;
            }
        }
    }
}

fn read_console_input() -> INPUT_RECORD {
    const NB_EVENTS: usize = 1;
    let mut input_buffer: [INPUT_RECORD; NB_EVENTS] = [INPUT_RECORD::default(); NB_EVENTS];
    let mut number_of_events_read = 0;
    loop {
        unsafe {
            ReadConsoleInputW(
                get_console_input_buffer(),
                &mut input_buffer,
                &mut number_of_events_read,
            )
            .expect("Failed to read console input");
        }
        if number_of_events_read == NB_EVENTS as u32 {
            break;
        }
    }
    return input_buffer[0];
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

async fn launch_clients(
    hosts: Vec<String>,
    username: &Option<String>,
    workspace_area: workspace::WorkspaceArea,
    number_of_consoles: i32,
) {
    let mut handles = vec![];
    for (index, host) in hosts.to_owned().into_iter().enumerate() {
        let _username = username.clone();
        let future = tokio::spawn(async move {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as i32,
                number_of_consoles,
                &workspace_area,
            );
            // TODO: probably keep track of the returned PROCESS_INFORMATION
            // to bring all clients to front when daemon is selected
            // or to close daemon if clients die
            launch_client_console(&host, _username, x, y, width, height);
        });
        handles.push(future);
    }
    for handle in handles {
        handle.await.unwrap();
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

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let daemon: Daemon = Daemon {
        hosts: args.hosts,
        username: args.username,
    };
    daemon.launch().await;
}
