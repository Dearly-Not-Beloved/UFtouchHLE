/*
 * mhp2g_save: Monster Hunter Portable 2nd G save converter
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version. See COPYING.
 */
//! Command-line interface. See README.md.

use mhp2g_save::game::{self, Platform, HASH_OFF, HEADER_LEN, META_OFF, SEED_OFF};
use mhp2g_save::psp::{Sfo, SAVE_FILE_NAME, SFO_FILE_NAME};
use mhp2g_save::{Error, Save};
use std::path::{Path, PathBuf};

const USAGE: &str = "\
Monster Hunter Portable 2nd G save converter (PSP <-> iOS)

Usage:
  mhp2g-save inspect <save>
  mhp2g-save convert --to ios <save> <savedata.dat>
  mhp2g-save convert --to psp <save> <output dir> --psp-sfo <PARAM.SFO>
  mhp2g-save convert --to psp --game-layer <save> <output file>
  mhp2g-save decrypt <save> <plaintext>
  mhp2g-save encrypt --platform ios|psp <plaintext> <output> [options]

<save> is an iOS savedata.dat, a PSP save directory or MHP2NDG.BIN (its
PARAM.SFO is read if it's next to it), a PSP save without the system layer,
or a plaintext.

Options:
  --template <save>     take the settings block (0x00-0x24) from this save
                        of the target platform
  --psp-sfo <PARAM.SFO> PARAM.SFO of an existing MHP2G PSP save; the output
                        directory gets an updated copy of it and the other
                        files next to it (icons)
  --game-layer          write a PSP save without the system layer
  --force               overwrite existing output files
";

#[derive(Default)]
struct Args {
    positional: Vec<String>,
    to: Option<Platform>,
    template: Option<PathBuf>,
    psp_sfo: Option<PathBuf>,
    game_layer: bool,
    force: bool,
}

fn parse_platform(s: Option<String>) -> Result<Platform, Error> {
    match s.as_deref() {
        Some("ios") => Ok(Platform::Ios),
        Some("psp") => Ok(Platform::Psp),
        _ => Err(Error::new("expected ios or psp")),
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, Error> {
    let mut parsed = Args::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" | "--platform" => parsed.to = Some(parse_platform(args.next())?),
            "--template" => parsed.template = args.next().map(PathBuf::from),
            "--psp-sfo" => parsed.psp_sfo = args.next().map(PathBuf::from),
            "--game-layer" => parsed.game_layer = true,
            "--force" => parsed.force = true,
            _ if arg.starts_with("--") => return Err(Error::new(format!("unknown option {arg}"))),
            _ => parsed.positional.push(arg),
        }
    }
    Ok(parsed)
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::new(format!("{}: {e}", path.display())))
}

fn write(path: &Path, data: &[u8], force: bool) -> Result<(), Error> {
    if path.exists() && !force {
        return Err(Error::new(format!(
            "{} exists; pass --force to overwrite it",
            path.display()
        )));
    }
    std::fs::write(path, data).map_err(|e| Error::new(format!("{}: {e}", path.display())))
}

fn load_save(path: &Path) -> Result<Save, Error> {
    let (file, sfo) = if path.is_dir() {
        (path.join(SAVE_FILE_NAME), path.join(SFO_FILE_NAME))
    } else {
        (path.to_path_buf(), path.with_file_name(SFO_FILE_NAME))
    };
    let params = if sfo.is_file() {
        Some(Sfo::parse(read(&sfo)?)?.params()?)
    } else {
        None
    };
    mhp2g_save::load(&read(&file)?, params)
}

/// Write a plaintext as a save of the platform.
fn write_save(plain: &[u8], to: Platform, output: &Path, args: &Args) -> Result<(), Error> {
    match to {
        Platform::Ios => write(output, &mhp2g_save::ios_file(plain), args.force),
        Platform::Psp if args.game_layer => write(output, &game::encrypt(plain), args.force),
        Platform::Psp => {
            let sfo_path = args.psp_sfo.as_deref().ok_or_else(|| {
                Error::new("a PSP save needs --psp-sfo (or --game-layer)".to_string())
            })?;
            let mut sfo = Sfo::parse(read(sfo_path)?)?;
            let file = mhp2g_save::psp_file(plain, &mut sfo)?;
            // Read the result back before writing anything.
            let check = mhp2g_save::load(&file, Some(sfo.params()?))?;
            assert_eq!(check.plain, plain);

            std::fs::create_dir_all(output)
                .map_err(|e| Error::new(format!("{}: {e}", output.display())))?;
            write(&output.join(SAVE_FILE_NAME), &file, args.force)?;
            write(&output.join(SFO_FILE_NAME), &sfo.0, args.force)?;
            let dir = sfo_path.parent().unwrap_or(Path::new("."));
            let entries = std::fs::read_dir(dir)
                .map_err(|e| Error::new(format!("{}: {e}", dir.display())))?;
            for entry in entries.flatten() {
                let name = entry.file_name();
                if entry.path().is_file() && name != SAVE_FILE_NAME && name != SFO_FILE_NAME {
                    write(&output.join(&name), &read(&entry.path())?, args.force)?;
                }
            }
            Ok(())
        }
    }
}

fn run(command: &str, args: Args) -> Result<(), Error> {
    let input = |i: usize| -> Result<&Path, Error> {
        args.positional
            .get(i)
            .map(Path::new)
            .ok_or_else(|| Error::new(format!("missing arguments\n\n{USAGE}")))
    };
    match command {
        "inspect" => {
            let save = load_save(input(0)?)?;
            let plain = &save.plain;
            let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>();
            println!("platform: {} ({:?})", save.platform.name(), save.format);
            println!("name:     {}", game::character_name(plain));
            println!("settings: {}", hex(&plain[..HEADER_LEN]).join(" "));
            println!("metadata: {}", hex(&plain[META_OFF..HASH_OFF]).join(" "));
            println!(
                "sha1:     {} (ok)",
                hex(&plain[HASH_OFF..SEED_OFF]).concat()
            );
            println!("seed:     0x{:08X}", game::seed_of(plain));
        }
        "convert" => {
            let to = args.to.ok_or_else(|| Error::new("convert needs --to"))?;
            let save = load_save(input(0)?)?;
            let template = match &args.template {
                Some(path) => {
                    let template = load_save(path)?;
                    if template.platform != to {
                        return Err(Error::new(format!(
                            "the template must be a {} save",
                            to.name()
                        )));
                    }
                    Some(template.plain)
                }
                None => None,
            };
            let plain = mhp2g_save::convert(&save.plain, save.platform, to, template.as_deref());
            write_save(&plain, to, input(1)?, &args)?;
            println!(
                "{} -> {}: {} ({})",
                save.platform.name(),
                to.name(),
                input(1)?.display(),
                game::character_name(&plain)
            );
        }
        "decrypt" => {
            let save = load_save(input(0)?)?;
            write(input(1)?, &save.plain, args.force)?;
            println!(
                "{} plaintext: {}",
                save.platform.name(),
                input(1)?.display()
            );
        }
        "encrypt" => {
            let to = args
                .to
                .ok_or_else(|| Error::new("encrypt needs --platform"))?;
            let plain = mhp2g_save::complete_plain(&read(input(0)?)?, to)?;
            write_save(&plain, to, input(1)?, &args)?;
            println!("{} save: {}", to.name(), input(1)?.display());
        }
        _ => return Err(Error::new(format!("unknown command {command}\n\n{USAGE}"))),
    }
    Ok(())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next().filter(|c| c != "--help" && c != "-h") else {
        print!("{USAGE}");
        return;
    };
    if let Err(e) = parse_args(args).and_then(|args| run(&command, args)) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
