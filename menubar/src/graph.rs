//! Menu-bar load visualization, styled after iStat Menus.
//!
//! Two renderers, both pure RGBA (no GUI types) and unit-tested:
//!
//! - [`render_tray`] — the menu-bar icon: a rounded translucent "well" (the
//!   iStat signature frame, legible on both light and dark menu bars) holding a
//!   histogram whose bars fade vertically like an area chart. **Height is the
//!   burn history; color encodes the selected view's limit-proximity** (teal →
//!   amber → red on the Apple system palette) — throttle by default, or tank /
//!   week / rate depending on the Settings choice. Height is scaled to
//!   `max(window peak, burn)` so steady moderate load stays a short bar.
//! - [`render_spark`] — a wide per-session area sparkline used inside dropdown
//!   submenus (macOS draws menu icons at 18 pt tall, aspect preserved).
//!
//! Everything renders at 2× ([`SCALE`]) for crisp Retina output.

use std::collections::VecDeque;

/// Pixels per point. 2 = Retina-crisp at macOS's fixed 18 pt icon height.
pub const SCALE: usize = 2;
/// Tray bar width / gap in pixels at [`SCALE`].
pub const BAR_W: usize = 4;
pub const BAR_GAP: usize = 2;
/// Number of tray bars (history slots). Kept compact — the graph is a
/// glance, not a chart.
pub const SLOTS: usize = 12;
/// Tray icon pixel dimensions (well padding included).
pub const PAD: usize = 2;
pub const ICON_W: usize = SLOTS * (BAR_W + BAR_GAP) + BAR_GAP + PAD * 2;
pub const ICON_H: usize = 18 * SCALE;
/// Sparkline slots (1 column = [`SPARK_COL`] px) and height.
pub const SPARK_SLOTS: usize = 44;
pub const SPARK_COL: usize = 2;
pub const SPARK_W: usize = SPARK_SLOTS * SPARK_COL;
pub const SPARK_H: usize = 18 * SCALE;

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

/// Apple system palette ramp by absolute load `v` (1.0 = at the burn
/// threshold): systemTeal → systemOrange → systemRed.
fn gradient(v: f64) -> (u8, u8, u8) {
    // teal (48,176,199) → orange (255,159,10) → red (255,69,58)
    let lerp = |a: f64, b: f64, t: f64| (a + (b - a) * t) as u8;
    let v = v.clamp(0.0, 1.0);
    if v < 0.6 {
        let t = v / 0.6;
        (lerp(48.0, 255.0, t), lerp(176.0, 159.0, t), lerp(199.0, 10.0, t))
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

/// Is `(x, y)` inside a rounded rect covering the whole `w`×`h` area?
fn in_rounded_rect(x: usize, y: usize, w: usize, h: usize, r: usize) -> bool {
    let (x, y, w, h, r) = (x as i64, y as i64, w as i64, h as i64, r as i64);
    // Corner circle centers.
    let cx = if x < r {
        r
    } else if x >= w - r {
        w - r - 1
    } else {
        return true;
    };
    let cy = if y < r {
        r
    } else if y >= h - r {
        h - r - 1
    } else {
        return true;
    };
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= r * r
}

/// The menu-bar icon. Bar heights are the burn history (activity over time);
/// `heat` colors the bars by the selected view's limit-proximity (0 = teal /
/// calm, 1 = red / at a limit). `heat: None` falls back to per-bar absolute
/// load (the "Rate" view) — teal → amber → red vs the burn threshold.
pub fn render_tray(samples: &[f64], burn: f64, heat: Option<f64>) -> Vec<u8> {
    let (w, h) = (ICON_W, ICON_H);
    let mut buf = vec![0u8; w * h * 4];
    let burn = if burn > 0.0 { burn } else { 1.0 };

    // The well: rounded, translucent gray — reads on light & dark menu bars.
    for y in 0..h {
        for x in 0..w {
            if in_rounded_rect(x, y, w, h, 3 * SCALE) {
                put(&mut buf, w, x, y, (127, 127, 132, 56));
            }
        }
    }

    let peak = samples.iter().cloned().fold(0.0_f64, f64::max);
    let scale_max = peak.max(burn);
    let usable_h = h - PAD * 2;

    let take = samples.len().min(SLOTS);
    let start = samples.len() - take;
    for (k, &sample) in samples[start..].iter().enumerate() {
        // Right-align: newest sample in the last slot.
        let slot = SLOTS - take + k;
        let x0 = PAD + BAR_GAP + slot * (BAR_W + BAR_GAP);
        let rel = (sample / scale_max).clamp(0.0, 1.0);
        let bar_h = ((rel * usable_h as f64).round() as usize).min(usable_h);
        if bar_h == 0 {
            continue;
        }
        let (r, g, b) = gradient(heat.unwrap_or(sample / burn));
        let top = PAD + usable_h - bar_h;
        for y in top..(PAD + usable_h) {
            // Vertical fade: bright at the tip, softer toward the base — the
            // area-chart feel iStat graphs have.
            let f = (y - top) as f64 / bar_h.max(1) as f64;
            let alpha = 255.0 - f * 90.0;
            for x in x0..(x0 + BAR_W) {
                put(&mut buf, w, x, y, (r, g, b, alpha as u8));
            }
        }
    }
    buf
}

/// A per-session area sparkline for dropdown submenus: translucent area fill
/// with a solid 2 px cap line, colored per column by absolute load.
pub fn render_spark(samples: &[f64], burn: f64) -> Vec<u8> {
    let (w, h) = (SPARK_W, SPARK_H);
    let mut buf = vec![0u8; w * h * 4];
    let burn = if burn > 0.0 { burn } else { 1.0 };

    // Hairline baseline so an idle session still shows a graph frame.
    for x in 0..w {
        for y in (h - SCALE)..h {
            put(&mut buf, w, x, y, (127, 127, 132, 80));
        }
    }

    let peak = samples.iter().cloned().fold(0.0_f64, f64::max);
    let scale_max = peak.max(burn);
    let usable_h = h - SCALE;

    let take = samples.len().min(SPARK_SLOTS);
    let start = samples.len() - take;
    for (k, &sample) in samples[start..].iter().enumerate() {
        let slot = SPARK_SLOTS - take + k;
        let x0 = slot * SPARK_COL;
        let rel = (sample / scale_max).clamp(0.0, 1.0);
        let col_h = ((rel * usable_h as f64).round() as usize).min(usable_h);
        if col_h == 0 {
            continue;
        }
        let (r, g, b) = gradient(sample / burn);
        let top = usable_h - col_h;
        let cap_end = (top + SCALE).min(usable_h);
        for y in top..usable_h {
            let alpha = if y < cap_end { 255 } else { 96 };
            for x in x0..(x0 + SPARK_COL) {
                put(&mut buf, w, x, y, (r, g, b, alpha));
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    const BURN: f64 = 40_000.0;

    fn pixel(buf: &[u8], w: usize, x: usize, y: usize) -> (u8, u8, u8, u8) {
        let i = (y * w + x) * 4;
        (buf[i], buf[i + 1], buf[i + 2], buf[i + 3])
    }

    /// Count strongly-opaque (bar) pixels in a tray slot, ignoring the well.
    fn bar_fill(buf: &[u8], slot: usize) -> usize {
        let x0 = PAD + BAR_GAP + slot * (BAR_W + BAR_GAP);
        let mut n = 0;
        for y in 0..ICON_H {
            for x in x0..(x0 + BAR_W) {
                if pixel(buf, ICON_W, x, y).3 > 120 {
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
    fn well_is_present_and_rounded() {
        let buf = render_tray(&[], BURN, None);
        // Center of the well: translucent but visible.
        let (_, _, _, a) = pixel(&buf, ICON_W, ICON_W / 2, ICON_H / 2);
        assert!(a > 0 && a < 120, "well should be translucent, alpha={a}");
        // Extreme corners are clipped by the rounding.
        assert_eq!(pixel(&buf, ICON_W, 0, 0).3, 0, "corner should be transparent");
        assert_eq!(
            pixel(&buf, ICON_W, ICON_W - 1, ICON_H - 1).3,
            0,
            "corner should be transparent"
        );
        // And no bars anywhere.
        assert_eq!(bar_fill(&buf, SLOTS - 1), 0);
    }

    #[test]
    fn newest_sample_is_rightmost_and_right_aligned() {
        let buf = render_tray(&[10_000.0], BURN, None);
        assert!(bar_fill(&buf, SLOTS - 1) > 0, "newest bar should be in last slot");
        assert_eq!(bar_fill(&buf, 0), 0, "left slots should be empty");
    }

    #[test]
    fn color_is_absolute_not_window_relative() {
        // Steady LOW load: must render teal (cool), not red — even though every
        // sample equals the window peak.
        let buf = render_tray(&[5_000.0; 10], BURN, None);
        let x = PAD + BAR_GAP + (SLOTS - 1) * (BAR_W + BAR_GAP);
        let y = (0..ICON_H)
            .find(|&y| pixel(&buf, ICON_W, x, y).3 > 120)
            .unwrap();
        let (r, _, b, _) = pixel(&buf, ICON_W, x, y);
        assert!(b > r, "low load should be teal, got r={r} b={b}");

        // At/over burn → red.
        let buf = render_tray(&[BURN * 1.5], BURN, None);
        let y = (0..ICON_H)
            .find(|&y| pixel(&buf, ICON_W, x, y).3 > 120)
            .unwrap();
        let (r, g, _, _) = pixel(&buf, ICON_W, x, y);
        assert!(r > 200 && g < 120, "over-burn should be red, got r={r} g={g}");
    }

    #[test]
    fn height_scales_to_burn_when_below_threshold() {
        let buf = render_tray(&[5_000.0; 5], BURN, None);
        let full = (ICON_H - PAD * 2) * BAR_W;
        let fill = bar_fill(&buf, SLOTS - 1);
        assert!(fill < full / 2, "5k of 40k burn should be a short bar: {fill}/{full}");
    }

    #[test]
    fn bars_fade_vertically() {
        let buf = render_tray(&[BURN], BURN, None);
        let x = PAD + BAR_GAP + (SLOTS - 1) * (BAR_W + BAR_GAP);
        let top_a = pixel(&buf, ICON_W, x, PAD).3;
        let bottom_a = pixel(&buf, ICON_W, x, ICON_H - PAD - 1).3;
        assert!(
            top_a > bottom_a + 40,
            "bar should fade downward: top={top_a} bottom={bottom_a}"
        );
    }

    #[test]
    fn spark_has_cap_and_translucent_area() {
        let buf = render_spark(&[BURN / 2.0; SPARK_SLOTS], BURN);
        let x = (SPARK_SLOTS - 1) * SPARK_COL;
        let usable = SPARK_H - SCALE;
        let top = (0..usable)
            .find(|&y| pixel(&buf, SPARK_W, x, y).3 > 0)
            .expect("column should be drawn");
        assert_eq!(pixel(&buf, SPARK_W, x, top).3, 255, "cap should be solid");
        let mid = (top + SCALE + usable) / 2;
        let a = pixel(&buf, SPARK_W, x, mid).3;
        assert!(a > 0 && a < 150, "area under cap should be translucent, alpha={a}");
    }

    #[test]
    fn spark_dimensions() {
        let buf = render_spark(&[1.0], BURN);
        assert_eq!(buf.len(), SPARK_W * SPARK_H * 4);
    }

    #[test]
    fn unicode_spark_reflects_shape() {
        let s = unicode_spark(&[0.0, 1.0, 2.0, 4.0]);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars[0], '▁');
        assert_eq!(chars[3], '█');
    }
}
