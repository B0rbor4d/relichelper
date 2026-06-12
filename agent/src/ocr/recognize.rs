//! Reward-screen recognition: crop the name boxes, preprocess, OCR.
//!
//! Crops are produced with the pure-Rust `image` crate; recognition shells out
//! to the `tesseract` binary (so there is no native-library linking). Works
//! headless on a saved screenshot — live screen capture (xcap) is wired in the
//! overlay phase.
//!
//! The raw OCR strings are returned as-is; callers snap them to canonical names
//! with [`crate::ocr::matcher::Matcher`].

use std::io;
use std::path::Path;
use std::process::Command;

use image::{imageops, DynamicImage, GenericImageView, ImageBuffer, Luma};

use super::regions::reward_name_boxes;

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

/// OCRs the four reward-name boxes of a reward-screen screenshot, returning the
/// raw recognised text for each tile (left to right).
pub fn recognize_reward_file(path: &Path) -> io::Result<Vec<String>> {
    let img = image::open(path).map_err(to_io)?;
    let (w, h) = img.dimensions();
    recognize_reward(&img, w, h)
}

/// As [`recognize_reward_file`] but on an already-decoded image (e.g. a live
/// screen capture).
pub fn recognize_reward(img: &DynamicImage, w: u32, h: u32) -> io::Result<Vec<String>> {
    let mut out = Vec::with_capacity(4);
    for (i, b) in reward_name_boxes(w, h).iter().enumerate() {
        let crop = img.crop_imm(b.x, b.y, b.w, b.h);
        let pre = preprocess(&crop);
        let tmp = std::env::temp_dir().join(format!("relich_ocr_reward_{i}.png"));
        pre.save(&tmp).map_err(to_io)?;
        out.push(ocr_image(&tmp)?);
    }
    Ok(out)
}

/// Minimum "redness" (`R - max(G, B)`) for a pixel to count as reward text.
/// The reward-name font is red, so this isolates it and drops the colourful /
/// white item icon, which would otherwise confuse Tesseract.
const TEXT_REDNESS_THRESHOLD: i16 = 40;

/// Prepares a crop for OCR: isolate the red name text, binarise to black text
/// on a white background, and 2× upscale for the small UI font.
fn preprocess(img: &DynamicImage) -> DynamicImage {
    let rgb = img.to_rgb8();
    let bin: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_fn(rgb.width(), rgb.height(), |x, y| {
            let p = rgb.get_pixel(x, y);
            let (r, g, b) = (p[0] as i16, p[1] as i16, p[2] as i16);
            if r - g.max(b) > TEXT_REDNESS_THRESHOLD {
                Luma([0]) // red text -> black
            } else {
                Luma([255]) // everything else -> white background
            }
        });
    let up = imageops::resize(
        &bin,
        bin.width() * 2,
        bin.height() * 2,
        imageops::FilterType::Lanczos3,
    );
    DynamicImage::ImageLuma8(up)
}

/// Runs `tesseract <image> stdout` treating the crop as a uniform text block.
fn ocr_image(path: &Path) -> io::Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("--psm")
        .arg("6")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    // Join wrapped lines into one string; the matcher normalizes whitespace.
    let text = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(text)
}
