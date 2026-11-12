//! Bakes each smart material preset onto a small prop and renders them to `smart.png`.
//! Run with `cargo run --release -p meshmind-render --example smart`.

use std::sync::Arc;

use glam::{Affine3A, Vec3};
use meshmind_core::smart::{bake, Preset, SmartMaterial};
use meshmind_core::uv::{unwrap, UnwrapOptions};
use meshmind_core::{
    ops, primitives, LightingPreset, Material, MaterialTextures, PolyMesh, Scene, Selection,
};
use meshmind_render::{render_sheet, View};

fn panel_box() -> PolyMesh {
    let m = ops::bevel(&primitives::cube(1.0), 0.04, 30.0, 3).unwrap();
    let mut m = m;
    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
        let sel = Selection::Facing {
            direction: dir,
            max_angle_deg: 2.0,
        };
        m = ops::inset(&m, &sel, 0.18).unwrap();
        m = ops::extrude(&m, &sel, -0.03).unwrap();
    }
    m
}

fn main() {
    let lighting = std::env::args()
        .nth(1)
        .and_then(|n| LightingPreset::from_name(&n))
        .unwrap_or_default();
    let mut scene = Scene::new();
    scene.lighting = lighting;
    let shapes: Vec<(Preset, PolyMesh)> = vec![
        (Preset::PaintedMetal, panel_box()),
        (
            Preset::RustyMetal,
            ops::bevel(&primitives::cylinder(0.45, 1.0, 40), 0.03, 30.0, 3).unwrap(),
        ),
        (
            Preset::BareMetal,
            ops::bevel(&primitives::cylinder(0.3, 0.8, 6), 0.03, 30.0, 3).unwrap(),
        ),
        (
            Preset::Wood,
            ops::bevel(&primitives::cube(1.0), 0.03, 30.0, 2).unwrap(),
        ),
        (
            Preset::Stone,
            ops::bevel(&primitives::cube(0.9), 0.06, 30.0, 1).unwrap(),
        ),
        (
            Preset::Plastic,
            ops::subdivide(&primitives::cube(0.8), 2, true).unwrap(),
        ),
    ];
    for (i, (preset, mut mesh)) in shapes.into_iter().enumerate() {
        unwrap(&mut mesh, &UnwrapOptions::default()).unwrap();
        let mut sm = SmartMaterial::new(preset);
        sm.wear = 0.6;
        sm.dirt = 0.5;
        if preset == Preset::Plastic {
            sm.color = Some([0.6, 0.08, 0.05]);
        }
        let baked = bake(&mesh, &sm, 1024).unwrap();
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        let placed = mesh.transformed(&Affine3A::from_translation(Vec3::new(
            col * 1.6,
            0.0,
            -row * 1.7,
        )));
        let id = scene.add_object(preset.name(), placed).unwrap();
        let mat = scene.add_material(Material {
            name: preset.name().into(),
            base_color: [1.0; 4],
            metallic: 1.0,
            roughness: 1.0,
            textures: Some(Arc::new(MaterialTextures {
                base_color: baked.base_color,
                metallic_roughness: baked.metallic_roughness,
                normal: baked.normal,
            })),
            ..Default::default()
        });
        scene.assign_material(id, mat).unwrap();
    }
    let img = render_sheet(&scene, &[View::ThreeQuarter], 1000);
    std::fs::write("smart.png", img.to_png()).unwrap();
    println!("wrote smart.png ({})", lighting.name());
}
