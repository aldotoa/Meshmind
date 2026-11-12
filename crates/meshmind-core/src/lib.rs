//! MeshMind core engine: scene graph, meshes, materials and primitives.
//!
//! This crate has no UI and no I/O beyond what callers ask for, so it can be
//! driven from the MCP server, the desktop app, or tests.

pub mod bevel;
pub mod csg;
pub mod deform;
pub mod export;
pub mod lighting;
pub mod material;
pub mod mesh;
pub mod ops;
pub mod poly;
pub mod primitives;
pub mod project;
pub mod scene;
pub mod shapes;
pub mod smart;
pub mod texture;
pub mod uv;

pub use csg::BooleanOp;
pub use lighting::{LightingParams, LightingPreset};
pub use material::{Material, MaterialId, MaterialTextures};
pub use mesh::{Aabb, Mesh};
pub use ops::{Axis, Selection};
pub use poly::{Face, PolyMesh};
pub use primitives::Primitive;
pub use scene::{Object, ObjectId, Scene, SceneError, Transform};
