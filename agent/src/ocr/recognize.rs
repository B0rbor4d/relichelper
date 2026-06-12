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

/// OCRs every name in a grid screenshot (relic refinement or inventory). The
/// grid is found by content (not fixed positions), so it works at any scroll
/// offset. Returns the raw recognised text per detected cell; the caller snaps
/// each to a canonical relic/item name.
pub fn recognize_grid_file(path: &Path) -> io::Result<Vec<String>> {
    let img = image::open(path).map_err(to_io)?;
    let cells = detect_text_cells(&img);
    recognize_boxes(&img, &cells, "grid")
}

/// Fraction of the image height that the white "xNN" count badge sits above a
/// grid cell's name, measured from real refinement captures. Tunable per setup
/// via `RELICHELPER_OCR_COUNT_OFFSET` (the badge position relative to the name
/// can shift with UI scale / theme).
const COUNT_BADGE_ABOVE_DEFAULT: f32 = 0.075;
const COUNT_BADGE_H: f32 = 0.060; // generous, to tolerate badge-position variance

fn count_badge_above() -> f32 {
    std::env::var("RELICHELPER_OCR_COUNT_OFFSET")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(COUNT_BADGE_ABOVE_DEFAULT)
}

/// A recognised refinement-grid cell.
#[derive(Debug, Clone)]
pub struct GridCell {
    pub name: String,
    /// Quantity from the white "xNN" badge, when readable.
    pub count: Option<u32>,
    /// Whether the relic is owned. Unowned relics carry a red "eye" marker above
    /// the icon (owned ones never do), which we detect reliably via the theme
    /// colour — far more robust than reading the small white count badge.
    pub owned: bool,
}

/// Eye-marker region above the icon, and the theme-red fraction above which we
/// consider the marker present (owned cells measure ~0, unowned ~0.02+).
const EYE_REGION_ABOVE: f32 = 0.10;
const EYE_REGION_W: f32 = 0.06;
const EYE_REGION_H: f32 = 0.05;
const EYE_RED_FRACTION: f32 = 0.008;

/// OCRs a grid screenshot returning, per detected cell, the name, the count
/// badge (when readable), and whether the relic is owned (no red eye marker).
pub fn recognize_grid_with_counts(path: &Path) -> io::Result<Vec<GridCell>> {
    let img = image::open(path).map_err(to_io)?;
    let (iw, ih) = img.dimensions();
    let rgb = img.to_rgb8();
    let mask = TextMask::from_rgb(theme_text_color());
    let cells = detect_text_cells(&img);

    let mut out = Vec::with_capacity(cells.len());
    for (i, b) in cells.iter().enumerate() {
        // Name (theme-coloured).
        let name_pre = preprocess(&img.crop_imm(b.x, b.y, b.w, b.h));
        let name_tmp = std::env::temp_dir().join(format!("relich_ocr_gridn_{i}.png"));
        name_pre.save(&name_tmp).map_err(to_io)?;
        let name = ocr_image(&name_tmp)?;

        // Count badge (red "xNN") sits at the icon's TOP-LEFT, above and left of
        // the centred name. Same red text as the name, so use the theme mask.
        let cy = b.y + b.h / 2;
        let cx = b.x + b.w / 2;
        let count_h = (COUNT_BADGE_H * ih as f32) as u32;
        let count_w = (0.075 * iw as f32) as u32;
        let count = Rect {
            x: cx.saturating_sub((0.055 * iw as f32) as u32),
            y: cy
                .saturating_sub((count_badge_above() * ih as f32) as u32)
                .saturating_sub(count_h / 2),
            w: count_w,
            h: count_h,
        }
        .clamped(iw, ih);
        // A white quiet-zone border is needed for Tesseract to read the short
        // "xNN" badge reliably.
        let count_pre =
            with_white_border(&preprocess(&img.crop_imm(count.x, count.y, count.w, count.h)), 30);
        let count_tmp = std::env::temp_dir().join(format!("relich_ocr_gridc_{i}.png"));
        count_pre.save(&count_tmp).map_err(to_io)?;
        let count = parse_count(&ocr_digits(&count_tmp)?);

        // Ownership: red "eye" marker above the icon (owned cells have none).
        let eye_w = (EYE_REGION_W * iw as f32) as u32;
        let eye_h = (EYE_REGION_H * ih as f32) as u32;
        let eye = Rect {
            x: cx.saturating_sub(eye_w / 2),
            y: (b.y + b.h / 2)
                .saturating_sub((EYE_REGION_ABOVE * ih as f32) as u32)
                .saturating_sub(eye_h / 2),
            w: eye_w,
            h: eye_h,
        }
        .clamped(iw, ih);
        let owned = red_fraction(&rgb, &mask, &eye) <= EYE_RED_FRACTION;

        out.push(GridCell { name, count, owned });
    }
    Ok(out)
}

/// Fraction of pixels in `rect` that match the theme text colour.
fn red_fraction(rgb: &image::RgbImage, mask: &TextMask, rect: &Rect) -> f32 {
    let total = rect.w * rect.h;
    if total == 0 {
        return 0.0;
    }
    let mut hits = 0u32;
    for y in rect.y..rect.y + rect.h {
        for x in rect.x..rect.x + rect.w {
            if mask.is_text(&rgb.get_pixel(x, y).0) {
                hits += 1;
            }
        }
    }
    hits as f32 / total as f32
}

/// Surrounds an image with a white border, giving Tesseract the quiet zone it
/// needs to read short strings like the count badge.
fn with_white_border(img: &DynamicImage, pad: u32) -> DynamicImage {
    let src = img.to_luma8();
    let (w, h) = (src.width(), src.height());
    let mut canvas = ImageBuffer::from_pixel(w + 2 * pad, h + 2 * pad, Luma([255u8]));
    imageops::overlay(&mut canvas, &src, pad as i64, pad as i64);
    DynamicImage::ImageLuma8(canvas)
}

/// Reads the count badge as a uniform text block (psm 6 handles the short,
/// stylised "xNN" better than single-line modes).
fn ocr_digits(path: &Path) -> io::Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .args(["--psm", "6"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parses a count badge string like "x35" / "X 35" into 35.
fn parse_count(raw: &str) -> Option<u32> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_count;

    #[test]
    fn parses_count_badges() {
        assert_eq!(parse_count("x35"), Some(35));
        assert_eq!(parse_count("X 14\n"), Some(14));
        assert_eq!(parse_count("x1"), Some(1));
        assert_eq!(parse_count(""), None);
        assert_eq!(parse_count("xx"), None);
    }
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
