//! Tool definitions and handlers. Descriptions are written for a model to read.

use glam::{EulerRot, Quat, Vec3};
use meshmind_core::{Material, Primitive, Scene};
use serde_json::{json, Value};

pub fn definitions() -> Value {
    json!([
        {
            "name": "create_primitive",
            "description": "Add a new object built from a basic shape, centred on the origin. \
                Returns its id. Start most models from one or more primitives.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "shape": { "type": "string", "enum": ["cube", "plane", "sphere", "cylinder"] },
                    "name": { "type": "string", "description": "Optional display name." },
                    "size": { "type": "number", "description": "Edge length for cube and plane. Default 1." },
                    "radius": { "type": "number", "description": "For sphere and cylinder. Default 0.5." },
                    "height": { "type": "number", "description": "For cylinder. Default 1." },
                    "segments": { "type": "integer", "description": "Around the axis, for sphere and cylinder. Default 32." },
                    "rings": { "type": "integer", "description": "Top to bottom, for sphere. Default 16." }
                },
                "required": ["shape"]
            }
        },
        {
            "name": "list_objects",
            "description": "List every object in the scene with its id, name, transform, \
                bounding box size, triangle count and material id.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "set_transform",
            "description": "Move, rotate or scale an object. Omitted fields stay as they are. \
                Rotation is Euler angles in degrees applied in X, Y, Z order.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "translation": { "$ref": "#/$defs/vec3" },
                    "rotation_degrees": { "$ref": "#/$defs/vec3" },
                    "scale": { "$ref": "#/$defs/vec3" }
                },
                "required": ["id"],
                "$defs": { "vec3": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 } }
            }
        },
        {
            "name": "set_material",
            "description": "Create a PBR material and assign it to an object. Colors are linear \
                RGB from 0 to 1. Metals use metallic 1; most other surfaces use 0. Lower \
                roughness is shinier.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Object to assign the material to." },
                    "name": { "type": "string" },
                    "base_color": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 4 },
                    "metallic": { "type": "number", "minimum": 0, "maximum": 1 },
                    "roughness": { "type": "number", "minimum": 0, "maximum": 1 },
                    "emissive": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }
                },
                "required": ["id"]
            }
        },
        {
            "name": "delete_object",
            "description": "Remove an object from the scene.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }
        }
    ])
}

/// Run a tool. `None` means the tool name is unknown.
pub fn call(scene: &mut Scene, name: &str, args: &Value) -> Option<Result<Value, String>> {
    Some(match name {
        "create_primitive" => create_primitive(scene, args),
        "list_objects" => Ok(list_objects(scene)),
        "set_transform" => set_transform(scene, args),
        "set_material" => set_material(scene, args),
        "delete_object" => object_id(args)
            .and_then(|id| scene.remove_object(id).map_err(|e| e.to_string()))
            .map(|o| json!({ "deleted": o.id })),
        _ => return None,
    })
}

fn num(args: &Value, key: &str, default: f32) -> Result<f32, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = v.as_f64().ok_or(format!("`{key}` must be a number"))? as f32;
            if n.is_finite() && n > 0.0 {
                Ok(n)
            } else {
                Err(format!("`{key}` must be a positive number"))
            }
        }
    }
}

fn int(args: &Value, key: &str, default: u32) -> Result<u32, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (3..=512).contains(n))
            .map(|n| n as u32)
            .ok_or(format!("`{key}` must be an integer from 3 to 512")),
    }
}

fn floats<const N: usize>(args: &Value, key: &str) -> Result<Option<[f32; N]>, String> {
    let Some(v) = args.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let arr = v
        .as_array()
        .ok_or(format!("`{key}` must be an array of {N} numbers"))?;
    if arr.len() != N {
        return Err(format!("`{key}` must have exactly {N} numbers"));
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(arr) {
        *o = x
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or(format!("`{key}` must contain only numbers"))? as f32;
    }
    Ok(Some(out))
}

fn object_id(args: &Value) -> Result<u32, String> {
    args.get("id")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .ok_or("`id` (object id) is required".into())
}

fn create_primitive(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let shape = args
        .get("shape")
        .and_then(Value::as_str)
        .ok_or("`shape` is required")?;
    let primitive = match shape {
        "cube" => Primitive::Cube {
            size: num(args, "size", 1.0)?,
        },
        "plane" => Primitive::Plane {
            size: num(args, "size", 1.0)?,
        },
        "sphere" => Primitive::UvSphere {
            radius: num(args, "radius", 0.5)?,
            segments: int(args, "segments", 32)?,
            rings: int(args, "rings", 16)?,
        },
        "cylinder" => Primitive::Cylinder {
            radius: num(args, "radius", 0.5)?,
            height: num(args, "height", 1.0)?,
            segments: int(args, "segments", 32)?,
        },
        other => {
            return Err(format!(
                "unknown shape `{other}`; use cube, plane, sphere or cylinder"
            ))
        }
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(primitive.default_name());
    let id = scene
        .add_object(name, primitive.build())
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "name": name }))
}

fn list_objects(scene: &Scene) -> Value {
    let objects: Vec<Value> = scene
        .objects()
        .iter()
        .map(|o| {
            let t = &o.transform;
            let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
            let size = o.mesh.bounds().map(|b| (b.size() * t.scale).to_array());
            json!({
                "id": o.id,
                "name": o.name,
                "translation": t.translation.to_array(),
                "rotation_degrees": [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                "scale": t.scale.to_array(),
                "size": size,
                "triangles": o.mesh.triangle_count(),
                "material": o.material,
            })
        })
        .collect();
    json!({ "objects": objects })
}

fn set_transform(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args)?;
    let translation = floats::<3>(args, "translation")?;
    let rotation = floats::<3>(args, "rotation_degrees")?;
    let scale = floats::<3>(args, "scale")?;
    let obj = scene.object_mut(id).map_err(|e| e.to_string())?;
    if let Some(t) = translation {
        obj.transform.translation = Vec3::from(t);
    }
    if let Some([x, y, z]) = rotation {
        obj.transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        );
    }
    if let Some(s) = scale {
        obj.transform.scale = Vec3::from(s);
    }
    Ok(json!({ "id": id }))
}

fn set_material(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args)?;
    scene.object(id).map_err(|e| e.to_string())?;
    let mut m = Material::default();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        m.name = name.into();
    }
    if let Some(Value::Array(c)) = args.get("base_color") {
        let alpha = if c.len() == 4 {
            c[3].as_f64().unwrap_or(1.0) as f32
        } else {
            1.0
        };
        let rgb = floats::<3>(&json!({ "c": c.iter().take(3).collect::<Vec<_>>() }), "c")?.unwrap();
        m.base_color = [rgb[0], rgb[1], rgb[2], alpha];
    }
    if let Some(v) = args.get("metallic").and_then(Value::as_f64) {
        m.metallic = v as f32;
    }
    if let Some(v) = args.get("roughness").and_then(Value::as_f64) {
        m.roughness = v as f32;
    }
    if let Some(e) = floats::<3>(args, "emissive")? {
        m.emissive = e;
    }
    let material = scene.add_material(m);
    scene
        .assign_material(id, material)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "material": material }))
}
