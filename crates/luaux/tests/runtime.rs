//! Golden runtime tests — compile fixtures, then *run* them against real Vide.
//!
//! Every other test checks that output is well-formed. This one checks that it
//! behaves: that Vide parents the children, that interpolated text tracks its
//! sources, that a nil child does not truncate its siblings. Those are claims
//! about semantics, and nothing short of execution verifies them.
//!
//! Vide and Lune both live outside this repository, so the test is opt-in and
//! reports why it skipped rather than passing silently:
//!
//! ```sh
//! git clone --depth 1 https://github.com/centau/vide
//! LUAUX_VIDE=vide LUAUX_LUNE=$(which lune) \
//!   cargo test -p luaux --test runtime -- --nocapture
//! ```
//!
//! Vide's `lib.luau` is the non-Roblox entry point; `init.luau` asserts it is
//! running inside Roblox. `Instance`, `Color3`, `UDim2` and `UDim` are shimmed
//! from `@lune/roblox`, and Vide's `apply.luau` requires its own `test/mock`, so
//! that directory is copied too.

use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &str = "tests/runtime/fixtures";

#[test]
fn generated_code_behaves_under_vide() {
    // `compile_verified` re-parses with full_moon, whose recursive descent has
    // large stack frames in debug builds — more than a test thread's 2 MB. The
    // CLI runs on a larger stack for the same reason; see `STACK` in
    // luaux-cli's main.rs.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .expect("spawn")
        .join()
        .expect("golden runtime thread");
}

fn run() {
    let (Some(vide), Some(lune)) = (env_path("LUAUX_VIDE"), env_path("LUAUX_LUNE")) else {
        eprintln!(
            "skipped: set LUAUX_VIDE to a Vide checkout and LUAUX_LUNE to a lune binary\n\
             \x20 e.g. LUAUX_VIDE=../vide LUAUX_LUNE=$(which lune) cargo test --test runtime"
        );
        return;
    };

    let workspace = std::env::temp_dir().join("luaux-runtime-golden");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(workspace.join("fixtures")).expect("create workspace");

    // Vide's non-Roblox entry point plus the mock its apply.luau requires.
    copy_tree(&vide.join("src"), &workspace.join("vide")).expect("copy vide/src");
    copy_tree(&vide.join("test"), &workspace.join("test")).expect("copy vide/test");

    let mut names = Vec::new();

    for fixture in fixtures() {
        let name = fixture
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .to_string();

        let source = std::fs::read_to_string(&fixture).expect("read fixture");

        // Fixtures import Vide themselves, so nothing is injected for
        // `create`/`read` — that plumbing has unit tests. What is exercised here
        // is the emitted code, including the inlined merge helper.
        let compiled = luaux::compile_verified(&source, &luaux::Vide, &luaux::Config::default())
            .map(|(output, _)| output)
            .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));

        assert_eq!(
            compiled.lines().count(),
            source.lines().count(),
            "{}: line count changed",
            fixture.display()
        );

        std::fs::write(
            workspace.join("fixtures").join(format!("{name}.luau")),
            compiled,
        )
        .expect("write compiled fixture");

        names.push(name);
    }

    assert!(!names.is_empty(), "no fixtures found under {FIXTURES}");
    names.sort();

    std::fs::write(workspace.join("run.luau"), runner(&names)).expect("write runner");

    let output = Command::new(&lune)
        .arg("run")
        .arg("run.luau")
        .current_dir(&workspace)
        .output()
        .expect("run lune");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("{stdout}");

    assert!(
        output.status.success(),
        "fixtures failed under Vide\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

/// The Lune script that shims Roblox, then runs each fixture inside a reactive
/// root so effects have somewhere to live.
fn runner(names: &[String]) -> String {
    let requires = names
        .iter()
        .map(|name| format!("\t{{ \"{name}\", require(\"./fixtures/{name}\") }},"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"local roblox = require("@lune/roblox")

-- Vide creates real Instances; Lune provides them, minus a few methods.
_G.Instance = roblox.Instance
_G.Color3 = roblox.Color3
_G.UDim2 = roblox.UDim2
_G.UDim = roblox.UDim

local vide = require("./vide/lib")

local failures = 0
local checks = 0

local fixtures = {{
{requires}
}}

for _, entry in fixtures do
    local name, fixture = entry[1], entry[2]

    vide.root(function()
        fixture(function(label, ok)
            checks += 1
            if not ok then
                failures += 1
                print(`  FAIL  {{name}}: {{label}}`)
            end
        end)
    end)
end

print(`{{checks}} check(s), {{failures}} failure(s)`)

if failures > 0 then
    process.exit(1)
end
"#
    )
}

fn fixtures() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(FIXTURES)
        .expect("read fixtures directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("luaux"))
        .collect();

    found.sort();
    found
}

fn env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var(key).ok()?;
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;

    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let destination = to.join(entry.file_name());

        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            std::fs::copy(&source, &destination)?;
        }
    }

    Ok(())
}
