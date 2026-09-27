//! Writing a skinned mesh out as glTF 2.0 binary (`.glb`).
//!
//! Two things need care, and both come from the shape of the decoded data:
//!
//! * **Per-corner texture coordinates.** The command stream gives every polygon *corner* its own UV,
//!   so one vertex can appear in several polygons with a different UV each time. glTF allows one UV
//!   per vertex, so vertices are split: a new vertex is made for each distinct `(vertex, uv)` pair.
//!   Positions get duplicated where the UVs differ, which is what the hardware did as well.
//! * **Winding.** The stream's triangle order is whatever the model author used, and glTF's front
//!   face is counter-clockwise. The mesh's signed volume decides whether the indices and normals need
//!   reversing; the choice is reported so it is never a silent guess.
//!
//! Normals are computed from the geometry, not read from the command stream. The `normal` command is
//! decoded but not applied yet, and the reference ignores it entirely, which is why its exports are
//! flat shaded.

use std::borrow::Cow;
use std::collections::HashMap;
use std::mem;

use anyhow::{Result, bail};
use gltf::json::{self, Index, validation::Checked::Valid, validation::USize64};

use crate::gpu::Geometry;
use crate::mesh::Mesh;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Vertices after splitting, ready to become glTF accessors.
#[derive(Default)]
pub struct Split {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// One index list per material group.
    pub groups: Vec<Vec<u32>>,
    /// True when the triangle order had to be reversed to make front faces face outwards.
    pub reversed: bool,
}

/// Area-weighted vertex normals, plus whether the winding had to be reversed.
///
/// The magnitude of a face's cross product is twice its area, so accumulating unnormalised cross
/// products weights large faces more - which is what we want.
fn normals_and_winding(geometry: &Geometry) -> (Vec<[f64; 3]>, bool) {
    let mut normals = vec![[0.0f64; 3]; geometry.positions.len()];
    let mut volume = 0.0f64;

    for group in &geometry.groups {
        for polygon in &group.polygons {
            for index in 1..polygon.len().saturating_sub(1) {
                let corners = [
                    polygon[0].vertex,
                    polygon[index].vertex,
                    polygon[index + 1].vertex,
                ];
                let (p0, p1, p2) = (
                    geometry.positions[corners[0]],
                    geometry.positions[corners[1]],
                    geometry.positions[corners[2]],
                );

                let face = cross(sub(p1, p0), sub(p2, p0));
                for corner in corners {
                    normals[corner] = [
                        normals[corner][0] + face[0],
                        normals[corner][1] + face[1],
                        normals[corner][2] + face[2],
                    ];
                }

                // divergence theorem: the signed volume, positive when the surface faces outwards
                volume += dot(p0, cross(p1, p2)) / 6.0;
            }
        }
    }

    let reversed = volume < 0.0;
    for normal in &mut normals {
        if reversed {
            *normal = [-normal[0], -normal[1], -normal[2]];
        }
        let length = dot(*normal, *normal).sqrt();
        *normal = if length > 1e-12 {
            [normal[0] / length, normal[1] / length, normal[2] / length]
        } else {
            // a vertex no face referenced, or a fully degenerate one
            [0.0, 1.0, 0.0]
        };
    }

    (normals, reversed)
}

/// Splits vertices so each one carries a single UV, and fans the polygons into triangles.
pub fn split(geometry: &Geometry) -> Split {
    let (normals, reversed) = normals_and_winding(geometry);
    let mut split = Split {
        reversed,
        ..Default::default()
    };
    let mut lookup: HashMap<(usize, usize), u32> = HashMap::new();

    for group in &geometry.groups {
        let mut indices = Vec::new();
        for polygon in &group.polygons {
            for index in 1..polygon.len().saturating_sub(1) {
                let mut fan = [&polygon[0], &polygon[index], &polygon[index + 1]];
                if reversed {
                    fan.reverse();
                }

                for corner in fan {
                    // an absent UV is its own split key, and becomes (0, 0)
                    let key = (corner.vertex, corner.uv.unwrap_or(usize::MAX));
                    let vertex = match lookup.get(&key) {
                        Some(existing) => *existing,
                        None => {
                            let position = geometry.positions[corner.vertex];
                            let normal = normals[corner.vertex];
                            let uv = match corner.uv {
                                Some(uv) => geometry.uvs[uv],
                                None => [0.0, 0.0],
                            };
                            let bone = geometry.bones[corner.vertex] as u16;

                            split.positions.push([
                                position[0] as f32,
                                position[1] as f32,
                                position[2] as f32,
                            ]);
                            split
                                .normals
                                .push([normal[0] as f32, normal[1] as f32, normal[2] as f32]);
                            split.uvs.push([uv[0] as f32, uv[1] as f32]);
                            split.joints.push([bone, 0, 0, 0]);
                            split.weights.push([1.0, 0.0, 0.0, 0.0]);

                            let index = (split.positions.len() - 1) as u32;
                            lookup.insert(key, index);
                            index
                        }
                    };
                    indices.push(vertex);
                }
            }
        }
        split.groups.push(indices);
    }

    split
}

/// A byte range inside the binary buffer.
#[derive(Clone, Copy)]
struct Chunk {
    offset: usize,
    length: usize,
}

/// Appends `bytes` to the buffer, 4-byte aligned, and reports where they landed.
fn push_bytes(bin: &mut Vec<u8>, bytes: &[u8]) -> Chunk {
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let offset = bin.len();
    bin.extend_from_slice(bytes);
    Chunk {
        offset,
        length: bytes.len(),
    }
}

macro_rules! float_bytes {
    ($name:ident, $shape:ty) => {
        fn $name(values: &[$shape]) -> Vec<u8> {
            let mut out = Vec::with_capacity(values.len() * mem::size_of::<$shape>());
            for value in values {
                for component in value.iter() {
                    out.extend_from_slice(&component.to_le_bytes());
                }
            }
            out
        }
    };
}

float_bytes!(bytes_f32x2, [f32; 2]);
float_bytes!(bytes_f32x3, [f32; 3]);
float_bytes!(bytes_f32x4, [f32; 4]);
float_bytes!(bytes_f32x16, [f32; 16]);

fn bytes_u16x4(values: &[[u16; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for value in values {
        for component in value {
            out.extend_from_slice(&component.to_le_bytes());
        }
    }
    out
}

fn bytes_u32(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Creates a buffer view plus an accessor over one byte range.
#[allow(clippy::too_many_arguments)]
fn accessor(
    root: &mut json::Root,
    buffer: Index<json::Buffer>,
    chunk: Chunk,
    count: usize,
    component: json::accessor::ComponentType,
    kind: json::accessor::Type,
    bounds: Option<(json::Value, json::Value)>,
) -> Index<json::Accessor> {
    let view = root.push(json::buffer::View {
        buffer,
        byte_length: USize64::from(chunk.length),
        byte_offset: Some(USize64::from(chunk.offset)),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: None,
    });

    root.push(json::Accessor {
        buffer_view: Some(view),
        byte_offset: Some(USize64(0)),
        count: USize64::from(count),
        component_type: Valid(json::accessor::GenericComponentType(component)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(kind),
        min: bounds.as_ref().map(|(min, _)| min.clone()),
        max: bounds.as_ref().map(|(_, max)| max.clone()),
        name: None,
        normalized: false,
        sparse: None,
    })
}

/// The axis-aligned bounds of a vertex list, as JSON numbers for the accessor.
fn position_bounds(positions: &[[f32; 3]]) -> (json::Value, json::Value) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for position in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (
        json::Value::from(Vec::from(min)),
        json::Value::from(Vec::from(max)),
    )
}

/// Builds a `.glb` holding the creature's bind-pose mesh and its skeleton.
///
/// Returns the file bytes and the split that produced them, so the caller can report what was
/// written without doing the work twice.
pub fn build(decoded: &Mesh, geometry: &Geometry, name: &str) -> Result<(Vec<u8>, Split)> {
    let split = split(geometry);

    if decoded.bones.is_empty() {
        bail!("the mesh has no bones, so there is no skeleton to write");
    }
    if split.positions.is_empty() || split.groups.iter().all(|group| group.is_empty()) {
        bail!("nothing to export: the geometry has no triangles");
    }

    let bind: Vec<[f32; 16]> = decoded
        .bones
        .iter()
        .map(|bone| bone.matrix.to_gltf_matrix())
        .collect();

    let mut inverse_bind: Vec<[f32; 16]> = Vec::with_capacity(decoded.bones.len());
    for bone in &decoded.bones {
        let inverse = bone
            .matrix
            .inverse()
            .ok_or_else(|| anyhow::anyhow!("bone `{}` has a degenerate bind matrix", bone.name))?;
        inverse_bind.push(inverse.to_gltf_matrix());
    }

    // one buffer, one view per accessor
    let mut bin = Vec::new();
    let positions_chunk = push_bytes(&mut bin, &bytes_f32x3(&split.positions));
    let normals_chunk = push_bytes(&mut bin, &bytes_f32x3(&split.normals));
    let uvs_chunk = push_bytes(&mut bin, &bytes_f32x2(&split.uvs));
    let joints_chunk = push_bytes(&mut bin, &bytes_u16x4(&split.joints));
    let weights_chunk = push_bytes(&mut bin, &bytes_f32x4(&split.weights));
    let inverse_bind_chunk = push_bytes(&mut bin, &bytes_f32x16(&inverse_bind));
    let index_chunks: Vec<Chunk> = split
        .groups
        .iter()
        .map(|indices| push_bytes(&mut bin, &bytes_u32(indices)))
        .collect();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let mut root = json::Root::default();
    root.asset = json::Asset {
        generator: Some("ff-export".to_string()),
        version: "2.0".to_string(),
        ..Default::default()
    };

    let buffer = root.push(json::Buffer {
        byte_length: USize64::from(bin.len()),
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    });

    let count = split.positions.len();
    let positions = accessor(
        &mut root,
        buffer,
        positions_chunk,
        count,
        json::accessor::ComponentType::F32,
        json::accessor::Type::Vec3,
        Some(position_bounds(&split.positions)),
    );
    let normals = accessor(
        &mut root,
        buffer,
        normals_chunk,
        count,
        json::accessor::ComponentType::F32,
        json::accessor::Type::Vec3,
        None,
    );
    let uvs = accessor(
        &mut root,
        buffer,
        uvs_chunk,
        count,
        json::accessor::ComponentType::F32,
        json::accessor::Type::Vec2,
        None,
    );
    let joints = accessor(
        &mut root,
        buffer,
        joints_chunk,
        count,
        json::accessor::ComponentType::U16,
        json::accessor::Type::Vec4,
        None,
    );
    let weights = accessor(
        &mut root,
        buffer,
        weights_chunk,
        count,
        json::accessor::ComponentType::F32,
        json::accessor::Type::Vec4,
        None,
    );
    let inverse_bind_accessor = accessor(
        &mut root,
        buffer,
        inverse_bind_chunk,
        decoded.bones.len(),
        json::accessor::ComponentType::F32,
        json::accessor::Type::Mat4,
        None,
    );
    // --- primitives, nodes, skin and scene ---

    // one primitive and one placeholder material per group; M5 supplies the textures
    let mut materials = Vec::new();
    for group in &geometry.groups {
        let key = match group.palette_base {
            Some(value) => value.to_string(),
            None => "none".to_string(),
        };
        materials.push(root.push(json::Material {
            name: Some(format!("material_{key}")),
            ..Default::default()
        }));
    }

    let mut primitives = Vec::new();
    for (index, chunk) in index_chunks.iter().enumerate() {
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(Valid(json::mesh::Semantic::Positions), positions);
        attributes.insert(Valid(json::mesh::Semantic::Normals), normals);
        attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), uvs);
        attributes.insert(Valid(json::mesh::Semantic::Joints(0)), joints);
        attributes.insert(Valid(json::mesh::Semantic::Weights(0)), weights);

        primitives.push(json::mesh::Primitive {
            attributes,
            indices: Some(accessor(
                &mut root,
                buffer,
                *chunk,
                split.groups[index].len(),
                json::accessor::ComponentType::U32,
                json::accessor::Type::Scalar,
                None,
            )),
            material: Some(materials[index]),
            mode: Valid(json::mesh::Mode::Triangles),
            targets: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
    }

    let mesh = root.push(json::Mesh {
        extensions: Default::default(),
        extras: Default::default(),
        name: Some(format!("{name}_mesh")),
        primitives,
        weights: None,
    });

    // The skeleton is flat: every joint is a child of the scene carrying its bind transform, which
    // is also how the reference models it.
    let joint_nodes: Vec<Index<json::Node>> = decoded
        .bones
        .iter()
        .zip(bind.iter())
        .map(|(bone, matrix)| {
            root.push(json::Node {
                name: Some(bone.name.clone()),
                matrix: Some(*matrix),
                ..Default::default()
            })
        })
        .collect();

    let skin = root.push(json::Skin {
        inverse_bind_matrices: Some(inverse_bind_accessor),
        joints: joint_nodes.clone(),
        skeleton: None,
        name: Some(format!("{name}_skeleton")),
        extensions: Default::default(),
        extras: Default::default(),
    });

    let mesh_node = root.push(json::Node {
        mesh: Some(mesh),
        skin: Some(skin),
        name: Some(name.to_string()),
        ..Default::default()
    });

    let mut scene_nodes = vec![mesh_node];
    scene_nodes.extend(joint_nodes);
    let scene = root.push(json::Scene {
        nodes: scene_nodes,
        name: Some("scene".to_string()),
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    let json_text = json::serialize::to_string(&root)?;
    let glb = gltf::binary::Glb {
        // `to_writer` recomputes the length and handles the chunk padding itself
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(json_text.into_bytes()),
        bin: Some(Cow::Owned(bin)),
    };

    Ok((glb.to_vec()?, split))
}
