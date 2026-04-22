//! Social preview image generation.
//!
//! Fetches the current GitHub star count for the csshw repository, overlays
//! it onto `res/social-preview-template.png` using the DejaVu Sans Mono font,
//! and saves the result to `res/social-preview.png`.

use ab_glyph::{FontArc, PxScale};
use anyhow::{Context, Result};
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;

/// All side-effecting operations required by this module.
///
/// Implement with mocks in tests to achieve zero network, filesystem, and
/// process side-effects.
pub trait SocialPreviewSystem {
    /// Fetch the current GitHub star count for `whme/csshw`.
    ///
    /// # Returns
    ///
    /// The `stargazers_count` value.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    fn fetch_star_count(&self) -> Result<u64>;

    /// Read `res/social-preview-template.png` as a [`DynamicImage`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or decoded.
    fn read_template_image(&self) -> Result<DynamicImage>;

    /// Read the raw bytes of `res/dejavu-sans-mono.book.ttf`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_font_bytes(&self) -> Result<Vec<u8>>;

    /// Save the generated image to `res/social-preview.png`.
    ///
    /// # Arguments
    ///
    /// * `img` - The fully composed RGBA image.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn save_output_image(&self, img: &RgbaImage) -> Result<()>;
}

/// Production implementation of [`SocialPreviewSystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl SocialPreviewSystem for RealSystem {
    fn fetch_star_count(&self) -> Result<u64> {
        let body = ureq::get("https://api.github.com/repos/whme/csshw")
            .set("User-Agent", "csshw-social-preview-xtask")
            .call()
            .context("failed to call GitHub API")?
            .into_string()
            .context("failed to read GitHub API response")?;
        let response: serde_json::Value =
            serde_json::from_str(&body).context("failed to parse GitHub API response")?;
        let stars = response["stargazers_count"].as_u64().unwrap_or(0);
        Ok(stars)
    }

    fn read_template_image(&self) -> Result<DynamicImage> {
        image::open("res/social-preview-template.png")
            .context("failed to open res/social-preview-template.png")
    }

    fn read_font_bytes(&self) -> Result<Vec<u8>> {
        std::fs::read("res/dejavu-sans-mono.book.ttf")
            .context("failed to read res/dejavu-sans-mono.book.ttf")
    }

    fn save_output_image(&self, img: &RgbaImage) -> Result<()> {
        img.save("res/social-preview.png")
            .context("failed to save res/social-preview.png")
    }
}

/// Generate the social preview image.
///
/// Fetches the star count, renders it onto the template image at position
/// (960, 153) in white at 25 px, and saves the result.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Errors
///
/// Returns an error if any step fails.
pub fn generate_social_preview<S: SocialPreviewSystem>(system: &S) -> Result<()> {
    let stars = system.fetch_star_count()?.to_string();
    let mut img = system.read_template_image()?.to_rgba8();
    let font_bytes = system.read_font_bytes()?;
    let font = FontArc::try_from_vec(font_bytes).context("failed to parse font")?;

    draw_text_mut(
        &mut img,
        Rgba([255, 255, 255, 255]),
        960,
        153,
        PxScale::from(25.0),
        &font,
        &stars,
    );

    system.save_output_image(&img)
}

#[cfg(test)]
#[path = "tests/test_social_preview.rs"]
mod tests;
