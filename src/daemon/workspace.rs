use std::ptr;

use windows::Win32::Foundation::RECT;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, ShowWindow, SW_SHOWDEFAULT, SW_SHOWMAXIMIZED,
};
use winit::event_loop::EventLoop;
use winit::monitor::MonitorHandle;

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale_factor: f64,
}

pub fn get_logical_workspace_size() -> WorkspaceArea {
    let event_loop = EventLoop::new();
    let monitor_handle: MonitorHandle = event_loop
        .primary_monitor()
        .expect("Failed to determine primary monitor.");

    let scale_factor = monitor_handle.scale_factor();
    drop(monitor_handle);
    drop(event_loop);

    let hwnd = unsafe { GetConsoleWindow() };
    // Maximize the window to retrieve the correct workspace dimensions
    unsafe { ShowWindow(hwnd, SW_SHOWMAXIMIZED) };
    let mut window_rect = RECT::default();
    unsafe { GetWindowRect(hwnd, ptr::addr_of_mut!(window_rect)) };
    // Restore window to original show state
    unsafe { ShowWindow(hwnd, SW_SHOWDEFAULT) };

    return WorkspaceArea {
        x: (window_rect.left as f64 / scale_factor) as i32,
        y: (window_rect.top as f64 / scale_factor) as i32,
        width: ((window_rect.right - window_rect.left) as f64 / scale_factor) as i32,
        height: ((window_rect.bottom - window_rect.top) as f64 / scale_factor) as i32,
        scale_factor: scale_factor,
    };
}
