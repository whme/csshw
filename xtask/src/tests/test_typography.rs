//! Tests for the typography module.

use std::path::Path;

use anyhow::Result;
use mockall::mock;

use crate::typography::{
    check_typography, is_blocklisted, scan_bytes, should_scan, TypographySystem, Violation,
};

mock! {
    TypographySystemMock {}
    impl TypographySystem for TypographySystemMock {
        fn list_tracked_files(&self) -> Result<Vec<String>>;
        fn file_size(&self, path: &Path) -> Result<u64>;
        fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
        fn log(&self, msg: &str);
    }
}

#[test]
fn test_is_blocklisted_flags_em_dash() {
    // Arrange / Act / Assert
    assert!(is_blocklisted('\u{2014}'));
}

#[test]
fn test_is_blocklisted_flags_en_dash() {
    assert!(is_blocklisted('\u{2013}'));
}

#[test]
fn test_is_blocklisted_flags_smart_quotes() {
    assert!(is_blocklisted('\u{2018}'));
    assert!(is_blocklisted('\u{2019}'));
    assert!(is_blocklisted('\u{201C}'));
    assert!(is_blocklisted('\u{201D}'));
}

#[test]
fn test_is_blocklisted_flags_ellipsis() {
    assert!(is_blocklisted('\u{2026}'));
}

#[test]
fn test_is_blocklisted_flags_arrows() {
    assert!(is_blocklisted('\u{2190}'));
    assert!(is_blocklisted('\u{2192}'));
    assert!(is_blocklisted('\u{21FF}'));
}

#[test]
fn test_is_blocklisted_flags_nbsp() {
    assert!(is_blocklisted('\u{00A0}'));
}

#[test]
fn test_is_blocklisted_allows_ascii() {
    for c in 0u32..=0x7F {
        let c = char::from_u32(c).unwrap();
        assert!(!is_blocklisted(c), "ASCII {c:?} flagged");
    }
}

#[test]
fn test_is_blocklisted_allows_emoji() {
    // Robot, check mark, cross mark, magnifier (used in CI workflow logs).
    assert!(!is_blocklisted('\u{1F916}'));
    assert!(!is_blocklisted('\u{2705}'));
    assert!(!is_blocklisted('\u{274C}'));
    assert!(!is_blocklisted('\u{1F50D}'));
}

#[test]
fn test_should_scan_includes_rust_files() {
    assert!(should_scan("src/main.rs"));
    assert!(should_scan("xtask/src/typography.rs"));
}

#[test]
fn test_should_scan_includes_markdown() {
    assert!(should_scan("AGENTS.md"));
    assert!(should_scan("docs/guide.md"));
}

#[test]
fn test_should_scan_includes_workflow_yaml() {
    assert!(should_scan(".github/workflows/_shared-ci.yml"));
    assert!(should_scan("config.yaml"));
}

#[test]
fn test_should_scan_includes_pre_commit_hook() {
    assert!(should_scan(".githooks/pre-commit"));
}

#[test]
fn test_should_scan_excludes_cargo_lock() {
    assert!(!should_scan("Cargo.lock"));
}

#[test]
fn test_should_scan_excludes_news_fragment_workflow() {
    assert!(!should_scan(".github/workflows/news-fragment-check.yml"));
}

#[test]
fn test_should_scan_excludes_github_pages_template() {
    assert!(!should_scan("templates/github-pages-index.html"));
}

#[test]
fn test_should_scan_excludes_social_preview_template() {
    assert!(!should_scan("templates/social-preview.html"));
}

#[test]
fn test_should_scan_excludes_unknown_extensions() {
    assert!(!should_scan("logo.png"));
    assert!(!should_scan("binary.exe"));
    assert!(!should_scan("README"));
}

#[test]
fn test_scan_bytes_clean_ascii_returns_empty() {
    // Arrange
    let bytes = b"// Plain ASCII\nfn main() {}\n";

    // Act
    let (violations, non_utf8) = scan_bytes("src/main.rs", bytes);

    // Assert
    assert!(violations.is_empty());
    assert!(!non_utf8);
}

#[test]
fn test_scan_bytes_emoji_not_flagged() {
    // Arrange
    let bytes = "println!(\"\u{1F916} done\");\n".as_bytes();

    // Act
    let (violations, non_utf8) = scan_bytes("src/main.rs", bytes);

    // Assert
    assert!(violations.is_empty());
    assert!(!non_utf8);
}

#[test]
fn test_scan_bytes_crlf_not_flagged() {
    // Arrange
    let bytes = b"// line one\r\n// line two\r\n";

    // Act
    let (violations, non_utf8) = scan_bytes("src/main.rs", bytes);

    // Assert
    assert!(violations.is_empty());
    assert!(!non_utf8);
}

#[test]
fn test_scan_bytes_em_dash_reported_with_position() {
    // Arrange: em-dash on line 2, after "ab".
    let bytes = "fn main() {}\nab\u{2014}cd\n".as_bytes();

    // Act
    let (violations, non_utf8) = scan_bytes("src/main.rs", bytes);

    // Assert
    assert!(!non_utf8);
    assert_eq!(
        violations,
        vec![Violation {
            path: "src/main.rs".to_owned(),
            line: 2,
            column: 3,
            character: '\u{2014}',
        }]
    );
}

#[test]
fn test_scan_bytes_arrow_reported() {
    // Arrange: rightwards arrow at start of line 1.
    let bytes = "\u{2192} foo".as_bytes();

    // Act
    let (violations, non_utf8) = scan_bytes("src/lib.rs", bytes);

    // Assert
    assert!(!non_utf8);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].character, '\u{2192}');
    assert_eq!(violations[0].line, 1);
    assert_eq!(violations[0].column, 1);
}

#[test]
fn test_scan_bytes_multiple_violations_reported() {
    // Arrange
    let bytes = "// \u{2014}\u{2026}\n".as_bytes();

    // Act
    let (violations, non_utf8) = scan_bytes("a.md", bytes);

    // Assert
    assert!(!non_utf8);
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].character, '\u{2014}');
    assert_eq!(violations[0].column, 4);
    assert_eq!(violations[1].character, '\u{2026}');
    assert_eq!(violations[1].column, 5);
}

#[test]
fn test_scan_bytes_invalid_utf8_flagged() {
    // Arrange: lone continuation byte.
    let bytes: &[u8] = &[0x66, 0x6F, 0x80, 0x6F];

    // Act
    let (violations, non_utf8) = scan_bytes("bin", bytes);

    // Assert
    assert!(violations.is_empty());
    assert!(non_utf8);
}

#[test]
fn test_check_typography_passes_when_all_clean() {
    // Arrange
    let mut mock = MockTypographySystemMock::new();
    mock.expect_list_tracked_files()
        .returning(|| Ok(vec!["src/main.rs".to_owned(), "Cargo.lock".to_owned()]));
    mock.expect_file_size()
        .withf(|p: &Path| p == Path::new("src/main.rs"))
        .returning(|_| Ok(20));
    mock.expect_read_file()
        .withf(|p: &Path| p == Path::new("src/main.rs"))
        .returning(|_| Ok(b"fn main() {}\n".to_vec()));
    // Cargo.lock must NOT be read because it is in ALLOWED_PATHS.

    // Act
    let result = check_typography(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_check_typography_fails_on_em_dash() {
    // Arrange
    let mut mock = MockTypographySystemMock::new();
    mock.expect_list_tracked_files()
        .returning(|| Ok(vec!["AGENTS.md".to_owned()]));
    mock.expect_file_size().returning(|_| Ok(20));
    mock.expect_read_file()
        .returning(|_| Ok("hello \u{2014} world\n".as_bytes().to_vec()));

    // Act
    let result = check_typography(&mock);

    // Assert
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("forbidden"), "unexpected error: {msg}");
}

#[test]
fn test_check_typography_skips_oversized_file() {
    // Arrange: a tracked file larger than the cap should be skipped
    // with a warning rather than blocking the run.
    let mut mock = MockTypographySystemMock::new();
    mock.expect_list_tracked_files()
        .returning(|| Ok(vec!["huge.md".to_owned()]));
    mock.expect_file_size().returning(|_| Ok(10 * 1024 * 1024));
    mock.expect_log().returning(|_| ());
    // read_file must NOT be called for an oversized file.

    // Act
    let result = check_typography(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_check_typography_skips_non_utf8_file() {
    // Arrange
    let mut mock = MockTypographySystemMock::new();
    mock.expect_list_tracked_files()
        .returning(|| Ok(vec!["weird.md".to_owned()]));
    mock.expect_file_size().returning(|_| Ok(4));
    mock.expect_read_file()
        .returning(|_| Ok(vec![0x66, 0x6F, 0x80, 0x6F]));
    mock.expect_log().returning(|_| ());

    // Act
    let result = check_typography(&mock);

    // Assert
    assert!(result.is_ok());
}
