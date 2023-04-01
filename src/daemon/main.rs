use clap::Parser;
use dissh::{
    serde::{serialization::Serialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    spawn_console_process,
    utils::{
        constants::{PIPE_NAME, PKG_NAME},
        get_console_input_buffer, wait_for_input,
    },
};
use tokio::{
    net::windows::named_pipe::{PipeMode, ServerOptions},
    sync::broadcast::{self, Sender},
    task::JoinHandle,
};
use win32console::console::WinConsole;
use windows::Win32::System::Console::{
    GetConsoleWindow, ReadConsoleInputW, INPUT_RECORD, INPUT_RECORD_0,
};
use windows::Win32::System::Threading::PROCESS_INFORMATION;
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

mod workspace;

const KEY_EVENT: u16 = 1;
const VK_ESCAPE: u16 = 0x1B;

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
    fn launch(&self) {
        WinConsole::set_title(&format!("{} daemon", PKG_NAME))
            .expect("Failed to set console window title.");

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

        self.run(self.launch_clients(&workspace_area, number_of_consoles));
    }

    fn run(&self, proc_infos: Vec<PROCESS_INFORMATION>) {
        //TODO: use tokio named_pipes
        // https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html
        // for IPC.
        // FIXME: directly reading from the input buffer prevents the automatic
        // printing of the typed input
        // Spawn one NamedPipeServer for each client and use the
        // broadcast queue https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html
        // to emit the read INPUT_RECORDS to all servers which will inturn send
        // them to the clients.
        let (sender, _) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(proc_infos.len());

        let server = self.launch_named_pipe_server(&sender);

        loop {
            let input_record = read_keyboard_input();
            match unsafe { input_record.KeyEvent }.wVirtualKeyCode {
                VK_ESCAPE => break,
                _ => (),
            }
            sender
                .send(
                    input_record.serialize().as_mut_vec()[..]
                        .try_into()
                        .unwrap(),
                )
                .unwrap();
        }

        drop(server);
        wait_for_input();
    }

    fn launch_named_pipe_server(
        &self,
        sender: &Sender<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>,
    ) -> Vec<JoinHandle<()>> {
        let mut server: Vec<JoinHandle<()>> = Vec::new();

        for (i, _) in self.hosts.iter().enumerate() {
            let named_pipe_server = ServerOptions::new()
                .access_outbound(true)
                .pipe_mode(PipeMode::Message)
                .create(PIPE_NAME)
                .unwrap();
            let mut receiver = sender.subscribe();
            server.push(tokio::spawn(async move {
                // wait for a client to connect
                named_pipe_server.connect().await.unwrap();
                println!("[{i}] Client has connected to named pipe server.");
                loop {
                    let ser_input_record = receiver.recv().await.unwrap();
                    println!("[{i}] Received serialized input record, sending it over named pipe");
                    // FIXME: catch and ignore broken pipe error (can happen if the client window get's closed)
                    named_pipe_server.try_write(&ser_input_record).unwrap();
                }
            }));
        }

        return server;
    }

    fn launch_clients(
        &self,
        workspace_area: &workspace::WorkspaceArea,
        number_of_consoles: i32,
    ) -> Vec<PROCESS_INFORMATION> {
        // TODO: use tokio runtimes to parallelize this process;
        let mut proc_infos: Vec<PROCESS_INFORMATION> = Vec::new();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as i32,
                number_of_consoles,
                workspace_area,
            );
            proc_infos.push(launch_client_console(host, x, y, width, height));
        }
        return proc_infos;
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

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch();
}
