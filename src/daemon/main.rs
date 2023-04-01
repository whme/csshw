use std::process::Command;
use std::{os::windows::process::CommandExt, process::Child};

use clap::Parser;
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;

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
        self.run(self.launch_clients());
    }

    fn run(&self, client_consoles: Vec<Child>) {
        //TODO: read from daemon console and publish
        // read user input to clients
    }

    fn launch_clients(&self) -> Vec<Child> {
        let mut client_consoles: Vec<Child> = Vec::new();
        let workspace_area = workspace::get_logical_workspace_size();
        let hosts_count = self.hosts.len();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) = determine_client_spacial_attributes(
                index as u32,
                hosts_count as u32,
                workspace_area,
            );
            client_consoles.push(launch_client_console(host, x, y, width, height));
        }
        return client_consoles;
    }
}

fn determine_client_spacial_attributes(
    index: u32,
    hosts_count: u32,
    workspace_area: workspace::WorkspaceArea,
) -> (u32, u32, u32, u32) {
    // FIXME: somehow we always have 0 columns
    // FIXME: account for title bar height
    // FIXME: account for daemon console itself
    let number_of_columns = hosts_count / workspace_area.height / MIN_CONSOLE_HEIGHT;
    if number_of_columns == 0 {
        let console_height = workspace_area.height / hosts_count;
        return (
            workspace_area.x as u32,
            workspace_area.y + index * console_height,
            workspace_area.width,
            console_height,
        );
    }
    let x = workspace_area.width / number_of_columns * (index % number_of_columns);
    let y = index / number_of_columns * workspace_area.height;
    return (
        workspace_area.x + x,
        workspace_area.y + y,
        workspace_area.width / number_of_columns,
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
        .expect("Failed to start daemon process.");
}

fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch();
}
