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

use super::regions::{reward_name_boxes, Rect};
use super::segment::find_runs;

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
    recognize_boxes(img, &reward_name_boxes(w, h), "reward")
}

/// OCRs every relic name in a refinement-grid screenshot. The grid is found by
/// content (not fixed positions), so it works at any scroll offset.
pub fn recognize_relic_grid_file(path: &Path) -> io::Result<Vec<String>> {
    let img = image::open(path).map_err(to_io)?;
    let cells = detect_text_cells(&img);
    recognize_boxes(&img, &cells, "grid")
}

/// Region of the screen that holds the relic/inventory grid, as fractions:
/// left of the right-hand detail panel, below the header, above the action bar.
const GRID_ROI: (f32, f32, f32, f32) = (0.02, 0.27, 0.72, 0.96);

/// Detects text cells in the grid ROI by projecting the isolated theme-coloured
/// text onto Y (to find name rows) then X within each row (to find individual
/// names). Returns padded rectangles in full-image coordinates — scroll
/// position is irrelevant because nothing is hard-coded.
fn detect_text_cells(img: &DynamicImage) -> Vec<Rect> {
    let rgb = img.to_rgb8();
    let (iw, ih) = (rgb.width(), rgb.height());
    let mask = TextMask::from_rgb(theme_text_color());

    let ox = (GRID_ROI.0 * iw as f32) as u32;
    let oy = (GRID_ROI.1 * ih as f32) as u32;
    let rw = ((GRID_ROI.2 * iw as f32) as u32 - ox) as usize;
    let rh = ((GRID_ROI.3 * ih as f32) as u32 - oy) as usize;

    // ROI text mask (true = theme-coloured text pixel).
    let mut m = vec![false; rw * rh];
    for ry in 0..rh {
        for rx in 0..rw {
            if mask.is_text(&rgb.get_pixel(ox + rx as u32, oy + ry as u32).0) {
                m[ry * rw + rx] = true;
            }
        }
    }

    // Y projection -> name rows.
    let row_profile: Vec<u32> = (0..rh)
        .map(|ry| (0..rw).filter(|&rx| m[ry * rw + rx]).count() as u32)
        .collect();
    let row_thr = (rw as u32 / 120).max(3);
    let row_gap = (rh / 90).max(3);
    let row_min = (rh / 90).max(6);

    let pad = 6u32;
    let mut cells = Vec::new();
    for (by0, by1) in find_runs(&row_profile, row_thr, row_gap, row_min) {
        let bh = (by1 - by0) as u32;
        // X projection within this row -> individual names.
        let col_profile: Vec<u32> = (0..rw)
            .map(|rx| (by0..by1).filter(|&ry| m[ry * rw + rx]).count() as u32)
            .collect();
        let col_thr = (bh / 6).max(1);
        let col_gap = (rw / 60).max(8);
        let col_min = (rw / 40).max(20);
        for (bx0, bx1) in find_runs(&col_profile, col_thr, col_gap, col_min) {
            cells.push(
                Rect {
                    x: (ox + bx0 as u32).saturating_sub(pad),
                    y: (oy + by0 as u32).saturating_sub(pad),
                    w: (bx1 - bx0) as u32 + 2 * pad,
                    h: bh + 2 * pad,
                }
                .clamped(iw, ih),
            );
        }
    }
    cells
}

/// Crops each box, preprocesses, and OCRs it; `tag` only names the temp files.
fn recognize_boxes(img: &DynamicImage, boxes: &[Rect], tag: &str) -> io::Result<Vec<String>> {
    let mut out = Vec::with_capacity(boxes.len());
    for (i, b) in boxes.iter().enumerate() {
        let crop = img.crop_imm(b.x, b.y, b.w, b.h);
        let pre = preprocess(&crop);
        let tmp = std::env::temp_dir().join(format!("relich_ocr_{tag}_{i}.png"));
        pre.save(&tmp).map_err(to_io)?;
        out.push(ocr_image(&tmp)?);
    }
    Ok(out)
}

/// How strongly a pixel must lean toward the theme's text colour to count as
/// text (margin between the colour's "high" and "low" channels).
const TEXT_COLOR_THRESHOLD: i16 = 40;

/// Isolates UI text of a given theme colour, hue-agnostically.
///
/// Warframe's UI theme (and custom themes) can render text red, gold, blue,
/// etc. The mask splits the target colour's channels into "high" (at/above its
/// own mean) and "low", and keeps pixels that are strong in *all* high channels
/// and weak in the low ones — `min(high) - max(low) > threshold`. For the
/// default red theme this reduces to `R - max(G, B)`; for gold it becomes
/// `min(R, G) - B`, etc. This drops the white/colourful item icon regardless of
/// hue.
struct TextMask {
    high: Vec<usize>,
    low: Vec<usize>,
}

impl TextMask {
    fn from_rgb(t: [u8; 3]) -> Self {
        let mean = (t[0] as u16 + t[1] as u16 + t[2] as u16) / 3;
        let mut high = Vec::new();
        let mut low = Vec::new();
        for (c, &v) in t.iter().enumerate() {
            if v as u16 >= mean {
                high.push(c);
            } else {
                low.push(c);
            }
        }
        Self { high, low }
    }

    fn is_text(&self, p: &[u8]) -> bool {
        let min_high = self.high.iter().map(|&c| p[c] as i16).min().unwrap_or(0);
        let max_low = self.low.iter().map(|&c| p[c] as i16).max().unwrap_or(0);
        min_high - max_low > TEXT_COLOR_THRESHOLD
    }
}

/// The theme text colour, from `RELICHELPER_OCR_TEXT_RGB` ("r,g,b") if set,
/// else the default red. Lets users with a custom UI theme retune without a
/// rebuild.
fn theme_text_color() -> [u8; 3] {
    if let Ok(s) = std::env::var("RELICHELPER_OCR_TEXT_RGB") {
        let parts: Vec<u8> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if let [r, g, b] = parts[..] {
            return [r, g, b];
        }
    }
    [255, 40, 40] // default Warframe red
}

/// Prepares a crop for OCR: isolate the theme-coloured text, binarise to black
/// text on white, and 2× upscale for the small UI font.
fn preprocess(img: &DynamicImage) -> DynamicImage {
    let rgb = img.to_rgb8();
    let mask = TextMask::from_rgb(theme_text_color());
    let bin: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_fn(rgb.width(), rgb.height(), |x, y| {
            if mask.is_text(&rgb.get_pixel(x, y).0) {
                Luma([0]) // text -> black
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
