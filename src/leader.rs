use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::core::{PWSTR};

use windows::Win32::System::Threading::CreateProcessA;

use windows::Win32::UI::WindowsAndMessaging::*;

const CREATE_NEW_CONSOLE: u32 = 0x00000010;

const SCREEN_HEIGHT: i16 = 2160;
const SCREEN_WIDTH: i16 = 3840;

#[derive(Debug)]
pub struct Leader {
    pub hosts: Vec<String>,
}

impl Leader {
    pub unsafe fn launch_followers(&self) {
        let number_of_windows = self.hosts.len();

        // TODO: contribute setting lpTitle startupinfo (or entire startupinfo)
        // to rust Command.
        // https://github.dev/rust-lang/rust/blob/adb4bfd25d3c1190b0e7433ef945221d8aeea427/library/std/src/sys/windows/process.rs#L330

        let mut followers: Vec<Child> = Vec::new();
        for host in self.hosts.iter() {
            let sub_process = CreateProcessA(
                PWSTR::default(),
                &format!("run 'python3 ~/dissh_test.py {};'", host),
                0,
                0,
                false,
                CREATE_NEW_CONSOLE,
                0,
                0,
                0,
                0,
            );

            let sub_command = Command::new("ubuntu")
                .args(&["run", &format!("python3 ~/dissh_test.py {};", host)])
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn()
                .expect("Failed to start ubuntu");
            followers.push(sub_command);
        }

        let mut i: i16 = 0;
        while let Some(follower) = followers.pop() {
            let hwnd = id_to_hwnd(follower.id());

            println!("{:?}", hwnd);

            unsafe {
                SetWindowPos(
                    hwnd,
                    hwnd,
                    0,
                    i32::from(0 + i * (SCREEN_HEIGHT / 3)),
                    SCREEN_WIDTH.into(),
                    (SCREEN_HEIGHT / 3).into(),
                    SWP_NOACTIVATE,
                );
            }

            i += 1;

            // let output = follower
            //     .wait_with_output()
            //     .expect("Failed to wait on ubuntu");
            // println!("{:?}", output.stdout);
            // println!("{:?}", output.stderr);
            // println!("{:?}", output.status.code());
        }
    }
}

#[derive(Debug)]

struct Data {
    id: u32,
    hwnd: HWND,
}

fn id_to_hwnd(id: u32) -> HWND {
    let mut data = Box::new(Data {
        id: id,
        hwnd: HWND(0),
    });

    unsafe {
        // TODO: instead of sleeping here, read from the processes stdout
        // to make sure the window has been opened
        std::thread::sleep(std::time::Duration::from_millis(2000));

        let handle_ptr: *mut Data = &mut *data;

        EnumWindows(Some(callback), LPARAM(handle_ptr as isize));
    };

    return data.hwnd;
}

unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut id = 0;

    let mut data: &mut Data = &mut *(lparam.0 as *mut Data);

    println!("{:?}", hwnd);

    id = GetWindowThreadProcessId(hwnd, Some(&mut id));

    println!("{:?}", id);

    if id == data.id.try_into().unwrap() && is_main_window(hwnd) {
        data.hwnd = hwnd;

        return BOOL(0);
    }

    return BOOL(1);
}

unsafe extern "system" fn is_main_window(handle: HWND) -> bool {
    return GetWindow(handle, GW_OWNER) == HWND(0) && IsWindowVisible(handle) == true;
}
