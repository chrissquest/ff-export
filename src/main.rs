//! `ff-export` command line: inspect a ROM, unpack archives, diff against a
//! known-good corpus.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};

use ff_export::{manifest, mar, mcm, names, nds};

const USAGE: &str = "\
ff-export - Fossil Fighters (NDS) asset extraction

USAGE:
    ff-export <command> [arguments]

COMMANDS:
    info    <rom>                       ROM header, file and archive counts
    list    <rom> [substring]           list ROM files (id, size, path)
    unpack  <rom> <path> <outdir>       decompress one MAR archive into NNNN.bin
    verify  <rom> <path> <refdir>       diff that output against a reference tree
    manifest <rom> [limit]              creature -> mesh/animation/texture mapping
    names   <rom> [all]                 the 116 creature id -> name pairs
    hexdump <rom> <path> <entry> [len]  raw stored bytes of one entry, for analysis
    help                                show this text
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("info") => cmd_info(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        Some("unpack") => cmd_unpack(&args[1..]),
        Some("verify") => cmd_verify(&args[1..]),
        Some("hexdump") => cmd_hexdump(&args[1..]),
        Some("manifest") => cmd_manifest(&args[1..]),
        Some("names") => cmd_names(&args[1..]),
        Some("help") | None | Some("-h") | Some("--help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(anyhow::anyhow!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn arg(args: &[String], index: usize, name: &str) -> Result<String> {
    match args.get(index) {
        Some(value) => Ok(value.clone()),
        None => Err(anyhow::anyhow!("missing <{name}> argument\n\n{USAGE}")),
    }
}

fn is_mar(rom: &nds::Rom, file: &nds::RomFile) -> bool {
    match rom.magic(file) {
        Ok(magic) => &magic[0..3] == b"MAR",
        Err(_) => false,
    }
}

/// Reference corpora name entries either `12.bin` or `0012.bin`; accept both.
fn reference_path(ref_dir: &std::path::Path, index: usize) -> Option<PathBuf> {
    for name in [format!("{index}.bin"), format!("{index:04}.bin")] {
        let candidate = ref_dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn cmd_info(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let rom = nds::Rom::open(&rom_path)?;
    let header = rom.header();

    println!("rom          : {rom_path}");
    println!(
        "title        : {:<12} game code {:<4} maker code {}",
        header.title, header.game_code, header.maker_code
    );
    println!(
        "size         : {} bytes ({:.1} MiB)",
        header.file_size,
        header.file_size as f64 / (1024.0 * 1024.0)
    );
    println!(
        "FNT          : {:#010x} + {:#x}",
        header.fnt_offset, header.fnt_size
    );
    println!(
        "FAT          : {:#010x} + {:#x}",
        header.fat_offset, header.fat_size
    );
    println!(
        "arm9 / arm7  : {:#x}+{:#x} / {:#x}+{:#x}",
        header.arm9_offset, header.arm9_size, header.arm7_offset, header.arm7_size
    );
    println!("files        : {}", rom.files().len());

    let archives = rom.files().iter().filter(|f| is_mar(&rom, f)).count();
    let bytes: u64 = rom.files().iter().map(|f| f.size as u64).sum();
    println!("MAR archives : {archives}");
    println!(
        "file bytes   : {} ({:.1} MiB)",
        bytes,
        bytes as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

fn cmd_list(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let filter = args.get(1).cloned().unwrap_or_default();
    let rom = nds::Rom::open(&rom_path)?;

    let mut shown = 0usize;
    for file in rom.files() {
        if !filter.is_empty() && !file.path.contains(&filter) {
            continue;
        }
        let magic = rom.magic(file).unwrap_or([0; 4]);
        let tag = if &magic[0..3] == b"MAR" { "MAR" } else { "" };
        println!("{:>5}  {:>9}  {:<4} {}", file.id, file.size, tag, file.path);
        shown += 1;
    }
    println!("--- {shown} of {} file(s) ---", rom.files().len());
    Ok(())
}

/// Opens a ROM and the archive at `path` inside it.
fn open_archive<'a>(rom: &'a nds::Rom, path: &str) -> Result<(mar::Archive<'a>, &'a nds::RomFile)> {
    let file = rom
        .find(path)
        .with_context(|| format!("`{path}` is not in the ROM"))?;
    let bytes = rom.bytes(file)?;
    let archive = mar::Archive::parse(bytes).with_context(|| format!("parsing `{path}`"))?;
    Ok((archive, file))
}

fn cmd_unpack(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let inner = arg(args, 1, "path")?;
    let out_dir = PathBuf::from(arg(args, 2, "outdir")?);

    let started = Instant::now();
    let rom = nds::Rom::open(&rom_path)?;
    let (archive, file) = open_archive(&rom, &inner)?;
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("creating {}", out_dir.display()))?;

    println!(
        "`{inner}`: {} entr{}, stored {} bytes",
        archive.len(),
        if archive.len() == 1 { "y" } else { "ies" },
        file.size
    );

    let mut codecs: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut written = 0usize;
    let mut produced: u64 = 0;
    let mut failures = 0usize;

    for index in 0..archive.len() {
        let stored = archive.stored(index)?;
        if let Ok(info) = mcm::info(stored) {
            for codec in info.codecs() {
                *codecs.entry(codec.name()).or_default() += 1;
            }
        }

        match archive.decompressed(index) {
            Ok(data) => {
                let path = out_dir.join(format!("{index:04}.bin"));
                std::fs::write(&path, &data)
                    .with_context(|| format!("writing {}", path.display()))?;
                produced += data.len() as u64;
                written += 1;
            }
            Err(error) => {
                failures += 1;
                if failures <= 5 {
                    eprintln!("  entry {index:04}: {error:#}");
                }
            }
        }
    }

    println!("wrote        : {written} file(s), {produced} bytes");
    if failures > 0 {
        println!("failed       : {failures}");
    }
    let codecs: Vec<String> = codecs.iter().map(|(k, v)| format!("{k} x{v}")).collect();
    println!("codecs       : {}", codecs.join(", "));
    println!("elapsed      : {:.2}s", started.elapsed().as_secs_f64());

    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let inner = arg(args, 1, "path")?;
    let ref_dir = PathBuf::from(arg(args, 2, "refdir")?);

    let started = Instant::now();
    let rom = nds::Rom::open(&rom_path)?;
    let (archive, _) = open_archive(&rom, &inner)?;

    let mut identical = 0usize;
    let mut differing = 0usize;
    let mut missing = 0usize;
    let mut failed = 0usize;

    for index in 0..archive.len() {
        let reference_path = match reference_path(&ref_dir, index) {
            Some(path) => path,
            None => {
                missing += 1;
                if missing <= 5 {
                    eprintln!("  entry {index:04}: no reference in {}", ref_dir.display());
                }
                continue;
            }
        };
        let reference = std::fs::read(&reference_path)
            .with_context(|| format!("reading {}", reference_path.display()))?;

        match archive.decompressed(index) {
            Ok(data) => {
                if data == reference {
                    identical += 1;
                } else {
                    differing += 1;
                    if differing <= 5 {
                        let first = data.iter().zip(reference.iter()).position(|(a, b)| a != b);
                        eprintln!(
                            "  entry {index:04}: differs at byte {:?} (ours {} bytes, reference {} bytes)",
                            first,
                            data.len(),
                            reference.len()
                        );
                    }
                }
            }
            Err(error) => {
                failed += 1;
                if failed <= 5 {
                    eprintln!("  entry {index:04}: {error:#}");
                }
            }
        }
    }

    println!("`{inner}`: {} entries", archive.len());
    println!("identical    : {identical}");
    println!("differing    : {differing}");
    println!("missing ref  : {missing}");
    println!("failed       : {failed}");
    println!("elapsed      : {:.2}s", started.elapsed().as_secs_f64());

    if differing == 0 && missing == 0 && failed == 0 {
        println!("RESULT: BYTE-IDENTICAL to the reference corpus");
    } else {
        println!("RESULT: MISMATCH");
    }

    Ok(())
}

/// Dumps the stored (still compressed) bytes of one entry, plus how its `MCM`
/// header parses. This is the tool for byte-level analysis when a header looks
/// wrong: it shows exactly what the index points at.
fn cmd_hexdump(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let inner = arg(args, 1, "path")?;
    let index: usize = arg(args, 2, "entry")?
        .parse()
        .context("entry index must be a number")?;
    let length: usize = match args.get(3) {
        Some(value) => value.parse().context("length must be a number")?,
        None => 0x40,
    };

    let rom = nds::Rom::open(&rom_path)?;
    let (archive, _) = open_archive(&rom, &inner)?;
    let stored = archive.stored(index)?;

    println!(
        "entry {index}: stored {} bytes, index declares {} decompressed",
        stored.len(),
        archive.entries[index].decompressed_size
    );

    match mcm::info(stored) {
        Ok(info) => println!(
            "  MCM header: decompressed={} maxChunk={:#x} chunks={} codec1={} codec2={}",
            info.decompressed_size,
            info.max_chunk_size,
            info.chunk_count,
            info.codec1.name(),
            info.codec2.name()
        ),
        Err(error) => println!("  MCM header: {error:#}"),
    }

    let length = length.min(stored.len());
    print!("  bytes:");
    for (i, byte) in stored[..length].iter().enumerate() {
        if i % 16 == 0 {
            print!("\n    ");
        }
        print!(" {byte:02X}");
    }
    println!();

    Ok(())
}

/// Prints the creature -> mesh/animation/texture mapping from the `3CL` manifest.
fn cmd_manifest(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let limit: usize = match args.get(1) {
        Some(value) => value.parse().context("limit must be a number")?,
        None => 8,
    };

    let rom = nds::Rom::open(&rom_path)?;
    let name_table = creature_names(&rom).ok();
    let (archive, _) = open_archive(&rom, "model/battle/arcdin")?;
    let data = archive.decompressed(0)?;
    let parsed = manifest::parse(&data)
        .context("parsing the 3CL manifest in model/battle/arcdin entry 0")?;

    println!(
        "manifest     : {} bytes, {} creatures, {} clips",
        data.len(),
        parsed.creatures.len(),
        parsed.clip_count()
    );

    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    for creature in &parsed.creatures {
        *histogram.entry(creature.clips.len()).or_default() += 1;
    }
    let summary: Vec<String> = histogram
        .iter()
        .map(|(clips, count)| format!("{clips} clips x{count}"))
        .collect();
    println!("clip counts  : {}", summary.join(", "));

    println!();
    println!(
        "{:>4}  {:<12}  {:<14}  {}",
        "id", "name", "clip slots", "mesh / animation / texture"
    );
    for creature in parsed.creatures.iter().take(limit) {
        let slots: Vec<String> = creature.clips.iter().map(|c| c.slot.to_string()).collect();
        let first = &creature.clips[0];
        let name = name_table
            .as_ref()
            .and_then(|names| names.get(creature.id - 1))
            .map(String::as_str)
            .unwrap_or("-");
        println!(
            "{:>4}  {:<12}  {:<14}  {}:{} / {}:{} / {}:{}",
            creature.id,
            name,
            slots.join(","),
            first.mesh.table_name,
            first.mesh.index,
            first.animation.table_name,
            first.animation.index,
            first.texture.table_name,
            first.texture.index
        );
    }
    if parsed.creatures.len() > limit {
        println!("      ... {} more", parsed.creatures.len() - limit);
    }

    Ok(())
}

/// Reads the creature name table out of the game's text archive.
fn creature_names(rom: &nds::Rom) -> Result<Vec<String>> {
    let (archive, _) = open_archive(rom, "text/japanese")?;
    let text = archive
        .decompressed(0)
        .context("decompressing text/japanese entry 0")?;
    names::creature_names(&text, names::CREATURE_COUNT)
}

/// Prints the creature id -> name table.
fn cmd_names(args: &[String]) -> Result<()> {
    let rom_path = arg(args, 0, "rom")?;
    let rom = nds::Rom::open(&rom_path)?;
    let all = creature_names(&rom)?;

    let show_all = args.get(1).map(String::as_str) == Some("all");
    let limit = if show_all { all.len() } else { 12 };

    println!("creatures    : {}", all.len());
    for (index, name) in all.iter().take(limit).enumerate() {
        println!("{:>4}  {}", index + 1, name);
    }
    if limit < all.len() {
        println!("      ... {} more (pass `all` to list every one)", all.len() - limit);
    }

    Ok(())
}
