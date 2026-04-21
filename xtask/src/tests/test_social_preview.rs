//! Tests for the social_preview module.

use image::{DynamicImage, RgbaImage};
use mockall::mock;

use crate::social_preview::{generate_social_preview, SocialPreviewSystem};

mock! {
    SocialPreviewSystemMock {}
    impl SocialPreviewSystem for SocialPreviewSystemMock {
        fn fetch_star_count(&self) -> anyhow::Result<u64>;
        fn read_template_image(&self) -> anyhow::Result<DynamicImage>;
        fn read_font_bytes(&self) -> anyhow::Result<Vec<u8>>;
        fn save_output_image(&self, img: &RgbaImage) -> anyhow::Result<()>;
    }
}

fn load_real_font_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("res")
        .join("dejavu-sans-mono.book.ttf");
    std::fs::read(&path).expect("failed to read res/dejavu-sans-mono.book.ttf")
}

fn blank_template() -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::new(1280, 640))
}

#[test]
fn test_generate_social_preview_draws_stars() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_fetch_star_count().returning(|| Ok(42));
    mock.expect_read_template_image()
        .returning(|| Ok(blank_template()));
    mock.expect_read_font_bytes()
        .returning(|| Ok(load_real_font_bytes()));
    mock.expect_save_output_image()
        .times(1)
        .returning(|_| Ok(()));

    // Act
    let result = generate_social_preview(&mock);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn test_generate_social_preview_propagates_fetch_error() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_fetch_star_count()
        .returning(|| Err(anyhow::anyhow!("network error")));
    mock.expect_read_template_image().never();
    mock.expect_read_font_bytes().never();
    mock.expect_save_output_image().never();

    // Act
    let result = generate_social_preview(&mock);

    // Assert
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("network error"));
}

#[test]
fn test_generate_social_preview_propagates_image_error() {
    // Arrange
    let mut mock = MockSocialPreviewSystemMock::new();
    mock.expect_fetch_star_count().returning(|| Ok(0));
    mock.expect_read_template_image()
        .returning(|| Err(anyhow::anyhow!("image load error")));
    mock.expect_read_font_bytes().never();
    mock.expect_save_output_image().never();

    // Act
    let result = generate_social_preview(&mock);

    // Assert
    assert!(result.is_err());
}
