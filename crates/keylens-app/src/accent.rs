//! Picks an accent colour from an app icon, so the panel takes on the colour of the app it shows.

/// Hue buckets of 15°.
const BUCKETS: usize = 24;
/// At most this many samples per side; an icon is 64–512 px and the colour needs no more.
const GRID: usize = 48;
/// Below this share of colourful pixels the icon counts as grey (Notion, Terminal…).
const MIN_COLOURFUL: f32 = 0.08;

/// Decodes a PNG app icon and returns its accent colour as RGB, or `None` if it has none.
pub fn from_png(png: &[u8]) -> Option<[u8; 3]> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let rgba = image.to_rgba8();
    dominant_color(rgba.as_raw(), rgba.width() as usize, rgba.height() as usize)
}

/// The most common vivid hue of an RGBA image, averaged over its pixels and adjusted to read well
/// as a UI accent. `None` when transparent and grey pixels make up nearly all of it.
pub fn dominant_color(rgba: &[u8], width: usize, height: usize) -> Option<[u8; 3]> {
    if width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return None;
    }
    let step_x = width.div_ceil(GRID).max(1);
    let step_y = height.div_ceil(GRID).max(1);
    let mut weight = [0f32; BUCKETS];
    let mut sum = [[0f32; 3]; BUCKETS];
    let (mut opaque, mut colourful) = (0usize, 0usize);
    for y in (0..height).step_by(step_y) {
        for x in (0..width).step_by(step_x) {
            let i = (y * width + x) * 4;
            let [r, g, b, a] = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
            if a < 128 {
                continue;
            }
            opaque += 1;
            let (h, s, v) = hsv(r, g, b);
            if s < 0.25 || v < 0.2 {
                continue;
            }
            colourful += 1;
            let bucket = ((h / 360.0 * BUCKETS as f32) as usize).min(BUCKETS - 1);
            let w = s * v;
            weight[bucket] += w;
            sum[bucket][0] += r as f32 * w;
            sum[bucket][1] += g as f32 * w;
            sum[bucket][2] += b as f32 * w;
        }
    }
    if opaque == 0 || (colourful as f32) < opaque as f32 * MIN_COLOURFUL {
        return None;
    }
    let best = (0..BUCKETS).max_by(|&a, &b| weight[a].total_cmp(&weight[b]))?;
    let avg = sum[best].map(|c| c / weight[best]);
    Some(readable(avg))
}

/// Hue in degrees, saturation and value in 0–1.
fn hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { delta / max };
    (h, s, max)
}

/// Keeps the hue but clamps saturation and lightness into a range that reads on both light and
/// dark panels: a pale yellow icon still gives a yellow you can see, a near-black navy a blue.
fn readable([r, g, b]: [f32; 3]) -> [u8; 3] {
    let (h, s, l) = hsl(r / 255.0, g / 255.0, b / 255.0);
    from_hsl(h, s.max(0.55), l.clamp(0.42, 0.58))
}

fn hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let (h, _, _) = hsv((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `size`×`size` image filled by `pixel(x, y)`.
    fn image(size: usize, pixel: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        (0..size * size)
            .flat_map(|i| pixel(i % size, i / size))
            .collect()
    }

    fn hue(rgb: [u8; 3]) -> f32 {
        hsv(rgb[0], rgb[1], rgb[2]).0
    }

    #[test]
    fn picks_the_main_colour_of_a_flat_icon() {
        let blue = image(64, |_, _| [40, 110, 240, 255]);
        let rgb = dominant_color(&blue, 64, 64).unwrap();
        assert!((hue(rgb) - 220.0).abs() < 8.0, "{rgb:?}");
    }

    #[test]
    fn the_larger_colour_wins_over_a_small_badge() {
        // Purple background with a small orange corner.
        let icon = image(64, |x, y| {
            if x < 12 && y < 12 {
                [250, 140, 20, 255]
            } else {
                [120, 40, 160, 255]
            }
        });
        let rgb = dominant_color(&icon, 64, 64).unwrap();
        assert!((hue(rgb) - 280.0).abs() < 10.0, "{rgb:?}");
    }

    #[test]
    fn grey_and_transparent_icons_have_no_accent() {
        let grey = image(64, |x, _| {
            let v = (x * 4) as u8;
            [v, v, v, 255]
        });
        assert_eq!(dominant_color(&grey, 64, 64), None);
        let clear = image(64, |_, _| [255, 0, 0, 0]);
        assert_eq!(dominant_color(&clear, 64, 64), None);
    }

    #[test]
    fn transparent_margins_do_not_count() {
        // A green circle-ish square in a mostly transparent canvas.
        let icon = image(64, |x, y| {
            if (20..44).contains(&x) && (20..44).contains(&y) {
                [30, 180, 90, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let rgb = dominant_color(&icon, 64, 64).unwrap();
        assert!((hue(rgb) - 145.0).abs() < 10.0, "{rgb:?}");
    }

    #[test]
    fn very_dark_or_pale_colours_are_made_readable() {
        let navy = image(8, |_, _| [10, 20, 70, 255]);
        let rgb = dominant_color(&navy, 8, 8).unwrap();
        let (_, _, l) = hsl(
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        );
        assert!((0.41..=0.59).contains(&l), "{rgb:?}");
    }

    #[test]
    fn garbage_is_not_a_png() {
        assert_eq!(from_png(b"not a png"), None);
    }
}
