//! Live "load" visualization for the menu bar, iStat-Menus style.
//!
//! macOS renders tray icons at a fixed 18 pt height (tray-icon scales whatever
//! it gets), so we render at 2× (36 px tall) for crisp Retina output. Bars use
//! Apple system colors on a green→amber→red ramp where **color encodes absolute
//! load vs. the burn threshold** and height is scaled to
//! `max(window peak, burn)` — a steady moderate load reads as steady green bars,
//! not a wall of red. A faint baseline keeps the graph legible at zero load.
//!
//! Everything here is pure (no GUI types) and unit-tested; `main.rs` wraps the
//! RGBA buffer into a `tray_icon::Icon`.

use std::collections::VecDeque;

/// Rendering scale (pixels per point). 2 = Retina-crisp at 18 pt.
pub const SCALE: usize = 2;
/// Bar width / gap in pixels at [`SCALE`].
pub const BAR_W: usize = 4;
pub const BAR_GAP: usize = 2;
/// Number of bars (history slots) shown.
pub const SLOTS: usize = 24;
/// Icon pixel dimensions.
pub const ICON_W: usize = SLOTS * (BAR_W + BAR_GAP);
pub const ICON_H: usize = 18 * SCALE;

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

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A Unicode block sparkline normalized to the local peak (for `--dump`).
pub fn unicode_spark(s: &[f64]) -> String {
    let max = s.iter().cloned().fold(0.0_f64, f64::max);
    s.iter()
        .map(|v| {
            let n = if max > 0.0 { (v / max).clamp(0.0, 1.0) } else { 0.0 };
            BLOCKS[((n * 7.0).round() as usize).min(7)]
        })
        .collect()
}

/// Apple system palette ramp: green → amber → red by absolute load `v` (0..=1,
/// where 1 = at/over the burn threshold).
fn gradient(v: f64) -> (u8, u8, u8) {
    // systemGreen (52,199,89) → systemOrange (255,159,10) → systemRed (255,69,58)
    let lerp = |a: f64, b: f64, t: f64| (a + (b - a) * t) as u8;
    let v = v.clamp(0.0, 1.0);
    if v < 0.6 {
        let t = v / 0.6;
        (lerp(52.0, 255.0, t), lerp(199.0, 159.0, t), lerp(89.0, 10.0, t))
    } else {
        let t = (v - 0.6) / 0.4;
        (255, lerp(159.0, 69.0, t), lerp(10.0, 58.0, t))
    }
}

#[inline]
fn put(buf: &mut [u8], w: usize, x: usize, y: usize, rgba: (u8, u8, u8, u8)) {
    let i = (y * w + x) * 4;
    buf[i] = rgba.0;
    buf[i + 1] = rgba.1;
    buf[i + 2] = rgba.2;
    buf[i + 3] = rgba.3;
}

/// Render the sample history as an [`ICON_W`]×[`ICON_H`] RGBA bar graph.
///
/// - newest sample = rightmost bar; bars are [`BAR_W`] px with [`BAR_GAP`] gaps
/// - bar **color** = `sample / burn` (absolute: red means at/over threshold)
/// - bar **height** = `sample / max(window peak, burn)` (relative shape)
/// - a faint gray baseline spans the full width so an idle graph is visible
pub fn render_rgba(samples: &[f64], burn: f64) -> Vec<u8> {
    let (w, h) = (ICON_W, ICON_H);
    let mut buf = vec![0u8; w * h * 4];
    let burn = if burn > 0.0 { burn } else { 1.0 };

    // Baseline: bottom SCALE rows, translucent gray (visible on light & dark).
    for y in (h - SCALE)..h {
        for x in 0..w {
            put(&mut buf, w, x, y, (128, 128, 128, 90));
        }
    }

    let peak = samples.iter().cloned().fold(0.0_f64, f64::max);
    let scale_max = peak.max(burn);
    let usable_h = h - SCALE; // above the baseline

    let take = samples.len().min(SLOTS);
    let start = samples.len() - take;
    for (k, &sample) in samples[start..].iter().enumerate() {
        // Right-align: newest sample ends at the last slot.
        let slot = SLOTS - take + k;
        let x0 = slot * (BAR_W + BAR_GAP);
        let rel = (sample / scale_max).clamp(0.0, 1.0);
        let bar_h = ((rel * usable_h as f64).round() as usize).min(usable_h);
        if bar_h == 0 {
            continue;
        }
        let color = gradient(sample / burn);
        for y in (usable_h - bar_h)..usable_h {
            for x in x0..(x0 + BAR_W) {
                put(&mut buf, w, x, y, (color.0, color.1, color.2, 255));
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    const BURN: f64 = 40_000.0;

    fn pixel(buf: &[u8], x: usize, y: usize) -> (u8, u8, u8, u8) {
        let i = (y * ICON_W + x) * 4;
        (buf[i], buf[i + 1], buf[i + 2], buf[i + 3])
    }

    /// Sum of alpha in a bar slot's column range, above the baseline.
    fn bar_fill(buf: &[u8], slot: usize) -> usize {
        let x0 = slot * (BAR_W + BAR_GAP);
        let mut n = 0;
        for y in 0..(ICON_H - SCALE) {
            for x in x0..(x0 + BAR_W) {
                if pixel(buf, x, y).3 > 0 {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn history_is_bounded_and_ordered() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.push(v);
        }
        assert_eq!(h.values(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn baseline_present_even_with_no_samples() {
        let buf = render_rgba(&[], BURN);
        let (_, _, _, a) = pixel(&buf, 0, ICON_H - 1);
        assert!(a > 0, "baseline missing");
        // Nothing above the baseline.
        assert_eq!(bar_fill(&buf, SLOTS - 1), 0);
    }

    #[test]
    fn newest_sample_is_rightmost_and_right_aligned() {
        let buf = render_rgba(&[10_000.0], BURN);
        assert!(bar_fill(&buf, SLOTS - 1) > 0, "newest bar should be in last slot");
        assert_eq!(bar_fill(&buf, 0), 0, "left slots should be empty");
    }

    #[test]
    fn color_is_absolute_not_window_relative() {
        // A steady LOW load: peak equals every sample. Under peak-relative
        // coloring this rendered full-height red; absolute coloring keeps it green.
        let buf = render_rgba(&[5_000.0; 10], BURN);
        let x = (SLOTS - 1) * (BAR_W + BAR_GAP);
        // Find the top-most filled pixel of the last bar and check its color.
        let y = (0..ICON_H - SCALE).find(|&y| pixel(&buf, x, y).3 > 0).unwrap();
        let (r, g, _, _) = pixel(&buf, x, y);
        assert!(g > r, "low load should be green, got r={r} g={g}");

        // At/over burn → red.
        let buf = render_rgba(&[BURN * 1.5], BURN);
        let y = (0..ICON_H - SCALE).find(|&y| pixel(&buf, x, y).3 > 0).unwrap();
        let (r, g, _, _) = pixel(&buf, x, y);
        assert!(r > 200 && g < 120, "over-burn should be red, got r={r} g={g}");
    }

    #[test]
    fn height_scales_to_burn_when_below_threshold() {
        // Low steady load must NOT render full-height bars.
        let buf = render_rgba(&[5_000.0; 5], BURN);
        let full = (ICON_H - SCALE) * BAR_W;
        let fill = bar_fill(&buf, SLOTS - 1);
        assert!(fill < full / 2, "5k of 40k burn should be a short bar: {fill}/{full}");
    }

    #[test]
    fn bar_gaps_are_transparent() {
        let buf = render_rgba(&[BURN; SLOTS], BURN);
        // The gap after the first bar's pixels must be empty above baseline.
        let gap_x = BAR_W; // first gap column
        for y in 0..(ICON_H - SCALE) {
            assert_eq!(pixel(&buf, gap_x, y).3, 0, "gap should be transparent at y={y}");
        }
    }

    #[test]
    fn unicode_spark_reflects_shape() {
        let s = unicode_spark(&[0.0, 1.0, 2.0, 4.0]);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars[0], '▁');
        assert_eq!(chars[3], '█');
    }
}
