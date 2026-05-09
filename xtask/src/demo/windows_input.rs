//! Thin wrappers around the Win32 input + window-enumeration APIs.
//!
//! Kept private to [`crate::demo`] so the rest of the module tree never
//! touches `unsafe`. Only [`crate::demo::RealSystem`] calls in here.
//! All functions return `anyhow::Error` instead of `windows::core::Error`
//! so callers compose with the rest of xtask uniformly.
//!
//! Non-Windows builds still compile (xtask is a workspace member) by
//! returning a clear "Windows-only" error from each entry point.

use anyhow::Result;

use super::{WindowInfo, WindowRect};

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::ffi::c_void;

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::System::Threading::AttachThreadInput;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    /// Closure-based EnumWindows callback context.
    ///
    /// We accumulate visible top-level windows with non-empty titles
    /// into a `Vec<WindowInfo>` passed via `LPARAM`.
    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is a `*mut Vec<WindowInfo>` we set in
        // enum_windows() below. The pointer is valid for the duration
        // of the EnumWindows call.
        let acc = unsafe { &mut *(lparam.0 as *mut Vec<WindowInfo>) };
        // SAFETY: HWND is valid for the duration of this callback.
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        if !visible {
            return BOOL(1);
        }
        // SAFETY: HWND valid; returns text length without trailing NUL.
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return BOOL(1);
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        // SAFETY: HWND valid; buffer length matches the slot count.
        let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if copied <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        let mut rect = RECT::default();
        // SAFETY: HWND valid; rect is a stack RECT we own.
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return BOOL(1);
        }
        acc.push(WindowInfo {
            hwnd: hwnd.0 as u64,
            title,
            rect: WindowRect {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
        });
        BOOL(1)
    }

    /// Enumerate visible top-level windows with non-empty titles.
    pub fn enum_windows() -> Result<Vec<WindowInfo>> {
        let mut acc: Vec<WindowInfo> = Vec::new();
        let lparam = LPARAM(&mut acc as *mut _ as isize);
        // SAFETY: enum_proc is a valid extern "system" callback;
        // EnumWindows blocks until iteration completes so `acc` stays
        // valid for the entire call.
        unsafe { EnumWindows(Some(enum_proc), lparam) }
            .map_err(|e| anyhow::anyhow!("EnumWindows failed: {e}"))?;
        Ok(acc)
    }

    /// Bring the window to the foreground using the standard
    /// `AttachThreadInput` workaround (Windows blocks
    /// `SetForegroundWindow` from background processes since Win2K).
    pub fn set_foreground(hwnd: u64) -> Result<()> {
        let target = HWND(hwnd as *mut c_void);
        // SAFETY: HWND value originates from a recent enum_windows()
        // call. Worst case it has been destroyed and the API returns
        // an error, which we propagate.
        let foreground = unsafe { GetForegroundWindow() };
        let mut fg_thread = 0u32;
        // SAFETY: foreground is the current foreground window handle
        // from the OS; the out-pointer is a stack u32.
        let _ = unsafe { GetWindowThreadProcessId(foreground, Some(&mut fg_thread)) };
        let mut target_thread = 0u32;
        // SAFETY: target is the window we want to focus; out-pointer
        // is a stack u32.
        let _ = unsafe { GetWindowThreadProcessId(target, Some(&mut target_thread)) };
        let attached = if fg_thread != 0 && target_thread != 0 && fg_thread != target_thread {
            // SAFETY: thread IDs come from GetWindowThreadProcessId.
            unsafe { AttachThreadInput(fg_thread, target_thread, true) }.as_bool()
        } else {
            false
        };
        // SAFETY: HWND validated at top of function.
        let ok = unsafe { SetForegroundWindow(target) }.as_bool();
        if attached {
            // SAFETY: must mirror the AttachThreadInput call above.
            let _ = unsafe { AttachThreadInput(fg_thread, target_thread, false) };
        }
        if !ok {
            anyhow::bail!("SetForegroundWindow returned FALSE for hwnd={hwnd:#x}");
        }
        Ok(())
    }

    /// Send a single Unicode codepoint via `SendInput(KEYEVENTF_UNICODE)`.
    pub fn send_unicode_char(c: char) -> Result<()> {
        // BMP characters fit in a single u16; supplementary plane
        // codepoints need surrogate pairs. We synthesise both halves
        // when needed.
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        for unit in units.iter().copied() {
            push_unicode(unit)?;
        }
        Ok(())
    }

    /// Send VK_DOWN + VK_UP for a single Unicode code unit.
    fn push_unicode(unit: u16) -> Result<()> {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_pair(&[down, up])
    }

    /// Send a virtual-key down + up pair.
    pub fn send_vk(vk: u16) -> Result<()> {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_pair(&[down, up])
    }

    fn send_pair(events: &[INPUT]) -> Result<()> {
        // SAFETY: events is a valid slice; SendInput reads `len`
        // entries each of size_of::<INPUT>().
        let sent = unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != events.len() {
            anyhow::bail!(
                "SendInput injected {sent}/{} events; the input desktop may be locked",
                events.len()
            );
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub(super) use imp::{enum_windows, send_unicode_char, send_vk, set_foreground};

#[cfg(not(target_os = "windows"))]
mod imp_stub {
    use super::*;

    /// Stub that errors on non-Windows hosts. The demo subcommand is
    /// Windows-only; this stub exists so `cargo check` on Linux still
    /// compiles the rest of the workspace.
    fn unsupported<T>() -> Result<T> {
        anyhow::bail!("record-demo is Windows-only; this is a non-Windows build")
    }

    pub fn enum_windows() -> Result<Vec<WindowInfo>> {
        unsupported()
    }
    pub fn set_foreground(_hwnd: u64) -> Result<()> {
        unsupported()
    }
    pub fn send_unicode_char(_c: char) -> Result<()> {
        unsupported()
    }
    pub fn send_vk(_vk: u16) -> Result<()> {
        unsupported()
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) use imp_stub::{enum_windows, send_unicode_char, send_vk, set_foreground};
