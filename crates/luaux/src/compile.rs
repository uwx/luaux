//! Compiling a `.luaux` source to `.luau`.
//!
//! The transform is local (PLAN.md §5.1): LuauX regions are replaced and every
//! other byte passes through untouched. Expressions captured inside LuauX are
//! themselves compiled, so `{cond and <X/> or nil}` works at any depth.

use crate::backend::{Backend, EmitContext, EmitError};
use crate::config::{Config, LintLevel};
use crate::lexer::{LexError, Lexer};
use crate::lint;
use crate::markup::{self, Attribute, AttributeValue, Child, Element, Fragment, Node};
use crate::markup_scan::Scanner;
use crate::resolve::{blank_luaux_regions, Resolver};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
    pub offset: usize,
    /// Length of the offending text, for underlining. Zero means "point here".
    pub length: usize,
    /// Suggestion shown separately from the message.
    pub help: Option<String>,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for CompileError {}

impl From<LexError> for CompileError {
    fn from(error: LexError) -> Self {
        Self {
            message: error.message,
            offset: error.offset,
            length: 0,
            help: None,
        }
    }
}

impl From<markup::ParseError> for CompileError {
    fn from(error: markup::ParseError) -> Self {
        // An empty hole is usually a deletion accident; a value-less one in an
        // attribute is someone trying to comment where a value is required.
        let help = if error.message.contains("is empty") {
            Some("remove it, or put an expression in it".to_string())
        } else if error.message.contains("contains no value") {
            Some("a comment only stands in for a child, not a prop".to_string())
        } else {
            None
        };

        Self {
            message: error.message,
            offset: error.offset,
            length: 0,
            help,
        }
    }
}

impl From<EmitError> for CompileError {
    fn from(error: EmitError) -> Self {
        Self {
            message: error.message,
            offset: error.offset,
            length: error.length,
            help: error.help,
        }
    }
}

/// A non-fatal diagnostic. Same shape as an error so the CLI renders both the
/// same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub offset: usize,
    pub length: usize,
    pub help: Option<String>,
}

pub fn compile(source: &str, backend: &dyn Backend) -> Result<String, CompileError> {
    Ok(compile_configured(source, backend, Config::default())?.0)
}

/// A compile that produced output, and everything it has to say about it.
///
/// `errors` is not a contradiction: a resolution error — an unknown element, an
/// attribute a class does not have — is **recovered** from, so the output is
/// complete and still Luau while being wrong at those positions. That split is
/// what lets a language server report every mistake at once and still hand the
/// generated file to a type checker, instead of one mistake costing the whole
/// file's analysis.
///
/// A caller that writes the output to disk must treat a non-empty `errors` as
/// failure — [`compile_configured`] does exactly that.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub output: String,
    pub warnings: Vec<Warning>,
    /// Resolution errors, in source order. Recovered from, never guessed at.
    pub errors: Vec<CompileError>,
}

/// Compiles with project aliases, recovering from resolution errors.
///
/// Parse errors still stop the compile and come back as `Err`: recovering from
/// `<Frame` with no `>` means deciding what the author meant, and a wrong guess
/// buries the real mistake under invented ones. Resolution errors need no such
/// guess — the tree is already built and each name is checked on its own — so
/// they are collected and the file carries on.
pub fn compile_recovering(
    source: &str,
    backend: &dyn Backend,
    config: Config,
) -> Result<Compiled, CompileError> {
    compile_inner(source, backend, config)
}

/// Compiles with project aliases from `luaux.toml` (PLAN.md §8).
///
/// Fails on the first resolution error, so output that reaches disk is never
/// knowingly wrong. Tooling that wants every error at once — and the output
/// alongside them — wants [`compile_recovering`].
pub fn compile_configured(
    source: &str,
    backend: &dyn Backend,
    config: Config,
) -> Result<(String, Vec<Warning>), CompileError> {
    let compiled = compile_inner(source, backend, config)?;

    match compiled.errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok((compiled.output, compiled.warnings)),
    }
}

fn compile_inner(
    source: &str,
    backend: &dyn Backend,
    config: Config,
) -> Result<Compiled, CompileError> {
    // Name resolution needs the whole file, so it runs once up front: blank the
    // LuauX regions (`.luaux` is not parseable as Luau), collect every binding,
    // and thread the result through — including into nested expressions, which
    // are compiled separately but must still see the file's bindings.
    let spans = luaux_spans(source)?;
    let blanked = blank_luaux_regions(source, &spans);
    let level = config.static_conditional_child;
    let imports = config.clone();
    let resolver = Resolver::new(&blanked, config);

    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut helpers = crate::backend::Helpers::default();
    let output = compile_with(
        source,
        backend,
        &resolver,
        level,
        &mut warnings,
        &mut errors,
        &mut helpers,
    )?;

    // Only the outermost call injects: nested expressions are compiled on their
    // own but are spliced back into this file, so their helper usage belongs to
    // this preamble.
    let output = crate::imports::inject(&output, helpers, resolver.bound(), &imports)?;

    // Source order, because a list of diagnostics is read top to bottom and
    // nested regions are compiled out of order.
    errors.sort_by_key(|error| error.offset);

    Ok(Compiled {
        output,
        warnings,
        errors,
    })
}

#[doc(hidden)]
pub fn luaux_spans_for_test(source: &str) -> Vec<(usize, usize)> {
    luaux_spans(source).unwrap_or_default()
}

/// Byte ranges of the outermost LuauX regions. Nested LuauX lies inside these, so
/// blanking the outer ranges is enough to make the file parseable.
fn luaux_spans(source: &str) -> Result<Vec<(usize, usize)>, CompileError> {
    let mut lexer = Lexer::new(source);
    let mut scanner = Scanner::new(source);
    let mut spans = Vec::new();

    while let Some(token) = lexer.next_token() {
        let token = token?;
        let lookahead = Lexer::at(source, token.end);

        if !scanner.feed(token, &lookahead) {
            continue;
        }

        let (_, end) = markup::parse_node(source, token.start)?;
        spans.push((token.start, end));

        lexer.seek(end);
        scanner.note_luaux_region();
    }

    Ok(spans)
}

#[allow(clippy::too_many_arguments)]
fn compile_with(
    source: &str,
    backend: &dyn Backend,
    resolver: &Resolver,
    level: LintLevel,
    warnings: &mut Vec<Warning>,
    errors: &mut Vec<CompileError>,
    helpers: &mut crate::backend::Helpers,
) -> Result<String, CompileError> {
    let mut lexer = Lexer::new(source);
    let mut scanner = Scanner::new(source);
    let context = EmitContext::new(source, resolver);

    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;

    // Lexing and LuauX parsing interleave. A `.luaux` file is not Luau end to end
    // — LuauX text is a different lexical mode where `don't`, a backtick, or `--`
    // has no Luau meaning — so the lexer runs only until a LuauX region opens,
    // then resumes past it.
    loop {
        let token = match lexer.next_token() {
            None => break,
            Some(token) => token?,
        };

        let lookahead = Lexer::at(source, token.end);

        if !scanner.feed(token, &lookahead) {
            continue;
        }

        let (mut node, end) = markup::parse_node(source, token.start)?;
        compile_embedded(
            &mut node, backend, resolver, level, warnings, errors, helpers,
        )?;

        out.push_str(&source[cursor..token.start]);

        // Whatever happens next, the errors already recorded are real and come
        // first. Losing them to a later fatal one would report the downstream
        // mistake and hide the upstream one.
        let emitted = backend.emit(&node, &context);
        errors.extend(context.take_errors().into_iter().map(CompileError::from));

        out.push_str(&emitted?);
        cursor = end;

        let used = context.helpers();
        helpers.create |= used.create;
        helpers.read |= used.read;
        helpers.merge_props |= used.merge_props;

        lexer.seek(end);
        scanner.note_luaux_region();
    }

    out.push_str(&source[cursor..]);
    Ok(out)
}

/// Compiles and then re-parses the output as Luau (PLAN.md §5.4, step 8).
///
/// A codegen bug that emits invalid Luau should surface here, not on the user's
/// next rojo sync. One parse turns a confusing downstream failure into a clear
/// internal error.
pub fn compile_verified(
    source: &str,
    backend: &dyn Backend,
    config: &Config,
) -> Result<(String, Vec<Warning>), CompileError> {
    let (output, warnings) = compile_configured(source, backend, config.clone())?;

    if let Err(errors) =
        full_moon::parse_fallible(&output, full_moon::LuaVersion::luau()).into_result()
    {
        let detail = errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; ");

        return Err(CompileError {
            message: format!(
                "internal error: the {} backend emitted invalid Luau — {detail}",
                backend.name()
            ),
            offset: 0,
            length: 0,
            help: Some("this is a luaux bug; please report it".into()),
        });
    }

    Ok((output, warnings))
}

/// Compiles LuauX appearing inside captured Luau expressions.
///
/// Expressions are held verbatim, so nested LuauX is still source text at this
/// point. Recursing here is what makes `{items:map(function() return <X/> end)}`
/// work.
#[allow(clippy::too_many_arguments)]
fn compile_embedded(
    node: &mut Node,
    backend: &dyn Backend,
    resolver: &Resolver,
    level: LintLevel,
    warnings: &mut Vec<Warning>,
    errors: &mut Vec<CompileError>,
    helpers: &mut crate::backend::Helpers,
) -> Result<(), CompileError> {
    match node {
        Node::Element(element) => {
            compile_element(element, backend, resolver, level, warnings, errors, helpers)
        }
        Node::Fragment(fragment) => compile_fragment(
            fragment, backend, resolver, level, warnings, errors, helpers,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_element(
    element: &mut Element,
    backend: &dyn Backend,
    resolver: &Resolver,
    level: LintLevel,
    warnings: &mut Vec<Warning>,
    errors: &mut Vec<CompileError>,
    helpers: &mut crate::backend::Helpers,
) -> Result<(), CompileError> {
    for attribute in &mut element.attributes {
        match attribute {
            Attribute::Spread { expression, .. } => {
                *expression = compile_with(
                    expression, backend, resolver, level, warnings, errors, helpers,
                )?;
            }
            Attribute::Named { value, .. } => {
                if let AttributeValue::Expression(expression) = value {
                    *expression = compile_with(
                        expression, backend, resolver, level, warnings, errors, helpers,
                    )?;
                }
            }
        }
    }

    compile_children(
        &mut element.children,
        backend,
        resolver,
        level,
        warnings,
        errors,
        helpers,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_fragment(
    fragment: &mut Fragment,
    backend: &dyn Backend,
    resolver: &Resolver,
    level: LintLevel,
    warnings: &mut Vec<Warning>,
    errors: &mut Vec<CompileError>,
    helpers: &mut crate::backend::Helpers,
) -> Result<(), CompileError> {
    // Text in an element becomes its `Text` property. A fragment is a plain
    // table with no element to carry it, and Vide's numeric slots take an
    // Instance, a table or a function — never a string. So the text has nowhere
    // to go, and the backend used to drop it without a word.
    if let Some(Child::Text { text, span }) = fragment
        .children
        .iter()
        .find(|child| matches!(child, Child::Text { .. }))
    {
        return Err(CompileError {
            message: "a fragment cannot hold text".to_string(),
            offset: span.start,
            length: span.end.saturating_sub(span.start),
            help: Some(format!(
                "a fragment is a plain table, so there is no element for the text \
                 to belong to; wrap it in one, as <TextLabel>{text}</TextLabel>"
            )),
        });
    }

    compile_children(
        &mut fragment.children,
        backend,
        resolver,
        level,
        warnings,
        errors,
        helpers,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_children(
    children: &mut [Child],
    backend: &dyn Backend,
    resolver: &Resolver,
    level: LintLevel,
    warnings: &mut Vec<Warning>,
    errors: &mut Vec<CompileError>,
    helpers: &mut crate::backend::Helpers,
) -> Result<(), CompileError> {
    for child in children {
        match child {
            Child::Node(node) => {
                compile_embedded(node, backend, resolver, level, warnings, errors, helpers)?
            }
            Child::Expression { expression, span } => {
                // §11.1 runs on the original text, where LuauX is still `<...>`.
                if level != LintLevel::Off {
                    let spans = luaux_spans(expression).unwrap_or_default();

                    if lint::has_unwrapped_luaux(expression, &spans) {
                        let warning = lint::static_conditional_child(span.start, expression.len());

                        if level == LintLevel::Error {
                            return Err(CompileError {
                                message: warning.message,
                                offset: warning.offset,
                                length: warning.length,
                                help: warning.help,
                            });
                        }

                        warnings.push(warning);
                    }
                }

                *expression = compile_with(
                    expression, backend, resolver, level, warnings, errors, helpers,
                )?;
                // Rule 2 — checked after compiling, so nested LuauX has already
                // become ordinary Luau and the expression parses.
                lint::check_conditional_child(expression, span.start)?;
            }
            Child::Text { .. } | Child::Comment { .. } => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Vide;

    /// Fixtures do not import Vide, so the factory-in-scope check would fire.
    /// That check is covered directly by `imports::tests`.
    fn test_config() -> Config {
        Config::with_create("create")
    }

    /// Bound on line 1 so the factory-in-scope check passes without changing
    /// any fixture's line count.
    const BINDING: &str = "local create = _G.create; ";

    fn try_build(source: &str) -> Result<String, CompileError> {
        compile_configured(&format!("{BINDING}{source}"), &Vide, test_config())
            .map(|(output, _)| output)
    }

    fn build(source: &str) -> String {
        strip_preamble(&try_build(source).expect("compile"))
    }

    /// Removes injected helper `require`s so a test can assert on the code it
    /// is actually about. Covered directly by `imports::tests`.
    fn strip_preamble(output: &str) -> String {
        let mut rest = output;

        while let Some(end) = rest.find("; ") {
            let head = &rest[..end];

            let injected = head.starts_with("local function __luaux_")
                || head == BINDING.trim_end_matches("; ");

            if !injected {
                break;
            }

            rest = &rest[end + 2..];
        }

        rest.to_string()
    }

    /// Message and help joined, so assertions can check either half.
    fn build_err(source: &str) -> String {
        let error = try_build(source).expect_err("should fail");
        match error.help {
            Some(help) => format!("{} — {help}", error.message),
            None => error.message,
        }
    }

    #[test]
    fn passes_through_sources_without_luaux() {
        let source = "local x = 1\nreturn x < 2\n";
        assert_eq!(build(source), source);
    }

    #[test]
    fn preserves_surrounding_source_exactly() {
        assert_eq!(
            build("local a = 1\nlocal e = <Frame/>\nreturn a"),
            "local a = 1\nlocal e = create(\"Frame\")({})\nreturn a"
        );
    }

    #[test]
    fn emits_attributes() {
        assert_eq!(
            build(r#"local e = <Frame Name='a' Size={UDim2.new(1, 0)} Visible />"#),
            "local e = create(\"Frame\")({ Name = 'a', Size = UDim2.new(1, 0), Visible = true })"
        );
    }

    #[test]
    fn emits_nested_children() {
        assert_eq!(
            build("local e = <Frame><TextLabel/></Frame>"),
            "local e = create(\"Frame\")({ create(\"TextLabel\")({}) })"
        );
    }

    #[test]
    fn emits_text_as_the_text_property() {
        assert_eq!(
            build("local e = <TextLabel>Hello</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"Hello\" })"
        );
    }

    #[test]
    fn interpolated_text_is_reactive() {
        // The headline case: `count` is a Vide source, so the text has to be a
        // thunk that reads it, not a string that stringifies the function.
        assert_eq!(
            build("local e = <TextLabel>Clicked {count} times</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = function() return `Clicked {__luaux_read(count)} times` end })"
        );
        assert_eq!(
            build("local e = <TextLabel>Name: {name}</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = function() return `Name: {__luaux_read(name)}` end })"
        );
    }

    #[test]
    fn a_lone_expression_needs_no_thunk() {
        // Vide already treats a function on a property key as a source, so
        // `Text = label` is right whether label is a source or a plain string.
        assert_eq!(
            build("local e = <TextButton>{label}</TextButton>"),
            "local e = create(\"TextButton\")({ Text = label })"
        );
    }

    #[test]
    fn literal_only_text_stays_a_plain_string() {
        assert_eq!(
            build("local e = <TextLabel>Hello</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"Hello\" })"
        );
    }

    #[test]
    fn text_children_override_a_text_attribute() {
        // PROPOSAL.md Rule 5.
        assert_eq!(
            build(r#"local e = <TextLabel Text="A">B</TextLabel>"#),
            "local e = create(\"TextLabel\")({ Text = \"B\" })"
        );
    }

    #[test]
    fn escapes_braces_and_backticks_in_text() {
        // PROPOSAL.md Examples 10 and 13.
        assert_eq!(
            build(r"local e = <TextLabel>literal \{text}</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"literal {text}\" })"
        );
        assert_eq!(
            build(r"local e = <TextLabel>slash \\{text}</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = function() return `slash \\\\{__luaux_read(text)}` end })"
        );
        assert_eq!(
            build("local e = <TextLabel>a ` b</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"a ` b\" })"
        );
    }

    #[test]
    fn expression_children_are_emitted_verbatim() {
        // Deliberately *not* wrapped in a one-element table: Vide iterates
        // children with generalised `for k, v in t`, which skips an absent key
        // rather than stopping at it, so a nil child cannot truncate its
        // siblings. Verified against real Vide in tests/runtime.
        assert_eq!(
            build("local e = <Frame>{cond and child or nil}</Frame>"),
            "local e = create(\"Frame\")({ cond and child or nil })"
        );
    }

    #[test]
    fn a_fragment_rejects_text_rather_than_dropping_it() {
        // A fragment has no element to carry a `Text` property, so text in one
        // has nowhere to go. It used to vanish silently.
        for source in [
            "local a = <>hello</>",
            "local a = <>Count: {n} items</>",
            "local a = <Frame><>text</></Frame>",
        ] {
            let error = build_err(source);
            assert!(
                error.contains("fragment cannot hold text"),
                "{source}: {error}"
            );
        }

        // An expression child is fine — a table slot can hold one.
        assert!(build("local a = <>{n}</>").contains("{ n }"));
    }

    #[test]
    fn emits_fragments_as_plain_tables() {
        // Vide recurses tables in numeric position, so a fragment needs no
        // runtime representation of its own.
        assert_eq!(
            build("local e = (<><Frame/><TextLabel/></>)"),
            "local e = ({ create(\"Frame\")({}), create(\"TextLabel\")({}) })"
        );
    }

    #[test]
    fn handles_luaux_text_the_luau_lexer_could_not() {
        // Apostrophes, backticks and `--` are meaningless in LuauX text but would
        // each derail a whole-file Luau tokenizer.
        assert_eq!(
            build("local e = <TextLabel>don't</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"don't\" })"
        );
        assert_eq!(
            build("local e = <TextLabel>a -- b</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"a -- b\" })"
        );
        assert_eq!(
            build("local e = <TextLabel>[[x]]</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"[[x]]\" })"
        );
    }

    #[test]
    fn resumes_lexing_correctly_after_a_region() {
        // The scanner treats a consumed region as an expression, so the `<`
        // that follows is a comparison, not a second region.
        assert_eq!(
            build("local ok = count < 2\nlocal e = <Frame/>\nlocal also = count < 3"),
            "local ok = count < 2\nlocal e = create(\"Frame\")({})\nlocal also = count < 3"
        );
    }

    #[test]
    fn bound_names_are_components() {
        assert_eq!(
            build("local Receipt = require('./Receipt')\nlocal e = <Receipt Name={n} />"),
            "local Receipt = require('./Receipt')\nlocal e = Receipt({ Name = n })"
        );
        // Dotted names never need a binding — no Roblox class contains a dot.
        assert_eq!(build("local e = <Foo.Bar/>"), "local e = Foo.Bar({})");
    }

    fn build_with(source: &str, config: &str) -> String {
        strip_preamble(
            &compile_configured(
                &format!("{BINDING}{source}"),
                &Vide,
                crate::Config::parse(config).expect("config"),
            )
            .expect("compile")
            .0,
        )
    }

    /// Warnings raised while compiling with the default config.
    fn warnings_for(source: &str) -> Vec<Warning> {
        compile_configured(&format!("{BINDING}{source}"), &Vide, test_config())
            .expect("compile")
            .1
    }

    fn build_with_err(source: &str, config: &str) -> String {
        let error = compile_configured(
            &format!("{BINDING}{source}"),
            &Vide,
            crate::Config::parse(config).expect("config"),
        )
        .expect_err("should fail");
        match error.help {
            Some(help) => format!("{} — {help}", error.message),
            None => error.message,
        }
    }

    #[test]
    fn text_children_are_double_quoted_and_escape_double_quotes() {
        // An apostrophe is common in UI copy and now needs no escape.
        assert_eq!(
            build("local e = <TextLabel>don't</TextLabel>"),
            "local e = create(\"TextLabel\")({ Text = \"don't\" })"
        );

        // A double quote does, and the result must still parse.
        let out = compile_verified(
            &format!("{BINDING}local e = <TextLabel>say \"hi\"</TextLabel>"),
            &Vide,
            &crate::Config::default(),
        );
        let out = out.expect("valid Luau").0;
        assert!(out.contains(r#"Text = "say \"hi\"""#), "{out}");
    }

    #[test]
    fn element_aliases_resolve_to_canonical_classes() {
        // The attribute keeps its single quotes: a string literal is the
        // author's own Luau, re-emitted byte for byte. Only strings luaux
        // *builds* — text children — are double-quoted.
        assert_eq!(
            build_with(
                "local e = <text Text='hi'/>",
                "[elements]\nTextLabel = \"text\"\n"
            ),
            "local e = create(\"TextLabel\")({ Text = 'hi' })"
        );
    }

    #[test]
    fn property_aliases_emit_canonical_names() {
        // Emitted code is identical regardless of the project's aliases.
        assert_eq!(
            build_with(
                "local e = <Frame bgColor={c}/>",
                "[properties.Frame]\nBackgroundColor3 = \"bgColor\"\n"
            ),
            "local e = create(\"Frame\")({ BackgroundColor3 = c })"
        );
    }

    #[test]
    fn an_alias_retires_the_original_spelling() {
        let message = build_with_err(
            "local e = <TextLabel/>",
            "[elements]\nTextLabel = \"text\"\n",
        );
        assert!(message.contains("use <text>"), "{message}");

        let message = build_with_err(
            "local e = <Frame BackgroundColor3={c}/>",
            "[properties.Frame]\nBackgroundColor3 = \"bgColor\"\n",
        );
        assert!(message.contains("use bgColor"), "{message}");
    }

    #[test]
    fn unknown_attributes_are_rejected() {
        // PROPOSAL.md's own examples carried this bug: a Frame has
        // BackgroundColor3, not Color3.
        let message = build_err("local e = <Frame Color3={c}/>");
        assert!(
            message.contains("no property or event named Color3"),
            "{message}"
        );
        assert!(message.contains("did you mean"), "{message}");

        // Inherited properties and events are both accepted.
        assert!(try_build("local e = <Frame BackgroundColor3={c}/>").is_ok());
        assert!(try_build("local e = <Frame Name='a'/>").is_ok());
        assert!(try_build("local e = <TextButton Activated={f}/>").is_ok());

        // Components take arbitrary props, so nothing is checked there.
        assert!(try_build("local Row = f()\nlocal e = <Row Whatever={1}/>").is_ok());
    }

    #[test]
    fn read_only_properties_are_rejected() {
        // ContentText exists on TextLabel but cannot be assigned.
        let message = build_err("local e = <TextLabel ContentText='x'/>");
        assert!(
            message.contains("no property or event named ContentText"),
            "{message}"
        );
    }

    #[test]
    fn unbound_names_are_rejected() {
        // The correctness gap this closes: without resolution, `<Frmae/>`
        // compiled to a call to an undefined global and only failed at runtime.
        let message = build_err("local e = <Frmae/>");
        assert!(message.contains("did you mean <Frame>"), "{message}");

        let message = build_err("local e = <Receipt/>");
        assert!(message.contains("not defined"), "{message}");
    }

    #[test]
    fn resolution_sees_bindings_from_the_whole_file() {
        // A component used inside a nested expression still resolves, even
        // though that expression is compiled on its own.
        assert!(try_build(
            "local Row = require('./Row')\nlocal e = <Frame>{cond and <Row/> or nil}</Frame>"
        )
        .is_ok());

        // And one declared *after* its use.
        assert!(try_build("local e = <Frame>{Row}</Frame>\nlocal function Row() end").is_ok());
    }

    #[test]
    fn compiles_luaux_nested_inside_expressions() {
        assert_eq!(
            build("local e = <Frame>{cond and <TextLabel/> or nil}</Frame>"),
            "local e = create(\"Frame\")({ cond and create(\"TextLabel\")({}) or nil })"
        );
    }

    #[test]
    fn compiles_luaux_inside_attribute_expressions() {
        assert_eq!(
            build("local e = <Frame Size={f(<TextLabel/>)} />"),
            "local e = create(\"Frame\")({ Size = f(create(\"TextLabel\")({})) })"
        );
    }

    #[test]
    fn emits_merge_props_for_spreads() {
        assert_eq!(
            build("local e = <Frame {props} Name={n} />"),
            "local e = create(\"Frame\")(__luaux_merge(props, { Name = n }))"
        );
    }

    #[test]
    fn compiles_multiple_sites() {
        assert_eq!(
            build("local a = <Frame/>\nlocal b = <TextLabel/>"),
            "local a = create(\"Frame\")({})\nlocal b = create(\"TextLabel\")({})"
        );
    }

    #[test]
    fn rejects_text_on_a_class_without_a_text_property() {
        assert!(build_err("local e = <Frame>hello</Frame>").contains("no Text property"));
    }

    /// Every resolution error at once, and output alongside them.
    ///
    /// Stopping at the first costs the author every other diagnostic *and* the
    /// generated Luau, so nothing downstream — a type checker, a language
    /// server — can say anything about the file either. One mistake should cost
    /// one diagnostic.
    fn recovered(source: &str) -> Compiled {
        compile_recovering(&format!("{BINDING}{source}"), &Vide, test_config())
            .expect("a resolution error is not fatal")
    }

    #[test]
    fn every_unknown_element_is_reported_not_just_the_first() {
        let compiled = recovered("local e = <Frmae><Recieve/><Buton/></Frmae>");

        let messages: Vec<&str> = compiled
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect();
        assert_eq!(messages.len(), 3, "{messages:#?}");

        assert!(messages[0].contains("Frmae"), "{messages:#?}");
        assert!(messages[1].contains("Recieve"), "{messages:#?}");
        assert!(messages[2].contains("Buton"), "{messages:#?}");
    }

    #[test]
    fn errors_are_reported_in_source_order() {
        let compiled = recovered("local e = <Frame><Buton/><Aaa/></Frame>");
        let offsets: Vec<usize> = compiled.errors.iter().map(|error| error.offset).collect();

        assert_eq!(offsets.len(), 2);
        assert!(offsets[0] < offsets[1], "{offsets:?}");
    }

    #[test]
    fn every_unknown_attribute_is_reported() {
        let compiled = recovered("local e = <Frame Nonsense={1} Rubbish={2}/>");
        assert_eq!(compiled.errors.len(), 2, "{:#?}", compiled.errors);
    }

    /// The output is still Luau, so whatever reads it next keeps working. That
    /// is the whole point of recovering rather than collecting.
    #[test]
    fn a_file_with_a_bad_tag_still_produces_luau() {
        let compiled = recovered("local e = <Frmae Size={1}/>\nlocal after = 1");

        assert!(compiled.output.contains("Frmae"), "{}", compiled.output);
        assert!(compiled.output.contains("after"), "{}", compiled.output);
        full_moon::parse_fallible(&compiled.output, full_moon::LuaVersion::luau())
            .into_result()
            .expect("recovered output is still Luau");
    }

    /// An unknown tag has an unknown class, so its attributes cannot be checked
    /// against anything — reporting them would be one mistake reported four
    /// times.
    #[test]
    fn attributes_of_an_unknown_element_do_not_cascade() {
        let compiled = recovered("local e = <Frmae Text='a' Size={1} Visible/>");
        assert_eq!(compiled.errors.len(), 1, "{:#?}", compiled.errors);
    }

    #[test]
    fn text_children_of_an_unknown_element_do_not_cascade() {
        let compiled = recovered("local e = <Frmae>hello</Frmae>");
        assert_eq!(compiled.errors.len(), 1, "{:#?}", compiled.errors);
    }

    /// Recovery is for resolution, not for parsing. Guessing what an unclosed
    /// tag meant buries the real mistake under invented ones.
    #[test]
    fn a_parse_error_is_still_fatal() {
        assert!(
            compile_recovering(&format!("{BINDING}local e = <Frame"), &Vide, test_config())
                .is_err()
        );
    }

    /// Output that reaches disk is never knowingly wrong, so the entry point the
    /// CLI uses still fails on the first error.
    #[test]
    fn the_writing_entry_point_still_fails() {
        assert!(try_build("local e = <Frmae/>").is_err());
    }

    #[test]
    fn rejects_bare_text_on_a_component() {
        assert!(build_err("local e = <Receipt>hello</Receipt>").contains("component"));
    }

    #[test]
    fn helpers_are_inlined_not_required() {
        // luaux emits no `require` at all: helpers are inlined and the factory
        // is the author's own import.
        let out = try_build("local e = <Frame>{--[[c]]}</Frame>").expect("compile");
        assert!(!out.contains("require"), "{out}");
    }

    #[test]
    fn inlines_the_merge_helper_only_when_a_spread_is_used() {
        let plain = try_build("local e = <Frame/>").expect("compile");
        assert!(!plain.contains("__luaux_merge"), "{plain}");

        let spread = try_build("local p = {}\nlocal e = <Frame {p}/>").expect("compile");
        assert!(spread.contains("local function __luaux_merge"), "{spread}");
    }

    #[test]
    fn inlines_read_only_when_text_interpolates() {
        let plain = try_build("local e = <TextLabel>Hi</TextLabel>").expect("compile");
        assert!(!plain.contains("__luaux_read"), "{plain}");

        let interpolated =
            try_build("local e = <TextLabel>Hi {name}</TextLabel>").expect("compile");
        assert!(
            interpolated.contains("local function __luaux_read"),
            "{interpolated}"
        );
    }

    #[test]
    fn helpers_used_only_inside_nested_expressions_are_still_inlined() {
        let out = try_build(
            "local cond = true\nlocal e = <Frame>{cond and <TextLabel>a {b}</TextLabel> or nil}</Frame>",
        )
        .expect("compile");
        assert!(out.contains("local function __luaux_read"), "{out}");
    }

    #[test]
    fn the_factory_must_be_in_scope() {
        let error = compile_configured("local e = <Frame/>", &Vide, Config::default())
            .expect_err("should fail");
        assert!(
            error.message.contains("`create` is not in scope"),
            "{error:?}"
        );

        // And a dotted factory checks its root.
        let ok = compile_configured(
            "local vide = require('./vide')\nlocal e = <Frame/>",
            &Vide,
            Config::with_create("vide.create"),
        );
        assert!(ok.is_ok(), "{ok:?}");
    }

    #[test]
    fn warns_about_conditional_children_that_never_update() {
        // §11.1 — the LuauX is built once, so the condition looks live but is not.
        let warnings = warnings_for("local e = <Frame>{cond() and <TextLabel/> or nil}</Frame>");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("built once"), "{warnings:?}");

        // Wrapped in a function, Vide tracks it — no warning.
        assert!(warnings_for(
            "local e = <Frame>{function() return cond() and <TextLabel/> or nil end}</Frame>"
        )
        .is_empty());

        // The idiomatic map form must not nag.
        assert!(warnings_for(
            "local e = <Frame>{items:map(function(i) return <TextLabel/> end)}</Frame>"
        )
        .is_empty());

        // No LuauX in the expression, nothing to say.
        assert!(warnings_for("local e = <Frame>{items}</Frame>").is_empty());
    }

    #[test]
    fn the_lint_level_is_configurable() {
        let source = "local e = <Frame>{cond() and <TextLabel/> or nil}</Frame>";

        let off = compile_configured(
            &format!("{BINDING}{source}"),
            &Vide,
            crate::Config::parse("[lints]\nstatic_conditional_child = \"off\"\n").expect("config"),
        )
        .expect("compile");
        assert!(off.1.is_empty());

        let escalated = compile_configured(
            &format!("{BINDING}{source}"),
            &Vide,
            crate::Config::parse("[lints]\nstatic_conditional_child = \"error\"\n")
                .expect("config"),
        );
        assert!(escalated.is_err());
    }

    #[test]
    fn comments_are_dropped_by_default_and_kept_on_request() {
        let source = "local e = <Frame><!-- why -->{--[[ how ]]}<TextLabel/></Frame>";

        // Default: no trace of them.
        // Emitted as Luau block comments, and — crucially — without a comma,
        // since a comment is not a table field.
        let kept = build(source);
        assert!(kept.contains("--[[ why ]]"), "{kept}");
        assert!(kept.contains("--[[ how ]]"), "{kept}");
        assert!(
            !kept.contains("]],"),
            "a comment must not take a comma: {kept}"
        );
    }

    #[test]
    fn a_multi_line_markup_comment_keeps_its_shape() {
        let source = format!(
            "{BINDING}local e = (\n  <Frame>\n    <!--\n      first line\n      second line\n    -->\n    <TextLabel>x</TextLabel>\n  </Frame>\n)"
        );
        let out = build(&source);

        // The opening bracket must not drag the first line up with it.
        assert!(out.contains("--[[\n      first line"), "{out}");
        assert!(out.contains("second line\n    ]]"), "{out}");

        // And the emission still spans exactly the lines the source did.
        assert_eq!(out.lines().count(), source.lines().count(), "{out}");
    }

    #[test]
    fn a_comment_ending_in_a_bracket_does_not_close_early() {
        // `]` meeting the closing `]]` would form `]]]` and terminate a byte
        // early, leaving a stray bracket as code.
        let out = compile_verified(
            &format!("{BINDING}local e = <Frame><!-- see list[1] --></Frame>"),
            &Vide,
            &crate::Config::default(),
        );
        assert!(out.is_ok(), "{out:?}");

        let nested = compile_verified(
            &format!("{BINDING}local e = <Frame><!-- a ]] b --></Frame>"),
            &Vide,
            &crate::Config::default(),
        );
        assert!(nested.is_ok(), "{nested:?}");
        assert!(
            nested.unwrap().0.contains("--[=["),
            "bracket level must rise"
        );
    }

    #[test]
    fn a_kept_comment_beside_no_value_still_emits_valid_luau() {
        // `{ --[[c]] }` — a table holding only a comment.
        let out = compile_verified(
            &format!("{BINDING}local e = <Frame><!-- only --></Frame>"),
            &Vide,
            &crate::Config::default(),
        );
        assert!(out.is_ok(), "{out:?}");
    }

    #[test]
    fn rejects_nil_on_the_left_of_and() {
        // PROPOSAL.md Rule 2. `cond and nil or x` is always `x` in Lua, so the
        // condition silently does nothing.
        let message = build_err("local e = <Frame>{cond and nil or child}</Frame>");
        assert!(message.contains("no effect"), "{message}");
        assert!(message.contains("if cond then"), "{message}");

        // The faithful shape is fine — an element is always truthy.
        assert!(try_build("local e = <Frame>{cond and child or nil}</Frame>").is_ok());
        assert!(try_build("local e = <Frame>{if cond then a else b}</Frame>").is_ok());
    }

    #[test]
    fn rejects_ambiguous_expression_beside_element_children() {
        // On a Text-bearing class, `{label}` could be the text or a child, and
        // guessing emits code that fails inside Vide at runtime.
        let message = build_err("local e = <TextButton>{label}<UICorner/></TextButton>");
        assert!(message.contains("unclear"), "unexpected: {message}");
        assert!(message.contains("Text={...}"), "unexpected: {message}");

        // Unambiguous either side of it.
        assert!(try_build("local e = <TextButton>Click<UICorner/></TextButton>").is_ok());
        assert!(try_build("local e = <TextButton>{label}</TextButton>").is_ok());
        assert!(try_build("local e = <Frame>{child}<TextLabel/></Frame>").is_ok());
    }

    /// Multi-line fixtures, shared by the line-preservation and validity tests.
    const MULTILINE_FIXTURES: &[&str] = &[
        "local e = (\n  <Frame\n    Size={size}\n    Visible\n  >\n    <TextLabel>Hi</TextLabel>\n  </Frame>\n)\n",
        "local e = (\n  <Frame>\n    <TextLabel>\n      Clicked {count} times\n    </TextLabel>\n    <UICorner/>\n  </Frame>\n)\n",
        "local e = (\n  <>\n    <Frame/>\n    <TextLabel/>\n  </>\n)\n",
        "local Button = f()\nlocal e = (\n  <Button\n    OnClick={function()\n      count(count() + 1)\n    end}\n  />\n)\n",
        "local e = (\n  <Frame>\n    {cond and (\n      <TextLabel/>\n    ) or nil}\n  </Frame>\n)\n",
        "local e = (\n  <Frame\n    {props}\n    Name={n}\n  />\n)\n",
    ];

    /// The generated `.luau` must have the same number of lines as the `.luaux`
    /// it came from (PLAN.md §5.5). Luau has no runtime source maps, so matching
    /// line numbers are the only thing making a stack trace or a luau-lsp
    /// diagnostic point at the right place.
    #[test]
    fn output_preserves_line_count() {
        for fixture in MULTILINE_FIXTURES {
            let compiled = build(fixture);
            assert_eq!(
                compiled.lines().count(),
                fixture.lines().count(),
                "line count changed\n--- in ---\n{fixture}\n--- out ---\n{compiled}"
            );
        }
    }

    #[test]
    fn entries_land_on_their_source_lines() {
        let source = "local e = (\n  <Frame\n    Size={size}\n    Visible\n  >\n    <TextLabel>Hi</TextLabel>\n  </Frame>\n)\n";
        let compiled = build(source);
        let lines: Vec<&str> = compiled.lines().collect();

        assert!(lines[1].contains("create(\"Frame\")"), "{compiled}");
        assert!(lines[2].contains("Size = size"), "{compiled}");
        assert!(lines[3].contains("Visible = true"), "{compiled}");
        assert!(lines[5].contains("create(\"TextLabel\")"), "{compiled}");
    }

    /// Every compiled fixture must re-parse as Luau. This is the guard that
    /// stops a codegen bug reaching the user's rojo sync.
    #[test]
    fn generated_output_is_valid_luau() {
        let fixtures = [
            "local e = <Frame/>",
            r#"local e = <Frame Name='a' Size={UDim2.new(1, 0)} Visible />"#,
            "local e = <Frame><TextLabel>Hi</TextLabel></Frame>",
            "local e = <TextLabel>Name: {name}</TextLabel>",
            "local e = (<><Frame/><TextLabel/></>)",
            "local e = <Frame>{cond and <TextLabel/> or nil}</Frame>",
            "local e = <Frame {props} Name={n} />",
            "local Receipt = f()\nlocal e = <Receipt Name={n}><TextLabel>Debit</TextLabel></Receipt>",
            "local e = <TextLabel>don't</TextLabel>",
            r"local e = <TextLabel>slash \\{text}</TextLabel>",
            "return function(props) return <Frame>{props.children}</Frame> end",
        ];

        // full_moon's recursive-descent parser has large stack frames in debug
        // builds — enough to exhaust a test thread's 2 MB on the inlined merge
        // helper, though release and the CLI's main thread are both fine. Give
        // it room rather than shrinking the fixtures to suit the harness.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                for fixture in fixtures.iter().chain(MULTILINE_FIXTURES.iter()) {
                    let bound = format!("{BINDING}{fixture}");
                    compile_verified(&bound, &Vide, &test_config())
                        .unwrap_or_else(|error| panic!("{fixture}\n  -> {error}"));
                }
            })
            .expect("spawn")
            .join()
            .expect("verification thread");
    }
}
