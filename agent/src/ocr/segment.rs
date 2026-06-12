//! Content-based segmentation of a projection profile into runs.
//!
//! Used to find text rows and columns in a screen capture regardless of scroll
//! position: project the isolated-text mask onto an axis (count of text pixels
//! per line), then split that 1-D profile into the bands that contain text.
//! Pure and unit-tested; the image side lives in [`super::recognize`].

/// Finds contiguous runs in `profile` whose value exceeds `threshold`.
///
/// Adjacent runs separated by a gap shorter than `min_gap` are merged (so a
/// word's inter-letter gaps don't split a line), and runs shorter than
/// `min_len` after merging are discarded (dropping specks/noise). Returns
/// half-open `[start, end)` ranges.
pub fn find_runs(
    profile: &[u32],
    threshold: u32,
    min_gap: usize,
    min_len: usize,
) -> Vec<(usize, usize)> {
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &v) in profile.iter().enumerate() {
        if v > threshold {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            raw.push((s, i));
        }
    }
    if let Some(s) = start {
        raw.push((s, profile.len()));
    }

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for run in raw {
        match merged.last_mut() {
            Some(last) if run.0 - last.1 < min_gap => last.1 = run.1,
            _ => merged.push(run),
        }
    }
    merged.retain(|(a, b)| b - a >= min_len);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_separated_bands() {
        // two text bands separated by a clear gap
        let p = [0, 0, 5, 6, 5, 0, 0, 0, 7, 8, 0];
        let runs = find_runs(&p, 1, 2, 1);
        assert_eq!(runs, vec![(2, 5), (8, 10)]);
    }

    #[test]
    fn merges_small_gaps_within_a_line() {
        // a one-column dip inside a band must not split it (min_gap = 2)
        let p = [4, 5, 0, 6, 4];
        assert_eq!(find_runs(&p, 1, 2, 1), vec![(0, 5)]);
    }

    #[test]
    fn drops_runs_below_min_len() {
        // a single speck is discarded when min_len = 2
        let p = [0, 9, 0, 5, 5, 0];
        assert_eq!(find_runs(&p, 1, 1, 2), vec![(3, 5)]);
    }

    #[test]
    fn respects_threshold() {
        let p = [2, 2, 8, 8, 2];
        assert_eq!(find_runs(&p, 5, 1, 1), vec![(2, 4)]);
    }

    #[test]
    fn handles_run_touching_the_end() {
        let p = [0, 3, 4];
        assert_eq!(find_runs(&p, 1, 1, 1), vec![(1, 3)]);
    }

    #[test]
    fn empty_profile_yields_nothing() {
        assert!(find_runs(&[], 1, 1, 1).is_empty());
    }
}
