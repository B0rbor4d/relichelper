//! Crop geometry for the screens we OCR.
//!
//! Coordinates are expressed as fractions of the screen and scaled to the
//! actual resolution, so a single layout works across monitors. Fractions were
//! measured from real 2560×1440 (16:9) captures of the void-fissure reward
//! screen; Warframe keeps this UI centered at 16:9, so on non-16:9 displays the
//! content rect should be the pillar/letter-boxed 16:9 area (caller's concern).

/// A pixel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// Clamps the rectangle so it stays within `w_max × h_max`.
    pub fn clamped(self, w_max: u32, h_max: u32) -> Rect {
        let x = self.x.min(w_max.saturating_sub(1));
        let y = self.y.min(h_max.saturating_sub(1));
        Rect {
            x,
            y,
            w: self.w.min(w_max - x),
            h: self.h.min(h_max - y),
        }
    }
}

// Reward-screen name boxes (measured from 2560×1440 captures).
// The four tiles are centered horizontally and symmetric about mid-screen.
const REWARD_TILE_CENTERS: [f32; 4] = [0.3090, 0.4363, 0.5637, 0.6910];
// Tile spacing is ~0.127 of the width; the box must stay under that to avoid
// catching neighbouring names. Long names wrap to two lines within the tile
// rather than widening, so a tile-width box captures them via the taller box.
const REWARD_BOX_W: f32 = 0.125; // ~320px at 2560 — under the 0.127 spacing
const REWARD_BOX_Y: f32 = 0.378; // top of the name block (icon dropped by threshold)
const REWARD_BOX_H: f32 = 0.062; // covers one- and two-line names

/// The four reward-name crop boxes for a screen of `width × height`.
pub fn reward_name_boxes(width: u32, height: u32) -> [Rect; 4] {
    let w = width as f32;
    let h = height as f32;
    let box_w = (REWARD_BOX_W * w).round() as u32;
    let box_h = (REWARD_BOX_H * h).round() as u32;
    let y = (REWARD_BOX_Y * h).round() as u32;

    std::array::from_fn(|i| {
        let cx = REWARD_TILE_CENTERS[i] * w;
        let x = (cx - box_w as f32 / 2.0).round().max(0.0) as u32;
        Rect {
            x,
            y,
            w: box_w,
            h: box_h,
        }
        .clamped(width, height)
    })
}

// Relic refinement grid (measured from 2560×1440 captures): 5 columns, the
// name in red below each icon, a white "xNN" count badge above-right. Rows are
// scanned generously; empty/Off-grid cells simply yield no confident match.
const RELIC_COL_CENTERS: [f32; 5] = [0.130, 0.270, 0.412, 0.553, 0.693];
const RELIC_ROW_NAME_Y: [f32; 4] = [0.321, 0.485, 0.649, 0.813];
const RELIC_NAME_W: f32 = 0.125;
const RELIC_NAME_H: f32 = 0.050; // tall enough to tolerate row-position variance

/// Name-text crop boxes for the relic refinement grid, row-major (top-left
/// first). Includes cells that may be empty; the caller filters by match score.
pub fn relic_grid_name_boxes(width: u32, height: u32) -> Vec<Rect> {
    let w = width as f32;
    let h = height as f32;
    let box_w = (RELIC_NAME_W * w).round() as u32;
    let box_h = (RELIC_NAME_H * h).round() as u32;

    let mut boxes = Vec::with_capacity(RELIC_ROW_NAME_Y.len() * RELIC_COL_CENTERS.len());
    for &ry in &RELIC_ROW_NAME_Y {
        let y = (ry * h - box_h as f32 / 2.0).max(0.0).round() as u32;
        for &cx in &RELIC_COL_CENTERS {
            let x = (cx * w - box_w as f32 / 2.0).max(0.0).round() as u32;
            boxes.push(
                Rect {
                    x,
                    y,
                    w: box_w,
                    h: box_h,
                }
                .clamped(width, height),
            );
        }
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relic_grid_is_five_by_five_in_bounds() {
        let boxes = relic_grid_name_boxes(2560, 1440);
        assert_eq!(boxes.len(), RELIC_ROW_NAME_Y.len() * RELIC_COL_CENTERS.len());
        for b in &boxes {
            assert!(b.x + b.w <= 2560 && b.y + b.h <= 1440);
            assert!(b.w > 0 && b.h > 0);
        }
        // First cell centered on column 1 / row 1.
        let first = boxes[0];
        assert!(((first.x + first.w / 2) as i32 - (0.130 * 2560.0) as i32).abs() <= 3);
    }

    #[test]
    fn four_boxes_match_measured_1440p_centers() {
        let boxes = reward_name_boxes(2560, 1440);
        let centers: Vec<u32> = boxes.iter().map(|b| b.x + b.w / 2).collect();
        // Measured tile centers were ~791/1117/1443/1769 (±a few px from rounding).
        for (got, want) in centers.iter().zip([791, 1117, 1443, 1769]) {
            assert!((*got as i32 - want).abs() <= 3, "center {got} vs {want}");
        }
    }

    #[test]
    fn boxes_are_symmetric_and_ordered() {
        let boxes = reward_name_boxes(2560, 1440);
        let cx: Vec<i32> = boxes.iter().map(|b| (b.x + b.w / 2) as i32).collect();
        assert!(cx.windows(2).all(|w| w[0] < w[1]), "left to right");
        // Symmetric about mid-screen (1280).
        assert_eq!(cx[0] + cx[3], cx[1] + cx[2]);
        assert!(((cx[0] + cx[3]) - 2 * 1280).abs() <= 4);
    }

    #[test]
    fn boxes_stay_in_bounds_across_resolutions() {
        for (w, h) in [(1920, 1080), (2560, 1440), (3840, 2160), (1280, 720)] {
            for b in reward_name_boxes(w, h) {
                assert!(b.x + b.w <= w, "{w}x{h}: x+w in bounds");
                assert!(b.y + b.h <= h, "{w}x{h}: y+h in bounds");
                assert!(b.w > 0 && b.h > 0);
            }
        }
    }

    #[test]
    fn scales_proportionally_to_1080p() {
        let boxes = reward_name_boxes(1920, 1080);
        let centers: Vec<u32> = boxes.iter().map(|b| b.x + b.w / 2).collect();
        // Same fractions at 1080p: 0.309*1920 ≈ 593, etc.
        for (got, want) in centers.iter().zip([593, 838, 1082, 1327]) {
            assert!((*got as i32 - want).abs() <= 3, "center {got} vs {want}");
        }
    }
}
