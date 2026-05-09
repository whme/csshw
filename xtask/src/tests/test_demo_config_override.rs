//! Tests for the `csshw-config.toml` override generator.
//!
//! Asserts the generator (a) writes one config file plus per-host
//! enter.bat files, (b) only writes host-specific files for the
//! intended host, and (c) emits a TOML body that targets cmd.exe via
//! a single `dispatcher.bat`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mockall::mock;

use crate::demo::{config_override, DemoSystem, WindowInfo};

mock! {
    DemoSystemMock {}
    impl DemoSystem for DemoSystemMock {
        fn workspace_root(&self) -> anyhow::Result<PathBuf>;
        fn ensure_dir(&self, path: &Path) -> anyhow::Result<()>;
        fn write_file(&self, path: &Path, content: &str) -> anyhow::Result<()>;
        fn copy_file(&self, from: &Path, to: &Path) -> anyhow::Result<()>;
        fn file_exists(&self, path: &Path) -> bool;
        fn enum_windows(&self) -> anyhow::Result<Vec<WindowInfo>>;
        fn set_foreground(&self, hwnd: u64) -> anyhow::Result<()>;
        fn send_unicode_char(&self, c: char) -> anyhow::Result<()>;
        fn send_vk(&self, vk: u16) -> anyhow::Result<()>;
        fn sleep(&self, duration: Duration);
        fn spawn_csshw(&self, exe: &Path, hosts: &[String], cwd: &Path) -> anyhow::Result<()>;
        fn terminate_csshw(&self) -> anyhow::Result<()>;
        fn start_recording(&self, out_raw: &Path) -> anyhow::Result<()>;
        fn stop_recording(&self, out_raw: &Path, out_gif: &Path) -> anyhow::Result<()>;
        fn print_info(&self, message: &str);
        fn print_debug(&self, message: &str);
    }
}

#[derive(Default, Clone)]
struct WriteCapture {
    files: Vec<(PathBuf, String)>,
}

fn capturing_mock() -> (MockDemoSystemMock, Arc<Mutex<WriteCapture>>) {
    let cap: Arc<Mutex<WriteCapture>> = Arc::new(Mutex::new(WriteCapture::default()));
    let mut mock = MockDemoSystemMock::new();
    mock.expect_ensure_dir().returning(|_| Ok(()));
    let slot = cap.clone();
    mock.expect_write_file().returning(move |p, c| {
        slot.lock()
            .unwrap()
            .files
            .push((p.to_path_buf(), c.to_string()));
        Ok(())
    });
    (mock, cap)
}

#[test]
fn test_generate_writes_config_and_per_host_bat() {
    // Arrange
    let (mock, cap) = capturing_mock();
    let demo_root = PathBuf::from("/demo");

    // Act
    let layout = config_override::generate(&mock, &demo_root, &["alpha", "bravo"]).unwrap();

    // Assert
    assert_eq!(layout.csshw_cwd, demo_root);
    let files = cap.lock().unwrap().files.clone();
    let names: Vec<String> = files
        .iter()
        .map(|(p, _)| p.display().to_string().replace('\\', "/"))
        .collect();
    assert!(names.iter().any(|n| n.ends_with("/csshw-config.toml")));
    assert!(names.iter().any(|n| n.ends_with("/dispatcher.bat")));
    assert!(names
        .iter()
        .any(|n| n.ends_with("fakehosts/alpha/enter.bat")));
    assert!(names
        .iter()
        .any(|n| n.ends_with("fakehosts/bravo/enter.bat")));
    // Shared README is written for both hosts.
    assert_eq!(
        names.iter().filter(|n| n.ends_with("README.txt")).count(),
        2
    );
}

#[test]
fn test_generate_writes_host_specific_file_only_for_owning_host() {
    // Arrange
    let (mock, cap) = capturing_mock();

    // Act
    config_override::generate(&mock, Path::new("/demo"), &["alpha", "charlie"]).unwrap();

    // Assert
    let files = cap.lock().unwrap().files.clone();
    let secret_writes: Vec<_> = files
        .iter()
        .filter(|(p, _)| p.display().to_string().ends_with("secret.txt"))
        .collect();
    assert_eq!(secret_writes.len(), 1, "secret.txt should appear once");
    let path = secret_writes[0].0.display().to_string().replace('\\', "/");
    assert!(path.contains("fakehosts/charlie/"), "got: {path}");
}

#[test]
fn test_generated_toml_targets_cmd_exe_via_dispatcher() {
    // Arrange
    let (mock, cap) = capturing_mock();

    // Act
    config_override::generate(&mock, Path::new("/demo"), &["alpha"]).unwrap();

    // Assert
    let files = cap.lock().unwrap().files.clone();
    let toml = files
        .iter()
        .find(|(p, _)| p.display().to_string().ends_with("csshw-config.toml"))
        .map(|(_, c)| c.clone())
        .expect("csshw-config.toml not written");
    assert!(toml.contains("program = \"cmd.exe\""), "toml: {toml}");
    assert!(
        toml.contains("{{USERNAME_AT_HOST}}"),
        "placeholder missing - toml: {toml}"
    );
    assert!(toml.contains("dispatcher.bat"), "toml: {toml}");
}

#[test]
fn test_dispatcher_bat_strips_user_prefix() {
    // Arrange
    let (mock, cap) = capturing_mock();

    // Act
    config_override::generate(&mock, Path::new("/demo"), &["alpha"]).unwrap();

    // Assert
    let files = cap.lock().unwrap().files.clone();
    let dispatcher = files
        .iter()
        .find(|(p, _)| p.display().to_string().ends_with("dispatcher.bat"))
        .map(|(_, c)| c.clone())
        .expect("dispatcher.bat not written");
    // Must use cmd's `:*@=` substring substitution. The
    // `for /f tokens=2 delims=@` form skips a leading `@` and
    // produces only one token, leaving HOST as `@alpha` and
    // breaking the call below with "the system cannot find the
    // path specified".
    assert!(
        dispatcher.contains(":*@="),
        "dispatcher should use substring substitution: {dispatcher}"
    );
    assert!(
        !dispatcher.contains("delims=@"),
        "dispatcher must not use `for /f delims=@` (mishandles leading @): {dispatcher}"
    );
    assert!(dispatcher.contains("fakehosts"), "dispatcher: {dispatcher}");
    assert!(dispatcher.contains("enter.bat"), "dispatcher: {dispatcher}");
}

#[test]
fn test_enter_bat_sets_prompt_and_cd() {
    // Arrange
    let (mock, cap) = capturing_mock();

    // Act
    config_override::generate(&mock, Path::new("/demo"), &["alpha"]).unwrap();

    // Assert
    let files = cap.lock().unwrap().files.clone();
    let bat = files
        .iter()
        .find(|(p, _)| p.display().to_string().ends_with("enter.bat"))
        .map(|(_, c)| c.clone())
        .expect("enter.bat not written");
    assert!(bat.contains("set PROMPT="), "bat: {bat}");
    assert!(bat.contains("cd /d"), "bat: {bat}");
    assert!(bat.contains("alpha"), "bat: {bat}");
}
