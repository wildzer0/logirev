//! Geometry and painting of the radial menu, independent of the platform.
//!
//! The menu is a ring of up to [`MAX_ITEMS`] sectors around the mouse
//! pointer. This module owns everything that can be reasoned about without
//! Windows: the size (scaled by the monitor DPI), the sector layout, the
//! hit-test used to know which item is under the pointer, the anti-aliased
//! picture of the ring (`paint`) and the compositing of the text masks that
//! the Windows layer draws with GDI (`composite_mask`).
//!
//! Keeping it here means the layout and the look can be unit-tested on any
//! operating system; `ui::windows::radial` only creates the window, draws the
//! labels and handles the mouse.

// The Windows overlay is the only consumer; other platforms run the tests.
#![allow(dead_code)]

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::icons;

/// Most items a menu can have (a sector each; more would not fit).
pub const MAX_ITEMS: usize = 8;
/// Longest label accepted in the configuration (the window ellipsizes).
pub const MAX_LABEL_CHARS: usize = 24;
/// Longest title accepted in the configuration.
pub const MAX_TITLE_CHARS: usize = 32;

/// Icons that can be named in the configuration, as `(name, glyph)`.
///
/// The glyphs come from the Phosphor font already embedded for the settings
/// window, so the menu adds no asset and no dependency.
pub const ICONS: &[(&str, &str)] = &[
    ("app", icons::APP_WINDOW),
    ("browser", icons::BROADCAST),
    ("calculator", icons::GAUGE),
    ("camera", icons::CAMERA),
    ("check", icons::CHECK_CIRCLE),
    ("close", icons::X_CIRCLE),
    ("code", icons::CODE),
    ("copy", icons::COPY),
    ("crosshair", icons::CROSSHAIR),
    ("cut", icons::SCISSORS),
    ("desktop", icons::DESKTOP),
    ("file", icons::FILE_TEXT),
    ("folder", icons::FOLDER_OPEN),
    ("grid", icons::SELECTION_ALL),
    ("info", icons::INFO),
    ("keyboard", icons::KEYBOARD),
    ("left", icons::ARROW_LEFT),
    ("light", icons::LIGHTBULB),
    ("lock", icons::LOCK),
    ("magic", icons::MAGIC_WAND),
    ("mute", icons::SPEAKER_X),
    ("next", icons::SKIP_FORWARD),
    ("paste", icons::CLIPBOARD),
    ("play", icons::PLAY_PAUSE),
    ("power", icons::POWER),
    ("prev", icons::SKIP_BACK),
    ("refresh", icons::ARROW_CLOCKWISE),
    ("right", icons::ARROW_RIGHT),
    ("save", icons::FLOPPY_DISK),
    ("settings", icons::GEAR),
    ("stack", icons::STACK),
    ("sun", icons::SUN),
    ("terminal", icons::TERMINAL_WINDOW),
    ("undo", icons::ARROW_COUNTER_CLOCKWISE),
    ("volume", icons::SPEAKER_HIGH),
    ("warning", icons::WARNING),
    ("windows", icons::SQUARES_FOUR),
];

/// The glyph of a named icon, if the name is known.
pub fn icon_glyph(name: &str) -> Option<&'static str> {
    ICONS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
        .map(|(_, g)| *g)
}

/// Sizes of the menu at one DPI, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Device pixels per DIP (1.0 at 96 DPI).
    pub scale: f32,
    /// Outer radius of the ring.
    pub radius: f32,
    /// Radius of the dead zone in the middle (the close button).
    pub inner: f32,
    /// Width of the soft shadow around the ring.
    pub shadow: f32,
    /// Manual brightness icon size.
    pub icon_size: f32,
    /// Label font size.
    pub label_size: f32,
    /// Title font size.
    pub title_size: f32,
}

/// Size of a sector separator and of the X in the middle, in DIP.
const LINE_DIP: f32 = 1.0;
const CLOSE_ARM_DIP: f32 = 6.0;
const CLOSE_THICKNESS_DIP: f32 = 1.6;

impl Metrics {
    /// Metrics for a monitor at `dpi` (96 = 100 %).
    pub fn new(dpi: u32) -> Self {
        Self::scaled(dpi.max(96) as f32 / 96.0)
    }

    /// Metrics for an explicit scale (used by tests and previews).
    pub fn scaled(scale: f32) -> Self {
        let scale = scale.max(0.5);
        Self {
            scale,
            radius: 140.0 * scale,
            inner: 48.0 * scale,
            shadow: 16.0 * scale,
            icon_size: 22.0 * scale,
            label_size: 12.0 * scale,
            title_size: 10.0 * scale,
        }
    }

    /// Outer radius of the visible panel (the ring plus its rim).
    pub fn panel_radius(&self) -> f32 {
        self.radius + PANEL_DIP * self.scale
    }

    /// Width and height of the picture, panel and shadow included.
    pub fn frame_size(&self) -> (u32, u32) {
        let half = (self.radius + PANEL_DIP * self.scale + self.shadow + 2.0 * self.scale).ceil();
        let size = (half * 2.0).max(1.0) as u32;
        (size, size)
    }

    /// Centre of the ring, relative to the top-left corner of the picture.
    pub fn center(&self) -> (f32, f32) {
        let (w, h) = self.frame_size();
        (w as f32 / 2.0, h as f32 / 2.0)
    }
}

/// A rectangle in picture pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Right edge (excluded).
    pub right: f32,
    /// Bottom edge (excluded).
    pub bottom: f32,
}

impl Rect {
    /// Width of the rectangle.
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// Height of the rectangle.
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Centre of the rectangle.
    pub fn center(&self) -> (f32, f32) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }

    /// Integer bounds `(left, top, width, height)`, rounded outwards.
    pub fn bounds(&self) -> (i32, i32, u32, u32) {
        let left = self.left.floor() as i32;
        let top = self.top.floor() as i32;
        let right = self.right.ceil() as i32;
        let bottom = self.bottom.ceil() as i32;
        (
            left,
            top,
            (right - left).max(0) as u32,
            (bottom - top).max(0) as u32,
        )
    }
}

/// What is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Item number `n`.
    Item(usize),
    /// The close button in the middle.
    Center,
    /// Outside the ring: cancel.
    Outside,
}

/// What to do when the button that opened the menu is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Run this item (the pointer was over it).
    Select(usize),
    /// Keep the menu open for a click (the pointer is still in the middle).
    KeepOpen,
    /// Close without doing anything (the pointer is outside the ring).
    Cancel,
}

/// The "hold, move, release" decision for a pointer at `(dx, dy)`.
pub fn release_action(layout: Layout, m: &Metrics, dx: f32, dy: f32) -> Release {
    match layout.hit(m, dx, dy) {
        Hit::Item(index) => Release::Select(index),
        Hit::Center => Release::KeepOpen,
        Hit::Outside => Release::Cancel,
    }
}

/// Sector layout of a menu with `count` items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    count: usize,
}

impl Layout {
    /// Layout with the given number of items (clamped to 1..=8).
    pub fn new(count: usize) -> Self {
        Self {
            count: count.clamp(1, MAX_ITEMS),
        }
    }

    /// Number of items.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Angle covered by one sector, in radians.
    pub fn step(&self) -> f32 {
        TAU / self.count as f32
    }

    /// Angle of the middle of sector `i` (screen coordinates: 0 = right,
    /// negative = up; sector 0 is centred at 12 o'clock).
    pub fn mid_angle(&self, i: usize) -> f32 {
        (i.min(self.count - 1)) as f32 * self.step() - FRAC_PI_2
    }

    /// Which item is at `(dx, dy)` from the centre.
    pub fn hit(&self, m: &Metrics, dx: f32, dy: f32) -> Hit {
        let d = (dx * dx + dy * dy).sqrt();
        if d < m.inner {
            return Hit::Center;
        }
        if d > m.radius {
            return Hit::Outside;
        }
        let angle = dy.atan2(dx);
        let step = self.step();
        let rel = (angle + FRAC_PI_2 + step / 2.0).rem_euclid(TAU);
        let index = ((rel / step).floor() as usize).min(self.count - 1);
        Hit::Item(index)
    }
}

/// Where one item's text goes, in picture pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Area that contains the icon and the label.
    pub bounds: Rect,
    /// Centre of the icon, when the item has one.
    pub icon: Option<(f32, f32)>,
    /// Area of the label (the platform ellipsizes it).
    pub label: Rect,
}

/// Text placements of the items, in item order.
pub fn item_placements(count: usize, m: &Metrics) -> Vec<Placement> {
    let layout = Layout::new(count);
    let (cx, cy) = m.center();
    let ring = (m.radius - m.inner).max(1.0);
    let label_r = m.inner + ring * 0.55;
    let arc = TAU * label_r / layout.count() as f32;
    // Keep the box inside the picture: the menu is clamped to the screen by
    // the window, but a label that pokes out of the picture would be cut.
    let room = ((m.radius + m.shadow + m.scale) - label_r).max(20.0 * m.scale);
    let width = (arc - 10.0 * m.scale).clamp(40.0 * m.scale, 2.0 * room);
    let height = (m.icon_size + m.label_size + 6.0 * m.scale).clamp(28.0 * m.scale, m.radius);
    (0..layout.count())
        .map(|i| {
            let a = layout.mid_angle(i);
            let (x, y) = (cx + a.cos() * label_r, cy + a.sin() * label_r);
            let bounds = Rect {
                left: x - width / 2.0,
                top: y - height / 2.0,
                right: x + width / 2.0,
                bottom: y + height / 2.0,
            };
            let icon = Some((x, bounds.top + m.icon_size / 2.0 + 2.0 * m.scale));
            let label = Rect {
                left: bounds.left + 2.0 * m.scale,
                top: bounds.top + m.icon_size + 4.0 * m.scale,
                right: bounds.right - 2.0 * m.scale,
                bottom: bounds.bottom,
            };
            Placement {
                bounds,
                icon,
                label,
            }
        })
        .collect()
}

/// Where the menu title goes (under the X in the middle), if it has one.
pub fn title_placement(title: Option<&str>, m: &Metrics) -> Option<Placement> {
    let title = title?.trim();
    if title.is_empty() {
        return None;
    }
    let (cx, cy) = m.center();
    let half = m.inner * 0.62;
    let top = cy + m.inner * 0.18;
    let bounds = Rect {
        left: cx - half,
        top,
        right: cx + half,
        bottom: (top + m.title_size * 1.8).min(cy + m.inner * 0.92),
    };
    Some(Placement {
        bounds,
        icon: None,
        label: bounds,
    })
}

/// A straight-alpha RGBA picture, rows from top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height * 4` bytes, RGBA order.
    pub rgba: Vec<u8>,
}

impl Frame {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width * height * 4],
        }
    }

    /// The colour of one pixel as `[r, g, b, a]`.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

// Colours of the menu. Kept here so the look is described in one place.
// A dark frosted panel holds separated wedges, so the wheel reads as one
// floating control instead of a thin ring.
const SHADOW: [f32; 3] = [0.0, 0.0, 0.0];
const PANEL_IN: [f32; 3] = [12.0, 16.0, 22.0];
const PANEL_OUT: [f32; 3] = [21.0, 26.0, 35.0];
const RING_IN: [f32; 3] = [35.0, 41.0, 52.0];
const RING_OUT: [f32; 3] = [45.0, 53.0, 67.0];
const HOVER_IN: [f32; 3] = [34.0, 211.0, 238.0];
const HOVER_OUT: [f32; 3] = [37.0, 99.0, 235.0];
const HOVER_RIM: [f32; 3] = [186.0, 242.0, 255.0];
const CENTER: [f32; 3] = [22.0, 27.0, 36.0];
const BORDER: [f32; 3] = [58.0, 72.0, 94.0];
const CLOSE: [f32; 3] = [148.0, 163.0, 184.0];
/// Angular gap between two wedges (radians).
const GAP: f32 = 0.022;
/// Extra panel around the ring, in DIP.
const PANEL_DIP: f32 = 8.0;

/// Number of samples per axis used to anti-alias the ring.
const SAMPLES: usize = 4;

/// Paints the wheel: a frosted panel, separated wedges, the centre and the
/// close button.
///
/// `hover` is the item under the pointer, highlighted in the accent colour.
pub fn paint(count: usize, hover: Option<usize>, m: &Metrics) -> Frame {
    let layout = Layout::new(count);
    let (w, h) = m.frame_size();
    let (cx, cy) = m.center();
    let mut frame = Frame::new(w as usize, h as usize);
    let step = layout.step();
    let line = LINE_DIP * m.scale;
    let panel = m.radius + PANEL_DIP * m.scale;
    let (close_y, arm) = (cy - 4.0 * m.scale, CLOSE_ARM_DIP * m.scale);
    let close = CLOSE_THICKNESS_DIP * m.scale;
    let samples = SAMPLES * SAMPLES;

    for y in 0..frame.height {
        for x in 0..frame.width {
            let mut sum = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    let (dx, dy) = (px - cx, py - cy);
                    let d = (dx * dx + dy * dy).sqrt();
                    let (color, a) = if d > panel {
                        let t = ((d - panel) / m.shadow).clamp(0.0, 1.0);
                        (SHADOW, (1.0 - t).powi(2) * 0.38)
                    } else if d > panel - line {
                        (BORDER, 1.0)
                    } else if d < m.inner {
                        let sdy = py - close_y;
                        let adx = dx.abs();
                        let ady = sdy.abs();
                        let on_x = ((adx - ady).abs() < close * 0.75
                            || (adx + sdy).abs() < close * 0.75)
                            && adx.max(ady) < arm;
                        if on_x {
                            (CLOSE, 1.0)
                        } else if d > m.inner - line {
                            (BORDER, 1.0)
                        } else {
                            (CENTER, 0.97)
                        }
                    } else if d > m.radius {
                        // The rim of the frosted panel around the wedges.
                        (lerp(PANEL_IN, PANEL_OUT, d / panel), 0.80)
                    } else {
                        let angle = dy.atan2(dx);
                        let rel = (angle + FRAC_PI_2 + step / 2.0).rem_euclid(TAU);
                        let sector = ((rel / step).floor() as usize).min(layout.count() - 1);
                        let inside = rel - sector as f32 * step;
                        // The gap is measured in pixels along the arc, so it
                        // keeps the same visual width at every radius.
                        let gap = GAP * d;
                        if inside * d < gap || (step - inside) * d < gap {
                            (lerp(PANEL_IN, PANEL_OUT, d / panel), 0.80)
                        } else {
                            let t = ((d - m.inner) / (m.radius - m.inner)).clamp(0.0, 1.0);
                            if hover == Some(sector) {
                                if d > m.radius - 2.2 * m.scale {
                                    (HOVER_RIM, 1.0)
                                } else {
                                    (lerp(HOVER_IN, HOVER_OUT, 0.15 + t * 0.75), 0.97)
                                }
                            } else {
                                (lerp(RING_IN, RING_OUT, t), 0.96)
                            }
                        }
                    };
                    sum[0] += color[0] * a;
                    sum[1] += color[1] * a;
                    sum[2] += color[2] * a;
                    alpha += a;
                }
            }
            if alpha > 0.0 {
                let i = (y * frame.width + x) * 4;
                let a = (alpha / samples as f32 * 255.0).round().clamp(0.0, 255.0) as u8;
                frame.rgba[i] = (sum[0] / alpha).round().clamp(0.0, 255.0) as u8;
                frame.rgba[i + 1] = (sum[1] / alpha).round().clamp(0.0, 255.0) as u8;
                frame.rgba[i + 2] = (sum[2] / alpha).round().clamp(0.0, 255.0) as u8;
                frame.rgba[i + 3] = a;
            }
        }
    }
    frame
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Alpha of each pixel of a BGRA image (GDI writes the coverage in all
/// colour channels; the red one is enough).
pub fn coverage_from_bgra(bytes: &[u8]) -> Vec<u8> {
    bytes.as_chunks::<4>().0.iter().map(|p| p[2]).collect()
}

/// Shrinks a grayscale image by `factor` (box filter).
pub fn downsample(
    gray: &[u8],
    width: usize,
    height: usize,
    factor: usize,
) -> (Vec<u8>, usize, usize) {
    let factor = factor.max(1);
    let (out_w, out_h) = (width / factor, height / factor);
    let mut out = vec![0u8; out_w * out_h];
    for y in 0..out_h {
        for x in 0..out_w {
            let mut sum = 0u32;
            for sy in 0..factor {
                let row = (y * factor + sy) * width;
                for sx in 0..factor {
                    sum += u32::from(gray[row + x * factor + sx]);
                }
            }
            out[y * out_w + x] = (sum / (factor * factor) as u32) as u8;
        }
    }
    (out, out_w, out_h)
}

/// Draws a coverage mask (white text) over `frame`, clipped to its edges.
pub fn composite_mask(
    frame: &mut Frame,
    at: (i32, i32),
    mask: &[u8],
    mask_width: usize,
    mask_height: usize,
    color: [u8; 3],
) {
    for y in 0..mask_height as i32 {
        let fy = at.1 + y;
        if fy < 0 || fy >= frame.height as i32 {
            continue;
        }
        for x in 0..mask_width as i32 {
            let fx = at.0 + x;
            if fx < 0 || fx >= frame.width as i32 {
                continue;
            }
            let coverage = mask[y as usize * mask_width + x as usize];
            if coverage == 0 {
                continue;
            }
            let src_a = f32::from(coverage) / 255.0;
            let i = (fy as usize * frame.width + fx as usize) * 4;
            let dst_a = f32::from(frame.rgba[i + 3]) / 255.0;
            let out_a = src_a + dst_a * (1.0 - src_a);
            if out_a <= 0.0 {
                continue;
            }
            for (c, &src_color) in color.iter().enumerate() {
                let dst = f32::from(frame.rgba[i + c]) * dst_a * (1.0 - src_a);
                let src = f32::from(src_color) * src_a;
                frame.rgba[i + c] = ((src + dst) / out_a).round().clamp(0.0, 255.0) as u8;
            }
            frame.rgba[i + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Converts straight RGBA to the premultiplied BGRA expected by
/// `UpdateLayeredWindow`.
pub fn to_premultiplied_bgra(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.rgba.len());
    for p in frame.rgba.as_chunks::<4>().0 {
        let a = p[3];
        let mul = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
        out.extend_from_slice(&[mul(p[2]), mul(p[1]), mul(p[0]), a]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Metrics {
        Metrics::scaled(0.5)
    }

    #[test]
    fn sector_zero_is_at_the_top() {
        let layout = Layout::new(4);
        let m = m();
        let r = m.radius * 0.75;
        assert_eq!(layout.hit(&m, 0.0, -r), Hit::Item(0));
        assert_eq!(layout.hit(&m, r, 0.0), Hit::Item(1));
        assert_eq!(layout.hit(&m, 0.0, r), Hit::Item(2));
        assert_eq!(layout.hit(&m, -r, 0.0), Hit::Item(3));
        assert_eq!(layout.hit(&m, 0.0, 0.0), Hit::Center);
        assert_eq!(layout.hit(&m, 500.0, 0.0), Hit::Outside);
    }

    #[test]
    fn every_count_has_a_sector_at_every_direction() {
        for count in 1..=MAX_ITEMS {
            let layout = Layout::new(count);
            let m = m();
            let r = m.radius * 0.8;
            let mut seen = vec![false; count];
            for (i, seen) in seen.iter_mut().enumerate() {
                let a = layout.mid_angle(i);
                let (dx, dy) = (a.cos() * r, a.sin() * r);
                if let Hit::Item(hit) = layout.hit(&m, dx, dy) {
                    assert_eq!(hit, i, "count {count}, item {i}");
                } else {
                    panic!("count {count}, item {i} not hit");
                }
                *seen = true;
            }
            assert!(seen.iter().all(|s| *s));
        }
    }

    #[test]
    fn boundary_between_two_items_belongs_to_the_next() {
        let layout = Layout::new(4);
        let m = m();
        let r = m.radius * 0.75;
        // 45 degrees clockwise from the top: the boundary between 0 and 1.
        let a = -FRAC_PI_2 + layout.step() / 2.0;
        assert_eq!(layout.hit(&m, a.cos() * r, a.sin() * r), Hit::Item(1));
    }

    #[test]
    fn item_text_fits_in_the_picture() {
        let m = m();
        let (w, h) = m.frame_size();
        for count in 1..=MAX_ITEMS {
            for p in item_placements(count, &m) {
                assert!(p.bounds.left >= 0.0 && p.bounds.top >= 0.0, "count {count}");
                assert!(
                    p.bounds.right <= w as f32 && p.bounds.bottom <= h as f32,
                    "count {count}: {:?}",
                    p.bounds
                );
                assert!(p.label.top >= p.bounds.top && p.label.bottom <= p.bounds.bottom);
                if let Some((x, y)) = p.icon {
                    assert!(x >= p.bounds.left && x <= p.bounds.right);
                    assert!(y >= p.bounds.top && y <= p.bounds.bottom);
                }
            }
        }
    }

    #[test]
    fn title_is_optional_and_inside_the_centre() {
        let m = m();
        assert!(title_placement(None, &m).is_none());
        assert!(title_placement(Some("  "), &m).is_none());
        let p = title_placement(Some("Work"), &m).expect("title");
        let (cx, cy) = m.center();
        assert!(p.bounds.left < cx && p.bounds.right > cx);
        assert!(p.bounds.top > cy && p.bounds.bottom < cy + m.inner);
    }

    #[test]
    fn paint_has_a_transparent_corner_and_an_opaque_centre() {
        let m = m();
        let frame = paint(4, None, &m);
        assert_eq!(frame.pixel(0, 0)[3], 0, "corner is transparent");
        let (cx, cy) = m.center();
        let p = frame.pixel(cx as usize, (cy + 10.0) as usize);
        assert_eq!(p[3], 247, "centre disc");
        // A point in the middle of the top sector.
        let p = frame.pixel(cx as usize, (cy - m.radius * 0.75) as usize);
        assert_eq!(p[3], 245, "ring");
        assert!(p[0] < 80 && p[2] > p[0], "dark slate ring, got {p:?}");
    }

    #[test]
    fn hovered_sector_is_accent_coloured() {
        let m = m();
        let (cx, cy) = m.center();
        let x = cx as usize;
        let y = (cy - m.radius * 0.75) as usize;
        let normal = paint(4, None, &m).pixel(x, y);
        let hovered = paint(4, Some(0), &m).pixel(x, y);
        assert!(hovered[2] > 180 && hovered[1] > 100, "cyan: {hovered:?}");
        assert_ne!(normal, hovered);
        // A different sector is unchanged.
        let left = (cx - m.radius * 0.75) as usize;
        assert_eq!(
            paint(4, None, &m).pixel(left, cy as usize),
            paint(4, Some(0), &m).pixel(left, cy as usize)
        );
    }

    #[test]
    fn close_button_is_drawn_in_the_middle() {
        let m = m();
        let frame = paint(4, None, &m);
        let (cx, cy) = m.center();
        let p = frame.pixel(cx as usize, (cy - 4.0 * m.scale) as usize);
        assert!(p[3] > 200, "opaque: {p:?}");
        assert!(p[0] > 100 && p[2] > p[0], "the pale close icon: {p:?}");
    }

    #[test]
    fn masks_are_downsampled_and_blended() {
        let gray = vec![
            0, 255, 255, 255, //
            0, 255, 255, 255,
        ];
        let (mask, w, h) = downsample(&gray, 4, 2, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(mask, vec![127, 255]);

        let mut frame = Frame {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 0],
        };
        composite_mask(&mut frame, (0, 0), &[255], 1, 1, [255, 255, 255]);
        assert_eq!(frame.pixel(0, 0), [255, 255, 255, 255]);
        // Outside the frame: ignored, no panic.
        composite_mask(&mut frame, (5, 5), &[255], 1, 1, [255, 255, 255]);
    }

    #[test]
    fn premultiplies_for_the_layered_window() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 128, 1, 2, 3, 0],
        };
        assert_eq!(
            to_premultiplied_bgra(&frame),
            vec![15, 10, 5, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn releasing_selects_keeps_or_cancels() {
        let layout = Layout::new(4);
        let m = m();
        let r = m.radius * 0.75;
        assert_eq!(release_action(layout, &m, 0.0, -r), Release::Select(0));
        assert_eq!(release_action(layout, &m, 0.0, 0.0), Release::KeepOpen);
        assert_eq!(release_action(layout, &m, 0.0, 500.0), Release::Cancel);
    }

    #[test]
    fn icon_names_are_known() {
        assert_eq!(icon_glyph("code"), Some(icons::CODE));
        assert_eq!(icon_glyph("CODE"), Some(icons::CODE));
        assert_eq!(icon_glyph("nope"), None);
        let mut names: Vec<_> = ICONS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ICONS.len());
    }
}
