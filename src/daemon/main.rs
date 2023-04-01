use std::process::Command;
use std::{os::windows::process::CommandExt, process::Child};
use std::{ptr, thread, time};

use clap::Parser;
use win32console::console::WinConsole;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowRect, MoveWindow, SM_CXBORDER, SM_CXPADDEDBORDER, SM_CYSIZE,
};

mod workspace;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

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
        let title_bar_height = unsafe {
            GetSystemMetrics(SM_CXBORDER)
                + GetSystemMetrics(SM_CYSIZE)
                + GetSystemMetrics(SM_CXPADDEDBORDER)
        };

        // The daemon console can be treated as a client console when it comes
        // to figuring out where to put it on the screen.
        // TODO: the daemon console should always be on the bottom left
        let (x, y, width, height) = determine_client_spacial_attributes(
            number_of_consoles - 1, // -1 because the index starts at 0
            number_of_consoles,
            &workspace_area,
            title_bar_height,
        );
        arrange_daemon_console(x, y, width, height);

        self.run(self.launch_clients(&workspace_area, number_of_consoles, title_bar_height));
    }

    fn run(&self, client_consoles: Vec<Child>) {
        //TODO: read from daemon console and publish
        // read user input to clients
        let mut i = 0;
        loop {
            i = i + 1;
            let mut window_rect = RECT::default();
            unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)) };
            println!("{:?}", window_rect);
            if i > 2000 {
                break;
            };
            thread::sleep(time::Duration::from_millis(100));
        }
        thread::sleep(time::Duration::from_millis(20000));
    }

    fn launch_clients(
        &self,
        workspace_area: &workspace::WorkspaceArea,
        number_of_consoles: i32,
        title_bar_height: i32,
    ) -> Vec<Child> {
        let mut client_consoles: Vec<Child> = Vec::new();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as i32,
                number_of_consoles,
                workspace_area,
                title_bar_height,
            );
            client_consoles.push(launch_client_console(host, x, y, width, height));
        }
        return client_consoles;
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
    title_bar_height: i32,
) -> (i32, i32, i32, i32) {
    println!("index: {index}");
    // FIXME: somehow we always have 0 columns
    // FIXME: now that we account for title bar height we miss almost half the screen
    // https://math.stackexchange.com/a/21734
    let height_width_ratio = workspace_area.height as f64 / workspace_area.width as f64;
    let number_of_columns = (number_of_consoles as f64 / height_width_ratio).sqrt() as i32;
    let console_width = workspace_area.width / number_of_columns;
    let console_height = (console_width as f64 * height_width_ratio) as i32;
    let x = workspace_area.width / number_of_columns * (index % number_of_columns);
    let y = index / number_of_columns * workspace_area.height;
    return (
        workspace_area.x + x,
        workspace_area.y + y,
        console_width,
        console_height,
    );
}

fn launch_client_console(host: &String, x: i32, y: i32, width: i32, height: i32) -> Child {
    // The first argument must be `--` to ensure all following arguments are treated
    // as positional arguments and not as options of they start with `-`.
    return Command::new(format!("{}-client", PKG_NAME))
        .args([
            "--".to_string(),
            host.to_string(),
            x.to_string(),
            y.to_string(),
            width.to_string(),
            height.to_string(),
        ])
        .creation_flags(CREATE_NEW_CONSOLE.0)
        .spawn()
        .expect("Failed to start client process.");
}

fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch();
}
