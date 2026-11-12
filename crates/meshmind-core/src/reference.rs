//! Reference images (concept art, photos) the agent compares its model with.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::texture::Texture;

/// One reference image and the view it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    /// The camera view it matches: front, back, left, right or three_quarter.
    pub view: String,
    pub image: Arc<Texture>,
}

/// Decode a PNG or JPEG file's bytes, shrinking it to at most `max_side`
/// pixels on its longest side.
pub fn decode_image(name: &str, bytes: &[u8], max_side: u32) -> Result<Texture, String> {
    let tex = if bytes.starts_with(b"\x89PNG") {
        Texture::from_png(name, bytes)?
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut dec = jpeg_decoder::Decoder::new(bytes);
        let pixels = dec.decode().map_err(|e| format!("bad JPEG: {e}"))?;
        let info = dec.info().ok_or("bad JPEG: no image info")?;
        let rgba: Vec<u8> = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => pixels
                .chunks_exact(3)
                .flat_map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
            other => return Err(format!("unsupported JPEG pixel format {other:?}")),
        };
        Texture {
            name: name.into(),
            width: info.width as u32,
            height: info.height as u32,
            pixels: rgba,
        }
    } else {
        return Err("reference images must be PNG or JPEG".into());
    };
    Ok(shrink(tex, max_side))
}

/// Box-filter an image down so its longest side is at most `max_side`.
pub fn shrink(t: Texture, max_side: u32) -> Texture {
    let longest = t.width.max(t.height);
    if longest <= max_side {
        return t;
    }
    let f = longest.div_ceil(max_side);
    let (w, h) = (t.width / f, t.height / f);
    let mut out = Texture::new(t.name.clone(), w, h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let p = t.get(x * f + dx, y * f + dy);
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            out.set(x, y, sum.map(|s| (s / (f * f)) as u8));
        }
    }
    out
}

/// Which pixels belong to the subject rather than the background. The
/// background is estimated per row from the two edge pixels, so plain and
/// vertically graded backdrops both work; transparent pixels are background.
pub fn subject_mask(t: &Texture) -> Vec<bool> {
    let (w, h) = (t.width, t.height);
    let mut mask = vec![false; (w * h) as usize];
    if w < 4 || h < 4 {
        return mask;
    }
    for y in 0..h {
        let (l, r) = (t.get(0, y), t.get(w - 1, y));
        for x in 0..w {
            let p = t.get(x, y);
            if p[3] < 128 {
                continue;
            }
            let far = |c: [u8; 4]| {
                (0..3)
                    .map(|i| (p[i] as i32 - c[i] as i32).abs())
                    .sum::<i32>()
                    > 60
            };
            mask[(y * w + x) as usize] = far(l) && far(r);
        }
    }
    mask
}

/// The subject's bounding box in a reference image (x0, y0, x1, y1).
pub fn subject_bounds(t: &Texture) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (t.width, t.height);
    let mask = subject_mask(t);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if mask[(y * w + x) as usize] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    // Ignore specks: the subject must be a real part of the picture.
    if x1 <= x0 + w / 20 || y1 <= y0 + h / 20 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_subject_on_a_plain_background() {
        let mut t = Texture::new("ref", 100, 80);
        for y in 0..80 {
            for x in 0..100 {
                let inside = (30..60).contains(&x) && (10..70).contains(&y);
                t.set(
                    x,
                    y,
                    if inside {
                        [200, 40, 40, 255]
                    } else {
                        [240, 240, 240, 255]
                    },
                );
            }
        }
        assert_eq!(subject_bounds(&t), Some((30, 10, 59, 69)));
        let png = t.to_png();
        let back = decode_image("ref", &png, 50).unwrap();
        assert_eq!((back.width, back.height), (50, 40));
        assert!(decode_image("x", b"GIF89a", 50).is_err());
    }
}
