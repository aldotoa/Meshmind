//! RGBA8 textures produced by baking.

use glam::{Vec2, Vec4};

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, top row first (glTF/PNG convention: v = 0 is the top).
    pub pixels: Vec<u8>,
}

impl Texture {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Texture {
        Texture {
            name: name.into(),
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let o = ((y * self.width + x) * 4) as usize;
        self.pixels[o..o + 4].copy_from_slice(&rgba);
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[o],
            self.pixels[o + 1],
            self.pixels[o + 2],
            self.pixels[o + 3],
        ]
    }

    /// Bilinear sample at `uv` (clamped), values 0 to 1, no colour conversion.
    pub fn sample(&self, uv: Vec2) -> Vec4 {
        let x = (uv.x.clamp(0.0, 1.0) * self.width as f32 - 0.5).max(0.0);
        let y = (uv.y.clamp(0.0, 1.0) * self.height as f32 - 0.5).max(0.0);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |x, y| Vec4::from(self.get(x, y).map(|c| c as f32 / 255.0));
        let top = px(x0, y0).lerp(px(x1, y0), fx);
        let bottom = px(x0, y1).lerp(px(x1, y1), fx);
        top.lerp(bottom, fy)
    }

    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(&self.pixels).expect("png data");
        w.finish().expect("png finish");
        out
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}
