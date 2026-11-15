//! Image thumbnails (protocol.md §13.2): fit in 160 × 120, AVIF, a small preview sent first and
//! a full variant sent on request.

use ravif::{BitDepth, Encoder, Img, RGB8};

pub const MAX_WIDTH: u32 = 160;
pub const MAX_HEIGHT: u32 = 120;
/// Largest preview: with its 21 bytes of FILE header it fills 3 fragments of 149 bytes on one
/// hop (§13.2).
pub const PREVIEW_MAX: usize = 426;
/// Largest full thumbnail.
pub const FULL_MAX: usize = 4_096;

/// Encoder qualities tried in turn, best first, until the image fits.
const FULL_QUALITIES: [f32; 7] = [80.0, 70.0, 60.0, 50.0, 40.0, 30.0, 20.0];
const PREVIEW_QUALITIES: [f32; 6] = [40.0, 30.0, 22.0, 15.0, 10.0, 5.0];
/// rav1e speed: slow is fine for 19,200 pixels.
const SPEED: u8 = 4;

/// The two variants of a thumbnail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u16,
    pub height: u16,
    pub preview: Vec<u8>,
    pub full: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// `rgba` is not `width × height × 4` bytes, or a side is zero.
    BadSize,
    /// Even the lowest quality does not fit.
    TooLarge,
    Encode,
}

/// A full-size AVIF of an RGBA8 image (alpha ignored), for the Internet bridge (§14).
pub fn encode_avif(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: f32,
) -> Result<Vec<u8>, ImageError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageError::BadSize)?;
    if width == 0 || height == 0 || rgba.len() != expected {
        return Err(ImageError::BadSize);
    }
    let pixels = downscale(rgba, width, height, width, height);
    Encoder::new()
        .with_quality(quality.clamp(1.0, 100.0))
        .with_speed(8)
        .with_bit_depth(BitDepth::Eight)
        .with_num_threads(Some(1))
        .encode_rgb(Img::new(pixels.as_slice(), width as usize, height as usize))
        .map(|e| e.avif_file)
        .map_err(|_| ImageError::Encode)
}

/// Size that fits in 160 × 120 with the same aspect ratio (never upscaled).
pub fn fit(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    if w <= MAX_WIDTH && h <= MAX_HEIGHT {
        return (w, h);
    }
    // Compare w/h with 160/120 without floats.
    if u64::from(w) * u64::from(MAX_HEIGHT) >= u64::from(h) * u64::from(MAX_WIDTH) {
        let nh = (u64::from(h) * u64::from(MAX_WIDTH) / u64::from(w)).max(1);
        (MAX_WIDTH, nh as u32)
    } else {
        let nw = (u64::from(w) * u64::from(MAX_HEIGHT) / u64::from(h)).max(1);
        (nw as u32, MAX_HEIGHT)
    }
}

/// The thumbnail of an RGBA8 image (alpha is ignored, composited on white).
pub fn thumbnail(rgba: &[u8], width: u32, height: u32) -> Result<Thumbnail, ImageError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageError::BadSize)?;
    if width == 0 || height == 0 || rgba.len() != expected {
        return Err(ImageError::BadSize);
    }
    let (w, h) = fit(width, height);
    let pixels = downscale(rgba, width, height, w, h);
    let full = encode_fitting(&pixels, w, h, &FULL_QUALITIES, FULL_MAX)?;
    let preview = encode_fitting(&pixels, w, h, &PREVIEW_QUALITIES, PREVIEW_MAX).or_else(|_| {
        // Too much detail for a preview: half the size, the system scales it up.
        let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
        let half = downscale_rgb(&pixels, w, h, hw, hh);
        encode_fitting(&half, hw, hh, &PREVIEW_QUALITIES, PREVIEW_MAX)
    })?;
    Ok(Thumbnail {
        width: w as u16,
        height: h as u16,
        preview,
        full,
    })
}

fn encode_fitting(
    pixels: &[RGB8],
    w: u32,
    h: u32,
    qualities: &[f32],
    max: usize,
) -> Result<Vec<u8>, ImageError> {
    for &q in qualities {
        let img = Img::new(pixels, w as usize, h as usize);
        let out = Encoder::new()
            .with_quality(q)
            .with_speed(SPEED)
            .with_bit_depth(BitDepth::Eight)
            .with_num_threads(Some(1))
            .encode_rgb(img)
            .map_err(|_| ImageError::Encode)?;
        if out.avif_file.len() <= max {
            return Ok(out.avif_file);
        }
    }
    Err(ImageError::TooLarge)
}

/// Area average from RGBA (alpha composited on white) to `dw × dh` RGB.
fn downscale(rgba: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<RGB8> {
    let px = |x: u32, y: u32| -> [u32; 3] {
        let i = (y as usize * sw as usize + x as usize) * 4;
        match rgba.get(i..i + 4) {
            Some(&[r, g, b, a]) => {
                let a = u32::from(a);
                let over = |c: u8| (u32::from(c) * a + 255 * (255 - a)) / 255;
                [over(r), over(g), over(b)]
            }
            _ => [255; 3],
        }
    };
    average(sw, sh, dw, dh, px)
}

fn downscale_rgb(src: &[RGB8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<RGB8> {
    let px = |x: u32, y: u32| -> [u32; 3] {
        src.get(y as usize * sw as usize + x as usize)
            .map_or([255; 3], |p| [p.r.into(), p.g.into(), p.b.into()])
    };
    average(sw, sh, dw, dh, px)
}

fn average(sw: u32, sh: u32, dw: u32, dh: u32, px: impl Fn(u32, u32) -> [u32; 3]) -> Vec<RGB8> {
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..dh {
        let (y0, y1) = span(y, dh, sh);
        for x in 0..dw {
            let (x0, x1) = span(x, dw, sw);
            let mut sum = [0u64; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = px(sx, sy);
                    for (s, c) in sum.iter_mut().zip(p) {
                        *s += u64::from(c);
                    }
                }
            }
            let n = (u64::from(x1 - x0) * u64::from(y1 - y0)).max(1);
            let c = |i: usize| (sum.get(i).copied().unwrap_or(0) / n).min(255) as u8;
            out.push(RGB8::new(c(0), c(1), c(2)));
        }
    }
    out
}

/// Source pixels `[a, b)` covered by destination pixel `i` of `dn` over `sn`.
fn span(i: u32, dn: u32, sn: u32) -> (u32, u32) {
    let a = (u64::from(i) * u64::from(sn) / u64::from(dn)) as u32;
    let b = ((u64::from(i + 1) * u64::from(sn)).div_ceil(u64::from(dn)) as u32).max(a + 1);
    (a, b.min(sn))
}

/// A mountain landscape at dusk (sky, two ridges, snow, a hut with a lit window), RGBA8: for the
/// demo network and tests.
pub fn sample_landscape(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let ridge1 = 0.45 + 0.15 * (fx * 7.0).sin() + 0.08 * (fx * 17.0).cos();
            let ridge2 = 0.62 + 0.06 * (fx * 11.0 + 1.0).sin();
            let mut c = [
                (40.0 + 180.0 * fy) as u8,
                (60.0 + 120.0 * fy) as u8,
                (140.0 + 60.0 * fy) as u8,
            ];
            if fy > ridge1 {
                c = if fy < ridge1 + 0.04 {
                    [235, 240, 245]
                } else {
                    [70, 80, 95]
                };
            }
            if fy > ridge2 {
                c = [40, 70, 45];
            }
            let hut = (0.6..0.7).contains(&fx) && (0.72..0.82).contains(&fy);
            if hut {
                c = if (0.63..0.66).contains(&fx) && (0.75..0.79).contains(&fy) {
                    [250, 200, 90]
                } else {
                    [120, 70, 40]
                };
            }
            v.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_the_box() {
        assert_eq!(fit(4032, 3024), (160, 120));
        assert_eq!(fit(3024, 4032), (90, 120));
        assert_eq!(fit(1000, 100), (160, 16));
        assert_eq!(fit(100, 50), (100, 50));
        assert_eq!(fit(0, 0), (1, 1));
    }

    #[test]
    fn photo_sizes() {
        let (w, h) = (1600, 1200);
        let t = thumbnail(&sample_landscape(w, h), w, h).unwrap();
        eprintln!(
            "thumbnail {}x{}: preview {} B, full {} B",
            t.width,
            t.height,
            t.preview.len(),
            t.full.len()
        );
        if let Ok(dir) = std::env::var("LICHEN_DUMP") {
            std::fs::write(format!("{dir}/preview.avif"), &t.preview).unwrap();
            std::fs::write(format!("{dir}/full.avif"), &t.full).unwrap();
        }
        assert_eq!((t.width, t.height), (160, 120));
        assert!(t.preview.len() <= PREVIEW_MAX);
        assert!(t.full.len() <= FULL_MAX && t.full.len() > t.preview.len());
        // An AVIF file: ftyp box, brand avif.
        assert_eq!(&t.full[4..12], b"ftypavif");
        assert_eq!(&t.preview[4..12], b"ftypavif");
    }

    #[test]
    fn bad_input() {
        assert_eq!(thumbnail(&[0; 15], 2, 2), Err(ImageError::BadSize));
        assert_eq!(thumbnail(&[], 0, 0), Err(ImageError::BadSize));
        assert_eq!(
            thumbnail(&[0; 4], u32::MAX, u32::MAX),
            Err(ImageError::BadSize)
        );
    }

    #[test]
    fn downscale_averages() {
        // 2×2 → 1×1: black, white, red, transparent (white).
        let rgba = [0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 9, 9, 9, 0];
        let p = downscale(&rgba, 2, 2, 1, 1);
        assert_eq!(p, vec![RGB8::new(191, 127, 127)]);
    }
}
