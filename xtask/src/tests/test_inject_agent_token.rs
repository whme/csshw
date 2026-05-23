//! Tests for the inject_agent_token module.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mockall::mock;

use crate::inject_agent_token::{inject_agent_token, InjectAgentTokenSystem};

mock! {
    InjectAgentTokenSystemMock {}
    impl InjectAgentTokenSystem for InjectAgentTokenSystemMock {
        fn env_var(&self, key: &str) -> Option<String>;
        fn current_dir(&self) -> anyhow::Result<PathBuf>;
        fn read_token_file(&self, path: &Path) -> anyhow::Result<Option<String>>;
        fn write_settings(&self, path: &Path, contents: &str) -> anyhow::Result<()>;
        fn log(&self, msg: &str);
    }
}

/// Pre-wire a mock so `env_var("PASEO_SOURCE_CHECKOUT_PATH")` and
/// `current_dir()` both return a consistent synthetic layout. The
/// returned tuple is `(source_checkout, worktree_cwd)`.
fn make_mock_with_layout(
    source: &str,
    cwd: &str,
    token_env: Option<&str>,
) -> (MockInjectAgentTokenSystemMock, PathBuf, PathBuf) {
    let source_path = PathBuf::from(source);
    let cwd_path = PathBuf::from(cwd);
    let mut mock = MockInjectAgentTokenSystemMock::new();

    let env_value = token_env.map(str::to_owned);
    mock.expect_env_var()
        .withf(|key| key == "PASEO_SOURCE_CHECKOUT_PATH")
        .returning(move |_| env_value.clone());

    let cwd_clone = cwd_path.clone();
    mock.expect_current_dir()
        .returning(move || Ok(cwd_clone.clone()));

    (mock, source_path, cwd_path)
}

#[test]
fn test_missing_token_file_is_noop() {
    // Arrange
    let (mut mock, source, _cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    let expected_token_path = source.join(".paseo").join("gh-token");
    mock.expect_read_token_file()
        .withf(move |path| path == expected_token_path)
        .returning(|_| Ok(None));
    mock.expect_write_settings().never();
    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let logged_clone = logged.clone();
    mock.expect_log().returning(move |msg| {
        logged_clone.lock().unwrap().push(msg.to_owned());
    });

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(result.is_ok());
    let logs = logged.lock().unwrap();
    assert_eq!(logs.len(), 1);
    assert!(
        logs[0].contains("no ") && logs[0].contains("gh-token"),
        "log should mention the missing token file: {}",
        logs[0]
    );
    assert!(
        logs[0].contains("CONTRIBUTING.md"),
        "log should point at CONTRIBUTING.md: {}",
        logs[0]
    );
}

#[test]
fn test_valid_fine_grained_token_writes_expected_json() {
    // Arrange
    let token = "github_pat_ABCDEF1234567890";
    let (mut mock, source, cwd) = make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    let expected_token_path = source.join(".paseo").join("gh-token");
    let token_owned = token.to_owned();
    mock.expect_read_token_file()
        .withf(move |path| path == expected_token_path)
        .returning(move |_| Ok(Some(token_owned.clone())));

    let written = Arc::new(Mutex::new(None::<(PathBuf, String)>));
    let written_clone = written.clone();
    let expected_settings_path = cwd.join(".claude").join("settings.local.json");
    mock.expect_write_settings()
        .withf(move |path, _| path == expected_settings_path)
        .times(1)
        .returning(move |path, contents| {
            *written_clone.lock().unwrap() = Some((path.to_path_buf(), contents.to_owned()));
            Ok(())
        });

    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(result.is_ok());
    let (_, contents) = written
        .lock()
        .unwrap()
        .clone()
        .expect("write_settings not invoked");
    assert_eq!(
        contents,
        "{\n  \"env\": {\n    \"GH_TOKEN\": \"github_pat_ABCDEF1234567890\",\n    \"GH_HOST\": \"github.com\"\n  }\n}\n"
    );
}

#[test]
fn test_whitespace_in_token_is_trimmed() {
    // Arrange
    let (mut mock, _source, _cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    mock.expect_read_token_file()
        .returning(|_| Ok(Some("  github_pat_ABC  \r\n".to_owned())));

    let written = Arc::new(Mutex::new(None::<String>));
    let written_clone = written.clone();
    mock.expect_write_settings()
        .times(1)
        .returning(move |_, contents| {
            *written_clone.lock().unwrap() = Some(contents.to_owned());
            Ok(())
        });
    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(result.is_ok());
    let contents = written
        .lock()
        .unwrap()
        .clone()
        .expect("write_settings not invoked");
    assert!(
        contents.contains("\"GH_TOKEN\": \"github_pat_ABC\""),
        "token should be trimmed before embedding: {contents}"
    );
    assert!(
        !contents.contains("  github_pat_ABC"),
        "embedded token must not contain whitespace: {contents}"
    );
}

#[test]
fn test_classic_token_is_accepted_with_warning() {
    // Arrange - classic tokens (`ghp_...`) are accepted to unblock
    // contributors who only have a classic token, but the user must
    // be warned because classic tokens cannot be scoped tightly.
    let token = "ghp_classicTokenAcceptedWithWarning";
    let (mut mock, _source, cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    let token_owned = token.to_owned();
    mock.expect_read_token_file()
        .returning(move |_| Ok(Some(token_owned.clone())));

    let written = Arc::new(Mutex::new(None::<String>));
    let written_clone = written.clone();
    let expected_settings_path = cwd.join(".claude").join("settings.local.json");
    mock.expect_write_settings()
        .withf(move |path, _| path == expected_settings_path)
        .times(1)
        .returning(move |_, contents| {
            *written_clone.lock().unwrap() = Some(contents.to_owned());
            Ok(())
        });

    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let logged_clone = logged.clone();
    mock.expect_log().returning(move |msg| {
        logged_clone.lock().unwrap().push(msg.to_owned());
    });

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(
        result.is_ok(),
        "classic tokens must be accepted: {result:?}"
    );
    let contents = written
        .lock()
        .unwrap()
        .clone()
        .expect("write_settings not invoked");
    assert!(
        contents.contains(&format!("\"GH_TOKEN\": \"{token}\"")),
        "classic token must be embedded verbatim: {contents}"
    );
    let logs = logged.lock().unwrap();
    let warn = logs
        .iter()
        .find(|line| line.starts_with("WARN"))
        .expect("classic token must emit a WARN log line");
    assert!(
        warn.contains("classic"),
        "warning must name the token kind: {warn}"
    );
    assert!(
        warn.contains("fine-grained"),
        "warning must recommend fine-grained PATs: {warn}"
    );
    assert!(
        warn.contains("CONTRIBUTING.md"),
        "warning must point at CONTRIBUTING.md: {warn}"
    );
}

#[test]
fn test_oauth_token_is_accepted_with_warning() {
    // Arrange - OAuth tokens (`gho_...`) take the same accept-with-warning
    // path as classic tokens; the wording must mention "OAuth" explicitly.
    let token = "gho_oauthTokenAcceptedWithWarning";
    let (mut mock, _source, cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    let token_owned = token.to_owned();
    mock.expect_read_token_file()
        .returning(move |_| Ok(Some(token_owned.clone())));

    let written = Arc::new(Mutex::new(None::<String>));
    let written_clone = written.clone();
    let expected_settings_path = cwd.join(".claude").join("settings.local.json");
    mock.expect_write_settings()
        .withf(move |path, _| path == expected_settings_path)
        .times(1)
        .returning(move |_, contents| {
            *written_clone.lock().unwrap() = Some(contents.to_owned());
            Ok(())
        });

    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let logged_clone = logged.clone();
    mock.expect_log().returning(move |msg| {
        logged_clone.lock().unwrap().push(msg.to_owned());
    });

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(result.is_ok(), "OAuth tokens must be accepted: {result:?}");
    let contents = written
        .lock()
        .unwrap()
        .clone()
        .expect("write_settings not invoked");
    assert!(
        contents.contains(&format!("\"GH_TOKEN\": \"{token}\"")),
        "OAuth token must be embedded verbatim: {contents}"
    );
    let logs = logged.lock().unwrap();
    let warn = logs
        .iter()
        .find(|line| line.starts_with("WARN"))
        .expect("OAuth token must emit a WARN log line");
    assert!(
        warn.contains("OAuth"),
        "warning must name the token kind: {warn}"
    );
    assert!(
        warn.contains("fine-grained"),
        "warning must recommend fine-grained PATs: {warn}"
    );
    assert!(
        warn.contains("CONTRIBUTING.md"),
        "warning must point at CONTRIBUTING.md: {warn}"
    );
}

#[test]
fn test_unrecognized_prefix_is_rejected() {
    // Arrange - tokens that do not start with any recognized GitHub
    // prefix must be rejected so we never inject arbitrary text.
    let (mut mock, _source, _cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    mock.expect_read_token_file()
        .returning(|_| Ok(Some("random_not_a_github_token".to_owned())));
    mock.expect_write_settings().never();
    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    let err = result.expect_err("unrecognized prefixes must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("github_pat_") && msg.contains("ghp_") && msg.contains("gho_"),
        "error must list all accepted prefixes: {msg}"
    );
    assert!(
        msg.contains("CONTRIBUTING.md"),
        "error must reference CONTRIBUTING.md: {msg}"
    );
}

#[test]
fn test_token_with_invalid_characters_is_rejected() {
    // Arrange - passes the `github_pat_` prefix check but contains
    // characters outside `[A-Za-z0-9_]`, which would break the JSON
    // template if embedded directly. Must be rejected before the
    // template is built.
    let (mut mock, _source, _cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    mock.expect_read_token_file()
        .returning(|_| Ok(Some("github_pat_AB\"injection".to_owned())));
    mock.expect_write_settings().never();
    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    let err = result.expect_err("tokens with invalid characters must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("alphabet") || msg.contains("[A-Za-z0-9_]"),
        "error must mention the alphabet constraint: {msg}"
    );
}

#[test]
fn test_empty_token_file_is_rejected() {
    // Arrange
    let (mut mock, _source, _cwd) =
        make_mock_with_layout("C:\\src", "C:\\worktree", Some("C:\\src"));
    mock.expect_read_token_file()
        .returning(|_| Ok(Some("   \n\t".to_owned())));
    mock.expect_write_settings().never();
    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    let err = result.expect_err("empty token files must be rejected");
    assert!(err.to_string().contains("empty"));
}

#[test]
fn test_missing_env_var_falls_back_to_current_dir() {
    // Arrange
    let (mut mock, _source, cwd) = make_mock_with_layout("C:\\unused", "C:\\fallback", None);
    let expected_token_path = cwd.join(".paseo").join("gh-token");
    mock.expect_read_token_file()
        .withf(move |path| path == expected_token_path)
        .returning(|_| Ok(None));
    mock.expect_write_settings().never();
    mock.expect_log().returning(|_| {});

    // Act
    let result = inject_agent_token(&mock);

    // Assert
    assert!(result.is_ok());
}
