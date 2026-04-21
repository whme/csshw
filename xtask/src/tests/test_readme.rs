//! Tests for the readme module.

use mockall::mock;

use crate::readme::{
    check_readme_help, extract_readme_help_section, normalize_help_output,
    replace_readme_help_section, update_readme_help, ReadmeSystem,
};

mock! {
    ReadmeSystemMock {}
    impl ReadmeSystem for ReadmeSystemMock {
        fn get_help_output(&self) -> anyhow::Result<String>;
        fn read_readme(&self) -> anyhow::Result<String>;
        fn write_readme(&self, content: &str) -> anyhow::Result<()>;
    }
}

fn make_readme(help_content: &str) -> String {
    format!(
        "# Title\r\n<!-- HELP_OUTPUT_START -->\r\n```cmd\r\ncsshw.exe --help\r\n{help_content}\r\n```\r\n<!-- HELP_OUTPUT_END -->\r\nMore content."
    )
}

#[test]
fn test_normalize_help_output_strips_whitespace_only_lines() {
    // Arrange
    let raw = "Usage: tool\n   \n  -h  help\n";

    // Act
    let result = normalize_help_output(raw);

    // Assert
    assert_eq!(result, "Usage: tool\r\n\r\n  -h  help");
}

#[test]
fn test_normalize_help_output_normalizes_line_endings() {
    // Arrange
    let raw = "line one\nline two\n";

    // Act
    let result = normalize_help_output(raw);

    // Assert
    assert_eq!(result, "line one\r\nline two");
}

#[test]
fn test_extract_readme_help_section_happy_path() {
    // Arrange
    let help = "Cluster SSH tool";
    let readme = make_readme(help);

    // Act
    let result = extract_readme_help_section(&readme).unwrap();

    // Assert
    assert_eq!(result, help);
}

#[test]
fn test_extract_readme_help_section_missing_start_marker() {
    // Arrange
    let readme = "no start marker here\r\n<!-- HELP_OUTPUT_END -->";

    // Act
    let result = extract_readme_help_section(readme);

    // Assert
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("HELP_OUTPUT_START"));
}

#[test]
fn test_extract_readme_help_section_missing_end_marker() {
    // Arrange
    let readme = "<!-- HELP_OUTPUT_START -->\r\n```cmd\r\ncsshw.exe --help\r\ncontent";

    // Act
    let result = extract_readme_help_section(readme);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("HELP_OUTPUT_END"));
}

#[test]
fn test_replace_readme_help_section_replaces_correctly() {
    // Arrange
    let new_help = "new help text";
    let readme = make_readme("old help text");

    // Act
    let result = replace_readme_help_section(&readme, new_help).unwrap();

    // Assert
    assert_eq!(result, make_readme(new_help));
}

#[test]
fn test_check_readme_help_passes_when_identical() {
    // Arrange
    let help = "Cluster SSH tool";
    let mut mock = MockReadmeSystemMock::new();
    mock.expect_get_help_output()
        .returning(move || Ok(help.to_owned()));
    mock.expect_read_readme()
        .returning(move || Ok(make_readme(help)));

    // Act
    let result = check_readme_help(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_check_readme_help_fails_when_different() {
    // Arrange
    let mut mock = MockReadmeSystemMock::new();
    mock.expect_get_help_output()
        .returning(|| Ok("new help".to_owned()));
    mock.expect_read_readme()
        .returning(|| Ok(make_readme("old help")));

    // Act
    let result = check_readme_help(&mock);

    // Assert
    assert!(result.is_err());
}

#[test]
fn test_update_readme_help_returns_false_when_up_to_date() {
    // Arrange
    let help = "Cluster SSH tool";
    let mut mock = MockReadmeSystemMock::new();
    mock.expect_get_help_output()
        .returning(move || Ok(help.to_owned()));
    mock.expect_read_readme()
        .returning(move || Ok(make_readme(help)));
    mock.expect_write_readme().never();

    // Act
    let result = update_readme_help(&mock).unwrap();

    // Assert
    assert!(!result, "should return false when no update needed");
}

#[test]
fn test_update_readme_help_returns_true_and_writes_new_content_when_different() {
    // Arrange
    let new_help = "new help text";
    let mut mock = MockReadmeSystemMock::new();
    mock.expect_get_help_output()
        .returning(move || Ok(new_help.to_owned()));
    mock.expect_read_readme()
        .returning(|| Ok(make_readme("old help")));

    let written = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let written_clone = written.clone();
    mock.expect_write_readme()
        .times(1)
        .returning(move |content| {
            *written_clone.lock().unwrap() = content.to_owned();
            Ok(())
        });

    // Act
    let result = update_readme_help(&mock).unwrap();

    // Assert
    assert!(result, "should return true when README was updated");
    assert_eq!(*written.lock().unwrap(), make_readme(new_help));
}
