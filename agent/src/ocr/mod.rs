//! OCR support for the reward screen and inventory screens.
//!
//! The language-independent core — fuzzy-matching OCR text to canonical names —
//! lives in [`matcher`] and is always built and tested. The screen-capture and
//! Tesseract recognition steps depend on system libraries and a live display,
//! so they will live behind the `ocr` cargo feature (added once validated on a
//! real machine with screenshots).

pub mod matcher;
pub mod regions;
pub mod segment;

#[cfg(feature = "ocr")]
pub mod recognize;

#[cfg(feature = "capture")]
pub mod capture;

pub use matcher::{Match, Matcher};
pub use regions::{reward_name_boxes, Rect};
