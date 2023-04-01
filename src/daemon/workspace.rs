use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::Shell::GetScaleFactorForMonitor;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

#[derive(Clone, Copy, Debug)]
pub enum Scaling {
    PHYSICAL,
    LOGICAL,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scaling: Scaling,
    scale_factor: f64,
}

impl WorkspaceArea {
    pub fn logical(&self) -> WorkspaceArea {
        match self.scaling {
            Scaling::LOGICAL => return self.clone(),
            Scaling::PHYSICAL => return self.convert_scaling(),
        }
    }

    pub fn physical(&self) -> WorkspaceArea {
        match self.scaling {
            Scaling::LOGICAL => return self.convert_scaling(),
            Scaling::PHYSICAL => return self.clone(),
        }
    }

    fn convert_scaling(&self) -> WorkspaceArea {
        let scale_factor = 1 as f64 / self.scale_factor;
        let x = self.x as f64 * scale_factor;
        let y = self.y as f64 * scale_factor;
        let width = self.width as f64 * scale_factor;
        let height = self.height as f64 * scale_factor;
        return WorkspaceArea {
            x: x as i32,
            y: y as i32,
            width: width as i32,
            height: height as i32,
            scaling: Scaling::LOGICAL,
            scale_factor: scale_factor,
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

pub fn get_workspace_area(scaling: Scaling) -> WorkspaceArea {
    let mut workspace_rect = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(ptr::addr_of_mut!(workspace_rect) as *const c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    let workspace_area = WorkspaceArea {
        x: workspace_rect.left,
        y: workspace_rect.top,
        width: workspace_rect.right - workspace_rect.left,
        height: workspace_rect.bottom - workspace_rect.top,
        scaling: Scaling::PHYSICAL,
        scale_factor: get_scale_factor(),
    };
    match scaling {
        Scaling::PHYSICAL => return workspace_area,
        Scaling::LOGICAL => return workspace_area.logical(),
    }
}
