//! Tests for the social_preview module.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mockall::mock;

use crate::social_preview::{generate_social_preview, SocialPreviewSystem};

mock! {
    SocialPreviewSystemMock {}
    impl SocialPreviewSystem for SocialPreviewSystemMock {
        fn workspace_root(&self) -> anyhow::Result<PathBuf>;
        fn env_var(&self, key: &str) -> Option<String>;
        fn ensure_parent_dir(&self, path: &Path) -> anyhow::Result<()>;
        fn check_docker_ready(&self) -> anyhow::Result<()>;
        fn docker_image_exists(&self, image: &str) -> bool;
        fn docker_pull(&self, image: &str) -> anyhow::Result<()>;
        fn run_docker(&self, args: &[String], envs: &[(String, String)]) -> anyhow::Result<()>;
        fn print_info(&self, message: &str);
        fn print_debug(&self, message: &str);
    }
}

/// Canonical workspace root used by tests. The helper functions operate
/// purely on strings and `PathBuf::join`, so the exact value does not
/// matter for what we assert.
fn workspace_root() -> PathBuf {
    PathBuf::from("ws-root")
}

/// Build a mock with `workspace_root`, `print_info`, and a no-op
/// `ensure_parent_dir`. Callers configure env/docker expectations.
fn base_mock() -> MockSocialPreviewSystemMock {
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    mock.expect_check_docker_ready().returning(|| Ok(()));
    mock.expect_docker_image_exists().returning(|_| true);
    mock.expect_ensure_parent_dir().returning(|_| Ok(()));
    mock.expect_print_info().returning(|_| ());
    mock.expect_print_debug().returning(|_| ());
    mock
}

/// Captured arguments from a single `run_docker` call.
#[derive(Clone, Default)]
struct DockerCall {
    args: Vec<String>,
    envs: Vec<(String, String)>,
}

/// Wire a `run_docker` expectation that captures its inputs into `slot`
/// and returns `Ok(())`.
fn capture_docker(mock: &mut MockSocialPreviewSystemMock, slot: Arc<Mutex<DockerCall>>) {
    mock.expect_run_docker()
        .times(1)
        .returning(move |args, envs| {
            *slot.lock().unwrap() = DockerCall {
                args: args.to_vec(),
                envs: envs.to_vec(),
            };
            Ok(())
        });
}

#[test]
fn test_generate_uses_default_out_and_no_token() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_env_var()
        .withf(|k| k == "GITHUB_TOKEN")
        .returning(|_| None);
    let captured = Arc::new(Mutex::new(DockerCall::default()));
    capture_docker(&mut mock, captured.clone());

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_ok());
    let call = captured.lock().unwrap().clone();
    assert_eq!(call.args[0], "run");
    assert!(call.args.iter().any(|a| a == "--rm"));
    // Workspace is bind-mounted to /workspace.
    assert!(call.args.iter().any(|a| a.ends_with(":/workspace")));
    // Default OUT_PATH inside the container.
    assert!(call
        .args
        .iter()
        .any(|a| a == "OUT_PATH=/workspace/target/social-preview/social-preview.png"));
    // Pinned image tag appears.
    assert!(call
        .args
        .iter()
        .any(|a| a.starts_with("mcr.microsoft.com/playwright:")));
    // Generator is invoked via sh -c, from the workspace root so its
    // workspace-relative input paths resolve correctly.
    assert!(call
        .args
        .iter()
        .any(|a| a.contains("node xtask/social-preview/generate.mjs")));
    // Without a token, no GITHUB_TOKEN env is forwarded.
    assert!(!call.envs.iter().any(|(k, _)| k == "GITHUB_TOKEN"));
    assert!(!call.args.iter().any(|a| a == "GITHUB_TOKEN"));
}

#[test]
fn test_generate_cli_token_takes_precedence_over_env() {
    // Arrange
    let mut mock = base_mock();
    // env_var is not consulted when a CLI token is provided; allow it
    // defensively so the mock never fails on an unexpected call.
    mock.expect_env_var()
        .returning(|_| Some("env-token".into()));
    let captured = Arc::new(Mutex::new(DockerCall::default()));
    capture_docker(&mut mock, captured.clone());

    // Act
    let result = generate_social_preview(&mock, None, Some("cli-token".into()));

    // Assert
    assert!(result.is_ok());
    let call = captured.lock().unwrap().clone();
    assert!(call
        .envs
        .iter()
        .any(|(k, v)| k == "GITHUB_TOKEN" && v == "cli-token"));
    let idx = call
        .args
        .iter()
        .position(|a| a == "GITHUB_TOKEN")
        .expect("expected GITHUB_TOKEN in docker args");
    assert_eq!(call.args[idx - 1], "-e");
}

#[test]
fn test_generate_falls_back_to_env_token() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_env_var()
        .withf(|k| k == "GITHUB_TOKEN")
        .returning(|_| Some("env-token".into()));
    let captured = Arc::new(Mutex::new(DockerCall::default()));
    capture_docker(&mut mock, captured.clone());

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_ok());
    let call = captured.lock().unwrap().clone();
    assert!(call
        .envs
        .iter()
        .any(|(k, v)| k == "GITHUB_TOKEN" && v == "env-token"));
}

#[test]
fn test_generate_custom_out_drives_ensure_parent_and_container_path() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    mock.expect_check_docker_ready().returning(|| Ok(()));
    mock.expect_docker_image_exists().returning(|_| true);
    mock.expect_env_var().returning(|_| None);
    mock.expect_print_info().returning(|_| ());
    mock.expect_print_debug().returning(|_| ());
    let ensured = Arc::new(Mutex::new(PathBuf::new()));
    let ensured_c = ensured.clone();
    mock.expect_ensure_parent_dir()
        .times(1)
        .returning(move |p| {
            *ensured_c.lock().unwrap() = p.to_path_buf();
            Ok(())
        });
    let captured = Arc::new(Mutex::new(DockerCall::default()));
    capture_docker(&mut mock, captured.clone());

    // Act
    let result =
        generate_social_preview(&mock, Some(PathBuf::from("custom/dir/preview.png")), None);

    // Assert
    assert!(result.is_ok());
    // host path = workspace_root.join(rel); on Windows join uses \ and
    // on Unix /, so compare component-wise.
    assert_eq!(
        ensured.lock().unwrap().as_path(),
        workspace_root().join("custom/dir/preview.png"),
    );
    let call = captured.lock().unwrap().clone();
    assert!(call
        .args
        .iter()
        .any(|a| a == "OUT_PATH=/workspace/custom/dir/preview.png"));
}

#[test]
fn test_generate_propagates_docker_error() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_env_var().returning(|_| None);
    mock.expect_run_docker()
        .times(1)
        .returning(|_, _| anyhow::bail!("docker exploded"));

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("docker exploded"));
}

#[test]
fn test_generate_rejects_parent_escape_in_out_path() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    // Path validation runs before every other side effect.
    mock.expect_check_docker_ready().never();
    mock.expect_docker_image_exists().never();
    mock.expect_docker_pull().never();
    mock.expect_ensure_parent_dir().never();
    mock.expect_run_docker().never();

    // Act — relative `..` that escapes the workspace resolves outside
    // `workspace_root` and is rejected.
    let result =
        generate_social_preview(&mock, Some(PathBuf::from("../outside/preview.png")), None);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_generate_accepts_dotdot_that_stays_inside_workspace() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_env_var().returning(|_| None);
    let captured = Arc::new(Mutex::new(DockerCall::default()));
    capture_docker(&mut mock, captured.clone());

    // Act — `sub/../preview.png` normalises to `preview.png` under the
    // workspace root, so the path is accepted.
    let result = generate_social_preview(&mock, Some(PathBuf::from("sub/../preview.png")), None);

    // Assert
    assert!(result.is_ok());
    let call = captured.lock().unwrap().clone();
    assert!(call
        .args
        .iter()
        .any(|a| a == "OUT_PATH=/workspace/preview.png"));
}

#[test]
fn test_generate_rejects_absolute_path_outside_workspace() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    mock.expect_check_docker_ready().never();
    mock.expect_docker_image_exists().never();
    mock.expect_docker_pull().never();
    mock.expect_ensure_parent_dir().never();
    mock.expect_run_docker().never();

    // Act — an absolute path whose root does not share a prefix with
    // `workspace_root` cannot be reached from inside the container and
    // is rejected. The particular path shape differs by platform but
    // the branch under test is the same.
    #[cfg(windows)]
    let outside = PathBuf::from(r"C:\somewhere-else\out\preview.png");
    #[cfg(not(windows))]
    let outside = PathBuf::from("/somewhere-else/out/preview.png");
    let result = generate_social_preview(&mock, Some(outside), None);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_generate_pulls_image_when_missing() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    mock.expect_check_docker_ready().returning(|| Ok(()));
    mock.expect_env_var().returning(|_| None);
    mock.expect_print_info().returning(|_| ());
    mock.expect_print_debug().returning(|_| ());
    mock.expect_ensure_parent_dir().returning(|_| Ok(()));
    mock.expect_docker_image_exists()
        .withf(|img| img.starts_with("mcr.microsoft.com/playwright:"))
        .returning(|_| false);
    mock.expect_docker_pull()
        .withf(|img| img.starts_with("mcr.microsoft.com/playwright:"))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_run_docker().times(1).returning(|_, _| Ok(()));

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_generate_skips_pull_when_image_present() {
    // Arrange
    let mut mock = base_mock();
    mock.expect_env_var().returning(|_| None);
    mock.expect_docker_pull().never();
    mock.expect_run_docker().times(1).returning(|_, _| Ok(()));

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_generate_surfaces_docker_not_ready_without_running_container() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_workspace_root()
        .returning(|| Ok(workspace_root()));
    mock.expect_print_info().returning(|_| ());
    mock.expect_check_docker_ready()
        .returning(|| anyhow::bail!("Docker daemon not reachable"));
    // Nothing downstream should run once the readiness check fails.
    mock.expect_docker_image_exists().never();
    mock.expect_docker_pull().never();
    mock.expect_ensure_parent_dir().never();
    mock.expect_run_docker().never();

    // Act
    let result = generate_social_preview(&mock, None, None);

    // Assert
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Docker daemon not reachable"));
}
