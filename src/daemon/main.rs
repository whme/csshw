use std::io;

use clap::Parser;
use dissh::{
    serde::{serialization::Serialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        constants::{PIPE_NAME, PKG_NAME},
        disable_processed_input_mode, get_console_input_buffer, set_console_title,
    },
};
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
    sync::broadcast::{self, Receiver, Sender},
};
use windows::Win32::System::Console::{
    GetConsoleWindow, ReadConsoleInputW, INPUT_RECORD, INPUT_RECORD_0,
};
use windows::Win32::System::Threading::PROCESS_INFORMATION;
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

mod workspace;

const KEY_EVENT: u16 = 1;

/// Daemon CLI. Manages client consoles and user input
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

struct Daemon {
    hosts: Vec<String>,
}

impl Daemon {
    async fn launch(&self) {
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

        launch_clients(self.hosts.to_vec(), workspace_area, number_of_consoles).await;

        self.run();
    }

    fn run(&self) {
        // FIXME: directly reading from the input buffer prevents the automatic
        // printing of the typed input
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(self.hosts.len());

        self.launch_named_pipe_server(&sender);

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

    fn launch_named_pipe_server(&self, sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>) {
        for _ in &self.hosts {
            let named_pipe_server = ServerOptions::new()
                .access_outbound(true)
                .pipe_mode(PipeMode::Message)
                .create(PIPE_NAME)
                .unwrap();
            let mut receiver = sender.subscribe();
            // TODO: we should keep track of the launched server routines,
            // once the client disconnects the routine should stop
            // and once all routines stopped the daemon should stop as well
            tokio::spawn(async move {
                named_pipe_server_routine(named_pipe_server, &mut receiver).await;
            });
        }
    }
}

fn arrange_daemon_console(x: i32, y: i32, width: i32, height: i32) {
    println!("{x} {y} {width} {height}");
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
        console_width,
        console_height,
    );
}

fn launch_client_console(
    host: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> PROCESS_INFORMATION {
    // The first argument must be `--` to ensure all following arguments are treated
    // as positional arguments and not as options of they start with `-`.
    return spawn_console_process(
        &format!("{PKG_NAME}-client.exe"),
        vec![
            "--",
            host,
            &x.to_string(),
            &y.to_string(),
            &width.to_string(),
            &height.to_string(),
        ],
    );
}

fn read_keyboard_input() -> INPUT_RECORD_0 {
    loop {
        // TODO: instead of using the ReadConsoleInputW function
        // we should register a LowLevelKeyboardProc hook
        // https://learn.microsoft.com/en-us/windows/win32/winmsg/hooks
        // Using the callbacks for WM_KEYDOWN and WM_KEYUP
        // (maybe additionally the SYS versions)
        // we should be able to build the INPUT_RECORDS ourselve
        // https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms644985(v=vs.85)
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
        let ser_input_record = receiver.recv().await.unwrap();
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
                    break;
                }
            }
        }
    }
}

async fn launch_clients(
    hosts: Vec<String>,
    workspace_area: workspace::WorkspaceArea,
    number_of_consoles: i32,
) {
    let mut handles = vec![];
    for (index, host) in hosts.to_owned().into_iter().enumerate() {
        let future = tokio::spawn(async move {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as i32,
                number_of_consoles,
                &workspace_area,
            );
            launch_client_console(&host, x, y, width, height);
        });
        handles.push(future);
    }
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch().await;
}
