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

    /// Decode an 8-bit RGB or RGBA PNG.
    pub fn from_png(name: impl Into<String>, bytes: &[u8]) -> Result<Texture, String> {
        let decoder = png::Decoder::new(bytes);
        let mut reader = decoder.read_info().map_err(|e| format!("bad PNG: {e}"))?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| format!("bad PNG: {e}"))?;
        if info.bit_depth != png::BitDepth::Eight {
            return Err("only 8-bit PNG textures are supported".into());
        }
        let pixels = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..info.buffer_size()]
                .chunks_exact(3)
                .flat_map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            other => return Err(format!("unsupported PNG colour type {other:?}")),
        };
        Ok(Texture {
            name: name.into(),
            width: info.width,
            height: info.height,
            pixels,
        })
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

/// Textures are stored in project files as base64 PNG, so files stay small
/// and can be inspected with ordinary tools.
impl serde::Serialize for Texture {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Texture", 2)?;
        st.serialize_field("name", &self.name)?;
        st.serialize_field(
            "png",
            &base64::engine::general_purpose::STANDARD.encode(self.to_png()),
        )?;
        st.end()
    }
}

impl<'de> serde::Deserialize<'de> for Texture {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use base64::Engine;
        #[derive(serde::Deserialize)]
        struct Raw {
            name: String,
            png: String,
        }
        let raw = Raw::deserialize(d)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw.png)
            .map_err(serde::de::Error::custom)?;
        Texture::from_png(raw.name, &bytes).map_err(serde::de::Error::custom)
    }
}
