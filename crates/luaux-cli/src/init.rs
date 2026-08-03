//! `luaux init` — scaffold a `luaux.toml`.
//!
//! Everything in the generated file is commented out, because luaux works
//! without any of it. The point is to show what *can* be set and what the
//! defaults are, not to impose settings nobody asked for.

use std::path::Path;

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

# Expression called to construct an element — TypeScript's jsxFactory. It has to
# be in scope in each file, which is why luaux never emits a require: a require
# is location-dependent, and your own import always works.
# [factory]
# create = "vide.create"   # default: "create"

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
# encloses, since it is built once and will not update.
# [lints]
# static_conditional_child = "warn"
"#;

pub fn run(directory: &Path) -> Result<(), String> {
    let path = directory.join("luaux.toml");

    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }

    std::fs::write(&path, TEMPLATE).map_err(|error| format!("{}: {error}", path.display()))?;

    println!("luaux: wrote {}", path.display());
    println!(
        "luaux: `create` must be in scope in each .luaux file — import Vide as you normally would"
    );

    Ok(())
}
