use std::mem;
use std::process::Command;
use std::{os::windows::process::CommandExt, process::Child};

use clap::Parser;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;
use winit::event_loop::EventLoop;
use winit::monitor::MonitorHandle;
use winit::platform::windows::MonitorHandleExtWindows;
use winit::window::Window;

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

pub struct Daemon {
    hosts: Vec<String>,
}

impl Daemon {
    pub fn launch(&self) {
        self.run(self.launch_clients());
    }

    fn run(&self, client_consoles: Vec<Child>) {
        //TODO: read from daemon console and publish
        // read user input to clients
    }

    fn launch_clients(&self) -> Vec<Child> {
        let mut client_consoles: Vec<Child> = Vec::new();
        let workspace_area = get_logical_workspace_size();
        let hosts_count = self.hosts.len();
        for (index, host) in self.hosts.iter().enumerate() {
            let (x, y, width, height) = self.determine_client_spacial_attributes(
                index as u32,
                hosts_count as u32,
                workspace_area,
            );
            client_consoles.push(launch_client_console(host, x, y, width, height));
        }
        return client_consoles;
    }

    fn determine_client_spacial_attributes(
        &self,
        index: u32,
        hosts_count: u32,
        workspace_area: WorkspaceArea,
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

#[derive(Clone, Copy)]
struct WorkspaceArea {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn get_logical_workspace_size() -> WorkspaceArea {
    let event_loop = EventLoop::new();
    let monitor_handle: MonitorHandle = event_loop
        .primary_monitor()
        .expect("Failed to determine primary monitor.");
    let hmonitor: HMONITOR = windows::Win32::Graphics::Gdi::HMONITOR(monitor_handle.hmonitor());
    let mut monitor_info: MONITORINFOEXW = unsafe { mem::zeroed() };
    monitor_info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    unsafe {
        GetMonitorInfoW(
            hmonitor,
            &mut monitor_info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    };
    let window = Window::new(&event_loop).unwrap();
    let scale_factor = window.scale_factor();
    return WorkspaceArea {
        x: (monitor_info.monitorInfo.rcMonitor.left as f64 / scale_factor) as u32,
        y: (monitor_info.monitorInfo.rcMonitor.top as f64 / scale_factor) as u32,
        width: ((monitor_info.monitorInfo.rcMonitor.right - monitor_info.monitorInfo.rcMonitor.left)
            as f64
            / scale_factor) as u32,
        height: ((monitor_info.monitorInfo.rcMonitor.bottom
            - monitor_info.monitorInfo.rcMonitor.top) as f64
            / scale_factor) as u32,
    };
}

fn main() {
    let args = Args::parse();
    let daemon = Daemon { hosts: args.hosts };
    daemon.launch();
}
