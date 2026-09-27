# ff-export

An experimental Nintendo DS asset extraction pipeline: point it at a DS ROM and it pulls the game's
assets out for use elsewhere. Currently aimed at Fossil Fighters.

## Status

Work in progress. The container, compression, mesh, skeleton, animation and texture layers work: a
creature exports to one `.glb` carrying its bind-pose mesh, its skeleton, all of its clips as named
glTF animations, and its textures as embedded PNGs.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 | ROM header, FNT, FAT | done |
| M1 | MAR archives, MCM chunk wrapper, LZ10, Huffman | done |
| M2 | `3CL` manifest → creature → mesh / animation / texture, plus creature names | done |
| M3 | mesh + skeleton → static `.glb` | done — verified against the reference numbers and re-read with the glTF crate's importer, for every creature |
| M4 | animation clips in the `.glb` | done — all of a creature's clips land in one file as named glTF animations |
| M5 | textures → PNG, embedded, materials named | done — every palette is checked entry for entry against the reference, and every material group across all 116 creatures resolves to its image |
| M6 | every creature exported + `manifest.json` | not started |

## Usage

```text
ff-export info     <rom>                       ROM header, file and archive counts
ff-export list     <rom> [substring]           list ROM files (id, size, path)
ff-export unpack   <rom> <path> <outdir>       decompress one MAR archive into NNNN.bin
ff-export verify   <rom> <path> <refdir>       diff that output against a reference tree
ff-export manifest <rom> [limit]               creature -> mesh/animation/texture mapping
ff-export names    <rom> [all]                 the 116 creature id -> name pairs
ff-export mesh     <rom> <creature> [clip]     parse one creature mesh and report its geometry
ff-export obj      <rom> <creature> [clip] <file.obj>  write the decoded mesh as Wavefront OBJ
ff-export export   <rom> <creature> [clip] <file.glb>  write mesh, skeleton, clips and textures
ff-export anim     <rom> <creature> <clip>     inspect one clip's animation block
ff-export hexdump  <rom> <path> <entry> [len]  raw stored bytes of one entry, for analysis
ff-export help                                 show this text
```

Example:

```powershell
cargo run --release -- info "Fossil Fighters (USA).nds"
cargo run --release -- unpack "Fossil Fighters (USA).nds" model/battle/arcdin .\out\arcdin
cargo run --release -- export "Fossil Fighters (USA).nds" 30 .\out\breme_030.glb
```

## Clip names

A creature's clips are named by slot. Slot 0 is always unused, and a creature carries between four
and seven of the remaining slots.

| Slot | Name | Notes |
| --- | --- | --- |
| 1 | `attack` | the whole combo; the game truncates it at run time for a one, two or three hit attack |
| 2 | `attack-secondary` | a distinct secondary attack; only 36 of the 116 creatures have one |
| 3 | `roar` | |
| 4 | `victory` | |
| 5 | `hurt` | |
| 6 | `hurt-critical` | |
| 7 | `idle` | |

## Requirements

Rust 2024 edition, and a ROM you obtained legally. No game data is committed to this repository.

