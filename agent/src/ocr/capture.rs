//! Live screen capture via `xcap`, for the daemon/overlay reward OCR.
//!
//! Needs a display at runtime (X11, or Wayland through the desktop portal /
//! XWayland), so it is gated behind the `capture` cargo feature and cannot be
//! exercised headlessly. Builds without a display.

use std::io;

use image::{DynamicImage, GenericImageView};

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

/// Captures the first monitor as an image. (Multi-monitor selection can be
/// refined once validated on a real display.)
pub fn capture_primary() -> io::Result<DynamicImage> {
    let monitors = xcap::Monitor::all().map_err(to_io)?;
    let monitor = monitors
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no monitor found"))?;
    let rgba = monitor.capture_image().map_err(to_io)?;
    Ok(DynamicImage::ImageRgba8(rgba))
}

/// Captures the screen and OCRs the void-fissure reward tiles, returning the raw
/// text per tile (left to right). Pair with the matcher to get item names.
pub fn capture_reward_tiles() -> io::Result<Vec<String>> {
    let img = capture_primary()?;
    let (w, h) = img.dimensions();
    super::recognize::recognize_reward(&img, w, h)
}
