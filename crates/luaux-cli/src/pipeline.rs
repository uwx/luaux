//! Compiling a source tree, shared by `build`, `check`, and `watch`.
//!
//! The three differ only in whether output is written and how the result is
//! reported, so they run the same pipeline rather than three drifting copies.

use crate::select::Selector;
use luaux::{CompileError, Config};
use std::path::{Path, PathBuf};

pub struct Options {
    pub source_root: PathBuf,
    /// Where compiled output goes. `None` writes each `X.luaux` beside itself as
    /// `X.luau`.
    pub out_root: Option<PathBuf>,
    /// `false` compiles and reports without touching the filesystem — `check`.
    pub write: bool,
}

impl Options {
    /// Where compiled output for `input` lands.
    pub fn output_for(&self, input: &Path) -> PathBuf {
        match &self.out_root {
            None => input.with_extension("luau"),
            Some(root) => {
                let relative = input.strip_prefix(&self.source_root).unwrap_or(input);
                root.join(relative).with_extension("luau")
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub compiled: usize,
    pub unchanged: usize,
    pub copied: usize,
    pub removed: usize,
    pub warnings: usize,
    pub failures: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failures == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "{} compiled, {} unchanged, {} copied, {} removed, {} warning(s), {} failed",
            self.compiled, self.unchanged, self.copied, self.removed, self.warnings, self.failures
        )
    }
}

/// Loads `luaux.toml`, walking up from the source root.
///
/// luaux used to detect where Vide lived and emit a `require` for it. That is
/// gone: a require is location-dependent, so no single configured string is
/// correct for every file, and an author's own import always is. All the
/// compiler needs now is that `[factory] create` names something in scope.
pub fn configure(source_root: &Path) -> Result<Config, String> {
    find_config(source_root)
}

/// The backend named by `[factory] backend`.
///
/// Boxed rather than matched at each call site: the choice is per-build and the
/// selection belongs in one place, not once per file.
fn backend(config: &Config) -> Box<dyn luaux::Backend> {
    match config.backend {
        luaux::config::BackendKind::Table => Box::new(luaux::Table),
        luaux::config::BackendKind::Element => Box::new(luaux::Element),
        luaux::config::BackendKind::Curried => Box::new(luaux::Curried),
    }
}

pub fn run(options: &Options, config: &Config) -> Report {
    let backend = backend(config);

    let mut report = Report::default();

    let selector = match Selector::new(&config.build.include, &config.build.exclude) {
        Ok(selector) => selector,
        Err(error) => {
            eprintln!("luaux: luaux.toml: {error}");
            report.failures += 1;
            return report;
        }
    };

    let mut inputs = Vec::new();
    if let Err(error) = collect_files(&options.source_root, &mut inputs) {
        eprintln!("luaux: {}: {error}", options.source_root.display());
        report.failures += 1;
        return report;
    }

    inputs.retain(|path| {
        path.extension().and_then(|e| e.to_str()) == Some("luaux")
            && selector.allows(relative(path, &options.source_root))
    });
    inputs.sort();

    for input in &inputs {
        let source = match std::fs::read_to_string(input) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("luaux: {}: {error}", input.display());
                report.failures += 1;
                continue;
            }
        };

        let path = input.display().to_string();

        let (compiled, warnings) = match luaux::compile_verified(&source, backend.as_ref(), config)
        {
            Ok(result) => result,
            Err(error) => {
                diagnose(&path, &source, &error, true);
                report.failures += 1;
                continue;
            }
        };

        for warning in warnings {
            diagnose(
                &path,
                &source,
                &CompileError {
                    message: warning.message,
                    offset: warning.offset,
                    length: warning.length,
                    help: warning.help,
                },
                false,
            );
            report.warnings += 1;
        }

        if !options.write {
            report.compiled += 1;
            continue;
        }

        let output = options.output_for(input);

        // Skipping an identical write keeps rojo from seeing an event for
        // content that did not move, and stops `watch` retriggering itself.
        if reads_same(&output, compiled.as_bytes()) {
            report.unchanged += 1;
            continue;
        }

        if let Err(error) = write_file(&output, compiled.as_bytes()) {
            eprintln!("luaux: {error}");
            report.failures += 1;
            continue;
        }

        println!("{} -> {}", input.display(), output.display());
        report.compiled += 1;
    }

    // Everything that is not a .luaux source has to reach the output tree too,
    // or rojo sees a broken one — .meta.json and .rbxmx as much as .luau
    // (PLAN.md §11.5).
    if options.write {
        if let Some(root) = &options.out_root {
            match passthrough(&options.source_root, root, &selector) {
                Ok(count) => report.copied = count,
                Err(error) => {
                    eprintln!("luaux: {error}");
                    report.failures += 1;
                }
            }

            if config.build.clean {
                match clean(&options.source_root, root) {
                    Ok(count) => report.removed = count,
                    Err(error) => {
                        eprintln!("luaux: {error}");
                        report.failures += 1;
                    }
                }
            }
        }
    }

    report
}

/// Deletes outputs whose source is gone.
///
/// Renaming `Foo.luaux` otherwise leaves `Foo.luau` behind for rojo to keep
/// syncing. Only ever touches the output tree, and `run` only calls this when an
/// output directory was given — cleaning in place would delete hand-written
/// files sitting beside their sources.
fn clean(source_root: &Path, out_root: &Path) -> Result<usize, String> {
    let mut outputs = Vec::new();
    collect_files(out_root, &mut outputs)
        .map_err(|error| format!("{}: {error}", out_root.display()))?;

    let mut removed = 0usize;

    for output in outputs {
        let relative = relative(&output, out_root).to_path_buf();
        let from_source = source_root.join(&relative);

        // A .luau may have come from either a .luaux or a copied .luau.
        let has_source = from_source.is_file()
            || (relative.extension().and_then(|e| e.to_str()) == Some("luau")
                && from_source.with_extension("luaux").is_file());

        if has_source {
            continue;
        }

        std::fs::remove_file(&output).map_err(|error| format!("{}: {error}", output.display()))?;
        println!("{} (removed, no source)", output.display());
        removed += 1;
    }

    Ok(removed)
}

/// Copies every non-source file into the output tree.
///
/// Deliberately not limited to `.luau`: a rojo `src/` routinely holds
/// `init.meta.json`, `*.model.json`, `.rbxmx`, and data files, none of which the
/// compiler touches but all of which rojo needs.
fn passthrough(source_root: &Path, out_root: &Path, selector: &Selector) -> Result<usize, String> {
    let mut sources = Vec::new();
    collect_files(source_root, &mut sources)
        .map_err(|error| format!("{}: {error}", source_root.display()))?;

    let mut copied = 0usize;

    for source in sources {
        if source.extension().and_then(|e| e.to_str()) == Some("luaux") {
            continue;
        }

        // Never clobber a compiled artifact with a stale hand-written file of
        // the same name; that would silently undo the compile.
        if source.with_extension("luaux").is_file() {
            continue;
        }

        let relative = relative(&source, source_root);

        if !selector.allows(relative) {
            continue;
        }

        let destination = out_root.join(relative);

        let contents =
            std::fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;

        if reads_same(&destination, &contents) {
            continue;
        }

        write_file(&destination, &contents)?;

        println!("{} -> {} (copied)", source.display(), destination.display());
        copied += 1;
    }

    Ok(copied)
}

fn reads_same(path: &Path, contents: &[u8]) -> bool {
    std::fs::read(path).is_ok_and(|existing| existing == contents)
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }

    std::fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

/// Every file under `path`, recursively. Filtering is the caller's job.
pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }

    if !path.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?.path();

        if entry.is_dir() {
            collect_files(&entry, out)?;
        } else {
            out.push(entry);
        }
    }

    Ok(())
}

fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// Walks up from `start` looking for `luaux.toml`, so it can live at the project
/// root while sources sit under `src/`. An absent file is not an error.
fn find_config(start: &Path) -> Result<Config, String> {
    for directory in ancestors(start) {
        if !directory.join("luaux.toml").is_file() {
            continue;
        }

        let (config, warnings) =
            Config::load_reporting(&directory).map_err(|error| error.message)?;

        for warning in warnings {
            eprintln!("luaux: {warning}");
        }

        return Ok(config);
    }

    Ok(Config::default())
}

/// `start` and its ancestors, starting with the directory containing `start`.
pub fn ancestors(start: &Path) -> Vec<PathBuf> {
    let absolute = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let first = if absolute.is_dir() {
        absolute.as_path()
    } else {
        absolute.parent().unwrap_or(absolute.as_path())
    };

    first.ancestors().map(Path::to_path_buf).collect()
}

/// Renders a diagnostic with its source line and an underline.
///
/// Error quality is most of a compiler's UX, and Luau has no runtime source
/// maps — so a diagnostic pointing at the exact `.luaux` column is the main
/// thing a user gets. Line-preserving codegen (PLAN.md §5.5) makes the numbers
/// here line up with the generated file too.
fn diagnose(path: &str, source: &str, error: &CompileError, fatal: bool) {
    let offset = error.offset.min(source.len());
    let length = error
        .length
        .clamp(1, source.len().saturating_sub(offset).max(1));

    let label = miette::LabeledSpan::new(error.help.clone(), offset, length);

    let diagnostic = miette::miette!(
        severity = if fatal {
            miette::Severity::Error
        } else {
            miette::Severity::Warning
        },
        labels = vec![label],
        "{}",
        error.message
    )
    .with_source_code(miette::NamedSource::new(path, source.to_string()).with_language("Lua"));

    eprintln!("{diagnostic:?}");
}
