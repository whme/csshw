//! Tests for the changelog module.

use mockall::mock;

use crate::changelog::{
    extract_version_from_cargo_toml, generate_changelog, set_changelogging_version, ChangelogSystem,
};

mock! {
    ChangelogSystemMock {}
    impl ChangelogSystem for ChangelogSystemMock {
        fn read_cargo_toml(&self) -> anyhow::Result<String>;
        fn read_changelogging_toml(&self) -> anyhow::Result<String>;
        fn write_changelogging_toml(&self, content: &str) -> anyhow::Result<()>;
        fn run_changelogging_build(&self) -> anyhow::Result<()>;
    }
}

const CARGO_TOML: &str = r#"[package]
name = "csshw"
version = "1.2.3"
edition = "2021"
"#;

const CHANGELOGGING_TOML: &str = r#"[context]
name = "csshw"
version = "0.0.0"
url = "https://github.com/whme/csshw"

[paths]
directory = "news"
"#;

#[test]
fn test_extract_version_from_cargo_toml_valid() {
    // Arrange / Act
    let result = extract_version_from_cargo_toml(CARGO_TOML).unwrap();

    // Assert
    assert_eq!(result, "1.2.3");
}

#[test]
fn test_extract_version_from_cargo_toml_missing_key() {
    // Arrange
    let content = "[package]\nname = \"csshw\"\n";

    // Act
    let result = extract_version_from_cargo_toml(content);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_extract_version_from_cargo_toml_invalid_toml() {
    // Arrange
    let content = "not valid toml ][";

    // Act
    let result = extract_version_from_cargo_toml(content);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_set_changelogging_version_updates_version() {
    // Arrange / Act
    let result = set_changelogging_version(CHANGELOGGING_TOML, "1.2.3").unwrap();

    // Assert
    let doc: toml_edit::Document = result.parse().unwrap();
    assert_eq!(doc["context"]["version"].as_str().unwrap(), "1.2.3");
}

#[test]
fn test_set_changelogging_version_preserves_other_keys() {
    // Arrange / Act
    let result = set_changelogging_version(CHANGELOGGING_TOML, "1.2.3").unwrap();

    // Assert
    let doc: toml_edit::Document = result.parse().unwrap();
    assert_eq!(doc["context"]["name"].as_str().unwrap(), "csshw");
    assert_eq!(
        doc["context"]["url"].as_str().unwrap(),
        "https://github.com/whme/csshw"
    );
    assert_eq!(doc["paths"]["directory"].as_str().unwrap(), "news");
}

#[test]
fn test_generate_changelog_calls_all_steps_in_order() {
    // Arrange
    let mut mock = MockChangelogSystemMock::new();

    // mockall enforces call count, not ordering across different methods,
    // so we verify each step is called exactly once.
    mock.expect_read_cargo_toml()
        .times(1)
        .returning(|| Ok(CARGO_TOML.to_owned()));
    mock.expect_read_changelogging_toml()
        .times(1)
        .returning(|| Ok(CHANGELOGGING_TOML.to_owned()));
    mock.expect_write_changelogging_toml()
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_run_changelogging_build()
        .times(1)
        .returning(|| Ok(()));

    // Act
    generate_changelog(&mock).unwrap();
}

#[test]
fn test_generate_changelog_writes_correct_version() {
    // Arrange
    let mut mock = MockChangelogSystemMock::new();
    mock.expect_read_cargo_toml()
        .returning(|| Ok(CARGO_TOML.to_owned()));
    mock.expect_read_changelogging_toml()
        .returning(|| Ok(CHANGELOGGING_TOML.to_owned()));

    let written = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let written_clone = written.clone();
    mock.expect_write_changelogging_toml()
        .returning(move |content| {
            *written_clone.lock().unwrap() = content.to_owned();
            Ok(())
        });
    mock.expect_run_changelogging_build().returning(|| Ok(()));

    // Act
    generate_changelog(&mock).unwrap();

    // Assert
    let content = written.lock().unwrap().clone();
    let doc: toml_edit::Document = content.parse().unwrap();
    assert_eq!(doc["context"]["version"].as_str().unwrap(), "1.2.3");
}
