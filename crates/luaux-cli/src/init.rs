//! `luaux init` — scaffold a `luaux.toml`.
//!
//! Everything outside the `[factory]` block is commented out, because luaux
//! works without any of it. The point is to show what *can* be set and what the
//! defaults are, not to impose settings nobody asked for.
//!
//! `--library` is the exception, and it writes a real block rather than a
//! commented one. That is deliberate: it is scaffolding, not a config key. A
//! `preset = "fusion"` setting would bake one library into the compiler and make
//! every library without a preset second-class, where a written-out block is
//! self-describing and a new library is one paste rather than a release
//! (backend-plan.md §6).

use std::path::Path;

/// A library's `[factory]` block, as a project would write it by hand.
struct Library {
    name: &'static str,
    /// Which binding the generated note tells the author to import.
    import: &'static str,
    block: &'static str,
}

const LIBRARIES: &[Library] = &[
    Library {
        name: "react",
        import: "React",
        block: r#"[factory]
backend = "element"                 # F(class, props, children)
create = "React.createElement"
event = "React.Event."              # trailing dot: [React.Event.Activated]
fragment = "React.Fragment"
interpolate = "plain"               # no per-prop reactivity to preserve
"#,
    },
    Library {
        name: "vide",
        import: "create",
        block: r#"[factory]
backend = "table"                   # F(class)(propsAndChildren)
create = "create"                   # or "vide.create", if you keep it in one binding
"#,
    },
    Library {
        name: "fluid",
        import: "fluid",
        // Fluid is Vide's arrangement in every respect that reaches luaux:
        // curried construction, children as numeric keys in the props table,
        // events as plain string keys, and a function on a property key as the
        // reactive value. So the block is Vide's with a different name.
        block: r#"[factory]
backend = "table"                   # F(class)(propsAndChildren)
create = "fluid.create"
"#,
    },
    Library {
        name: "fusion",
        import: "scope",
        block: r#"[factory]
backend = "table"                   # same arrangement as Vide, contents moved
create = "scope:New"
children = "Children"
event = "OnEvent"                   # called: [OnEvent("Activated")]
compute = "scope:Computed"
use = "use"                         # the reader inside compute's callback
"#,
    },
];

const TEMPLATE: &str = r#"# luaux configuration. Every setting is optional.

# Where the build reads and writes, and which files it considers.
# [build]
# in = "src"
# out = "build"
# include = ["**"]
# exclude = ["**/*.spec.luaux"]
# clean = true             # delete outputs whose source is gone

# Output is line-for-line with the source so stack traces and luau-lsp
# diagnostics point at the right .luaux line, and comments are always carried
# through. Run stylua over the output if you want it formatted; luaux does not
# reformat your Luau.

# Rename elements. An override retires the original spelling, so once TextLabel
# is renamed, <TextLabel> is an error and <text> is the name.
# [elements]
# TextLabel = "text"

# Rename properties for every class.
# [properties]
# TextColor3 = "textColor"

# Rename properties for one class. Beats the table above, so mapping a name to
# itself opts that class out of a global rename.
# [properties.Frame]
# BackgroundColor3 = "bgColor"

# off | warn | error. Warns when a child expression contains LuauX that no function
# encloses, since it is built once and will not update. Defaults off under the
# element backend, where the component body re-runs and a bare conditional child
# is ordinary.
# [lints]
# static_conditional_child = "warn"
"#;

/// The commented `[factory]` block, for an `init` with no `--library`.
///
/// The blocks side by side rather than one: which keys matter depends
/// entirely on which library you are pointing at, and a single commented
/// example would only be right for one of them.
const FACTORY: &str = r#"# How luaux lowers an element. Every name here has to be in scope in each
# .luaux file, which is why luaux never emits a require: a require is
# location-dependent, and your own import always works.
#
# `luaux init --library react|vide|fluid|fusion` writes one of these for you.
#
# React — F(class, props, children):
# [factory]
# backend = "element"
# create = "React.createElement"
# event = "React.Event."
# fragment = "React.Fragment"
# interpolate = "plain"
#
# Vide — F(class)(propsAndChildren). Fluid is the same, with fluid.create:
# [factory]
# backend = "table"
# create = "create"
#
# Fusion — the same arrangement, with the contents moved:
# [factory]
# backend = "table"
# create = "scope:New"
# children = "Children"
# event = "OnEvent"
# compute = "scope:Computed"

"#;

pub fn run(directory: &Path, library: Option<&str>) -> Result<(), String> {
    let path = directory.join("luaux.toml");

    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }

    let (block, import) = match library {
        None => (FACTORY.to_string(), None),
        Some(name) => {
            let library = LIBRARIES
                .iter()
                .find(|library| library.name == name)
                .ok_or_else(|| {
                    format!(
                        "unknown library `{name}` — try one of {}",
                        LIBRARIES
                            .iter()
                            .map(|library| library.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;

            (format!("{}\n", library.block), Some(library.import))
        }
    };

    let contents = format!("{TEMPLATE}\n{block}");
    std::fs::write(&path, contents).map_err(|error| format!("{}: {error}", path.display()))?;

    println!("luaux: wrote {}", path.display());

    match import {
        Some(import) => println!(
            "luaux: `{import}` must be in scope in each .luaux file — import it as you normally would"
        ),
        None => println!("luaux: set [factory] for your UI library, or rerun with --library"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scaffolded block has to be a config luaux itself accepts. Shipping
    /// a template that fails to parse would turn `init` into the first thing a
    /// new project has to debug.
    #[test]
    fn every_library_block_parses() {
        for library in LIBRARIES {
            luaux::Config::parse(library.block)
                .unwrap_or_else(|error| panic!("{}: {error}", library.name));
        }
    }

    /// The commented blocks are documentation, and documentation that drifts
    /// from the code is worse than none. Uncommenting each one has to still
    /// produce a config that parses.
    #[test]
    fn every_commented_block_parses_once_uncommented() {
        let mut current = String::new();

        for line in FACTORY.lines() {
            let line = line.trim_start_matches('#').trim_start();

            if line.starts_with("[factory]") {
                if !current.is_empty() {
                    luaux::Config::parse(&current).unwrap_or_else(|e| panic!("{current}\n{e}"));
                }
                current = String::from("[factory]\n");
                continue;
            }

            if !current.is_empty() && line.contains('=') {
                current.push_str(line);
                current.push('\n');
            }
        }

        assert!(!current.is_empty(), "no blocks found");
        luaux::Config::parse(&current).unwrap_or_else(|e| panic!("{current}\n{e}"));
    }

    #[test]
    fn the_template_parses() {
        luaux::Config::parse(&format!("{TEMPLATE}\n{FACTORY}")).expect("template");
    }

    #[test]
    fn an_unknown_library_lists_the_known_ones() {
        let error = run(Path::new("."), Some("roact")).expect_err("should fail");
        assert!(error.contains("react, vide, fluid, fusion"), "{error}");
    }
}
