use std::process::Command;
use std::{os::windows::process::CommandExt, process::Child};
use std::{thread, time};

use clap::Parser;
use win32console::console::WinConsole;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MoveWindow, SM_CXBORDER, SM_CXPADDEDBORDER, SM_CYSIZE,
};

mod workspace;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const MIN_CONSOLE_HEIGHT: u32 = 100;

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

        let workspace_area = workspace::get_logical_workspace_size();
        // +1 to account for the daemon console
        let number_of_consoles = (self.hosts.len() + 1) as u32;
        let title_bar_height = unsafe {
            GetSystemMetrics(SM_CXBORDER)
                + GetSystemMetrics(SM_CYSIZE)
                + GetSystemMetrics(SM_CXPADDEDBORDER)
        } as u32;

        // The daemon console can be treated as a client console when it comes
        // to figuring out where to put it on the screen.
        let (x, y, width, height) = determine_client_spacial_attributes(
            number_of_consoles - 1, // -1 because the index starts at 0
            number_of_consoles,
            &workspace_area,
            title_bar_height,
        );
        // for some reason the daemon console window uses physical size instead of logical
        // so we convert logical size back to physical
        arrange_daemon_console(
            (x as f64 * workspace_area.scale_factor) as i32,
            (y as f64 * workspace_area.scale_factor) as i32,
            (width as f64 * workspace_area.scale_factor) as i32,
            (height as f64 * workspace_area.scale_factor) as i32,
        );

        self.run(self.launch_clients(&workspace_area, number_of_consoles, title_bar_height));
    }

    fn run(&self, client_consoles: Vec<Child>) {
        //TODO: read from daemon console and publish
        // read user input to clients
        thread::sleep(time::Duration::from_millis(20000));
    }

    fn launch_clients(
        &self,
        workspace_area: &workspace::WorkspaceArea,
        number_of_consoles: u32,
        title_bar_height: u32,
    ) -> Vec<Child> {
        let mut client_consoles: Vec<Child> = Vec::new();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as u32,
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
    println!("{} {} {} {}", x, y, width, height);
    unsafe {
        MoveWindow(GetConsoleWindow(), x, y, width, height, true);
    }
}

fn determine_client_spacial_attributes(
    index: u32,
    number_of_consoles: u32,
    workspace_area: &workspace::WorkspaceArea,
    title_bar_height: u32,
) -> (u32, u32, u32, u32) {
    // FIXME: somehow we always have 0 columns
    // FIXME: now that we account for title bar height we miss almost half the screen
    // https://math.stackexchange.com/a/21734
    let number_of_columns = number_of_consoles / workspace_area.height / MIN_CONSOLE_HEIGHT;
    if number_of_columns == 0 {
        let console_height = (workspace_area.height / number_of_consoles) - title_bar_height;
        return (
            workspace_area.x as u32,
            workspace_area.y + index * console_height,
            workspace_area.width,
            console_height,
        );
    }
    let x = workspace_area.width / number_of_columns * (index % number_of_columns);
    let y = (index / number_of_columns * workspace_area.height) - title_bar_height;
    return (
        workspace_area.x + x,
        workspace_area.y + y,
        (workspace_area.width / number_of_columns) - title_bar_height,
        MIN_CONSOLE_HEIGHT,
    );
}

fn launch_client_console(host: &String, x: u32, y: u32, width: u32, height: u32) -> Child {
    return Command::new(format!("{}-client", PKG_NAME))
        .args([
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
