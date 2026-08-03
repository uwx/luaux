//! `luaux watch` — rebuild on change, to run beside `rojo serve`.

use crate::pipeline::{self, Options};
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Editors save in bursts — write, rename, chmod — so events are coalesced
/// rather than triggering a rebuild each.
const DEBOUNCE: Duration = Duration::from_millis(120);

pub fn run(options: Options) -> Result<(), String> {
    let config_path = pipeline::ancestors(&options.source_root)
        .into_iter()
        .map(|directory| directory.join("luaux.toml"))
        .find(|path| path.is_file());

    let (sender, receiver) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |event| {
        // A send failure only means the receiver is gone, i.e. we are exiting.
        let _ = sender.send(event);
    })
    .map_err(|error| format!("watch: {error}"))?;

    watcher
        .watch(&options.source_root, RecursiveMode::Recursive)
        .map_err(|error| format!("watch {}: {error}", options.source_root.display()))?;

    // Config changes alias, lint, and import behaviour, so they must rebuild too.
    if let Some(path) = &config_path {
        let _ = watcher.watch(path, RecursiveMode::NonRecursive);
    }

    println!("luaux: watching {}", options.source_root.display());

    match &config_path {
        Some(path) => println!("luaux: watching {}", path.display()),
        // notify cannot watch a file that does not exist, and watching the whole
        // project root to catch one that might appear is worse than saying so.
        None => println!("luaux: no luaux.toml found — restart to pick one up"),
    }

    build_once(&options);

    loop {
        // Block until something happens, then drain the burst.
        let Ok(first) = receiver.recv() else {
            return Ok(());
        };

        let mut paths = event_paths(first);
        while let Ok(next) = receiver.recv_timeout(DEBOUNCE) {
            paths.extend(event_paths(next));
        }

        if !paths.iter().any(|path| relevant(path, &options)) {
            continue;
        }

        println!();
        build_once(&options);
    }
}

fn build_once(options: &Options) {
    let config = match pipeline::configure(&options.source_root) {
        Ok(config) => config,
        Err(error) => {
            // Keep watching: a broken luaux.toml is usually mid-edit.
            eprintln!("luaux: {error}");
            return;
        }
    };

    let report = pipeline::run(options, &config);
    println!("luaux: {}", report.summary());
}

fn event_paths(event: notify::Result<notify::Event>) -> Vec<PathBuf> {
    event.map(|event| event.paths).unwrap_or_default()
}

/// Whether a changed path should trigger a rebuild.
///
/// Output is filtered out deliberately. When `out_root` sits inside the source
/// tree — or output is written beside its input — our own writes would otherwise
/// wake the watcher and rebuild forever. Skipping identical writes in the
/// pipeline already breaks that loop; this stops it starting.
fn relevant(path: &Path, options: &Options) -> bool {
    if let Some(out_root) = &options.out_root {
        if under(path, out_root) {
            return false;
        }
    }

    match path.extension().and_then(|extension| extension.to_str()) {
        Some("luaux") => true,
        // Passthrough inputs, but not our own emitted output.
        Some("luau") => options.out_root.is_some() && !is_generated(path),
        _ => path.file_name().is_some_and(|name| name == "luaux.toml"),
    }
}

/// A `.luau` sitting next to a `.luaux` of the same name is ours.
fn is_generated(path: &Path) -> bool {
    path.with_extension("luaux").is_file()
}

fn under(path: &Path, root: &Path) -> bool {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}
