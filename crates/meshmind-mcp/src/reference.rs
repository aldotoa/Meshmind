//! Reference images: load concept art and compare the model with it.

use std::sync::Arc;

use meshmind_core::reference::{decode_image, subject_bounds, subject_mask, Reference};
use meshmind_core::Scene;
use meshmind_render::{render_view_framed, Image, View};
use serde_json::{json, Value};

use crate::tools::ToolOutput;

const VIEWS: [&str; 5] = ["front", "back", "left", "right", "three_quarter"];

pub fn set_reference(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("`path` (a PNG or JPEG file) is required")?;
    let view = args.get("view").and_then(Value::as_str).unwrap_or("front");
    if !VIEWS.contains(&view) {
        return Err(format!("`view` must be one of {}", VIEWS.join(", ")));
    }
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| view.to_string());
    let bytes = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
    let image = decode_image(&name, &bytes, 1024)?;
    let (w, h) = (image.width, image.height);
    let found = subject_bounds(&image).is_some();
    scene.references.retain(|r| r.name != name);
    scene.references.push(Reference {
        name: name.clone(),
        view: view.to_string(),
        image: Arc::new(image),
    });
    let mut out = json!({ "name": name, "view": view, "width": w, "height": h,
        "references": scene.references.iter().map(|r| r.name.clone()).collect::<Vec<_>>() });
    if !found {
        out["note"] = json!("Couldn't separate the subject from the background, so measurements will use the whole image. A plain background works best.");
    }
    Ok(out)
}

/// Columns covered in one row of a mask, as (first, last).
fn row_span(mask: &dyn Fn(u32, u32) -> bool, y: u32, x0: u32, x1: u32) -> Option<(u32, u32)> {
    let first = (x0..=x1).find(|&x| mask(x, y))?;
    let last = (x0..=x1).rev().find(|&x| mask(x, y))?;
    Some((first, last))
}

pub fn compare_to_reference(scene: &Scene, args: &Value) -> Result<ToolOutput, String> {
    if scene.references.is_empty() {
        return Err("no reference image yet: call set_reference with a PNG or JPEG first".into());
    }
    let r = match args.get("name").and_then(Value::as_str) {
        Some(n) => scene
            .references
            .iter()
            .find(|r| r.name == n)
            .ok_or(format!("no reference named `{n}`"))?,
        None => scene.references.last().expect("not empty"),
    };
    let view = View::from_name(&r.view).unwrap_or(View::Front);
    let size = args
        .get("size")
        .and_then(Value::as_u64)
        .unwrap_or(448)
        .clamp(192, 768) as u32;
    let render = render_view_framed(scene, view, size, None, None);
    if render.coverage.is_empty() {
        return Err("the scene is empty".into());
    }
    let refimg = &r.image;
    // Reference panel: scaled to the render's height.
    let rw = (refimg.width as f32 * size as f32 / refimg.height as f32)
        .round()
        .max(1.0) as u32;
    let scale = refimg.height as f32 / size as f32;
    let mut ref_panel = Image::new(rw, size);
    for y in 0..size {
        for x in 0..rw {
            let p = refimg.get(
                ((x as f32 + 0.5) * scale).min(refimg.width as f32 - 1.0) as u32,
                ((y as f32 + 0.5) * scale).min(refimg.height as f32 - 1.0) as u32,
            );
            let o = ((y * rw + x) * 4) as usize;
            ref_panel.pixels[o..o + 4].copy_from_slice(&[p[0], p[1], p[2], 255]);
        }
    }

    // Model silhouette box in the render, subject box in the reference panel.
    let covered = |x: u32, y: u32| render.coverage[(y * size + x) as usize] > 127;
    let (mut mx0, mut my0, mut mx1, mut my1) = (size, size, 0, 0);
    for y in 0..size {
        if let Some((a, b)) = row_span(&covered, y, 0, size - 1) {
            mx0 = mx0.min(a);
            mx1 = mx1.max(b);
            my0 = my0.min(y);
            my1 = my1.max(y);
        }
    }
    if mx1 <= mx0 || my1 <= my0 {
        return Err("the model is not visible from this view".into());
    }
    let subject = subject_bounds(refimg);
    let (sx0, sy0, sx1, sy1) = subject
        .map(|(a, b, c, d)| {
            (
                (a as f32 / scale) as u32,
                (b as f32 / scale) as u32,
                ((c as f32 / scale) as u32).min(rw - 1),
                ((d as f32 / scale) as u32).min(size - 1),
            )
        })
        .unwrap_or((0, 0, rw - 1, size - 1));

    // Map model pixels onto the reference: match heights, centre horizontally.
    let k = (sy1 - sy0) as f32 / (my1 - my0) as f32;
    // Align the centres of mass, which a cast shadow or a prop held to one
    // side moves less than the bounding box.
    let full_mask = subject_mask(refimg);
    let centroid = |n: u32, m: &dyn Fn(u32, u32) -> bool, w: u32| {
        let (mut sum, mut count) = (0f64, 0f64);
        for y in 0..n {
            for x in 0..w {
                if m(x, y) {
                    sum += x as f64;
                    count += 1.0;
                }
            }
        }
        (sum / count.max(1.0)) as f32
    };
    let mcx = centroid(size, &covered, size);
    let scx = if subject.is_some() {
        centroid(
            refimg.height,
            &|x, y| full_mask[(y * refimg.width + x) as usize],
            refimg.width,
        ) / scale
    } else {
        (sx0 + sx1) as f32 / 2.0
    };
    let model_at = |x: u32, y: u32| -> bool {
        let mx = ((x as f32 - scx) / k + mcx).round();
        let my = ((y as f32 - sy0 as f32) / k + my0 as f32).round();
        mx >= 0.0
            && my >= 0.0
            && (mx as u32) < size
            && (my as u32) < size
            && covered(mx as u32, my as u32)
    };

    // Overlay panel: the reference with the model's outline drawn on it.
    let mut overlay = Image::new(rw, size);
    overlay.pixels.copy_from_slice(&ref_panel.pixels);
    for y in 1..size - 1 {
        for x in 1..rw - 1 {
            let c = model_at(x, y);
            let edge = c != model_at(x + 1, y)
                || c != model_at(x, y + 1)
                || c != model_at(x - 1, y)
                || c != model_at(x, y - 1);
            if edge {
                let o = ((y * rw + x) * 4) as usize;
                overlay.pixels[o..o + 4].copy_from_slice(&[0, 230, 255, 255]);
            }
        }
    }

    // Width profile: compare widths in ten height bands.
    let ref_mask = |x: u32, y: u32| -> bool {
        let fx = ((x as f32 + 0.5) * scale).min(refimg.width as f32 - 1.0) as u32;
        let fy = ((y as f32 + 0.5) * scale).min(refimg.height as f32 - 1.0) as u32;
        subject.is_some() && full_mask[(fy * refimg.width + fx) as usize]
    };
    let mut bands = Vec::new();
    let mut notes = Vec::new();
    if subject.is_some() {
        for b in 0..10 {
            let y = sy0 + ((sy1 - sy0) as f32 * (b as f32 + 0.5) / 10.0) as u32;
            let rwid = row_span(&ref_mask, y, sx0, sx1)
                .map(|(a, c)| (c - a) as f32)
                .unwrap_or(0.0);
            let mwid = row_span(&model_at, y, 0, rw - 1)
                .map(|(a, c)| (c - a) as f32)
                .unwrap_or(0.0);
            let h = (sy1 - sy0) as f32;
            bands.push(json!({ "band": b, "reference_width": (rwid / h * 100.0).round() / 100.0, "model_width": (mwid / h * 100.0).round() / 100.0 }));
            if rwid > h * 0.03 || mwid > h * 0.03 {
                let ratio = mwid / rwid.max(1.0);
                if !(0.85..=1.15).contains(&ratio) {
                    notes.push(format!(
                        "{} at {}% down from the top (band {b}): the model is {:.0}% of the reference's width",
                        if ratio > 1.0 { "Too wide" } else { "Too narrow" },
                        b * 10 + 5,
                        ratio * 100.0
                    ));
                }
            }
        }
    }
    let ref_aspect = (sx1 - sx0) as f32 / (sy1 - sy0).max(1) as f32;
    let model_aspect = (mx1 - mx0) as f32 / (my1 - my0) as f32;

    // Sheet: reference | render | overlay.
    let gap = 4;
    let mut sheet = Image::new(rw * 2 + size + gap * 2, size);
    sheet
        .pixels
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&[255, 255, 255, 255]));
    sheet.blit(&ref_panel, 0, 0);
    sheet.blit(&render, rw + gap, 0);
    sheet.blit(&overlay, rw + size + gap * 2, 0);

    let data = json!({
        "reference": r.name,
        "view": r.view,
        "panels": ["reference", "model", "model outline (cyan) over the reference, matched by height"],
        "subject_found": subject.is_some(),
        "width_to_height": { "reference": (ref_aspect * 100.0).round() / 100.0, "model": (model_aspect * 100.0).round() / 100.0 },
        "width_by_height_band": bands,
        "differences": notes,
        "next": "Look at the three panels. Fix silhouette and proportion differences first (big shapes), then missing parts, then surface detail and materials. Compare again after each round."
    });
    Ok(ToolOutput {
        data,
        image_png: Some(sheet.to_png()),
    })
}
