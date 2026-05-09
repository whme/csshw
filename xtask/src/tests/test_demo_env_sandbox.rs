//! Tests for the sandbox env provider.
//!
//! These tests exercise the pure-string `.wsb` rendering and the
//! sentinel poll loop; the full `run` orchestration depends on
//! [`crate::demo::DemoSystem::spawn_sandbox`] which actually starts
//! `WindowsSandbox.exe` and is therefore covered indirectly only
//! (the side effect is mocked, but the real recording flow is
//! exercised end-to-end inside the sandbox itself).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mockall::mock;

use crate::demo::env::sandbox::{prepare_layout, render_wsb, wait_for_sentinel};
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
fn test_prepare_layout_resolves_known_paths_under_workspace() {
    // Arrange / Act
    let layout = prepare_layout(Path::new("C:\\ws"));

    // Assert
    let s = |p: &Path| p.display().to_string().replace('\\', "/");
    assert!(s(&layout.demo_root).ends_with("ws/target/demo"));
    assert!(s(&layout.bin_dir).ends_with("ws/target/demo/bin"));
    assert!(s(&layout.assets_dir).ends_with("ws/xtask/demo-assets"));
    assert!(s(&layout.out_dir).ends_with("ws/target/demo/out"));
    // work_dir lives under the writable out mount so files the
    // in-VM xtask writes (and the binaries the host builds for it)
    // surface on the host without an extra copy.
    assert!(s(&layout.work_dir).ends_with("ws/target/demo/out/work"));
    // build_target_dir is `<work_dir>/target` so cargo's debug exes
    // land at the path xtask's local provider expects
    // (`<workspace>/target/debug/csshw.exe`).
    assert!(s(&layout.build_target_dir).ends_with("ws/target/demo/out/work/target"));
    assert!(s(&layout.wsb_path).ends_with("ws/target/demo/csshw-demo.wsb"));
    assert!(s(&layout.sentinel).ends_with("ws/target/demo/out/done.flag"));
    assert!(s(&layout.sandbox_gif).ends_with("ws/target/demo/out/csshw.gif"));
}

#[test]
fn test_render_wsb_pins_mount_layout_and_logon_command() {
    // Arrange
    let layout = prepare_layout(Path::new("C:\\ws"));

    // Act
    let body = render_wsb(&layout, false);

    // Assert: every required mount point is present and routed
    // to the canonical sandbox-side path.
    assert!(body.contains("<Configuration>"), "{body}");
    assert!(body.contains("<MappedFolders>"), "{body}");
    assert!(
        body.contains("<SandboxFolder>C:\\demo\\bin</SandboxFolder>"),
        "{body}"
    );
    assert!(
        body.contains("<SandboxFolder>C:\\demo\\assets</SandboxFolder>"),
        "{body}"
    );
    assert!(
        body.contains("<SandboxFolder>C:\\demo\\out</SandboxFolder>"),
        "{body}"
    );
    // The workspace itself is intentionally not mounted: the host
    // builds the binaries directly into the writable out mount.
    assert!(
        !body.contains("<SandboxFolder>C:\\demo\\repo</SandboxFolder>"),
        "the legacy whole-workspace mount must not regress: {body}"
    );
    // The previous design carried a separate read-only stage mount;
    // the writable out mount now subsumes it.
    assert!(
        !body.contains("<SandboxFolder>C:\\demo\\stage</SandboxFolder>"),
        "the old stage mount must not reappear: {body}"
    );
    // The out folder is the only writable mount.
    let ro_count = body.matches("<ReadOnly>true</ReadOnly>").count();
    let rw_count = body.matches("<ReadOnly>false</ReadOnly>").count();
    assert_eq!(ro_count, 2, "expected 2 RO mounts: {body}");
    assert_eq!(rw_count, 1, "expected 1 RW mount: {body}");
    // LogonCommand routes through the bootstrap script.
    assert!(body.contains("<LogonCommand>"), "{body}");
    assert!(body.contains("sandbox-bootstrap.ps1"), "{body}");
    // Hardening attributes that should never silently regress.
    assert!(body.contains("<VGpu>Disable</VGpu>"), "{body}");
    assert!(
        body.contains("<ProtectedClient>Enable</ProtectedClient>"),
        "{body}"
    );
}

#[test]
fn test_render_wsb_passes_no_overlay_flag_when_set() {
    // Arrange
    let layout = prepare_layout(Path::new("C:\\ws"));

    // Act
    let with_flag = render_wsb(&layout, true);
    let without_flag = render_wsb(&layout, false);

    // Assert
    assert!(
        with_flag.contains("-NoOverlay"),
        "with-flag should pass -NoOverlay: {with_flag}"
    );
    assert!(
        !without_flag.contains("-NoOverlay"),
        "default render should not pass -NoOverlay: {without_flag}"
    );
}

#[test]
fn test_render_wsb_uses_workspace_host_path_for_out_mount() {
    // Arrange
    let layout = prepare_layout(Path::new("D:\\some place\\ws"));

    // Act
    let body = render_wsb(&layout, false);

    // Assert: the writable out mount carries the full host path
    // through unescaped (Windows paths cannot contain XML special
    // chars).
    assert!(
        body.contains("<HostFolder>D:\\some place\\ws\\target\\demo\\out</HostFolder>"),
        "host path leaks straight to XML: {body}"
    );
}

#[test]
fn test_wait_for_sentinel_returns_when_file_appears() {
    // Arrange: report missing for two polls then present. The
    // 20-second liveness grace window means is_sandbox_running is
    // never queried in this fast-success path.
    let mut mock = quiet_mock();
    let calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = calls.clone();
    mock.expect_path_exists().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        *n >= 3
    });
    let sleeps: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let sleep_slot = sleeps.clone();
    mock.expect_sleep().returning(move |_| {
        *sleep_slot.lock().unwrap() += 1;
    });

    // Act
    let res = wait_for_sentinel(&mock, Path::new("/dev/null/done.flag"));

    // Assert
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(*calls.lock().unwrap(), 3);
    // Two misses cause two sleeps; the third hit returns
    // immediately without sleeping.
    assert_eq!(*sleeps.lock().unwrap(), 2);
}

#[test]
fn test_wait_for_sentinel_bails_when_sandbox_closes_after_grace_window() {
    // Arrange: the sentinel never appears. is_sandbox_running is
    // only consulted after the grace window of poll iterations,
    // which the mocked sleeps make zero-cost in wall-clock terms.
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| false);
    mock.expect_sleep().returning(|_| ());
    let liveness_calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let liveness_slot = liveness_calls.clone();
    mock.expect_is_sandbox_running().returning(move || {
        *liveness_slot.lock().unwrap() += 1;
        false
    });

    // Act
    let res = wait_for_sentinel(&mock, Path::new("/dev/null/done.flag"));

    // Assert
    let err = res.expect_err("expected an error when the sandbox disappears");
    let msg = err.to_string();
    assert!(
        msg.contains("sandbox VM is no longer running"),
        "error should explain the sandbox closure: {msg}"
    );
    // Liveness is queried exactly once - the first check after the
    // grace window fires the bail.
    assert_eq!(*liveness_calls.lock().unwrap(), 1);
}
