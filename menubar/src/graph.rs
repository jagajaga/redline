//! Live "load" visualization for the menu bar, iStat-Menus style: a rolling
//! history of total tokens/min rendered as a small bar-graph icon with a
//! green→amber→red gradient. Pure logic (history + renderers) so it's testable;
//! `main.rs` turns the RGBA buffer into a `tray_icon::Icon`.

use std::collections::VecDeque;

/// A fixed-capacity ring of recent samples (oldest → newest).
pub struct History {
    samples: VecDeque<f64>,
    cap: usize,
}

impl History {
    pub fn new(cap: usize) -> Self {
        History {
            samples: VecDeque::with_capacity(cap),
            cap: cap.max(1),
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(v.max(0.0));
    }

    pub fn values(&self) -> Vec<f64> {
        self.samples.iter().copied().collect()
    }
}

/// Normalize to `0.0..=1.0` against the peak, with a small floor so a nonzero
/// flat line still shows. Returns all-zeros if every sample is zero.
fn normalize(s: &[f64]) -> Vec<f64> {
    let max = s.iter().cloned().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return vec![0.0; s.len()];
    }
    s.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
}

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A Unicode block sparkline (for `--dump` and tests).
pub fn unicode_spark(s: &[f64]) -> String {
    normalize(s)
        .into_iter()
        .map(|v| BLOCKS[((v * 7.0).round() as usize).min(7)])
        .collect()
}

/// Green (low) → amber → red (high) for a normalized level.
fn gradient(v: f64) -> (u8, u8, u8) {
    let lerp = |a: f64, b: f64, t: f64| (a + (b - a) * t) as u8;
    if v < 0.5 {
        let t = v / 0.5;
        (lerp(90.0, 240.0, t), lerp(210.0, 200.0, t), lerp(120.0, 70.0, t))
    } else {
        let t = (v - 0.5) / 0.5;
        (lerp(240.0, 240.0, t), lerp(200.0, 80.0, t), lerp(70.0, 80.0, t))
    }
}

/// Render the history as an `w`×`h` RGBA bar graph, newest sample on the right,
/// transparent background. Bars are colored by their own height.
pub fn render_rgba(s: &[f64], w: usize, h: usize) -> Vec<u8> {
    let mut buf = vec![0u8; w * h * 4];
    if w == 0 || h == 0 {
        return buf;
    }
    let norm = normalize(s);
    // Map the most recent `w` samples to columns, right-aligned.
    let take = norm.len().min(w);
    let start = norm.len() - take;
    for col in 0..take {
        let v = norm[start + col];
        let x = w - take + col; // right-align
        let filled = (v * h as f64).round() as usize;
        let (r, g, b) = gradient(v);
        for row in 0..filled.min(h) {
            let y = h - 1 - row; // fill from the bottom
            let idx = (y * w + x) * 4;
            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
            buf[idx + 3] = 255;
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_ordered() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.push(v);
        }
        assert_eq!(h.values(), vec![2.0, 3.0, 4.0]); // oldest dropped
    }

    #[test]
    fn unicode_spark_reflects_shape() {
        let s = unicode_spark(&[0.0, 1.0, 2.0, 4.0]);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars.len(), 4);
        assert_eq!(chars[0], '▁'); // lowest
        assert_eq!(chars[3], '█'); // peak
        // All-zero → all lowest block.
        assert!(unicode_spark(&[0.0, 0.0]).chars().all(|c| c == '▁'));
    }

    #[test]
    fn rgba_dimensions_and_taller_bars_for_higher_values() {
        let w = 4;
        let h = 10;
        let buf = render_rgba(&[1.0, 10.0], w, h);
        assert_eq!(buf.len(), w * h * 4);

        // Count filled (alpha>0) pixels in the last two columns.
        let filled_in_col = |x: usize| {
            (0..h)
                .filter(|&y| buf[(y * w + x) * 4 + 3] > 0)
                .count()
        };
        // Two samples right-aligned into columns w-2 and w-1.
        let low = filled_in_col(w - 2); // value 1.0 → small
        let high = filled_in_col(w - 1); // value 10.0 → full
        assert!(high > low, "high={high} should exceed low={low}");
        assert_eq!(high, h, "peak sample should fill the column");
    }

    #[test]
    fn gradient_moves_green_to_red() {
        let (r_lo, _g, _b) = gradient(0.0);
        let (r_hi, _g2, b_hi) = gradient(1.0);
        assert!(r_hi > r_lo); // more red at the top
        assert!(b_hi <= 90);
    }
}
