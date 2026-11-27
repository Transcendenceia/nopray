//! `nopray`: layout, audit, map and review tool for hand-authored machine code (DMCS-1.1).
//!
//! The tool never chooses or encodes instructions. It copies authored bytes into declared
//! slots, decodes them for review, and checks them against the map and the security policy.

mod annotate;
mod audit;
mod context;
mod diff;
mod elf;
mod esp;
mod fixup;
mod hexfile;
mod image;
mod import;
mod intent;
mod isa;
mod manifest;
mod render;
mod syscalls;

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::annotate::Annotation;
use crate::audit::Severity;
use crate::manifest::{AppDir, Isa, Os};

#[derive(Debug, Parser)]
#[command(
    name = "nopray",
    version,
    about = "Lay out, audit and map hand-authored machine code (NOPRAY / DMCS-1.1)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lay out slots into build/<output>, build/image.hex and build/manifest.lock.json.
    Build { dir: PathBuf },
    /// Audit the map and bytes; writes build/audit.json and build/MAP.md.
    Audit {
        dir: PathBuf,
        /// Binary that must be byte-identical to the map (e.g. the deployed file).
        #[arg(long)]
        binary: Option<PathBuf>,
        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,
    },
    /// Regenerate slot-file comments (disassembly, labels, syscall names) from the bytes.
    Annotate {
        dir: PathBuf,
        /// Report stale annotations without rewriting.
        #[arg(long)]
        check: bool,
    },
    /// Draft a manifest for an existing ELF binary.
    Propose {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "x86-64")]
        isa: String,
        /// First address of read-only data inside the executable segment (hex).
        #[arg(long, value_parser = parse_hex)]
        code_end: Option<u64>,
    },
    /// Draft a container-granularity manifest for an ESP32-S3 loader image (`esp-image`).
    ProposeEsp {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "0.0.0")]
        version: String,
        /// Who produced the bytes (recorded in `[provenance]`).
        #[arg(long)]
        authored_by: String,
        /// Provenance date (YYYY-MM-DD).
        #[arg(long)]
        date: String,
    },
    /// Extract slot files from a binary using a manifest, and prove the rebuild is identical.
    Import {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Slot-level review of the changes between two app directories.
    Diff { old: PathBuf, new: PathBuf },
    /// Everything needed to change one slot: contract, callers, state, free room, fields.
    Context {
        dir: PathBuf,
        /// Slot or label name.
        symbol: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Append the slot's current annotated listing.
        #[arg(long)]
        listing: bool,
    },
    /// Print a systemd `SystemCallFilter=` allow-list from the audited syscall sites.
    Seccomp { dir: PathBuf },
}

fn parse_hex(s: &str) -> Result<u64, String> {
    let t = s.trim_start_matches("0x");
    u64::from_str_radix(t, 16).map_err(|e| format!("invalid hex `{s}`: {e}"))
}

fn parse_isa(s: &str) -> Result<Isa> {
    let v: Isa = serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .with_context(|| format!("unknown isa `{s}`"))?;
    Ok(v)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("nopray: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Build { dir } => {
            let app = AppDir::load(&dir)?;
            let img = image::build(&app)?;
            let out = app.build_dir();
            fs::create_dir_all(&out)?;
            let bin = out.join(&app.manifest.target.output);
            let _ = fs::remove_file(&bin); // avoid ETXTBSY when replacing a running binary
            fs::write(&bin, &img.bytes)?;
            set_executable(&bin)?;
            fs::write(out.join("image.hex"), image::to_plain_hex(&img.bytes))?;
            fs::write(
                out.join("manifest.lock.json"),
                serde_json::to_string_pretty(&img.lock(&app))? + "\n",
            )?;
            println!(
                "built {} ({} bytes) sha256 {}",
                bin.display(),
                img.bytes.len(),
                image::sha256_hex(&img.bytes)
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Audit {
            dir,
            binary,
            strict,
        } => {
            let app = AppDir::load(&dir)?;
            let img = image::build(&app)?;
            let default_ref = app.build_dir().join(&app.manifest.target.output);
            let reference_path = binary.or_else(|| default_ref.exists().then_some(default_ref));
            let reference = match &reference_path {
                Some(p) => Some((
                    p.display().to_string(),
                    fs::read(p).with_context(|| format!("reading {}", p.display()))?,
                )),
                None => None,
            };
            let mut report = audit::run(
                &app,
                &img,
                reference.as_ref().map(|(l, b)| (l.as_str(), b.as_slice())),
            );
            for (name, state) in annotate::run(&app, true)? {
                if state == Annotation::Stale {
                    report.findings.push(audit::Finding {
                        severity: Severity::Warning,
                        code: "A001",
                        slot: Some(name),
                        addr: None,
                        message: "slot comments are stale: run `nopray annotate` and review the regenerated disassembly".into(),
                    });
                }
            }
            let out = app.build_dir();
            fs::create_dir_all(&out)?;
            fs::write(
                out.join("audit.json"),
                serde_json::to_string_pretty(&report)? + "\n",
            )?;
            fs::write(out.join("MAP.md"), render::map(&app, &report))?;
            print!("{}", render::summary(&report));
            println!(
                "wrote {} and {}",
                out.join("audit.json").display(),
                out.join("MAP.md").display()
            );
            let failed = report.count(Severity::Error) > 0
                || (strict && report.count(Severity::Warning) > 0);
            Ok(if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Annotate { dir, check } => {
            let app = AppDir::load(&dir)?;
            let results = annotate::run(&app, check)?;
            let mut stale = false;
            for (name, state) in &results {
                if *state != Annotation::Unchanged {
                    println!("{name}: {state:?}");
                    stale |= *state == Annotation::Stale;
                }
            }
            println!("{} slot file(s) checked", results.len());
            Ok(if stale {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Propose {
            binary,
            name,
            isa,
            code_end,
        } => {
            let bytes =
                fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
            let opts = import::ProposeOpts {
                name,
                isa: parse_isa(&isa)?,
                code_end,
            };
            print!("{}", import::propose(&bytes, &opts)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::ProposeEsp {
            binary,
            name,
            version,
            authored_by,
            date,
        } => {
            let bytes =
                fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
            let output = binary
                .file_name()
                .context("binary path has no file name")?
                .to_string_lossy()
                .into_owned();
            let opts = esp::ProposeOpts {
                name,
                version,
                output,
                authored_by,
                date,
            };
            print!("{}", esp::propose(&bytes, &opts)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Import {
            binary,
            manifest,
            out,
        } => {
            let bytes =
                fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
            let app = import::import(&bytes, &manifest, &out)?;
            println!(
                "imported {} slots into {} (rebuild is byte-identical)",
                app.manifest.slots.len(),
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Diff { old, new } => {
            let old = AppDir::load(&old)?;
            let new = AppDir::load(&new)?;
            print!("{}", diff::run(&old, &new)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Context {
            dir,
            symbol,
            json,
            listing,
        } => {
            let app = AppDir::load(&dir)?;
            let img = image::build(&app)?;
            let report = audit::run(&app, &img, None);
            let ctx = context::build(&app, &img, &report, &symbol)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&ctx)?);
            } else {
                print!("{}", context::render(&ctx));
            }
            if listing {
                let index = app
                    .manifest
                    .slots
                    .iter()
                    .position(|s| s.name == ctx.slot)
                    .expect("context slot exists");
                let ls = img
                    .slots
                    .iter()
                    .find(|l| l.index == index)
                    .expect("slot loaded");
                let text = annotate::render_slot(
                    &app,
                    &app.manifest.slots[index],
                    &ls.bytes,
                    &ls.intents,
                    &ls.fixups,
                );
                println!("\n```text\n{text}```");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Seccomp { dir } => {
            let app = AppDir::load(&dir)?;
            if app.manifest.target.os != Os::Linux {
                bail!("seccomp filters apply to Linux targets only");
            }
            let img = image::build(&app)?;
            let report = audit::run(&app, &img, None);
            let unresolved = report
                .syscall_sites
                .iter()
                .filter(|s| s.name.is_none())
                .count();
            if unresolved > 0 {
                bail!(
                    "{unresolved} syscall site(s) are unresolved; a static allow-list would be incomplete"
                );
            }
            let names: Vec<&str> = report.syscall_set.iter().map(String::as_str).collect();
            println!(
                "# {} sites, {} distinct syscalls, audited from the bytes",
                report.syscall_sites.len(),
                names.len()
            );
            println!("SystemCallArchitectures=native");
            println!("SystemCallFilter={}", names.join(" "));
            println!(
                "# No SystemCallErrorNumber=: any other syscall kills the process (SIGSYS), the right reaction for hand-written code."
            );
            if let Some(sc) = &app.manifest.syscalls {
                let unused: Vec<&String> = sc
                    .allow
                    .iter()
                    .filter(|a| !report.syscall_set.contains(*a))
                    .collect();
                if !unused.is_empty() {
                    eprintln!("note: allow-listed but never used (trim the manifest): {unused:?}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(unix)]
fn set_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path)?.permissions();
    p.set_mode(0o755);
    fs::set_permissions(path, p)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_: &std::path::Path) -> Result<()> {
    Ok(())
}
