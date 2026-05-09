//! Tests for the vendored binary cache module.
//!
//! All side effects (download, hash, extract, fs) flow through
//! [`crate::demo::DemoSystem`], so the cache logic in
//! [`crate::demo::bin`] is exercised against `mockall`-generated
//! mocks with zero network or filesystem effects. The tests focus
//! on the state-machine: cache hit fast path, cold-cache happy
//! path, SHA mismatch, post-extract entry-binary check, and the
//! nested-archive flow Carnac relies on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mockall::mock;

use crate::demo::bin::{ensure_pin, Pin};
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
        fn cargo_build_csshw(&self, workspace: &Path) -> anyhow::Result<()>;
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

const FAKE: Pin = Pin {
    name: "fake",
    url: "https://example.test/fake.zip",
    sha256: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    archive_name: "fake.zip",
    exe_rel: "bin/fake.exe",
    inner_archive: None,
};

const FAKE_NESTED: Pin = Pin {
    name: "fake_nested",
    url: "https://example.test/outer.zip",
    sha256: "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    archive_name: "outer.zip",
    exe_rel: "lib/net45/Inner.exe",
    inner_archive: Some("inner.nupkg"),
};

#[test]
fn test_cache_hit_skips_download_and_extract() {
    // Arrange: the entry binary is already present, so ensure_pin
    // must not touch the network or invoke extract.
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| true);
    mock.expect_http_download().times(0);
    mock.expect_sha256_file().times(0);
    mock.expect_extract_archive().times(0);
    mock.expect_ensure_dir().times(0);

    // Act
    let path = ensure_pin(&mock, &FAKE, Path::new("/cache")).unwrap();

    // Assert
    let s = path.display().to_string().replace('\\', "/");
    assert!(s.ends_with("fake/bin/fake.exe"), "got {s}");
}

#[test]
fn test_cold_cache_downloads_verifies_extracts_and_returns_path() {
    // Arrange: first path_exists check (entry exe) returns false,
    // second (after extract) returns true. http_download writes the
    // archive, sha256 matches the pin, extract_archive succeeds.
    let mut mock = quiet_mock();
    let exists_calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = exists_calls.clone();
    mock.expect_path_exists().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        // Call sequence: check entry (miss) -> ensure_dir -> download
        // -> sha256 -> extract -> check entry (hit).
        *n != 1
    });
    mock.expect_ensure_dir().returning(|_| Ok(()));
    let download_url: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let download_dest: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let url_slot = download_url.clone();
    let dest_slot = download_dest.clone();
    mock.expect_http_download().returning(move |u, p| {
        *url_slot.lock().unwrap() = Some(u.to_string());
        *dest_slot.lock().unwrap() = Some(p.to_path_buf());
        Ok(())
    });
    mock.expect_sha256_file()
        .returning(|_| Ok(FAKE.sha256.to_string()));
    let extracted_archive: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let extracted_dest: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let arch_slot = extracted_archive.clone();
    let dest_slot2 = extracted_dest.clone();
    mock.expect_extract_archive().returning(move |a, d| {
        *arch_slot.lock().unwrap() = Some(a.to_path_buf());
        *dest_slot2.lock().unwrap() = Some(d.to_path_buf());
        Ok(())
    });

    // Act
    let path = ensure_pin(&mock, &FAKE, Path::new("/cache")).unwrap();

    // Assert
    assert_eq!(
        download_url.lock().unwrap().as_deref(),
        Some(FAKE.url),
        "downloaded from the pin URL"
    );
    let dest = download_dest.lock().unwrap().clone().unwrap();
    let dest_s = dest.display().to_string().replace('\\', "/");
    assert!(
        dest_s.ends_with("fake/fake.zip"),
        "archive landed under cache dir: {dest_s}"
    );
    let archive_arg = extracted_archive.lock().unwrap().clone().unwrap();
    assert_eq!(archive_arg, dest, "extract_archive received the download");
    let extract_dest = extracted_dest.lock().unwrap().clone().unwrap();
    let extract_s = extract_dest.display().to_string().replace('\\', "/");
    assert!(
        extract_s.ends_with("/cache/fake") || extract_s.ends_with("cache/fake"),
        "extract dest is the cache dir: {extract_s}"
    );
    let returned = path.display().to_string().replace('\\', "/");
    assert!(returned.ends_with("fake/bin/fake.exe"), "got {returned}");
}

#[test]
fn test_sha_mismatch_fails_loudly_without_extracting() {
    // Arrange
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| false);
    mock.expect_ensure_dir().returning(|_| Ok(()));
    mock.expect_http_download().returning(|_, _| Ok(()));
    mock.expect_sha256_file().returning(|_| {
        Ok("badbadbadbadbadbadbadbadbadbadbadbadbadbadbadbadbadbadbadbadbad0".to_string())
    });
    mock.expect_extract_archive().times(0);

    // Act
    let err = ensure_pin(&mock, &FAKE, Path::new("/cache"))
        .expect_err("expected SHA mismatch")
        .to_string();

    // Assert
    assert!(err.contains("SHA-256 mismatch"), "got: {err}");
    assert!(err.contains("fake"), "got: {err}");
}

#[test]
fn test_sha_compare_is_case_insensitive() {
    // Arrange: pin is lower-case, simulator returns the upper-case
    // digest PowerShell's `Get-FileHash` produces.
    let mut mock = quiet_mock();
    let exists_calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let slot = exists_calls.clone();
    mock.expect_path_exists().returning(move |_| {
        let mut n = slot.lock().unwrap();
        *n += 1;
        *n != 1
    });
    mock.expect_ensure_dir().returning(|_| Ok(()));
    mock.expect_http_download().returning(|_, _| Ok(()));
    mock.expect_sha256_file()
        .returning(|_| Ok(FAKE.sha256.to_uppercase()));
    mock.expect_extract_archive().returning(|_, _| Ok(()));

    // Act
    let res = ensure_pin(&mock, &FAKE, Path::new("/cache"));

    // Assert
    assert!(res.is_ok(), "{res:?}");
}

#[test]
fn test_missing_entry_binary_after_extract_errors() {
    // Arrange: SHA verifies, extract reports success, but the
    // entry exe is never produced; ensure_pin must surface that
    // explicitly so a stale [`Pin::exe_rel`] is loud.
    let mut mock = quiet_mock();
    mock.expect_path_exists().returning(|_| false);
    mock.expect_ensure_dir().returning(|_| Ok(()));
    mock.expect_http_download().returning(|_, _| Ok(()));
    mock.expect_sha256_file()
        .returning(|_| Ok(FAKE.sha256.to_string()));
    mock.expect_extract_archive().returning(|_, _| Ok(()));

    // Act
    let err = ensure_pin(&mock, &FAKE, Path::new("/cache"))
        .expect_err("expected entry-binary check to fail")
        .to_string();

    // Assert
    assert!(err.contains("missing after extracting"), "got: {err}");
}

#[test]
fn test_inner_archive_is_extracted_after_outer() {
    // Arrange: model the Carnac case. The first extract produces
    // the inner nupkg; the second extract surfaces the entry exe.
    let mut mock = quiet_mock();
    let exists_seen: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let exists_clone = exists_seen.clone();
    let exe_rel = FAKE_NESTED.exe_rel;
    let inner_name = FAKE_NESTED.inner_archive.unwrap();
    mock.expect_path_exists().returning(move |p| {
        exists_clone.lock().unwrap().push(p.to_path_buf());
        let s = p.display().to_string().replace('\\', "/");
        // Entry exe missing on first poll; appears after second
        // extract. Inner archive is "present" once we are queried
        // for it (after the first extract call returns).
        if s.ends_with(exe_rel) {
            // The first time the entry exe is checked is the cold-
            // cache fast-path; after extraction we want it present.
            let calls = exists_clone
                .lock()
                .unwrap()
                .iter()
                .filter(|q| {
                    q.display()
                        .to_string()
                        .replace('\\', "/")
                        .ends_with(exe_rel)
                })
                .count();
            return calls > 1;
        }
        if s.ends_with(inner_name) {
            return true;
        }
        false
    });
    mock.expect_ensure_dir().returning(|_| Ok(()));
    mock.expect_http_download().returning(|_, _| Ok(()));
    mock.expect_sha256_file()
        .returning(|_| Ok(FAKE_NESTED.sha256.to_string()));
    let extracts: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let ex_slot = extracts.clone();
    mock.expect_extract_archive().returning(move |a, _| {
        ex_slot.lock().unwrap().push(a.to_path_buf());
        Ok(())
    });

    // Act
    let path = ensure_pin(&mock, &FAKE_NESTED, Path::new("/cache")).unwrap();

    // Assert: the recorder must extract the outer archive first
    // (so the nupkg appears) and then the inner nupkg.
    let calls = extracts.lock().unwrap().clone();
    let names: Vec<String> = calls
        .iter()
        .map(|p| p.display().to_string().replace('\\', "/"))
        .collect();
    assert_eq!(names.len(), 2, "expected outer + inner extract: {names:?}");
    assert!(
        names[0].ends_with("/outer.zip") || names[0].ends_with("outer.zip"),
        "outer first: {names:?}"
    );
    assert!(names[1].ends_with(inner_name), "inner second: {names:?}");
    let final_path = path.display().to_string().replace('\\', "/");
    assert!(final_path.ends_with(exe_rel), "got {final_path}");
}

#[test]
fn test_inner_archive_missing_after_outer_extract_errors() {
    // Arrange: outer extract succeeds but never produces the
    // declared inner archive. ensure_pin must fail loudly so a
    // stale Pin::inner_archive name is caught.
    let mut mock = quiet_mock();
    let inner_name = FAKE_NESTED.inner_archive.unwrap();
    mock.expect_path_exists().returning(move |p| {
        let s = p.display().to_string().replace('\\', "/");
        // Entry exe + inner archive both missing throughout.
        let _ = inner_name;
        let _ = s;
        false
    });
    mock.expect_ensure_dir().returning(|_| Ok(()));
    mock.expect_http_download().returning(|_, _| Ok(()));
    mock.expect_sha256_file()
        .returning(|_| Ok(FAKE_NESTED.sha256.to_string()));
    let outer_extracts: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let slot = outer_extracts.clone();
    mock.expect_extract_archive().returning(move |a, _| {
        slot.lock()
            .unwrap()
            .insert(a.display().to_string().replace('\\', "/"));
        Ok(())
    });

    // Act
    let err = ensure_pin(&mock, &FAKE_NESTED, Path::new("/cache"))
        .expect_err("expected inner-archive check to fail")
        .to_string();

    // Assert
    assert!(
        err.contains("inner archive") && err.contains("missing"),
        "got: {err}"
    );
    // Only the outer archive was extracted before the bail.
    let names = outer_extracts.lock().unwrap().clone();
    assert_eq!(names.len(), 1, "only outer extract: {names:?}");
    assert!(
        names.iter().any(|n| n.ends_with("outer.zip")),
        "outer was extracted: {names:?}"
    );
}
