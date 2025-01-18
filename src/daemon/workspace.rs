use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::Shell::GetScaleFactorForMonitor;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SystemParametersInfoW, SM_CXFIXEDFRAME, SM_CXSIZEFRAME, SM_CYFIXEDFRAME,
    SM_CYSIZEFRAME, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::utils::is_windows_10;

/// Possible scalings for any pixel related values
#[derive(Clone, Copy, Debug)]
pub enum Scaling {
    /// Pixel values represent the actual physical pixel of the screen.
    Physical,
    /// Pixel values are normalized. A scale factor needs to be applied to get physical values.
    Logical,
}

/// The available workspace area on the primary monitor
///
/// Also includes `fixed_frame` and `size_frame` attributes respecitvely indicating
/// the thickness of the frame around the perimeter of a window and the thickness
/// of the sizing border around the perimeter of a window.
///
/// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics
#[derive(Clone, Copy, Debug)]
pub struct WorkspaceArea {
    /// The `x` coordinate of the workspace area in pixels.
    /// From the top left of the screen.
    pub x: i32,
    /// The `y` coordinate of the workspace area in pixels.
    /// From the top left of the screen.
    pub y: i32,
    /// The width in pixels of the workspace area.
    pub width: i32,
    /// The height in pixels of the workspace area.
    pub height: i32,
    /// The scaling of the pixels. Logical or Physical
    pub scaling: Scaling,
    /// The thickness of the frame around the perimeter of a window on the x-axis.
    pub x_fixed_frame: i32,
    /// The thickness of the frame around the perimeter of a window on the y-axis.
    pub y_fixed_frame: i32,
    /// The thickness of the sizing border around the perimter of a window on the x-axis.
    pub x_size_frame: i32,
    /// The thickness of the sizing border around the perimter of a window on the y-axis.
    pub y_size_frame: i32,
    /// The scale factor of the primary monitor.
    scale_factor: f64,
}

impl WorkspaceArea {
    /// Returns the workspace area in logical scaling.
    pub fn logical(&self) -> WorkspaceArea {
        match self.scaling {
            Scaling::Logical => return *self,
            Scaling::Physical => return self.convert_scaling(),
        }
    }

    /// Converts physical to logical scaling.
    ///
    /// # Returns
    ///
    /// The workspace area in logical scaling.
    fn convert_scaling(&self) -> WorkspaceArea {
        let scale_factor = 1_f64 / self.scale_factor;
        let x = self.x as f64 * scale_factor;
        let y = self.y as f64 * scale_factor;
        let width = self.width as f64 * scale_factor;
        let height = self.height as f64 * scale_factor;
        return WorkspaceArea {
            x: x as i32,
            y: y as i32,
            width: width as i32,
            height: height as i32,
            scaling: Scaling::Logical,
            scale_factor,
            x_fixed_frame: self.x_fixed_frame,
            y_fixed_frame: self.y_fixed_frame,
            x_size_frame: self.x_size_frame,
            y_size_frame: self.y_size_frame,
        };
    }
}

/// Returns a handle to the primary monitor.
fn get_primary_monitor() -> HMONITOR {
    // By convention the primary monitor has it's upper left corner as 0,0.
    return unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
}

/// Returns the scaling factor of the primary monitor.
fn get_scale_factor() -> f64 {
    let scale_factor = unsafe {
        GetScaleFactorForMonitor(get_primary_monitor())
            .expect("Failed to retrieve scale factor for primary monitor")
            .0
    };
    // https://learn.microsoft.com/en-us/windows/win32/api/shtypes/ne-shtypes-device_scale_factor#constants
    return (scale_factor / 100).into();
}

/// Returns the available workspace area on the primary monitor in the specified scaling minus the space
/// occupied by the daemon console.
///
/// # Arguments
///
/// * `scaling`                 - The desired scaling for the workspace area. Physical or logical.
///                               This does not control which scaling is used by the system but only
///                               in which scalin the returned values are.
/// * `daemon_console_height`   - Height of the daemon console that will be substraced
///                               from the workspace area height.
///
/// # Returns
///
/// The available workspace area on the primary monitor in the specified scaling minus the space
/// occupied by the daemon console.
pub fn get_workspace_area(scaling: Scaling, daemon_console_height: i32) -> WorkspaceArea {
    let mut workspace_rect = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(ptr::addr_of_mut!(workspace_rect) as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .unwrap();
    }
    let x_fixed_frame = unsafe { GetSystemMetrics(SM_CXFIXEDFRAME) };
    let y_fixed_frame = unsafe { GetSystemMetrics(SM_CYFIXEDFRAME) };
    let x_size_frame = unsafe { GetSystemMetrics(SM_CXSIZEFRAME) };
    let y_size_frame = unsafe { GetSystemMetrics(SM_CYSIZEFRAME) };
    let workspace_area = WorkspaceArea {
        x: workspace_rect.left - (x_fixed_frame + x_size_frame),
        y: workspace_rect.top,
        width: workspace_rect.right - workspace_rect.left
            + (if is_windows_10() { -x_size_frame } else { 0 }),
        height: workspace_rect.bottom - workspace_rect.top - daemon_console_height,
        scaling: Scaling::Physical,
        scale_factor: get_scale_factor(),
        x_fixed_frame,
        y_fixed_frame,
        x_size_frame,
        y_size_frame,
    };
    match scaling {
        Scaling::Physical => return workspace_area,
        Scaling::Logical => return workspace_area.logical(),
    }
}
