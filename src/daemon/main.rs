use clap::Parser;
use dissh::{
    print_std_handles, serde_input_record, spawn_console_process, wait_for_input, PKG_NAME,
};
use win32console::console::WinConsole;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::PROCESS_INFORMATION;
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

mod workspace;

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

    fn run(&self, _proc_infos: Vec<PROCESS_INFORMATION>) {
        //TODO: read from daemon console and publish
        // read user input to clients
        print_std_handles();
        serde_input_record();
        wait_for_input();
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

fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch();
}
