//! `luaux.toml` — element and property aliases (PLAN.md §8).
//!
//! ```toml
//! [elements]              # overrides for default (Roblox) element names
//! TextLabel = "text"
//!
//! [properties]            # applies to all elements
//! TextColor3 = "textColor"
//!
//! [properties.Frame]      # per-class; beats the global table
//! BackgroundColor3 = "bgColor"
//! ```
//!
//! Overrides are **exclusive**: renaming a name retires the original, so once
//! `TextLabel = "text"` is declared, `<TextLabel>` is an error and `<text>` is
//! the spelling. That is what makes a project's vocabulary consistent rather
//! than offering two ways to write everything.
//!
//! A per-class entry can map a name to itself — `[properties.TextLabel]
//! TextColor3 = "TextColor3"` — to opt one class back out of a global rename.

use crate::roblox;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    elements: HashMap<String, String>,
    #[serde(default)]
    properties: HashMap<String, PropertyEntry>,
    #[serde(default)]
    lints: RawLints,
    #[serde(default)]
    factory: RawFactory,

    #[serde(default)]
    build: RawBuild,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBuild {
    #[serde(rename = "in")]
    input: Option<String>,
    #[serde(rename = "out")]
    output: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    clean: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFactory {
    create: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLints {
    static_conditional_child: Option<String>,
}

/// A blanket renaming scheme, set with `all` in `[elements]` or `[properties]`.
///
/// `Pascal` is the identity: Roblox's own spelling, and the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Casing {
    #[default]
    Pascal,
    Camel,
    Snake,
    Flat,
}

impl Casing {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "PascalCase" => Some(Self::Pascal),
            "camelCase" => Some(Self::Camel),
            "snake_case" => Some(Self::Snake),
            "flatcase" => Some(Self::Flat),
            _ => None,
        }
    }

    /// Rewrites a Roblox name into this scheme.
    ///
    /// Word boundaries are taken from the canonical PascalCase spelling, so
    /// `UICorner` splits as `UI` + `Corner` rather than per capital, giving
    /// `ui_corner` instead of `u_i_corner`.
    pub fn apply(&self, name: &str) -> String {
        if *self == Casing::Pascal {
            return name.to_string();
        }

        if *self == Casing::Flat {
            return name.to_lowercase();
        }

        let mut words: Vec<String> = Vec::new();
        let chars: Vec<char> = name.chars().collect();
        let mut start = 0;

        for index in 1..chars.len() {
            let previous = chars[index - 1];
            let current = chars[index];
            let next = chars.get(index + 1).copied();

            // lower→upper ends a word: `textLabel` -> `text` `Label`.
            // upper→upper→lower ends one too: `UICorner` -> `UI` `Corner`.
            let boundary = (previous.is_lowercase() || previous.is_numeric())
                && current.is_uppercase()
                || previous.is_uppercase()
                    && current.is_uppercase()
                    && next.is_some_and(char::is_lowercase);

            if boundary {
                words.push(chars[start..index].iter().collect());
                start = index;
            }
        }

        words.push(chars[start..].iter().collect());

        match self {
            Casing::Snake => words
                .iter()
                .map(|word| word.to_lowercase())
                .collect::<Vec<_>>()
                .join("_"),
            Casing::Camel => words
                .iter()
                .enumerate()
                .map(|(index, word)| {
                    if index == 0 {
                        word.to_lowercase()
                    } else {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => {
                                first.to_uppercase().collect::<String>()
                                    + &chars.as_str().to_lowercase()
                            }
                            None => String::new(),
                        }
                    }
                })
                .collect(),
            Casing::Pascal | Casing::Flat => unreachable!("handled above"),
        }
    }
}

/// Picks the canonical name when several collapse onto one under a casing.
///
/// Roblox ships deprecated spellings beside modern ones (`brickColor` beside
/// `BrickColor`), and they differ only by case. Prefer whatever is not
/// deprecated; if that leaves no single winner — `FormFactor`/`formFactor` are
/// both deprecated — fall back to byte order, which puts the uppercase form
/// first. See docs/adr/0001-casing-key.md.
pub fn preferred<'a>(candidates: &mut Vec<&'a str>) -> Option<&'a str> {
    if candidates.len() > 1 && candidates.iter().any(|name| !roblox::is_deprecated(name)) {
        candidates.retain(|name| !roblox::is_deprecated(name));
    }

    candidates.sort_unstable();
    candidates.first().copied()
}

/// How a lint reports. `Warn` is the default for `static_conditional_child`: the
/// pattern it catches is usually a mistake, but a conditional on a constant is
/// legitimate and would otherwise be unsilenceable (PLAN.md §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LintLevel {
    Off,
    #[default]
    Warn,
    Error,
}

impl LintLevel {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PropertyEntry {
    /// `TextColor3 = "textColor"` — a global rename.
    Alias(String),
    /// `[properties.Frame]` — a per-class table.
    PerClass(HashMap<String, String>),
}

/// Resolved alias tables, keyed for lookup by what the user *writes*.
#[derive(Debug, Clone)]
pub struct Config {
    /// alias → canonical class name
    element_alias: HashMap<String, String>,
    /// canonical class name → alias that replaced it
    element_renamed: HashMap<String, String>,
    /// canonical property → alias, applying to every class
    global_properties: HashMap<String, String>,
    /// class → (canonical property → alias)
    class_properties: HashMap<String, HashMap<String, String>>,
    /// `[elements] all` — a blanket scheme, beaten by any explicit entry.
    element_casing: Casing,
    /// `[properties] all` — likewise, for properties and events.
    property_casing: Casing,
    /// §11.1 — LuauX in a child expression that no function encloses.
    pub static_conditional_child: LintLevel,
    /// Paths and file selection.
    pub build: Build,
    /// Expression called to construct an element — TypeScript's `jsxFactory`.
    ///
    /// Naming, not shape: whatever this points at must still match Vide's
    /// `create(class)(propsAndChildren)` contract. A different *shape* needs a
    /// backend, not a name.
    ///
    /// Trimmed and checked to lower to Luau when it came from
    /// [`Config::parse`]. [`Config::with_create`] does neither, so a caller
    /// building a config by hand owns that.
    pub create: String,
}

/// Bare `create`, matching the common `local create = vide.create`. Projects
/// that keep Vide in one binding set `create = "vide.create"`.
pub const DEFAULT_CREATE: &str = "create";

/// Which files a build considers, and where they go.
#[derive(Debug, Clone)]
pub struct Build {
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    /// Globs a file must match to be considered. A pattern with no `/` matches
    /// at any depth, as in gitignore.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Delete outputs whose source is gone, so a renamed file leaves no stale
    /// twin for rojo to keep syncing.
    pub clean: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            include: vec!["**".to_string()],
            exclude: Vec::new(),
            clean: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            element_alias: HashMap::new(),
            element_renamed: HashMap::new(),
            global_properties: HashMap::new(),
            class_properties: HashMap::new(),
            element_casing: Casing::default(),
            property_casing: Casing::default(),
            static_conditional_child: LintLevel::default(),
            build: Build::default(),
            create: DEFAULT_CREATE.to_string(),
        }
    }
}

impl Config {
    /// The blanket scheme for element names, for tooling that has to render or
    /// complete names in the project's own spelling.
    pub fn element_casing(&self) -> Casing {
        self.element_casing
    }

    /// The blanket scheme for properties and events.
    pub fn property_casing(&self) -> Casing {
        self.property_casing
    }

    /// A config whose only non-default setting is the element factory.
    ///
    /// Takes the factory as given — no trimming, and none of the checking
    /// [`Config::parse`] does. A value that will not lower to Luau still
    /// reaches the backend from here and still comes back as luaux's own
    /// "please report it" internal error, so a caller taking this from user
    /// input wants `parse` instead.
    pub fn with_create(create: impl Into<String>) -> Self {
        Self {
            create: create.into(),
            ..Self::default()
        }
    }

    /// Loads `luaux.toml` from `directory`, or returns an empty config if there
    /// is none. Absent config is not an error; luaux works without one.
    pub fn load(directory: &Path) -> Result<Self, ConfigError> {
        Self::load_reporting(directory).map(|(config, _)| config)
    }

    /// Loads, also returning notes about settings that are accepted but inert.
    pub fn load_reporting(directory: &Path) -> Result<(Self, Vec<String>), ConfigError> {
        let path = directory.join("luaux.toml");

        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), Vec::new()))
            }
            Err(error) => {
                return Err(ConfigError {
                    message: format!("{}: {error}", path.display()),
                })
            }
        };

        Self::parse_reporting(&text)
    }

    /// Parses, discarding warnings. For callers that only need the config.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        Self::parse_reporting(text).map(|(config, _)| config)
    }

    /// Parses, also returning notes about settings that are accepted but inert.
    pub fn parse_reporting(text: &str) -> Result<(Self, Vec<String>), ConfigError> {
        let raw: RawConfig = toml::from_str(text).map_err(|error| ConfigError {
            message: format!("luaux.toml: {}", error.message()),
        })?;

        let mut config = Config::default();

        if let Some(level) = &raw.lints.static_conditional_child {
            config.static_conditional_child =
                LintLevel::parse(level).ok_or_else(|| ConfigError {
                    message: format!(
                        "luaux.toml: [lints] static_conditional_child = \"{level}\" is not one of \
                         off, warn, error"
                    ),
                })?;
        }

        let warnings = Vec::new();

        config.build.input = raw.build.input.map(PathBuf::from);
        config.build.output = raw.build.output.map(PathBuf::from);
        if let Some(include) = raw.build.include {
            config.build.include = include;
        }
        if let Some(exclude) = raw.build.exclude {
            config.build.exclude = exclude;
        }
        config.build.clean = raw.build.clean.unwrap_or(false);

        if let Some(create) = raw.factory.create {
            let create = create.trim();

            if create.is_empty() {
                return Err(ConfigError {
                    message: "luaux.toml: [factory] create cannot be empty".to_string(),
                });
            }

            validate_create(create)?;
            config.create = create.to_string();
        }

        // `all` is reserved in both tables. No Roblox class or member is named
        // `all`, so claiming it costs nothing.
        if let Some(value) = raw.elements.get("all") {
            config.element_casing = Casing::parse(value).ok_or_else(|| ConfigError {
                message: format!(
                    "luaux.toml: [elements] all = \"{value}\" is not one of PascalCase, \
                     camelCase, snake_case, flatcase"
                ),
            })?;
        }

        for (class, alias) in raw.elements.iter().filter(|(key, _)| *key != "all") {
            if !roblox::is_class(class) {
                return Err(ConfigError {
                    message: format!(
                        "luaux.toml: [elements] {class} is not a creatable Roblox class{}",
                        suggest_class(class)
                    ),
                });
            }

            if let Some(existing) = config.element_alias.get(alias) {
                return Err(ConfigError {
                    message: format!(
                        "luaux.toml: [elements] {existing} and {class} both claim the alias \
                         \"{alias}\""
                    ),
                });
            }

            config.element_alias.insert(alias.clone(), class.clone());
            config.element_renamed.insert(class.clone(), alias.clone());
        }

        if let Some(PropertyEntry::Alias(value)) = raw.properties.get("all") {
            config.property_casing = Casing::parse(value).ok_or_else(|| ConfigError {
                message: format!(
                    "luaux.toml: [properties] all = \"{value}\" is not one of PascalCase, \
                     camelCase, snake_case, flatcase"
                ),
            })?;
        }

        for (key, entry) in raw.properties.iter().filter(|(key, _)| *key != "all") {
            match entry {
                PropertyEntry::Alias(alias) => {
                    if !roblox::is_member_name(key) {
                        return Err(ConfigError {
                            message: format!(
                                "luaux.toml: [properties] {key} is not a property or event of any \
                                 Roblox class{}",
                                suggest_member_anywhere(key)
                            ),
                        });
                    }
                    config.global_properties.insert(key.clone(), alias.clone());
                }
                PropertyEntry::PerClass(entries) => {
                    if !roblox::is_class(key) {
                        return Err(ConfigError {
                            message: format!(
                                "luaux.toml: [properties.{key}] is not a creatable Roblox class{}",
                                suggest_class(key)
                            ),
                        });
                    }

                    for property in entries.keys() {
                        if !roblox::has_property(key, property) && !roblox::is_event(key, property)
                        {
                            return Err(ConfigError {
                                message: format!(
                                    "luaux.toml: [properties.{key}] {key} has no property or event \
                                     named {property}{}",
                                    suggest_member(key, property)
                                ),
                            });
                        }
                    }

                    config.class_properties.insert(key.clone(), entries.clone());
                }
            }
        }

        Ok((config, warnings))
    }

    /// Maps a written tag to a canonical class name.
    ///
    /// `Ok(None)` means the tag is not an alias and should be resolved normally.
    pub fn resolve_element(&self, written: &str) -> Result<Option<&str>, String> {
        if let Some(class) = self.element_alias.get(written) {
            return Ok(Some(class));
        }

        // Writing the original name of something that was renamed.
        if let Some(alias) = self.element_renamed.get(written) {
            if alias != written {
                return Err(format!(
                    "<{written}> was renamed by luaux.toml; use <{alias}>"
                ));
            }
        }

        if self.element_casing != Casing::Pascal {
            // An explicit entry already had its chance above, so `all` only
            // ever answers for names it did not claim.
            let mut matches: Vec<&str> = roblox::creatable_classes()
                .filter(|class| !self.element_renamed.contains_key(*class))
                .filter(|class| self.element_casing.apply(class) == written)
                .collect();

            if let Some(class) = preferred(&mut matches) {
                return Ok(Some(class));
            }

            // Under a blanket rename the canonical spelling retires, exactly as
            // a single override retires the name it replaces.
            if roblox::is_class(written) {
                return Err(format!(
                    "<{written}> was renamed by [elements] all; use <{}>",
                    self.element_casing.apply(written)
                ));
            }
        }

        Ok(None)
    }

    /// How `class` is spelled in this project — its alias, if one was configured.
    ///
    /// The inverse of [`Config::resolve_element`], and the question tooling asks:
    /// offering `<TextLabel>` in a project that renamed it to `text` would be
    /// offering the one spelling that is an error, since overrides are exclusive.
    pub fn element_name<'a>(&'a self, class: &'a str) -> &'a str {
        self.element_renamed
            .get(class)
            .map_or(class, String::as_str)
    }

    /// How a canonical property or event is spelled on `class`.
    ///
    /// Per-class entries beat the global table, matching
    /// [`Config::resolve_property`] — including the identity override a class
    /// uses to opt back out of a global rename.
    pub fn property_name<'a>(&'a self, class: &str, canonical: &'a str) -> &'a str {
        if let Some(alias) = self
            .class_properties
            .get(class)
            .and_then(|entries| entries.get(canonical))
        {
            return alias;
        }

        self.global_properties
            .get(canonical)
            .map_or(canonical, String::as_str)
    }

    /// Maps a written attribute on `class` to its canonical property name.
    pub fn resolve_property(&self, class: &str, written: &str) -> Result<String, String> {
        // Per-class entries beat the global table, so a class can opt back out
        // of a global rename by mapping a name to itself.
        let effective: HashMap<&str, &str> = self
            .global_properties
            .iter()
            .map(|(canonical, alias)| (canonical.as_str(), alias.as_str()))
            .chain(
                self.class_properties
                    .get(class)
                    .into_iter()
                    .flatten()
                    .map(|(canonical, alias)| (canonical.as_str(), alias.as_str())),
            )
            .collect();

        for (canonical, alias) in &effective {
            if *alias == written {
                return Ok((*canonical).to_string());
            }
        }

        if let Some(alias) = effective.get(written) {
            if *alias != written {
                return Err(format!("{written} was renamed by luaux.toml; use {alias}"));
            }
        }

        if self.property_casing != Casing::Pascal {
            let mut matches: Vec<&str> = roblox::properties(class)
                .chain(roblox::events(class))
                .filter(|member| !effective.contains_key(*member))
                .filter(|member| self.property_casing.apply(member) == written)
                .collect();

            if let Some(member) = preferred(&mut matches) {
                return Ok(member.to_string());
            }

            if roblox::has_property(class, written) || roblox::is_event(class, written) {
                return Err(format!(
                    "{written} was renamed by [properties] all; use {}",
                    self.property_casing.apply(written)
                ));
            }
        }

        Ok(written.to_string())
    }
}

/// Rejects a `[factory] create` that will not lower to Luau.
///
/// Checked in the shape it is *emitted* into — `create("Frame")({})` — rather
/// than as an expression on its own, because `scope:New` is a legal factory and
/// not a legal expression: Luau requires a `:` call's arguments to follow
/// immediately. Checking the call shape also keeps the curried and indexed
/// forms that already worked — `vide.create()`, `ui.factories[1]` — so this
/// rejects nothing that used to compile.
///
/// Checked here because the alternative is not a wrong error but a misdirected
/// one. A factory that does not lower to Luau reaches the backend, emits, and
/// comes back out of [`crate::compile_verified`] as *"internal error: the vide
/// backend emitted invalid Luau — this is a luaux bug; please report it"*,
/// which sends someone to the issue tracker over a typo in their own config.
///
/// Not everything wrong is catchable here, and it does not need to be.
/// `(vide.create)` lowers perfectly well and is still refused — by the in-scope
/// check in [`crate::imports`], which is the other half of the pair. This asks
/// whether the value lowers to Luau; that asks whether it names something the
/// file can reach.
fn validate_create(create: &str) -> Result<(), ConfigError> {
    let reject = |reason: &str| {
        Err(ConfigError {
            message: format!(
                "luaux.toml: [factory] create = \"{create}\" {reason}; it has to name a \
                 function, as create, vide.create, or scope:New"
            ),
        })
    };

    // `..` is Luau's concatenation operator, so `vide..create` *parses* — as a
    // string, which is not callable. The probe below would wave it through and
    // the typo would survive to runtime, so it is caught by name first.
    if create.contains("..") {
        return reject("has a `.` with no name beside it");
    }

    // Parenthesised so the value has to be *one* expression. Bare, `vide create`
    // parses as two statements — `local _ = vide`, then a call — because Luau
    // needs no separator between them. The probe would pass, and the emitted
    // `local e = vide create("Frame")({})` would split the same way and quietly
    // build nothing.
    let probe = format!("local _ = ({create}(\"Frame\")({{}}))");

    if full_moon::parse_fallible(&probe, full_moon::LuaVersion::luau())
        .into_result()
        .is_err()
    {
        return reject("is not something luaux can call");
    }

    Ok(())
}

fn suggest_class(name: &str) -> String {
    match roblox::closest_class(name) {
        Some(class) => format!("; did you mean {class}?"),
        None => String::new(),
    }
}

fn suggest_member(class: &str, name: &str) -> String {
    match roblox::closest_members(class, name).as_slice() {
        [] => String::new(),
        [one] => format!("; did you mean {one}?"),
        [rest @ .., last] => format!("; did you mean {} or {last}?", rest.join(", ")),
    }
}

fn suggest_member_anywhere(name: &str) -> String {
    match roblox::closest_member_anywhere(name) {
        Some(member) => format!("; did you mean {member}?"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Config {
        Config::parse(text).expect("parse")
    }

    fn parse_err(text: &str) -> String {
        Config::parse(text).expect_err("should fail").message
    }

    #[test]
    fn lints_default_to_warn_and_are_configurable() {
        assert_eq!(parse("").static_conditional_child, LintLevel::Warn);
        assert_eq!(
            parse("[lints]\nstatic_conditional_child = \"off\"\n").static_conditional_child,
            LintLevel::Off
        );
        assert_eq!(
            parse("[lints]\nstatic_conditional_child = \"error\"\n").static_conditional_child,
            LintLevel::Error
        );
    }

    #[test]
    fn rejects_an_unknown_lint_level() {
        let error = parse_err("[lints]\nstatic_conditional_child = \"loud\"\n");
        assert!(error.contains("not one of off, warn, error"), "{error}");
    }

    #[test]
    fn the_element_factory_defaults_to_bare_create() {
        assert_eq!(parse("").create, DEFAULT_CREATE);
        assert_eq!(
            parse("[factory]\ncreate = \"vide.create\"\n").create,
            "vide.create"
        );
    }

    #[test]
    fn rejects_an_empty_factory() {
        assert!(parse_err("[factory]\ncreate = \"\"\n").contains("cannot be empty"));
        assert!(parse_err("[factory]\ncreate = \"   \"\n").contains("cannot be empty"));
    }

    /// Anything that lowers to a call is accepted, not only a dotted name. The
    /// curried and indexed forms compiled before this check existed, so they
    /// have to keep compiling.
    #[test]
    fn accepts_every_factory_that_lowers_to_a_call() {
        for create in [
            "create",
            "vide.create",
            "scope:New",
            "a.b.c",
            "_v1.x2",
            "vide.create()",
            "ui.factories[1]",
        ] {
            let config = parse(&format!("[factory]\ncreate = \"{create}\"\n"));
            assert_eq!(config.create, create);

            // Acceptance has to mean the emitted call parses, not merely that
            // the string survived the config.
            let probe = format!("local _ = ({create}(\"Frame\")({{}}))");
            assert!(
                full_moon::parse_fallible(&probe, full_moon::LuaVersion::luau())
                    .into_result()
                    .is_ok(),
                "{create}"
            );
        }
    }

    /// Each of these used to reach the backend, emit, and come back out of
    /// `compile_verified` as "internal error … this is a luaux bug; please
    /// report it" — sending someone to the issue tracker over their own typo.
    #[test]
    fn rejects_a_factory_that_will_not_lower_to_luau() {
        for (create, reason) in [
            ("vide..create", "with no name beside it"),
            ("vide.", "not something luaux can call"),
            (".create", "not something luaux can call"),
            ("create(", "not something luaux can call"),
            // Two statements, not one expression: `local _ = vide` then a call.
            ("vide create", "not something luaux can call"),
            ("1 + 1", "not something luaux can call"),
            // A colon has to be the last separator: `a:b` must be followed by
            // its arguments, so `a:b.c` has no callable spelling.
            ("scope:New:Now", "not something luaux can call"),
            ("scope:New.Now", "not something luaux can call"),
            ("scope:", "not something luaux can call"),
            // Reserved words are not field names — `t.end` is a syntax error.
            ("end", "not something luaux can call"),
            ("a.end", "not something luaux can call"),
        ] {
            let error = parse_err(&format!("[factory]\ncreate = \"{create}\"\n"));
            assert!(error.contains("[factory] create"), "{create}: {error}");
            // The reason, not just the fixed wrapper `reject` puts around every
            // one — otherwise a single blanket message would satisfy the lot.
            assert!(error.contains(reason), "{create}: {error}");
        }
    }

    /// The half this check deliberately does not cover, recorded so the split
    /// stays visible: `(vide.create)` lowers to perfectly good Luau, and it is
    /// the in-scope check in `imports` that refuses it — for `(vide`, which is
    /// not a binding.
    #[test]
    fn a_factory_that_lowers_but_names_nothing_is_left_to_the_scope_check() {
        assert_eq!(
            parse("[factory]\ncreate = \"(vide.create)\"\n").create,
            "(vide.create)"
        );
    }

    #[test]
    fn a_factory_is_trimmed() {
        // Otherwise the stray bytes reach both the in-scope check and the
        // emitted call.
        assert_eq!(
            parse("[factory]\ncreate = \" vide.create \"\n").create,
            "vide.create"
        );
    }

    #[test]
    fn reads_build_paths_and_selection() {
        let config = parse(
            "[build]\nin = \"src\"\nout = \"build\"\ninclude = [\"**\"]\nexclude = [\"**/*.spec.luaux\"]\nclean = true\n",
        );

        assert_eq!(config.build.input.unwrap().to_str(), Some("src"));
        assert_eq!(config.build.output.unwrap().to_str(), Some("build"));
        assert_eq!(config.build.exclude, ["**/*.spec.luaux"]);
        assert!(config.build.clean);
    }

    #[test]
    fn build_defaults_are_inert() {
        let config = parse("");
        assert!(config.build.input.is_none());
        assert!(config.build.output.is_none());
        assert!(!config.build.clean);
        // Everything is considered unless narrowed.
        assert_eq!(config.build.include, ["**"]);
    }

    #[test]
    fn casing_splits_on_canonical_word_boundaries() {
        // Boundaries come from the PascalCase spelling, so an acronym stays
        // whole: UICorner is UI + Corner, never U + I + Corner.
        for (name, snake, camel, flat) in [
            ("TextLabel", "text_label", "textLabel", "textlabel"),
            ("UICorner", "ui_corner", "uiCorner", "uicorner"),
            (
                "UIAspectRatioConstraint",
                "ui_aspect_ratio_constraint",
                "uiAspectRatioConstraint",
                "uiaspectratioconstraint",
            ),
            (
                "BackgroundColor3",
                "background_color3",
                "backgroundColor3",
                "backgroundcolor3",
            ),
            ("Frame", "frame", "frame", "frame"),
        ] {
            assert_eq!(Casing::Snake.apply(name), snake, "{name}");
            assert_eq!(Casing::Camel.apply(name), camel, "{name}");
            assert_eq!(Casing::Flat.apply(name), flat, "{name}");
            assert_eq!(Casing::Pascal.apply(name), name, "{name}");
        }
    }

    #[test]
    fn casing_is_injective_over_every_class() {
        // If two classes collapsed onto one spelling, `all` would be ambiguous
        // for elements. Measured at zero; this guards against an API dump that
        // introduces one.
        for casing in [Casing::Snake, Casing::Camel, Casing::Flat] {
            let mut seen: HashMap<String, &str> = HashMap::new();
            for class in roblox::creatable_classes() {
                if let Some(other) = seen.insert(casing.apply(class), class) {
                    panic!("{casing:?}: {other} and {class} collide");
                }
            }
        }
    }

    #[test]
    fn an_explicit_entry_beats_the_blanket_scheme() {
        let config = parse("[elements]\nall = \"camelCase\"\nTextLabel = \"text\"\n");

        // The override wins, and takes its class out of the scheme entirely.
        assert_eq!(config.resolve_element("text").unwrap(), Some("TextLabel"));
        assert!(config.resolve_element("textLabel").unwrap().is_none());

        // Everything it did not claim still follows `all`.
        assert_eq!(config.resolve_element("frame").unwrap(), Some("Frame"));
        assert_eq!(
            config.resolve_element("uiCorner").unwrap(),
            Some("UICorner")
        );
    }

    #[test]
    fn a_blanket_scheme_retires_the_canonical_spelling() {
        let config = parse("[elements]\nall = \"snake_case\"\n");
        let error = config.resolve_element("Frame").expect_err("retired");
        assert!(error.contains("use <frame>"), "{error}");
    }

    #[test]
    fn a_collision_prefers_the_name_roblox_has_not_deprecated() {
        // ChildAdded and childAdded are both on Instance, so this pair is
        // inherited by every class (docs/adr/0001-casing-key.md).
        let config = parse("[properties]\nall = \"snake_case\"\n");
        assert_eq!(
            config.resolve_property("Frame", "child_added").unwrap(),
            "ChildAdded"
        );

        let camel = parse("[properties]\nall = \"camelCase\"\n");
        assert_eq!(
            camel.resolve_property("Part", "brickColor").unwrap(),
            "BrickColor"
        );
    }

    #[test]
    fn a_collision_with_no_undeprecated_name_is_still_deterministic() {
        // FormFactor and formFactor are both deprecated, so the tiebreak is byte
        // order, which puts the uppercase spelling first.
        let config = parse("[properties]\nall = \"camelCase\"\n");
        let resolved = config.resolve_property("Part", "formFactor");
        assert_eq!(resolved.unwrap(), "FormFactor");
    }

    #[test]
    fn properties_follow_their_own_scheme_and_overrides() {
        let config = parse("[properties]\nall = \"snake_case\"\nBackgroundColor3 = \"bg\"\n");
        assert_eq!(
            config
                .resolve_property("Frame", "background_transparency")
                .unwrap(),
            "BackgroundTransparency"
        );
        assert_eq!(
            config.resolve_property("Frame", "bg").unwrap(),
            "BackgroundColor3"
        );
        // The override removed it from the scheme.
        assert_eq!(
            config
                .resolve_property("Frame", "background_color3")
                .unwrap(),
            "background_color3"
        );
    }

    #[test]
    fn rejects_an_unknown_casing() {
        assert!(parse_err("[elements]\nall = \"kebab-case\"\n").contains("PascalCase"));
        assert!(parse_err("[properties]\nall = \"KEBAB\"\n").contains("snake_case"));
    }

    #[test]
    fn a_genuinely_unknown_section_still_errors() {
        // So a typo is caught rather than silently ignored.
        assert!(parse_err("[buld]\nclean = true\n").contains("unknown field"));
    }

    #[test]
    fn an_absent_config_is_not_an_error() {
        let config = Config::load(Path::new("/nonexistent-directory-for-luaux"));
        assert!(config.is_ok());
    }

    #[test]
    fn resolves_element_aliases() {
        let config = parse("[elements]\nTextLabel = \"text\"\n");
        assert_eq!(config.resolve_element("text"), Ok(Some("TextLabel")));
        // Unaliased names pass through untouched.
        assert_eq!(config.resolve_element("Frame"), Ok(None));
    }

    #[test]
    fn an_override_retires_the_original_name() {
        let config = parse("[elements]\nTextLabel = \"text\"\n");
        let error = config.resolve_element("TextLabel").expect_err("retired");
        assert!(error.contains("use <text>"), "{error}");
    }

    #[test]
    fn rejects_duplicate_element_aliases() {
        let error = parse_err("[elements]\nTextLabel = \"text\"\nTextButton = \"text\"\n");
        assert!(error.contains("both claim the alias"), "{error}");
    }

    #[test]
    fn rejects_unknown_element_keys() {
        let error = parse_err("[elements]\nFrmae = \"frame\"\n");
        assert!(error.contains("not a creatable Roblox class"), "{error}");
        assert!(error.contains("did you mean Frame?"), "{error}");
    }

    #[test]
    fn resolves_global_property_aliases() {
        let config = parse("[properties]\nTextColor3 = \"textColor\"\n");
        assert_eq!(
            config.resolve_property("TextLabel", "textColor"),
            Ok("TextColor3".into())
        );
        // And retires the original spelling.
        assert!(config.resolve_property("TextLabel", "TextColor3").is_err());
    }

    #[test]
    fn per_class_entries_beat_the_global_table() {
        let config = parse(
            "[properties]\nBackgroundColor3 = \"bg\"\n\n[properties.Frame]\nBackgroundColor3 = \"bgColor\"\n",
        );
        assert_eq!(
            config.resolve_property("Frame", "bgColor"),
            Ok("BackgroundColor3".into())
        );
        // The global alias no longer applies to Frame...
        assert_eq!(config.resolve_property("Frame", "bg"), Ok("bg".into()));
        // ...but still applies elsewhere.
        assert_eq!(
            config.resolve_property("TextLabel", "bg"),
            Ok("BackgroundColor3".into())
        );
    }

    #[test]
    fn a_class_can_opt_out_of_a_global_rename() {
        // PROPOSAL.md's identity-override trick.
        let config = parse(
            "[properties]\nTextColor3 = \"textColor\"\n\n[properties.TextLabel]\nTextColor3 = \"TextColor3\"\n",
        );
        assert_eq!(
            config.resolve_property("TextLabel", "TextColor3"),
            Ok("TextColor3".into())
        );
        assert_eq!(
            config.resolve_property("TextButton", "textColor"),
            Ok("TextColor3".into())
        );
    }

    /// Every spelling offered must be one that resolves, or a project's
    /// completions would suggest the errors its own config created.
    #[test]
    fn the_offered_spelling_is_the_one_that_resolves() {
        let config = parse(
            "[elements]\nTextLabel = \"text\"\n\n[properties]\nTextColor3 = \"textColor\"\n\n\
             [properties.Frame]\nBackgroundColor3 = \"bgColor\"\n",
        );

        assert_eq!(config.element_name("TextLabel"), "text");
        assert_eq!(config.element_name("Frame"), "Frame");
        assert_eq!(
            config.resolve_element(config.element_name("TextLabel")),
            Ok(Some("TextLabel"))
        );

        assert_eq!(config.property_name("TextLabel", "TextColor3"), "textColor");
        assert_eq!(config.property_name("Frame", "BackgroundColor3"), "bgColor");
        // Untouched names are spelled as themselves.
        assert_eq!(config.property_name("Frame", "Name"), "Name");

        for (class, canonical) in [("TextLabel", "TextColor3"), ("Frame", "BackgroundColor3")] {
            assert_eq!(
                config.resolve_property(class, config.property_name(class, canonical)),
                Ok(canonical.to_string()),
                "{class}.{canonical}"
            );
        }
    }

    #[test]
    fn an_identity_override_is_offered_as_the_original_name() {
        let config = parse(
            "[properties]\nTextColor3 = \"textColor\"\n\n[properties.TextLabel]\nTextColor3 = \"TextColor3\"\n",
        );

        assert_eq!(
            config.property_name("TextLabel", "TextColor3"),
            "TextColor3"
        );
        assert_eq!(
            config.property_name("TextButton", "TextColor3"),
            "textColor"
        );
    }

    #[test]
    fn rejects_unknown_property_keys() {
        let error = parse_err("[properties]\nTextColour3 = \"textColor\"\n");
        assert!(error.contains("not a property or event of any"), "{error}");
        assert!(error.contains("TextColor3"), "{error}");
    }

    #[test]
    fn rejects_per_class_keys_the_class_does_not_have() {
        let error = parse_err("[properties.Frame]\nText = \"label\"\n");
        assert!(
            error.contains("has no property or event named Text"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unknown_per_class_tables() {
        let error = parse_err("[properties.Frmae]\nName = \"id\"\n");
        assert!(error.contains("not a creatable Roblox class"), "{error}");
    }
}
