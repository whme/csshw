use std::process::Command;
use std::{os::windows::process::CommandExt, process::Child};

use clap::Parser;
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;
use winit::{dpi::PhysicalSize, event_loop::EventLoop, monitor::MonitorHandle};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const MIN_CONSOLE_HEIGHT: u32 = 200;

/// Daemon CLI. Manages client consoles and user input
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

pub struct Daemon {
    hosts: Vec<String>,
    monitor_size: PhysicalSize<u32>,
}

impl Daemon {
    pub fn default() -> Daemon {
        Daemon {
            hosts: Vec::new(),
            monitor_size: get_primary_monitor().size(),
        }
    }

    pub fn launch(&self) {
        self.launch_clients();
        self.run();
    }

    fn run(&self) {
        //TODO: read from daemon console and publish
        // read user input to clients
    }

    fn launch_clients(&self) {
        let hosts_count = self.hosts.len();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) =
                self.determine_client_spacial_attributes(index as u32, hosts_count as u32);
            self.launch_client_console(host, x, y, width, height);
        }
    }

    fn launch_client_console(&self, host: &String, x: u32, y: u32, width: u32, height: u32) {
        // TODO: store the resulting Child in a vector for later re-use
        Command::new(format!("{}-client", PKG_NAME))
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

    fn determine_client_spacial_attributes(
        &self,
        index: u32,
        hosts_count: u32,
    ) -> (u32, u32, u32, u32) {
        let number_of_columns = hosts_count / self.monitor_size.height / MIN_CONSOLE_HEIGHT;
        if number_of_columns == 0 {
            let console_height = self.monitor_size.height / hosts_count;
            return (
                0 as u32,
                index * console_height,
                self.monitor_size.width,
                console_height,
            );
        }
        let x = self.monitor_size.width / number_of_columns * (index % number_of_columns);
        let y = index / number_of_columns * self.monitor_size.height;
        return (
            x,
            y,
            self.monitor_size.width / number_of_columns,
            MIN_CONSOLE_HEIGHT,
        );
    }
}

fn get_primary_monitor() -> MonitorHandle {
    return EventLoop::new()
        .primary_monitor()
        .expect("Failed to determine primary monitor.");
}

fn main() {
    let args = Args::parse();
    let daemon = Daemon {
        hosts: args.hosts,
        ..Daemon::default()
    };
    daemon.launch();
}
