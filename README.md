# ff-export

A Fossil Fighters (Nintendo DS) asset exporter: ROM → `.glb` (mesh + skeleton + animations +
embedded textures) plus a manifest.

This is an **independent reimplementation**. It replaces the two fan tools that were used to
explore the game's formats, so the pipeline is reproducible from the ROM alone.

## Status

| Milestone | Scope | State |
| --- | --- | --- |
| M0 | ROM header, FNT, FAT | **done** — 8,271 files, 8,041 MAR archives, matches the validated counts |
| M1 | MAR container, MCM chunk wrapper, LZ10 | **done** — verified byte-exact, see below |
| M2 | `3CL` manifest → creature → mesh/animation/texture | not started |
| M3 | mesh + skeleton → static `.glb` | not started |
| M4 | animation clips in the `.glb` | not started |
| M5 | textures → PNG, embedded | not started |
| M6 | all clips merged per creature + `manifest.json`, all 116 creatures | not started |

### Verification

Every layer is gated on a byte comparison rather than an eyeball. M1 unpacks `model/battle/arcdin`
(1,452 entries, 88,461,560 bytes) and diffs it against payloads produced by an unrelated
implementation:

```
entries      : 1452
identical    : 1447
failed       : 0
elapsed      : 0.48 s (release build)
```

The five remaining entries are texture-class records where that reference corpus is itself wrong.
Diffing all three available outputs shows our bytes match a second, independent unpack **exactly**
(0 bytes differing), while the first corpus is off by 363, 5, 12, 5 and 2 bytes, first at offsets
516 and 18434. So M1 is 1,452 / 1,452 against the trustworthy reference.

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

## Format notes worth keeping

* **This game's FNT is a non-standard variant.** File entries carry no id field; ids are implicit and
  sequential from the directory's `firstFileId`. Directory entries are followed by
  `u16 0xF000 | dirId`. The root directory's parent slot holds the directory count.
* **The `MCM` header and the chunk header use different codec id spaces.** In the `MCM` header
  `0 = none, 1 = run length, 2 = LZ10, 3 = Huffman`; in the 4-byte header that prefixes a compressed
  chunk `1 = LZ10, 2 = Huffman, 3 = run length`. Reading one with the other's numbering silently
  mislabels every entry — it cost a debugging round here, so both mapping functions exist and are
  unit-tested.
* Only `none` and LZ10 are implemented. Across all 8,041 archives the `MCM` codec fields hold
  `none` 29,269, `lzss` 9,158, `huffman` 2,143 and `rle` 668 times, but Huffman and run length never
  occur in the 3D path. They fail loudly rather than guessing.

## Dependencies

Stage 1 uses `anyhow`, `gltf` and `png` to reach a verified `.glb` quickly. Stage 2 replaces them
with dependency-free implementations once the output is confirmed.

## Provenance and licensing

The formats here were derived from byte-level analysis of a legally obtained personal ROM, from the
public Nintendo DS hardware documentation, and from format notes written during the earlier
exploration in the `chris-wgpu` project. **No source code from any fan tool was copied**, and the
GPL-3.0 tools used during exploration are not linked, vendored or required.

No licence has been chosen for this repository yet.

No ROM, extracted asset or other game data is committed; see `.gitignore`.
