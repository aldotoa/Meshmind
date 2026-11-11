//! Renders a small hard-surface demo model to `demo.png`.
//! Run with `cargo run -p meshmind-render --example demo`.

use glam::Vec3;
use meshmind_core::{csg, ops, primitives, BooleanOp, Material, Scene, Selection};
use meshmind_render::{render_sheet, View};

fn main() {
    let mut scene = Scene::new();

    // A bevelled crate with a recessed panel on top and a hole through it.
    let body = ops::bevel(&primitives::cube(1.0), 0.06, 30.0, 4).unwrap();
    let up = Selection::Facing {
        direction: Vec3::Y,
        max_angle_deg: 5.0,
    };
    let body = ops::inset(&body, &up, 0.25).unwrap();
    let body = ops::extrude(&body, &Selection::Faces { indices: vec![2] }, -0.05).unwrap();
    let drill = primitives::cylinder(0.15, 2.0, 32).transformed(&glam::Affine3A::from_rotation_x(
        std::f32::consts::FRAC_PI_2,
    ));
    let body = csg::boolean(&body, &drill, BooleanOp::Difference);
    let id = scene.add_object("Crate", body).unwrap();
    let paint = scene.add_material(Material {
        name: "Paint".into(),
        base_color: [0.05, 0.2, 0.08, 1.0],
        roughness: 0.45,
        ..Default::default()
    });
    scene.assign_material(id, paint).unwrap();

    let ball = ops::subdivide(&primitives::cube(0.5), 3, true).unwrap();
    let id = scene.add_object("Ball", ball).unwrap();
    scene.object_mut(id).unwrap().transform.translation = Vec3::new(1.0, -0.25, 0.3);
    let gold = scene.add_material(Material {
        name: "Gold".into(),
        base_color: [1.0, 0.75, 0.3, 1.0],
        metallic: 1.0,
        roughness: 0.25,
        ..Default::default()
    });
    scene.assign_material(id, gold).unwrap();

    let sheet = render_sheet(
        &scene,
        &[View::Front, View::Right, View::Top, View::ThreeQuarter],
        384,
    );
    std::fs::write("demo.png", sheet.to_png()).unwrap();
    println!("wrote demo.png");
}
