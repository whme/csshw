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

#[derive(Clone, Copy, Debug)]
pub enum Scaling {
    Physical,
    Logical,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scaling: Scaling,
    pub x_fixed_frame: i32,
    pub y_fixed_frame: i32,
    pub x_size_frame: i32,
    pub y_size_frame: i32,
    scale_factor: f64,
}

impl WorkspaceArea {
    pub fn logical(&self) -> WorkspaceArea {
        match self.scaling {
            Scaling::Logical => return *self,
            Scaling::Physical => return self.convert_scaling(),
        }
    }

    #[allow(dead_code)]
    pub fn physical(&self) -> WorkspaceArea {
        match self.scaling {
            Scaling::Logical => return self.convert_scaling(),
            Scaling::Physical => return *self,
        }
    }

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

fn get_primary_monitor() -> HMONITOR {
    // By convention the primary monitor has it's upper left corner as 0,0.
    return unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
}

fn get_scale_factor() -> f64 {
    let scale_factor = unsafe {
        GetScaleFactorForMonitor(get_primary_monitor())
            .expect("Failed to retrieve scale factor for primary monitor")
            .0
    };
    // https://learn.microsoft.com/en-us/windows/win32/api/shtypes/ne-shtypes-device_scale_factor#constants
    return (scale_factor / 100).into();
}

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
