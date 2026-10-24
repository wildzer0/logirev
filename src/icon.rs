//! The logirev icon, drawn in code.
//!
//! A stylised trackball: a cyan-to-blue disc holding a glossy light ball.
//! Drawing it in code (instead of shipping image files) gives a sharp icon
//! at every size the system asks for — tray, window title bar, taskbar and
//! the `.exe` file itself (`build.rs` includes this file to produce the
//! `.ico` resource).
//!
//! This file must stay free of crate dependencies: `build.rs` compiles it on
//! its own through `#[path]`.

/// Renders the icon as `size`×`size` RGBA pixels (straight alpha, rows from
/// top to bottom).
pub fn draw_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0f32; (size * size * 4) as usize];

    // Soft edge: 1 inside, 0 outside, linear across one pixel.
    let cover = |x: f32, y: f32, cx: f32, cy: f32, r: f32| {
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        (r - d + 0.5).clamp(0.0, 1.0)
    };
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };

    const CYAN: [f32; 3] = [34.0, 211.0, 238.0];
    const BLUE: [f32; 3] = [37.0, 99.0, 235.0];
    const NAVY: [f32; 3] = [23.0, 37.0, 84.0];
    const PEARL: [f32; 3] = [248.0, 250.0, 252.0];
    const SKY: [f32; 3] = [165.0, 205.0, 250.0];
    const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

    let (bx, by, br) = (s * 0.47, s * 0.47, s * 0.27); // the ball
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut layers: Vec<([f32; 3], f32)> = Vec::with_capacity(4);

            // 1. Disc with a diagonal gradient.
            let disc = cover(fx, fy, s / 2.0, s / 2.0, s * 0.47);
            layers.push((lerp(CYAN, BLUE, (fx + fy) / (2.0 * s)), disc));
            // 2. Soft shadow ring around the ball (the socket).
            let ring = cover(fx, fy, bx + s * 0.02, by + s * 0.03, br + s * 0.05);
            layers.push((NAVY, ring * 0.35 * disc));
            // 3. The ball, lighter towards the top-left.
            let ball = cover(fx, fy, bx, by, br);
            let shade = (((fx - bx) + (fy - by)) / (2.0 * br) + 0.6).clamp(0.0, 1.0);
            layers.push((lerp(PEARL, SKY, shade), ball));
            // 4. Specular highlight.
            let spot = cover(fx, fy, bx - br * 0.38, by - br * 0.42, br * 0.28);
            layers.push((WHITE, spot * 0.9));

            // Composite the layers ("over" operator, straight alpha).
            let (mut rgb, mut a) = ([0f32; 3], 0f32);
            for (color, alpha) in layers {
                let out_a = alpha + a * (1.0 - alpha);
                if out_a > 0.0 {
                    for c in 0..3 {
                        rgb[c] = (color[c] * alpha + rgb[c] * a * (1.0 - alpha)) / out_a;
                    }
                }
                a = out_a;
            }
            let i = ((y * size + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&rgb);
            px[i + 3] = a * 255.0;
        }
    }
    px.into_iter()
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
        .collect()
}

/// The icon with a battery gauge along the bottom edge (tray icon).
///
/// The bar is green above 50 %, amber above 20 %, red below; while
/// charging it is cyan.
pub fn draw_with_battery(size: u32, percent: u8, charging: bool) -> Vec<u8> {
    let mut px = draw_rgba(size);
    let s = size as f32;
    let height = (s * 0.22).round().max(4.0) as u32;
    let top = size - height;
    let fill = match (charging, percent) {
        (true, _) => [34, 211, 238],
        (false, 51..) => [22, 163, 74],
        (false, 21..) => [217, 119, 6],
        (false, _) => [220, 38, 38],
    };
    let border = 1u32;
    let inner = size - 2 * border;
    let filled = (f32::from(percent.min(100)) / 100.0 * inner as f32).round() as u32;
    for y in top..size {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            let edge = y == top || y == size - 1 || x < border || x >= size - border;
            let color = if edge {
                [15, 23, 42]
            } else if x - border < filled.max(1) {
                fill
            } else {
                [51, 65, 85]
            };
            px[i..i + 3].copy_from_slice(&color);
            px[i + 3] = 255;
        }
    }
    px
}

/// Encodes several sizes of the icon as a Windows `.ico` file (32-bit
/// bitmaps with alpha, the format every Windows version understands).
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| (s, bmp_entry(s))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]); // reserved, type = icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 }; // 0 means 256
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(&data);
    }
    out
}

/// One `.ico` image: BITMAPINFOHEADER + BGRA rows (bottom-up) + AND mask.
fn bmp_entry(size: u32) -> Vec<u8> {
    let rgba = draw_rgba(size);
    let mask_stride = (size.div_ceil(32) * 4) as usize;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // colour + mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // no compression, sizes and palette unused
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, mask_stride * size as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_round_opaque_icon() {
        let px = draw_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        assert_eq!(px[3], 0, "corners are transparent");
        let centre = ((16 * 32 + 16) * 4) as usize;
        assert_eq!(px[centre + 3], 255, "centre is opaque");
    }

    #[test]
    fn draws_a_battery_gauge() {
        let px = draw_with_battery(16, 60, false);
        // Bottom row, middle: inside the bar, opaque.
        let i = ((14 * 16 + 4) * 4) as usize;
        assert_eq!(&px[i..i + 4], &[22, 163, 74, 255]);
        let i = ((14 * 16 + 13) * 4) as usize;
        assert_eq!(&px[i..i + 4], &[51, 65, 85, 255]);
    }

    #[test]
    fn encodes_ico_directory() {
        let ico = ico(&[16, 32]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        let first_size = u32::from_le_bytes(ico[14..18].try_into().unwrap()) as usize;
        assert_eq!(first_size, 40 + 16 * 16 * 4 + 4 * 16);
    }
}
