//! Tests for the recorder module.
//!
//! Only the trait-driven helpers are exercised here.
//! [`crate::demo::recorder::spawn_ffmpeg_gdigrab`] and
//! [`crate::demo::recorder::stop_ffmpeg_and_encode`] talk directly
//! to `std::process::Command` (they are only ever called from
//! [`crate::demo::RealSystem`]) and would require a real ffmpeg /
//! gifski to exercise; the trait-level callers in `mod.rs` cover
//! that path indirectly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mockall::mock;

use crate::demo::recorder::wait_for_capture_baseline;
use crate::demo::{DemoSystem, WindowInfo};

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
        fn is_sandbox_running(&self) -> bool;
        fn cargo_build_demo_artifacts(&self, workspace: &Path, target_dir: &Path) -> anyhow::Result<()>;
        fn print_info(&self, message: &str);
        fn print_debug(&self, message: &str);
    }
}

fn quiet_mock() -> MockDemoSystemMock {
    let mut mock = MockDemoSystemMock::new();
    mock.expect_print_info().returning(|_| ());
    mock.expect_print_debug().returning(|_| ());
    mock
}

#[test]
fn test_baseline_returns_when_size_threshold_reached() {
    // Arrange: ffmpeg writes the header on the second poll.
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| true);
    let polls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = polls.clone();
    mock.expect_file_size().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        // First poll: empty file. Second poll: well past 8 KiB.
        if *n == 1 {
            Ok(0)
        } else {
            Ok(64 * 1024)
        }
    });
    let sleeps: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let sleep_slot = sleeps.clone();
    mock.expect_sleep().returning(move |_| {
        *sleep_slot.lock().unwrap() += 1;
    });

    // Act
    let res = wait_for_capture_baseline(&mock, Path::new("/tmp/raw.mkv"));

    // Assert
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(*sleeps.lock().unwrap(), 1, "only one poll-sleep before hit");
}

#[test]
fn test_baseline_ignores_transient_size_failures() {
    // Arrange: simulate the Windows "ffmpeg holds an exclusive
    // write handle" case by returning Err on the first poll.
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| true);
    let polls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = polls.clone();
    mock.expect_file_size().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        if *n == 1 {
            Err(anyhow::anyhow!("ERROR_SHARING_VIOLATION"))
        } else {
            Ok(16 * 1024)
        }
    });
    mock.expect_sleep().returning(|_| ());

    // Act
    let res = wait_for_capture_baseline(&mock, Path::new("/tmp/raw.mkv"));

    // Assert
    assert!(res.is_ok(), "{res:?}");
}

#[test]
fn test_baseline_skips_polling_until_file_exists() {
    // Arrange: file appears on the third poll and is large enough.
    let mut mock = quiet_mock();
    let exist_polls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = exist_polls.clone();
    mock.expect_path_exists().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        *n >= 3
    });
    mock.expect_file_size().returning(|_| Ok(64 * 1024));
    mock.expect_sleep().returning(|_| ());

    // Act
    let res = wait_for_capture_baseline(&mock, Path::new("/tmp/raw.mkv"));

    // Assert
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(*exist_polls.lock().unwrap(), 3);
}
