//! Tests for the release module.

use mockall::mock;
use semver::Version;

use crate::release::{
    create_release_tag, determine_release_type, prepare_release, set_cargo_toml_version,
    suggest_next_version, ReleaseSystem, ReleaseType,
};

mock! {
    ReleaseSystemMock {}
    impl ReleaseSystem for ReleaseSystemMock {
        fn git_status_porcelain(&self) -> anyhow::Result<String>;
        fn git_current_branch(&self) -> anyhow::Result<String>;
        fn git_checkout_new_branch(&self, name: &str) -> anyhow::Result<()>;
        fn git_add(&self, files: &[String]) -> anyhow::Result<()>;
        fn git_commit(&self, message: &str, no_verify: bool) -> anyhow::Result<()>;
        fn git_push(&self, args: &[String]) -> anyhow::Result<()>;
        fn git_tag_list(&self, tag: &str) -> anyhow::Result<String>;
        fn git_log_latest_subject(&self) -> anyhow::Result<String>;
        fn git_fetch(&self) -> anyhow::Result<()>;
        fn git_rev_list_count_behind(&self, branch: &str) -> anyhow::Result<u32>;
        fn git_create_annotated_tag(&self, tag: &str, message: &str) -> anyhow::Result<()>;
        fn git_push_tag(&self, tag: &str) -> anyhow::Result<()>;
        fn read_cargo_toml(&self) -> anyhow::Result<String>;
        fn write_cargo_toml(&self, content: &str) -> anyhow::Result<()>;
        fn cargo_update_workspace(&self) -> anyhow::Result<()>;
        fn generate_changelog(&self) -> anyhow::Result<()>;
        fn prompt_user(&self, message: &str) -> anyhow::Result<String>;
    }
}

fn cargo_toml_with_version(version: &str) -> String {
    format!(
        "[workspace]\nmembers = [\"xtask\"]\n\n[package]\nname = \"csshw\"\nversion = \"{version}\"\nedition = \"2021\"\n"
    )
}

fn clean_mock() -> MockReleaseSystemMock {
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_status_porcelain()
        .returning(|| Ok(String::new()));
    mock
}

// ── suggest_next_version ──────────────────────────────────────────────────────

#[test]
fn test_suggest_next_version_minor_from_main() {
    // Arrange
    let version = Version::parse("0.18.1").unwrap();

    // Act
    let (release_type, next) = suggest_next_version(&version, "main").unwrap();

    // Assert
    assert_eq!(release_type, ReleaseType::Minor);
    assert_eq!(next, Version::parse("0.19.0").unwrap());
}

#[test]
fn test_suggest_next_version_patch_from_maintenance() {
    // Arrange
    let version = Version::parse("0.18.1").unwrap();

    // Act
    let (release_type, next) = suggest_next_version(&version, "0.18-maintenance").unwrap();

    // Assert
    assert_eq!(release_type, ReleaseType::Patch);
    assert_eq!(next, Version::parse("0.18.2").unwrap());
}

#[test]
fn test_suggest_next_version_error_on_unknown_branch() {
    // Arrange
    let version = Version::parse("0.18.1").unwrap();

    // Act
    let result = suggest_next_version(&version, "feature/something");

    // Assert
    assert!(result.is_err());
}

// ── determine_release_type ────────────────────────────────────────────────────

#[test]
fn test_determine_release_type_major() {
    let current = Version::parse("1.0.0").unwrap();
    let next = Version::parse("2.0.0").unwrap();
    assert_eq!(determine_release_type(&current, &next), ReleaseType::Major);
}

#[test]
fn test_determine_release_type_minor() {
    let current = Version::parse("1.0.0").unwrap();
    let next = Version::parse("1.1.0").unwrap();
    assert_eq!(determine_release_type(&current, &next), ReleaseType::Minor);
}

#[test]
fn test_determine_release_type_patch() {
    let current = Version::parse("1.0.0").unwrap();
    let next = Version::parse("1.0.1").unwrap();
    assert_eq!(determine_release_type(&current, &next), ReleaseType::Patch);
}

// ── set_cargo_toml_version ────────────────────────────────────────────────────

#[test]
fn test_set_cargo_toml_version_updates_version() {
    // Arrange
    let content = cargo_toml_with_version("0.18.1");

    // Act
    let result = set_cargo_toml_version(&content, "1.0.0").unwrap();

    // Assert
    let doc: toml_edit::Document = result.parse().unwrap();
    assert_eq!(doc["package"]["version"].as_str().unwrap(), "1.0.0");
}

#[test]
fn test_set_cargo_toml_version_preserves_other_fields() {
    // Arrange
    let content = cargo_toml_with_version("0.18.1");

    // Act
    let result = set_cargo_toml_version(&content, "1.0.0").unwrap();

    // Assert
    let doc: toml_edit::Document = result.parse().unwrap();
    assert_eq!(doc["package"]["name"].as_str().unwrap(), "csshw");
    assert_eq!(doc["package"]["edition"].as_str().unwrap(), "2021");
}

// ── prepare_release ───────────────────────────────────────────────────────────

#[test]
fn test_prepare_release_aborts_when_working_tree_dirty() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_status_porcelain()
        .returning(|| Ok("M src/main.rs\n".to_owned()));
    mock.expect_git_current_branch().never();

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not clean"));
}

#[test]
fn test_prepare_release_aborts_on_unexpected_branch() {
    // Arrange
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("feature/something".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_prompt_user().never();

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_prepare_release_happy_path_main_branch() {
    // Arrange
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("main".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("y".to_owned()));
    mock.expect_git_checkout_new_branch()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_write_cargo_toml()
        .withf(|content| content.contains("\"0.19.0\""))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_cargo_update_workspace()
        .times(1)
        .returning(|| Ok(()));
    mock.expect_generate_changelog()
        .times(1)
        .returning(|| Ok(()));
    mock.expect_git_add().times(1).returning(|_| Ok(()));
    mock.expect_git_commit()
        .withf(|msg, no_verify| msg == "Version 0.19.0" && *no_verify)
        .times(1)
        .returning(|_, _| Ok(()));
    mock.expect_git_push()
        .withf(|args| {
            args == [
                "-u".to_owned(),
                "origin".to_owned(),
                "0.19-maintenance".to_owned(),
            ]
        })
        .times(1)
        .returning(|_| Ok(()));

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_prepare_release_happy_path_maintenance_branch() {
    // Arrange
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("y".to_owned()));
    mock.expect_git_checkout_new_branch().never();
    mock.expect_write_cargo_toml()
        .withf(|content| content.contains("\"0.18.2\""))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_cargo_update_workspace()
        .times(1)
        .returning(|| Ok(()));
    mock.expect_generate_changelog()
        .times(1)
        .returning(|| Ok(()));
    mock.expect_git_add().times(1).returning(|_| Ok(()));
    mock.expect_git_commit()
        .withf(|msg, no_verify| msg == "Version 0.18.2" && *no_verify)
        .times(1)
        .returning(|_, _| Ok(()));
    mock.expect_git_push()
        .withf(|args| args.is_empty())
        .times(1)
        .returning(|_| Ok(()));

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

// ── create_release_tag ────────────────────────────────────────────────────────

#[test]
fn test_create_release_tag_aborts_when_not_on_maintenance() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("main".to_owned()));
    mock.expect_read_cargo_toml().never();

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("maintenance branch"));
}

#[test]
fn test_create_release_tag_aborts_when_tag_exists() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_git_tag_list()
        .returning(|_| Ok("0.18.1\n".to_owned()));
    mock.expect_git_log_latest_subject().never();

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[test]
fn test_create_release_tag_aborts_when_commit_message_mismatch() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_git_tag_list().returning(|_| Ok(String::new()));
    mock.expect_git_log_latest_subject()
        .returning(|| Ok("fix something".to_owned()));
    mock.expect_git_fetch().never();

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("does not match expected"));
}

#[test]
fn test_create_release_tag_aborts_when_behind_remote() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_git_tag_list().returning(|_| Ok(String::new()));
    mock.expect_git_log_latest_subject()
        .returning(|| Ok("Version 0.18.1".to_owned()));
    mock.expect_git_fetch().returning(|| Ok(()));
    mock.expect_git_rev_list_count_behind().returning(|_| Ok(2));
    mock.expect_prompt_user().never();

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("behind remote"));
}

#[test]
fn test_create_release_tag_happy_path() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_git_tag_list().returning(|_| Ok(String::new()));
    mock.expect_git_log_latest_subject()
        .returning(|| Ok("Version 0.18.1".to_owned()));
    mock.expect_git_fetch().returning(|| Ok(()));
    mock.expect_git_rev_list_count_behind().returning(|_| Ok(0));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("y".to_owned()));
    mock.expect_git_create_annotated_tag()
        .withf(|tag, msg| tag == "0.18.1" && msg == "Version 0.18.1")
        .times(1)
        .returning(|_, _| Ok(()));
    mock.expect_git_push_tag()
        .withf(|tag| tag == "0.18.1")
        .times(1)
        .returning(|_| Ok(()));

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_create_release_tag_cancelled_by_user() {
    // Arrange
    let mut mock = MockReleaseSystemMock::new();
    mock.expect_git_current_branch()
        .returning(|| Ok("0.18-maintenance".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_git_tag_list().returning(|_| Ok(String::new()));
    mock.expect_git_log_latest_subject()
        .returning(|| Ok("Version 0.18.1".to_owned()));
    mock.expect_git_fetch().returning(|| Ok(()));
    mock.expect_git_rev_list_count_behind().returning(|_| Ok(0));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("n".to_owned()));
    mock.expect_git_create_annotated_tag().never();

    // Act
    let result = create_release_tag(&mock);

    // Assert
    assert!(result.is_ok());
}
