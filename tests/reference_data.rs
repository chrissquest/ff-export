//! Gates the pipeline against the reference data in `externals/`.
//!
//! `externals/` is not committed - it holds the ROM, the extracted corpora and the third-party
//! tools used during exploration - so every test here skips cleanly when it is absent. A fresh
//! clone still passes; running locally tells us whether the implementation still matches the
//! reference byte for byte.

use std::path::Path;

use ff_export::{anim, gltf_out, gpu, manifest, mar, mesh, names, nds};

const ROM: &str = "externals/Fossil Fighters (USA).nds";
const MANIFEST_JSON: &str = "externals/knowledge/arcdin.3cl.json";
const ARCDIN_DUMP: &str = "externals/extracted/mcm_tree/model/battle/arcdin";

/// Entry indices whose *reference* copy is known to be wrong (see README). Our output matches a
/// second, independent unpack exactly for these, so they are pinned rather than ignored.
const KNOWN_BAD_REFERENCE_ENTRIES: [usize; 5] = [108, 449, 583, 655, 1191];

fn rom() -> Option<nds::Rom> {
    if !Path::new(ROM).exists() {
        eprintln!("skipping: {ROM} is not present");
        return None;
    }
    Some(nds::Rom::open(ROM).expect("the ROM should parse"))
}

fn arcdin<'a>(rom: &'a nds::Rom) -> mar::Archive<'a> {
    let file = rom
        .find("model/battle/arcdin")
        .expect("model/battle/arcdin should be in the ROM");
    mar::Archive::parse(rom.bytes(file).unwrap()).expect("arcdin should parse")
}

#[test]
fn m0_rom_listing_matches_the_reference_counts() {
    let Some(rom) = rom() else { return };

    assert_eq!(rom.files().len(), 8271, "files found by the FNT walk");

    let archives = rom
        .files()
        .iter()
        .filter(|file| &rom.magic(file).unwrap_or([0; 4])[0..3] == b"MAR")
        .count();
    assert_eq!(archives, 8041, "MAR archives");
}

#[test]
fn m1_arcdin_decompresses_byte_for_byte() {
    let Some(rom) = rom() else { return };
    if !Path::new(ARCDIN_DUMP).exists() {
        eprintln!("skipping: {ARCDIN_DUMP} is not present");
        return;
    }

    let archive = arcdin(&rom);
    assert_eq!(archive.len(), 1452, "entry count");

    let mut checked = 0;
    let mut mismatches = Vec::new();

    for index in 0..archive.len() {
        // The reference corpus names entries `12.bin`, not `0012.bin`.
        let reference_path = Path::new(ARCDIN_DUMP).join(format!("{index}.bin"));
        if !reference_path.exists() {
            continue;
        }
        let reference = std::fs::read(&reference_path).expect("reference entry should read");
        let ours = archive
            .decompressed(index)
            .unwrap_or_else(|error| panic!("entry {index} failed to decompress: {error:#}"));

        if ours != reference {
            mismatches.push(index);
        }
        checked += 1;
    }

    assert!(checked > 1400, "expected to check most entries, checked {checked}");
    assert_eq!(
        mismatches, KNOWN_BAD_REFERENCE_ENTRIES,
        "apart from the pinned reference bugs, every entry must match byte for byte"
    );
}

#[test]
fn m2_creature_names_match_the_reference_csv() {
    let Some(rom) = rom() else { return };
    let csv_path = Path::new("externals/knowledge/creature_names.csv");
    if !csv_path.exists() {
        eprintln!("skipping: {} is not present", csv_path.display());
        return;
    }

    // The names live in entry 0 of text/japanese, which is Huffman compressed.
    let file = rom
        .find("text/japanese")
        .expect("text/japanese should be in the ROM");
    let archive = mar::Archive::parse(rom.bytes(file).unwrap()).unwrap();
    let text = archive
        .decompressed(0)
        .expect("the text archive should decompress");

    let names = names::creature_names(&text, names::CREATURE_COUNT).expect("names should be found");
    assert_eq!(names.len(), names::CREATURE_COUNT);

    // The reference CSV is `"Index","Name"` with indices 1..=116.
    let csv = std::fs::read_to_string(csv_path).expect("reference csv should read");
    let mut expected = Vec::new();
    for line in csv.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut fields = trimmed.split(',');
        let index: usize = fields
            .next()
            .expect("index field")
            .trim_matches('"')
            .parse()
            .expect("index should be a number");
        let name = fields.next().expect("name field").trim_matches('"').to_string();
        assert_eq!(index, expected.len() + 1, "the reference csv should be in order");
        expected.push(name);
    }

    assert_eq!(expected.len(), names::CREATURE_COUNT, "reference csv row count");
    assert_eq!(names, expected, "the ROM's name table should match the reference");
}

#[test]
fn m2_manifest_matches_the_reference_json() {
    let Some(rom) = rom() else { return };
    if !Path::new(MANIFEST_JSON).exists() {
        eprintln!("skipping: {MANIFEST_JSON} is not present");
        return;
    }

    let archive = arcdin(&rom);
    let data = archive.decompressed(0).expect("entry 0 is the 3CL manifest");
    let parsed = manifest::parse(&data).expect("the manifest should parse");

    let text = std::fs::read_to_string(MANIFEST_JSON).expect("reference json should read");
    let reference: serde_json::Value = serde_json::from_str(&text).expect("reference json should parse");
    let slots = reference["vivosaurs"]
        .as_array()
        .expect("reference json should have a vivosaurs array");

    // Expected: (id, clip slots, (mesh, animation, texture) index triples), in slot order.
    let mut expected: Vec<(usize, Vec<usize>, Vec<(u32, u32, u32)>)> = Vec::new();
    for (slot, value) in slots.iter().enumerate() {
        if value.is_null() {
            continue;
        }
        let id = value["_id"].as_u64().expect("_id") as usize;
        assert_eq!(id, slot, "the reference _id should equal its array slot");

        let mut clip_slots = Vec::new();
        let mut triples = Vec::new();
        for (clip, entry) in value["animations"].as_array().expect("animations").iter().enumerate() {
            if entry.is_null() {
                continue;
            }
            clip_slots.push(clip);
            triples.push((
                entry["mesh"]["index"].as_u64().expect("mesh index") as u32,
                entry["animation"]["index"].as_u64().expect("animation index") as u32,
                entry["texture"]["index"].as_u64().expect("texture index") as u32,
            ));
        }
        expected.push((id, clip_slots, triples));
    }

    assert_eq!(
        parsed.creatures.len(),
        expected.len(),
        "creature count should match the reference"
    );

    for (ours, (id, clip_slots, triples)) in parsed.creatures.iter().zip(&expected) {
        assert_eq!(ours.id, *id, "creature id");

        let our_slots: Vec<usize> = ours.clips.iter().map(|clip| clip.slot).collect();
        assert_eq!(&our_slots, clip_slots, "clip slots for creature {id}");

        let our_triples: Vec<(u32, u32, u32)> = ours
            .clips
            .iter()
            .map(|clip| (clip.mesh.index, clip.animation.index, clip.texture.index))
            .collect();
        assert_eq!(&our_triples, triples, "mesh/animation/texture triples for creature {id}");
    }

    // Every vivosaur part lives in the `arcdin` archive, which is a useful sanity check on the
    // name offsets: if they were misread the names would come out as garbage.
    for creature in &parsed.creatures {
        for clip in &creature.clips {
            assert_eq!(clip.mesh.table_name, "arcdin");
            assert_eq!(clip.animation.table_name, "arcdin");
            assert_eq!(clip.texture.table_name, "arcdin");
        }
    }
}

/// Decodes one mesh entry into its geometry.
fn decode(archive: &mar::Archive<'_>, mesh_index: u32) -> (mesh::Mesh, gpu::Geometry) {
    let data = archive
        .decompressed(mesh_index as usize)
        .unwrap_or_else(|error| panic!("mesh entry {mesh_index} failed to decompress: {error:#}"));
    let decoded = mesh::parse(&data)
        .unwrap_or_else(|error| panic!("mesh entry {mesh_index} failed to parse: {error:#}"));
    let stream = gpu::parse_stream(&decoded.commands)
        .unwrap_or_else(|error| panic!("mesh entry {mesh_index} command stream failed: {error:#}"));
    let geometry = gpu::build_geometry(
        &decoded,
        stream
            .world_root_bone_count()
            .unwrap_or_else(|error| panic!("mesh entry {mesh_index}: {error:#}")),
        stream
            .gpu_commands()
            .unwrap_or_else(|error| panic!("mesh entry {mesh_index}: {error:#}")),
    )
    .unwrap_or_else(|error| panic!("mesh entry {mesh_index} geometry failed: {error:#}"));
    (decoded, geometry)
}

#[test]
fn m3_breme_matches_the_reference_numbers() {
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);

    // Breme's first clip: mesh 384, the numbers the reference export produced.
    let (decoded, geometry) = decode(&archive, 384);

    assert_eq!(decoded.scale, 32.0, "mesh scale");
    assert_eq!(decoded.keyframe_count, 257, "authored frames");
    assert_eq!(decoded.bones.len(), 21, "bone count");
    assert_eq!(
        decoded.bones[0].name, "waist",
        "the bone table starts with the waist"
    );
    assert!(
        decoded.bones.iter().all(|bone| bone.matrix.is_finite()),
        "every bind matrix must be finite"
    );

    assert_eq!(geometry.positions.len(), 242, "unique vertices");
    assert_eq!(geometry.face_count(), 391, "faces");
    assert_eq!(geometry.triangle_count(), 408, "triangles");
    assert_eq!(geometry.corner_count(), 1190, "polygon corners");
    assert_eq!(geometry.groups.len(), 2, "material groups");

    let (min, max) = geometry.bounds().expect("geometry has bounds");
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    for (axis, expected) in [(0, 4.001), (1, 9.041), (2, 10.328)] {
        assert!(
            (size[axis] - expected).abs() < 0.01,
            "bound size on axis {axis} was {:.3}, expected {expected}",
            size[axis]
        );
    }
}

#[test]
fn m3_every_clip_of_every_creature_decodes() {
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);
    let parsed = manifest::parse(&archive.decompressed(0).unwrap()).unwrap();

    let mut clips = 0usize;
    for creature in &parsed.creatures {
        for clip in &creature.clips {
            let (decoded, geometry) = decode(&archive, clip.mesh.index);
            let context = format!("creature {} clip {}", creature.id, clip.slot);

            assert!(!decoded.bones.is_empty(), "{context}: no bones");
            assert!(
                decoded.bones.iter().all(|bone| bone.matrix.is_finite()),
                "{context}: a bind matrix is not finite"
            );
            assert!(
                geometry.positions.len() > 32,
                "{context}: only {} vertices",
                geometry.positions.len()
            );
            assert!(
                geometry.triangle_count() > 32,
                "{context}: only {} triangles",
                geometry.triangle_count()
            );
            for group in &geometry.groups {
                for polygon in &group.polygons {
                    assert!(polygon.len() >= 3, "{context}: a polygon has {} corners", polygon.len());
                    for corner in polygon {
                        if let Some(uv) = corner.uv {
                            assert!(
                                uv < geometry.uvs.len(),
                                "{context}: UV index {uv} is out of range"
                            );
                        }
                        assert!(
                            corner.vertex < geometry.positions.len(),
                            "{context}: vertex index {} is out of range",
                            corner.vertex
                        );
                    }
                }
            }

            clips += 1;
        }
    }

    assert_eq!(clips, 698, "the whole roster should decode");
}

#[test]
fn m3_exported_glb_reads_back() {
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);
    let (decoded, geometry) = decode(&archive, 384);
    let (bytes, split) = gltf_out::build(&decoded, &geometry, "Breme").expect("the glb should build");

    // The crate's own reader has to accept what we wrote - that is the spec check.
    let (document, buffers, _images) =
        gltf::import_slice(&bytes).expect("the exported glb should import");

    let mesh = document.meshes().next().expect("one mesh");
    let primitives: Vec<_> = mesh.primitives().collect();
    assert_eq!(primitives.len(), 2, "one primitive per material");

    let mut vertices = 0usize;
    let mut triangles = 0usize;
    for primitive in &primitives {
        let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

        assert!(reader.read_positions().is_some(), "positions");
        assert!(reader.read_normals().is_some(), "normals");
        assert!(reader.read_tex_coords(0).is_some(), "uvs");
        assert!(reader.read_joints(0).is_some(), "joint indices");
        assert!(reader.read_weights(0).is_some(), "joint weights");

        // both primitives index the same vertex list, so this is not a sum
        let count = reader.read_positions().unwrap().count();
        assert_eq!(
            count,
            split.positions.len(),
            "each primitive should index the shared vertex list"
        );
        vertices = count;

        let indices: Vec<u32> = reader
            .read_indices()
            .expect("indices")
            .into_u32()
            .collect();
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
        for index in &indices {
            assert!(
                (*index as usize) < split.positions.len(),
                "index {index} is out of range"
            );
        }
        triangles += indices.len() / 3;
    }

    assert_eq!(
        vertices,
        split.positions.len(),
        "every split vertex should be in the file"
    );
    assert_eq!(triangles, 408, "the reference triangle count");

    let skin = document.skins().next().expect("a skin");
    assert_eq!(skin.joints().count(), 21, "every bone should be a joint");

    let named = document.nodes().filter(|node| node.name().is_some()).count();
    assert!(named >= 22, "the mesh node plus 21 named bones, found {named}");
    assert!(
        document.nodes().any(|node| node.name() == Some("waist")),
        "bones keep the game's names"
    );
}

#[test]
fn m3_every_creature_exports_a_glb() {
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);
    let parsed = manifest::parse(&archive.decompressed(0).unwrap()).unwrap();

    let mut exported = 0usize;
    for creature in &parsed.creatures {
        let clip = &creature.clips[0];
        let (decoded, geometry) = decode(&archive, clip.mesh.index);
        let name = format!("vivosaur_{:03}", creature.id);

        let (bytes, split) = gltf_out::build(&decoded, &geometry, &name)
            .unwrap_or_else(|error| panic!("creature {} clip {}: {error:#}", creature.id, clip.slot));

        assert_eq!(&bytes[0..4], b"glTF", "creature {} magic", creature.id);
        assert!(!split.positions.is_empty(), "creature {} has no vertices", creature.id);

        let (document, _buffers, _images) = gltf::import_slice(&bytes)
            .unwrap_or_else(|error| panic!("creature {} does not import: {error:?}", creature.id));

        let skin = document
            .skins()
            .next()
            .unwrap_or_else(|| panic!("creature {} has no skin", creature.id));
        assert_eq!(
            skin.joints().count(),
            decoded.bones.len(),
            "creature {}: every bone should be a joint",
            creature.id
        );

        exported += 1;
    }

    assert_eq!(exported, 116, "every creature should export");
}

#[test]
fn m4_animation_frame_counts_match_the_reference() {
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);
    let parsed = manifest::parse(&archive.decompressed(0).unwrap()).unwrap();
    let breme = parsed.find(30).expect("Breme is creature 30");

    // the frame counts the reference export produced, per clip slot
    for (slot, frames) in [(1, 257), (3, 90), (4, 200), (5, 80), (6, 80), (7, 60)] {
        let clip = breme
            .clips
            .iter()
            .find(|clip| clip.slot == slot)
            .unwrap_or_else(|| panic!("Breme should have clip slot {slot}"));
        let animation =
            anim::parse(&archive.decompressed(clip.animation.index as usize).unwrap())
                .unwrap_or_else(|error| panic!("clip {slot} failed to parse: {error:#}"));

        assert_eq!(animation.frame_count, frames, "clip {slot} frame count");
        assert_eq!(animation.bone_count, 21, "clip {slot} bone count");
        assert_eq!(
            animation.transforms.len(),
            frames * 21,
            "clip {slot} keyframe count"
        );
    }
}

#[test]
fn m4_frame_zero_is_the_bind_pose() {
    // The keyframes are in the same space as the bind matrices, which is what lets them drive glTF
    // joint nodes directly with no bind-pose composition.
    let Some(rom) = rom() else { return };
    let archive = arcdin(&rom);
    let parsed = manifest::parse(&archive.decompressed(0).unwrap()).unwrap();
    let clip = parsed
        .find(30)
        .expect("Breme")
        .clips
        .iter()
        .find(|clip| clip.slot == 7)
        .expect("the idle clip");

    let (decoded, _) = decode(&archive, clip.mesh.index);
    let animation = anim::parse(&archive.decompressed(clip.animation.index as usize).unwrap())
        .expect("the idle animation should parse");

    assert_eq!(animation.bone_count, decoded.bones.len());
    for bone in 0..animation.bone_count {
        let pose = animation.transform(bone, 0).expect("frame 0");
        let bind = &decoded.bones[bone].matrix;
        for axis in 0..3 {
            assert!(
                (pose.translation[axis] - bind.translation[axis]).abs() < 1e-9,
                "bone {bone} (`{}`) axis {axis}: frame 0 translation {} differs from the bind pose {}",
                decoded.bones[bone].name,
                pose.translation[axis],
                bind.translation[axis]
            );
        }
    }
}
