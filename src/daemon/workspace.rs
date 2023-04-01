use std::mem;

use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO, MONITORINFOEXW};
use winit::event_loop::EventLoop;
use winit::monitor::MonitorHandle;
use winit::platform::windows::MonitorHandleExtWindows;
use winit::window::Window;

#[derive(Clone, Copy)]
pub struct WorkspaceArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn get_logical_workspace_size() -> WorkspaceArea {
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
