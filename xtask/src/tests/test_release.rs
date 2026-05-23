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
        fn git_checkout(&self, name: &str) -> anyhow::Result<()>;
        fn git_branch_exists_local(&self, name: &str) -> anyhow::Result<bool>;
        fn git_branch_exists_origin(&self, name: &str) -> anyhow::Result<bool>;
        fn git_add(&self, files: &[String]) -> anyhow::Result<()>;
        fn git_commit(&self, message: &str, no_verify: bool) -> anyhow::Result<()>;
        fn git_push(&self, args: &[String]) -> anyhow::Result<()>;
        fn gh_pr_create(&self, base: &str) -> anyhow::Result<()>;
        fn git_tag_list(&self, tag: &str) -> anyhow::Result<String>;
        fn git_log_latest_subject(&self) -> anyhow::Result<String>;
        fn git_fetch(&self) -> anyhow::Result<()>;
        fn git_rev_list_count_behind(&self, branch: &str) -> anyhow::Result<u32>;
        fn git_rev_list_count_ahead(&self, branch: &str) -> anyhow::Result<u32>;
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
    let doc: toml_edit::DocumentMut = result.parse().unwrap();
    assert_eq!(doc["package"]["version"].as_str().unwrap(), "1.0.0");
}

#[test]
fn test_set_cargo_toml_version_preserves_other_fields() {
    // Arrange
    let content = cargo_toml_with_version("0.18.1");

    // Act
    let result = set_cargo_toml_version(&content, "1.0.0").unwrap();

    // Assert
    let doc: toml_edit::DocumentMut = result.parse().unwrap();
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

/// Add common expectations for the post-maintenance-branch portion of a
/// successful minor release flow: version bump, commit, push release branch,
/// open PR. The caller is responsible for the maintenance-branch setup
/// expectations.
fn expect_minor_release_tail(mock: &mut MockReleaseSystemMock) {
    mock.expect_git_checkout_new_branch()
        .withf(|name| name == "release-0.19.0")
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
                "release-0.19.0".to_owned(),
            ]
        })
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_gh_pr_create()
        .withf(|base| base == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
}

fn main_branch_minor_mock() -> MockReleaseSystemMock {
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("main".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("y".to_owned()));
    mock.expect_git_fetch().times(1).returning(|| Ok(()));
    mock
}

#[test]
fn test_prepare_release_minor_creates_maintenance_branch_when_missing() {
    // Arrange: neither local nor origin has the maintenance branch.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(false));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(false));
    mock.expect_git_checkout_new_branch()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
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
    mock.expect_git_checkout().never();
    expect_minor_release_tail(&mut mock);

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_prepare_release_minor_pushes_existing_local_maintenance_branch() {
    // Arrange: maintenance branch exists locally only.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(false));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
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
    expect_minor_release_tail(&mut mock);

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_prepare_release_minor_checks_out_remote_only_maintenance_branch() {
    // Arrange: maintenance branch exists only on origin.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(false));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    // No push of the maintenance branch when it already exists on origin; the
    // only push comes from `expect_minor_release_tail` for the release branch.
    expect_minor_release_tail(&mut mock);

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_prepare_release_minor_uses_existing_maintenance_branch_when_up_to_date() {
    // Arrange: maintenance branch exists locally and on origin, local is current.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_git_rev_list_count_behind()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(0));
    mock.expect_git_rev_list_count_ahead()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(0));
    expect_minor_release_tail(&mut mock);

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_prepare_release_minor_bails_when_existing_maintenance_branch_behind() {
    // Arrange: both exist, but local is behind origin.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_git_rev_list_count_behind()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(3));
    mock.expect_write_cargo_toml().never();
    mock.expect_gh_pr_create().never();

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("behind origin"), "unexpected error: {err}");
}

#[test]
fn test_prepare_release_minor_bails_when_existing_maintenance_branch_ahead() {
    // Arrange: both exist, local has unpushed commits.
    let mut mock = main_branch_minor_mock();
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_git_rev_list_count_behind()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(0));
    mock.expect_git_rev_list_count_ahead()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(2));
    mock.expect_write_cargo_toml().never();
    mock.expect_gh_pr_create().never();

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("ahead of origin"), "unexpected error: {err}");
}

/// Custom-version patch from `main` (current 0.19.0 -> 0.19.2): the task must
/// switch to the existing `0.19-maintenance` branch and push the version
/// bump directly, without creating a release-branch PR.
#[test]
fn test_prepare_release_patch_from_main_uses_existing_maintenance_branch() {
    // Arrange: on main, current version 0.19.0; user enters custom patch 0.19.2.
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("main".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.19.0")));
    let mut prompts = vec!["n".to_owned(), "0.19.2".to_owned()].into_iter();
    mock.expect_prompt_user()
        .times(2)
        .returning(move |_| Ok(prompts.next().unwrap()));
    mock.expect_git_fetch().times(1).returning(|| Ok(()));
    mock.expect_git_branch_exists_local()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_branch_exists_origin()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(true));
    mock.expect_git_checkout()
        .withf(|name| name == "0.19-maintenance")
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_git_rev_list_count_behind()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(0));
    mock.expect_git_rev_list_count_ahead()
        .withf(|name| name == "0.19-maintenance")
        .returning(|_| Ok(0));
    // No release-branch creation, no PR for a patch release.
    mock.expect_git_checkout_new_branch().never();
    mock.expect_gh_pr_create().never();
    mock.expect_write_cargo_toml()
        .withf(|content| content.contains("\"0.19.2\""))
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
        .withf(|msg, no_verify| msg == "Version 0.19.2" && *no_verify)
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

#[test]
fn test_prepare_release_aborts_on_fetch_failure() {
    // Arrange: on main preparing a minor release, but fetch fails.
    let mut mock = clean_mock();
    mock.expect_git_current_branch()
        .returning(|| Ok("main".to_owned()));
    mock.expect_read_cargo_toml()
        .returning(|| Ok(cargo_toml_with_version("0.18.1")));
    mock.expect_prompt_user()
        .times(1)
        .returning(|_| Ok("y".to_owned()));
    mock.expect_git_fetch()
        .times(1)
        .returning(|| Err(anyhow::anyhow!("network down")));
    mock.expect_git_branch_exists_local().never();
    mock.expect_git_branch_exists_origin().never();
    mock.expect_write_cargo_toml().never();
    mock.expect_gh_pr_create().never();

    // Act
    let result = prepare_release(&mock);

    // Assert
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("failed to fetch from origin"),
        "unexpected error: {err}"
    );
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
    mock.expect_gh_pr_create().never();
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
