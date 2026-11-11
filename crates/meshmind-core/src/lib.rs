//! MeshMind core engine: scene graph, meshes, materials and primitives.
//!
//! This crate has no UI and no I/O beyond what callers ask for, so it can be
//! driven from the MCP server, the desktop app, or tests.

pub mod csg;
pub mod export;
pub mod material;
pub mod mesh;
pub mod ops;
pub mod poly;
pub mod primitives;
pub mod scene;

pub use csg::BooleanOp;
pub use material::{Material, MaterialId};
pub use mesh::{Aabb, Mesh};
pub use ops::{Axis, Selection};
pub use poly::{Face, PolyMesh};
pub use primitives::Primitive;
pub use scene::{Object, ObjectId, Scene, SceneError, Transform};
