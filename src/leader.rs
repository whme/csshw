use std::mem::size_of;

use windows::Win32::Foundation::HWND;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Threading::*;

use whoami::username;

const SCREEN_HEIGHT: u32 = 2160;
const SCREEN_WIDTH: u32 = 3840;

struct FollowerInformation {
    process_information: PROCESS_INFORMATION,
    window_title: (PWSTR, Vec<u16>),
    hwnd: HWND,
}

#[derive(Debug)]
pub struct Leader {
    pub hosts: Vec<String>,
    pub wsl_distro: String,
}

impl Leader {
    pub fn launch_followers(&self) {
        let number_of_windows = u32::try_from(self.hosts.len()).expect("Too many hosts");

        // TODO: contribute setting lpTitle startupinfo (or entire startupinfo)
        // to rust Command.
        // https://github.dev/rust-lang/rust/blob/adb4bfd25d3c1190b0e7433ef945221d8aeea427/library/std/src/sys/windows/process.rs#L330

        let mut i: u32 = 0;
        let mut followers: Vec<FollowerInformation> = Vec::new();
        for host in self.hosts.iter() {
            let cmd = format!(
                "{} run python3 /home/d070791/dissh_test.py {}",
                self.wsl_distro, host
            )
            .into_pwstr();
            let window_title = format!("dissh - {}@{}", username(), host).into_pwstr();
            let mut startupinfo = STARTUPINFOW::default();
            startupinfo.cb = size_of::<STARTUPINFOW>() as u32;
            startupinfo.lpTitle = window_title.0;
            startupinfo.dwX = 0;
            startupinfo.dwY = 0 + i * (SCREEN_HEIGHT / (number_of_windows + 1));
            startupinfo.dwXSize = SCREEN_WIDTH;
            startupinfo.dwYSize = (SCREEN_HEIGHT / (number_of_windows + 1)).into();
            startupinfo.dwFlags = STARTF_USEPOSITION | STARTF_USESIZE;
            let mut process_information = PROCESS_INFORMATION::default();
            unsafe {
                CreateProcessW(
                    PCWSTR::null(),
                    cmd.0,
                    None,
                    None,
                    false,
                    CREATE_NEW_CONSOLE,
                    None,
                    PCWSTR::null(),
                    &startupinfo,
                    &mut process_information,
                )
                .expect("Failed to create process");
                followers.push(FollowerInformation {
                    process_information: process_information,
                    window_title: window_title,
                    hwnd: HWND::default(),
                });
            }
            i += 1;
        }
    }
}

trait IntoPWSTR {
    fn into_pwstr(self) -> (PWSTR, Vec<u16>);
}

impl IntoPWSTR for &str {
    fn into_pwstr(self) -> (PWSTR, Vec<u16>) {
        let mut encoded = self.encode_utf16().chain([0u16]).collect::<Vec<u16>>();

        (PWSTR(encoded.as_mut_ptr()), encoded)
    }
}
impl IntoPWSTR for String {
    fn into_pwstr(self) -> (PWSTR, Vec<u16>) {
        let mut encoded = self.encode_utf16().chain([0u16]).collect::<Vec<u16>>();

        (PWSTR(encoded.as_mut_ptr()), encoded)
    }
}
