//! Tests for the demo driver.
//!
//! All side effects route through the [`DemoSystem`] trait, so the
//! driver is fully mockable without any Windows API or filesystem
//! contact.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mockall::mock;

use crate::demo::dsl::Step;
use crate::demo::{driver, DemoSystem, WindowInfo, WindowRect};

mock! {
    DemoSystemMock {}
    impl DemoSystem for DemoSystemMock {
        fn workspace_root(&self) -> anyhow::Result<PathBuf>;
        fn ensure_dir(&self, path: &Path) -> anyhow::Result<()>;
        fn write_file(&self, path: &Path, content: &str) -> anyhow::Result<()>;
        fn copy_file(&self, from: &Path, to: &Path) -> anyhow::Result<()>;
        fn enum_windows(&self) -> anyhow::Result<Vec<WindowInfo>>;
        fn set_foreground(&self, hwnd: u64) -> anyhow::Result<()>;
        fn send_unicode_char(&self, c: char) -> anyhow::Result<()>;
        fn send_vk(&self, vk: u16) -> anyhow::Result<()>;
        fn sleep(&self, duration: Duration);
        fn spawn_csshw(&self, exe: &Path, hosts: &[String], cwd: &Path) -> anyhow::Result<()>;
        fn terminate_csshw(&self) -> anyhow::Result<()>;
        fn start_recording(&self, out_raw: &Path) -> anyhow::Result<()>;
        fn stop_recording(&self, out_raw: &Path, out_gif: &Path) -> anyhow::Result<()>;
        fn path_exists(&self, path: &Path) -> bool;
        fn file_size(&self, path: &Path) -> anyhow::Result<u64>;
        fn http_download(&self, url: &str, dest: &Path) -> anyhow::Result<()>;
        fn sha256_file(&self, path: &Path) -> anyhow::Result<String>;
        fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> anyhow::Result<()>;
        fn spawn_sandbox(&self, wsb_path: &Path) -> anyhow::Result<()>;
        fn terminate_sandbox(&self) -> anyhow::Result<()>;
        fn print_info(&self, message: &str);
        fn print_debug(&self, message: &str);
    }
}

/// Build a mock with no-op `print_*` and `sleep` so callers only set
/// expectations on the calls they actually want to assert.
fn base_mock() -> MockDemoSystemMock {
    let mut mock = MockDemoSystemMock::new();
    mock.expect_print_info().returning(|_| ());
    mock.expect_print_debug().returning(|_| ());
    mock.expect_sleep().returning(|_| ());
    mock
}

/// Single window with a stable rect, used by tests that expect
/// `WaitForWindow` and `Focus` to succeed on the first poll.
fn one_window(title: &str) -> Vec<WindowInfo> {
    vec![WindowInfo {
        hwnd: 0xDEAD,
        title: title.to_string(),
        rect: WindowRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    }]
}

#[test]
fn test_no_record_skips_capture_calls() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_start_recording().times(0);
    mock.expect_stop_recording().times(0);
    let steps = vec![
        Step::StartCapture,
        Step::Sleep(Duration::from_millis(1)),
        Step::StopCapture,
    ];

    // Act
    let res = driver::run(&mock, &steps, Path::new("ignored.gif"), true);

    // Assert
    assert!(res.is_ok());
}

#[test]
fn test_capture_calls_are_paired_when_recording() {
    // Arrange
    let mut mock = base_mock();
    let captured_raw: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let captured_gif: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let raw_slot = captured_raw.clone();
    mock.expect_start_recording().times(1).returning(move |p| {
        *raw_slot.lock().unwrap() = Some(p.to_path_buf());
        Ok(())
    });
    let gif_slot = captured_gif.clone();
    mock.expect_stop_recording()
        .times(1)
        .returning(move |_raw, gif| {
            *gif_slot.lock().unwrap() = Some(gif.to_path_buf());
            Ok(())
        });
    let steps = vec![Step::StartCapture, Step::StopCapture];

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    assert!(res.is_ok());
    assert_eq!(
        captured_raw.lock().unwrap().as_deref(),
        Some(Path::new("/x/csshw.mkv"))
    );
    assert_eq!(
        captured_gif.lock().unwrap().as_deref(),
        Some(Path::new("/x/csshw.gif"))
    );
}

#[test]
fn test_capture_is_cleaned_up_on_step_error() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_start_recording().times(1).returning(|_| Ok(()));
    // The Type step below will fail because send_unicode_char errors;
    // the driver MUST still call stop_recording.
    mock.expect_stop_recording()
        .times(1)
        .returning(|_, _| Ok(()));
    mock.expect_send_unicode_char()
        .returning(|_| Err(anyhow::anyhow!("simulated failure")));
    let steps = vec![
        Step::StartCapture,
        Step::Type {
            text: "x".into(),
            per_char_delay: Duration::from_millis(0),
        },
        Step::StopCapture,
    ];

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("Type") || err.contains("simulated"),
        "got: {err}"
    );
}

#[test]
fn test_wait_for_window_succeeds_when_match_appears() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_enum_windows()
        .returning(|| Ok(one_window("daemon [csshw]")));
    let steps = vec![
        Step::StartCapture,
        Step::WaitForWindow {
            title_regex: r"(?i)daemon".to_string(),
            timeout: Duration::from_millis(500),
            stable_for: Duration::from_millis(0),
        },
        Step::StopCapture,
    ];
    mock.expect_start_recording().returning(|_| Ok(()));
    mock.expect_stop_recording().returning(|_, _| Ok(()));

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    assert!(res.is_ok(), "{res:?}");
}

#[test]
fn test_wait_for_window_times_out_when_no_match() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_enum_windows()
        .returning(|| Ok(one_window("not the right window")));
    let steps = vec![
        Step::StartCapture,
        Step::WaitForWindow {
            title_regex: r"(?i)daemon".to_string(),
            timeout: Duration::from_millis(50),
            stable_for: Duration::from_millis(0),
        },
        Step::StopCapture,
    ];
    mock.expect_start_recording().returning(|_| Ok(()));
    mock.expect_stop_recording().returning(|_, _| Ok(()));

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    let err = res.expect_err("expected timeout").to_string();
    assert!(
        err.contains("WaitForWindow") || err.contains("stabilised"),
        "got: {err}"
    );
}

#[test]
fn test_focus_calls_set_foreground_with_matching_hwnd() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_enum_windows()
        .returning(|| Ok(one_window("alpha@alpha-fake")));
    let captured_hwnd: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let slot = captured_hwnd.clone();
    mock.expect_set_foreground().times(1).returning(move |h| {
        *slot.lock().unwrap() = Some(h);
        Ok(())
    });
    let steps = vec![
        Step::StartCapture,
        Step::Focus {
            title_regex: r"(?i)alpha".to_string(),
        },
        Step::StopCapture,
    ];
    mock.expect_start_recording().returning(|_| Ok(()));
    mock.expect_stop_recording().returning(|_, _| Ok(()));

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    assert!(res.is_ok());
    assert_eq!(*captured_hwnd.lock().unwrap(), Some(0xDEAD));
}

#[test]
fn test_type_text_translates_newline_to_vk_return() {
    // Arrange
    let mut mock = base_mock();
    let unicode_chars: Arc<Mutex<Vec<char>>> = Arc::new(Mutex::new(Vec::new()));
    let vk_codes: Arc<Mutex<Vec<u16>>> = Arc::new(Mutex::new(Vec::new()));
    let cs = unicode_chars.clone();
    mock.expect_send_unicode_char().returning(move |c| {
        cs.lock().unwrap().push(c);
        Ok(())
    });
    let vs = vk_codes.clone();
    mock.expect_send_vk().returning(move |vk| {
        vs.lock().unwrap().push(vk);
        Ok(())
    });
    let steps = vec![
        Step::StartCapture,
        Step::Type {
            text: "ab\r".into(),
            per_char_delay: Duration::from_millis(0),
        },
        Step::StopCapture,
    ];
    mock.expect_start_recording().returning(|_| Ok(()));
    mock.expect_stop_recording().returning(|_, _| Ok(()));

    // Act
    let res = driver::run(&mock, &steps, Path::new("/x/csshw.gif"), false);

    // Assert
    assert!(res.is_ok());
    assert_eq!(*unicode_chars.lock().unwrap(), vec!['a', 'b']);
    // 0x0D is VK_RETURN.
    assert_eq!(*vk_codes.lock().unwrap(), vec![0x0Du16]);
}
