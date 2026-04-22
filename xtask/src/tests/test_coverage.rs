//! Tests for the coverage module.

use mockall::mock;

use crate::coverage::{run_coverage, CoverageSystem};

mock! {
    CoverageSystemMock {}
    impl CoverageSystem for CoverageSystemMock {
        fn read_nightly_version_file(&self) -> anyhow::Result<String>;
        fn list_installed_toolchains(&self) -> anyhow::Result<String>;
        fn install_toolchain(&self, toolchain: &str) -> anyhow::Result<()>;
        fn run_cargo_llvm_cov(&self, toolchain: &str, args: &[String]) -> anyhow::Result<()>;
        fn print_info(&self, message: &str);
    }
}

const TOOLCHAIN: &str = "nightly-2026-04-20";

/// Build a mock with the version file returning [`TOOLCHAIN`] and `print_info`
/// accepting any call. Callers configure the remaining expectations.
fn base_mock() -> MockCoverageSystemMock {
    let mut mock = MockCoverageSystemMock::new();
    mock.expect_read_nightly_version_file()
        .returning(|| Ok(TOOLCHAIN.to_owned()));
    mock.expect_print_info().returning(|_| ());
    mock
}

#[test]
fn test_run_coverage_installs_toolchain_when_missing() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_list_installed_toolchains()
        .returning(|| Ok("stable-x86_64-pc-windows-msvc (default)\n".to_owned()));
    mock.expect_install_toolchain()
        .withf(|t| t == TOOLCHAIN)
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_run_cargo_llvm_cov().returning(|_, _| Ok(()));

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_run_coverage_skips_install_when_present() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_list_installed_toolchains().returning(|| {
        Ok(format!(
            "stable-x86_64-pc-windows-msvc (default)\n{TOOLCHAIN}-x86_64-pc-windows-msvc\n"
        ))
    });
    mock.expect_install_toolchain().never();
    mock.expect_run_cargo_llvm_cov().returning(|_, _| Ok(()));

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_run_coverage_calls_llvm_cov_in_correct_order() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_list_installed_toolchains()
        .returning(|| Ok(format!("{TOOLCHAIN}-x86_64-pc-windows-msvc\n")));
    mock.expect_install_toolchain().never();

    let call_counter = std::sync::Arc::new(std::sync::Mutex::new(0u32));

    // Expect exactly 4 llvm-cov invocations in order: clean, test, xml report, html report.
    let counter = call_counter.clone();
    mock.expect_run_cargo_llvm_cov()
        .times(4)
        .returning(move |toolchain, args| {
            assert_eq!(toolchain, TOOLCHAIN);
            let mut count = counter.lock().unwrap();
            match *count {
                0 => {
                    assert_eq!(args[0], "clean");
                    assert_eq!(args[1], "--workspace");
                }
                1 => {
                    assert_eq!(args[0], "--all-features");
                    assert_eq!(args[1], "--workspace");
                }
                2 => {
                    assert_eq!(args[0], "report");
                    assert_eq!(args[1], "--cobertura");
                }
                3 => {
                    assert_eq!(args[0], "report");
                    assert_eq!(args[1], "--html");
                }
                _ => panic!("unexpected call #{count}"),
            }
            *count += 1;
            Ok(())
        });

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_ok());
    assert_eq!(*call_counter.lock().unwrap(), 4);
}

#[test]
fn test_run_coverage_fails_on_version_file_read_error() {
    // Arrange
    let mut mock = MockCoverageSystemMock::new();
    mock.expect_read_nightly_version_file()
        .returning(|| anyhow::bail!("file not found"));
    mock.expect_print_info().returning(|_| ());

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("file not found"));
}

#[test]
fn test_run_coverage_fails_on_toolchain_install_error() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_list_installed_toolchains()
        .returning(|| Ok(String::new()));
    mock.expect_install_toolchain()
        .returning(|_| anyhow::bail!("install failed"));

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("install failed"));
}

#[test]
fn test_run_coverage_fails_on_llvm_cov_error() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_list_installed_toolchains()
        .returning(|| Ok(format!("{TOOLCHAIN}-x86_64-pc-windows-msvc\n")));
    mock.expect_install_toolchain().never();
    mock.expect_run_cargo_llvm_cov()
        .returning(|_, _| anyhow::bail!("coverage failed"));

    // Act
    let result = run_coverage(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("coverage failed"));
}
