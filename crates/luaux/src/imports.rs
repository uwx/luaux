//! Supplying what generated code needs, without emitting a single `require`.
//!
//! Two kinds of name appear in output, and neither is a module reference:
//!
//! * **luaux's own helpers** — merging spread props, and reading a value that
//!   may be a source. Both are a line long and are **inlined** into files that
//!   use them. They are implementation details of a syntax feature, not
//!   something an author asked for, so making them install a package would be
//!   backwards.
//! * **The element factory** — whatever `[factory] create` names, defaulting to
//!   bare `create`. luaux only checks it is in scope; the author imports Vide in
//!   whatever style their project uses.
//!
//! That split is why luaux does not resolve module paths. A require is
//! *location-dependent* — `script.Parent.Packages.vide` means something
//! different from a file one directory deeper, and a relative path likewise — so
//! a single configured string cannot be correct for every file. An author's own
//! import always is. Resolving requires across a codebase is darklua's job, and
//! luaux composes with it rather than duplicating it.
//!
//! Injection is **line-preserving** (PLAN.md §5.5): helpers go on the same line
//! as the first statement. That is uglier than a clean preamble and it is the
//! right trade — a stack trace pointing at the wrong line costs more than tidy
//! generated output.

use crate::backend::Helpers;
use crate::compile::CompileError;
use crate::config::Config;
use crate::lexer::Lexer;
use std::collections::HashSet;

/// Merges prop groups for spread attributes.
///
/// String keys are last-wins so source order decides precedence; numeric keys
/// concatenate so a spread and literal children can coexist.
///
/// The return is annotated `any` deliberately. A merge of heterogeneous tables
/// has no type Luau can express, and left to inference the result comes out as
/// `{*error-type*}` — which makes every spread a type error at its call site:
///
/// ```text
/// TypeError: Expected this to be 'ButtonProps', but got '{*error-type*}'
/// ```
///
/// `any` costs the checking of a spread's *result* against the component's props
/// and buys back the checking of everything else in the file. That is the better
/// side of the trade while the alternative is an error on every use.
pub const MERGE_HELPER: &str = "__luaux_merge";

/// Reads a value that may be a Vide source.
///
/// Interpolated text builds a string, and a source is a function — so it has to
/// be called, while a plain value must pass through untouched.
pub const READ_HELPER: &str = "__luaux_read";

/// Both helpers on one line each, so injection stays line-preserving.
const MERGE_HELPER_SOURCE: &str = "local function __luaux_merge(...): any local m, n = {}, 0 \
for i = 1, select(\"#\", ...) do local g = select(i, ...) if g ~= nil then for k, v in g do \
if type(k) == \"number\" then n += 1 m[n] = v else m[k] = v end end end end return m end";

const READ_HELPER_SOURCE: &str =
    "local function __luaux_read(v) return if type(v) == \"function\" then v() else v end";

/// Prepends the helpers this output uses, and checks the factory is reachable.
pub fn inject(
    output: &str,
    helpers: Helpers,
    bound: &HashSet<String>,
    config: &Config,
) -> Result<String, CompileError> {
    if helpers.create {
        // Only the root of a dotted expression can be a binding: for
        // `vide.create` that is `vide`.
        let root = config
            .create
            .split('.')
            .next()
            .unwrap_or(&config.create)
            .trim();

        if !bound.contains(root) {
            return Err(CompileError {
                message: format!("`{root}` is not in scope"),
                offset: 0,
                length: 0,
                help: Some(format!(
                    "import it, or point [factory] create at something else \
                     (currently `{}`)",
                    config.create
                )),
            });
        }
    }

    let mut statements = Vec::new();

    if helpers.merge_props && !bound.contains(MERGE_HELPER) {
        statements.push(MERGE_HELPER_SOURCE);
    }

    if helpers.read && !bound.contains(READ_HELPER) {
        statements.push(READ_HELPER_SOURCE);
    }

    if statements.is_empty() {
        return Ok(output.to_string());
    }

    let preamble = format!("{}; ", statements.join("; "));

    Ok(match first_statement_offset(output) {
        Some(offset) => format!("{}{preamble}{}", &output[..offset], &output[offset..]),
        // Nothing but comments; there is no code to support anyway.
        None => output.to_string(),
    })
}

/// Offset of the first non-trivia token.
///
/// Injecting here rather than at byte zero keeps Luau's leading directives
/// working — `--!strict` and friends must precede all code, so putting a
/// `local` above them would silently disable strict mode.
fn first_statement_offset(source: &str) -> Option<usize> {
    let mut lexer = Lexer::new(source);

    while let Some(token) = lexer.next_token() {
        let token = token.ok()?;
        if !token.is_trivia() {
            return Some(token.start);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// Left to inference the merged table comes out as `{*error-type*}`, which
    /// makes every spread a type error where its result is used — so a file with
    /// one spread loses the type checking of everything else in it.
    #[test]
    fn the_merge_helper_has_a_usable_return_type() {
        assert!(
            MERGE_HELPER_SOURCE.contains("__luaux_merge(...): any"),
            "{MERGE_HELPER_SOURCE}"
        );
    }

    fn all() -> Helpers {
        Helpers {
            create: true,
            read: true,
            merge_props: true,
        }
    }

    #[test]
    fn inlines_helpers_with_no_config_and_no_dependency() {
        let out = inject(
            "local x = 1",
            all(),
            &bound(&["create"]),
            &Config::default(),
        )
        .expect("inject");

        assert!(out.contains("local function __luaux_merge"), "{out}");
        assert!(out.contains("local function __luaux_read"), "{out}");
        assert!(!out.contains("require"), "no dependency: {out}");
    }

    #[test]
    fn inlines_only_what_is_used() {
        let helpers = Helpers {
            read: true,
            ..Default::default()
        };
        let out = inject("local x = 1", helpers, &bound(&[]), &Config::default()).expect("inject");

        assert!(out.contains("__luaux_read"), "{out}");
        assert!(!out.contains("__luaux_merge"), "{out}");
    }

    #[test]
    fn respects_a_helper_the_author_already_defined() {
        let out = inject(
            "local x = 1",
            all(),
            &bound(&["create", MERGE_HELPER]),
            &Config::default(),
        )
        .expect("inject");
        assert!(!out.contains("local function __luaux_merge"), "{out}");
    }

    #[test]
    fn requires_the_factory_to_be_in_scope() {
        let helpers = Helpers {
            create: true,
            ..Default::default()
        };
        let error = inject("local x = 1", helpers, &bound(&[]), &Config::default())
            .expect_err("should fail");

        assert!(
            error.message.contains("`create` is not in scope"),
            "{error:?}"
        );
    }

    #[test]
    fn checks_only_the_root_of_a_dotted_factory() {
        let helpers = Helpers {
            create: true,
            ..Default::default()
        };
        let config = Config::with_create("vide.create");

        // `vide` is the binding; `create` is a field on it.
        assert!(inject("local x = 1", helpers, &bound(&["vide"]), &config).is_ok());

        let error =
            inject("local x = 1", helpers, &bound(&["create"]), &config).expect_err("should fail");
        assert!(
            error.message.contains("`vide` is not in scope"),
            "{error:?}"
        );
    }

    #[test]
    fn injects_nothing_when_no_helper_is_used() {
        let source = "local x = 1";
        let out =
            inject(source, Helpers::default(), &bound(&[]), &Config::default()).expect("inject");
        assert_eq!(out, source);
    }

    #[test]
    fn preserves_the_line_count() {
        let source = "--!strict\nlocal x = 1\nreturn x";
        let out = inject(source, all(), &bound(&["create"]), &Config::default()).expect("inject");
        assert_eq!(out.lines().count(), source.lines().count(), "{out}");
    }

    #[test]
    fn goes_after_leading_directives_and_comments() {
        // `--!strict` must stay first, or strict mode silently turns off.
        let source = "--!strict\n-- a note\n\nlocal x = 1";
        let out = inject(source, all(), &bound(&["create"]), &Config::default()).expect("inject");

        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "--!strict");
        assert_eq!(lines[1], "-- a note");
        assert!(lines[3].ends_with("local x = 1"), "{out}");
    }

    #[test]
    fn leaves_a_comment_only_file_alone() {
        let source = "-- nothing here\n";
        let out = inject(source, all(), &bound(&["create"]), &Config::default()).expect("inject");
        assert_eq!(out, source);
    }
}
