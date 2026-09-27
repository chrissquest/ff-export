# ff-export

An experimental Nintendo DS asset extraction pipeline: point it at a DS ROM and it pulls the game's
assets out for use elsewhere. Currently aimed at Fossil Fighters.

## Status

Early work in progress. The container and compression layers work; no glTF is written yet.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 | ROM header, FNT, FAT | done |
| M1 | MAR archives, MCM chunk wrapper, LZ10, Huffman | done |
| M2 | `3CL` manifest → creature → mesh / animation / texture, plus creature names | done |
| M3 | mesh + skeleton → static `.glb` | done — verified against the reference numbers and re-read with the glTF crate's importer, for every creature |
| M4 | animation clips in the `.glb` | not started |
| M5 | textures → PNG, embedded | not started |
| M6 | all clips merged per creature + `manifest.json` | not started |

## Usage

```text
ff-export info    <rom>                       ROM header, file and archive counts
ff-export list    <rom> [substring]           list ROM files (id, size, path)
ff-export unpack  <rom> <path> <outdir>       decompress one MAR archive into NNNN.bin
ff-export verify  <rom> <path> <refdir>       diff that output against a reference tree
ff-export hexdump <rom> <path> <entry> [len]  raw stored bytes of one entry
```

Example:

```powershell
cargo run --release -- info "Fossil Fighters (USA).nds"
cargo run --release -- unpack "Fossil Fighters (USA).nds" model/battle/arcdin .\out\arcdin
```

## Requirements

Rust 2024 edition, and a ROM you obtained legally. No game data is committed to this repository.
