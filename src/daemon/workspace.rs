use crate::utils::windows::WindowsApi;
use windows::Win32::UI::WindowsAndMessaging::{
    SM_CXFIXEDFRAME, SM_CXMAXIMIZED, SM_CXSIZEFRAME, SM_CYFIXEDFRAME, SM_CYMAXIMIZED,
    SM_CYSIZEFRAME,
};

/// The available workspace area on the primary monitor
///
/// Also includes `fixed_frame` and `size_frame` attributes respecitvely indicating
/// the thickness of the frame around the perimeter of a window and the thickness
/// of the sizing border around the perimeter of a window.
///
/// <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics>
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
    /// The thickness of the frame around the perimeter of a window on the x-axis.
    pub x_fixed_frame: i32,
    /// The thickness of the frame around the perimeter of a window on the y-axis.
    pub y_fixed_frame: i32,
    /// The thickness of the sizing border around the perimter of a window on the x-axis.
    pub x_size_frame: i32,
    /// The thickness of the sizing border around the perimter of a window on the y-axis.
    pub y_size_frame: i32,
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
pub fn get_workspace_area<W: WindowsApi>(
    windows_api: &W,
    daemon_console_height: i32,
) -> WorkspaceArea {
    let workspace_width = windows_api.get_system_metrics(SM_CXMAXIMIZED) - 1;
    let workspace_height = windows_api.get_system_metrics(SM_CYMAXIMIZED) - 1;
    let x_fixed_frame = windows_api.get_system_metrics(SM_CXFIXEDFRAME);
    let y_fixed_frame = windows_api.get_system_metrics(SM_CYFIXEDFRAME);
    let x_size_frame = windows_api.get_system_metrics(SM_CXSIZEFRAME);
    let y_size_frame = windows_api.get_system_metrics(SM_CYSIZEFRAME);
    return WorkspaceArea {
        x: 0,
        y: 0,
        width: workspace_width,
        height: workspace_height - daemon_console_height,
        x_fixed_frame,
        y_fixed_frame,
        x_size_frame,
        y_size_frame,
    };
}

#[cfg(test)]
#[path = "../tests/daemon/test_workspace.rs"]
mod test_mod;
