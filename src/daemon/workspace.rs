use std::mem;

use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO, MONITORINFOEXW};
use winit::event_loop::EventLoop;
use winit::monitor::MonitorHandle;
use winit::platform::windows::MonitorHandleExtWindows;

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
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
    let scale_factor = monitor_handle.scale_factor();
    return WorkspaceArea {
        x: (monitor_info.monitorInfo.rcMonitor.left as f64 / scale_factor) as u32,
        y: (monitor_info.monitorInfo.rcMonitor.top as f64 / scale_factor) as u32,
        width: ((monitor_info.monitorInfo.rcMonitor.right - monitor_info.monitorInfo.rcMonitor.left)
            as f64
            / scale_factor) as u32,
        height: ((monitor_info.monitorInfo.rcMonitor.bottom
            - monitor_info.monitorInfo.rcMonitor.top) as f64
            / scale_factor) as u32,
        scale_factor: scale_factor,
    };
}
