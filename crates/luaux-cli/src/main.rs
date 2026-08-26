//! luaux CLI.
//!
//! * `build <src> [out]` — compile `.luaux` to `.luau` and copy plain `.luau`
//!   through (§11.5).
//! * `check <src>` — the same, without writing anything.
//! * `watch <src> [out]` — rebuild on change, to run beside `rojo serve`.
//! * `init [dir] [--library <name>]` — scaffold a `luaux.toml`.
//! * `scan <path>...` — report where the lexer believes LuauX begins. Pointed at a
//!   tree of plain `.luau`, every hit is a false positive; that is the Phase 0
//!   acceptance check (PLAN.md §5.2).

mod init;
mod pipeline;
mod select;
mod watch;

use pipeline::Options;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Stack for the thread every command runs on.
///
/// `compile_verified` re-parses its own output with full_moon, whose recursive
/// descent has large stack frames in debug builds. The inlined merge helper is
/// deep enough to exhaust the main thread's default stack — 1 MB on Windows —
/// so `luaux build` on any file containing a spread aborted with a stack
/// overflow before compiling anything. The test suites already run on a larger
/// stack for exactly this reason (see `compile::tests`); the CLI needs the same.
const STACK: usize = 16 * 1024 * 1024;

/// Rust's own exit code for a panic, which running off the main thread would
/// otherwise turn into an ordinary `1` — the code a failed *compile* returns.
/// A wrapper script that tells "luaux crashed" from "the source has errors"
/// depends on those staying distinct.
const PANICKED: u8 = 101;

fn main() -> ExitCode {
    // Named, so a panic reports `thread 'luaux'` rather than `<unnamed>` and a
    // pasted bug report still says where it came from.
    let spawned = std::thread::Builder::new()
        .name("luaux".to_string())
        .stack_size(STACK)
        .spawn(run);

    match spawned {
        // The panic has already printed through the default hook, so the exit
        // code is all that is left to carry.
        Ok(handle) => handle.join().unwrap_or(ExitCode::from(PANICKED)),
        // If a thread cannot be spawned at all, running on the main stack is
        // better than not running: only a deep parse needs the extra room.
        Err(_) => run(),
    }
}

fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: &[String] = args.get(1..).unwrap_or(&[]);

    match args.first().map(String::as_str) {
        Some("build") => build(rest, true),
        Some("check") => build(rest, false),
        Some("watch") => start_watch(rest),
        Some("init") => run_init(rest),
        Some("scan") if !rest.is_empty() => scan(rest),
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  luaux build [src] [out]   compile .luaux -> .luau");
    eprintln!("  luaux check [src]         compile without writing");
    eprintln!("  luaux watch [src] [out]   rebuild on change");
    eprintln!("  luaux init [dir]          scaffold a luaux.toml");
    eprintln!("    --library <name>        write a [factory] block: react, vide, fluid, fusion");
    eprintln!("  luaux scan <path>...      report where LuauX is detected");
    eprintln!();
    eprintln!("  paths default to [build] in/out in luaux.toml; arguments override them.");
    eprintln!("  with no [out], each X.luaux is written beside itself as X.luau.");
    eprintln!("  every non-source file is copied into [out] when one is given.");
}

/// Resolves paths and config together.
///
/// CLI arguments win over `[build] in`/`out`, so a config can set the usual
/// paths while `luaux build other/src` still overrides them. Config is looked up
/// from the source root when one was given, and from the working directory when
/// it was not — which is what makes a bare `luaux build` work.
fn plan(args: &[String], write: bool) -> Result<(Options, luaux::Config), String> {
    let anchor = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let config = pipeline::configure(&anchor)?;

    let source_root = match args.first() {
        Some(path) => PathBuf::from(path),
        None => config
            .build
            .input
            .clone()
            .ok_or_else(|| "no source given, and luaux.toml sets no [build] in".to_string())?,
    };

    let out_root = match args.get(1) {
        Some(path) => Some(PathBuf::from(path)),
        None => config.build.output.clone(),
    };

    if !source_root.exists() {
        return Err(format!("{}: not found", source_root.display()));
    }

    Ok((
        Options {
            source_root,
            out_root,
            write,
        },
        config,
    ))
}

fn build(args: &[String], write: bool) -> ExitCode {
    let (options, config) = match plan(args, write) {
        Ok(planned) => planned,
        Err(error) => {
            eprintln!("luaux: {error}");
            return ExitCode::FAILURE;
        }
    };

    let report = pipeline::run(&options, &config);
    println!("\n{}", report.summary());

    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn start_watch(args: &[String]) -> ExitCode {
    let options = match plan(args, true) {
        Ok((options, _)) => options,
        Err(error) => {
            eprintln!("luaux: {error}");
            return ExitCode::FAILURE;
        }
    };

    match watch::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("luaux: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `luaux init [dir] [--library <name>]`.
///
/// The flag is accepted in either position, since `init --library react` and
/// `init . --library react` are both natural to type.
fn run_init(args: &[String]) -> ExitCode {
    let mut directory = Path::new(".");
    let mut library = None;
    let mut rest = args.iter();

    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--library" => match rest.next() {
                Some(name) => library = Some(name.as_str()),
                None => {
                    eprintln!("luaux: --library needs a name");
                    return ExitCode::FAILURE;
                }
            },
            other => directory = Path::new(other),
        }
    }

    match init::run(directory, library) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("luaux: {error}");
            ExitCode::FAILURE
        }
    }
}

fn scan(paths: &[String]) -> ExitCode {
    let mut files: Vec<PathBuf> = Vec::new();

    for path in paths {
        if let Err(error) = pipeline::collect_files(Path::new(path), &mut files) {
            eprintln!("luaux: {path}: {error}");
            return ExitCode::FAILURE;
        }
    }

    files.retain(|path| {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("luau") | Some("lua")
        )
    });
    files.sort();
    files.dedup();

    let mut sites = 0usize;
    let mut failures = 0usize;
    let mut skipped = 0usize;

    for file in &files {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("luaux: {}: {error}", file.display());
                failures += 1;
                continue;
            }
        };

        // luaux requires UTF-8 source. Some Lua test corpora embed raw bytes in
        // string literals; those are out of scope rather than a lexer failure.
        let Ok(source) = String::from_utf8(bytes) else {
            skipped += 1;
            continue;
        };

        let tokens = match luaux::tokenize(&source) {
            Ok(tokens) => tokens,
            Err(error) => {
                eprintln!("luaux: {}: lex error: {error}", file.display());
                failures += 1;
                continue;
            }
        };

        for site in luaux::find_luaux_sites(&source, &tokens) {
            let (line, column) = line_and_column(&source, site.offset);
            println!(
                "{}:{}:{}: luaux starts here — {}",
                file.display(),
                line,
                column,
                snippet(&source, site.offset)
            );
            sites += 1;
        }
    }

    println!(
        "\n{} file(s), {} luaux site(s), {} failure(s), {} skipped (not utf-8)",
        files.len(),
        sites,
        failures,
        skipped
    );

    if failures > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rfind('\n').map_or(offset, |at| offset - at - 1) + 1;
    (line, column)
}

fn snippet(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at);
    source[start..end].trim()
}
