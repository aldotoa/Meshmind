//! Renders sharp, chamfered and rounded versions of the same shapes to `bevels.png`.
//! Run with `cargo run -p meshmind-render --example bevels`.

use glam::{Affine3A, Vec3};
use meshmind_core::{ops, primitives, Material, PolyMesh, Scene, Selection};
use meshmind_render::{render_sheet, View};

fn stepped_block() -> PolyMesh {
    let m = ops::subdivide(&primitives::cube(1.0), 1, false).unwrap();
    let quarter = m
        .faces
        .iter()
        .position(|f| {
            m.face_normal(f).y > 0.9 && m.face_centroid(f).x > 0.0 && m.face_centroid(f).z > 0.0
        })
        .unwrap();
    ops::extrude(
        &m,
        &Selection::Faces {
            indices: vec![quarter],
        },
        0.3,
    )
    .unwrap()
}

fn main() {
    let mut scene = Scene::new();
    let paint = scene.add_material(Material {
        name: "Paint".into(),
        base_color: [0.5, 0.18, 0.04, 1.0],
        roughness: 0.4,
        ..Default::default()
    });
    let shapes: [(&str, PolyMesh); 3] = [
        ("block", primitives::cube(1.0)),
        ("cylinder", primitives::cylinder(0.5, 1.0, 48)),
        ("stepped", stepped_block()),
    ];
    for (row, (name, shape)) in shapes.iter().enumerate() {
        let versions = [
            shape.clone(),
            ops::bevel(shape, 0.08, 30.0, 1).unwrap(),
            ops::bevel(shape, 0.08, 30.0, 5).unwrap(),
        ];
        for (col, mesh) in versions.into_iter().enumerate() {
            let placed = mesh.transformed(&Affine3A::from_translation(Vec3::new(
                col as f32 * 1.5,
                0.0,
                -(row as f32) * 1.6,
            )));
            let id = scene.add_object(format!("{name} {col}"), placed).unwrap();
            scene.assign_material(id, paint).unwrap();
        }
    }
    let img = render_sheet(&scene, &[View::ThreeQuarter], 900);
    std::fs::write("bevels.png", img.to_png()).unwrap();
    println!("wrote bevels.png (columns: sharp, chamfer, rounded)");
}
