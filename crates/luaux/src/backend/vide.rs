//! Vide backend (PLAN.md §6).
//!
//! Vide's `create` takes one flat table where string keys are properties and
//! events and numeric keys are children, so LuauX maps onto it almost 1:1. Vide
//! handles parenting, fragment recursion, event connection, ordering, and
//! reactive updates — leaving the compiler only the compile-time work.
//!
//! Output is line-preserving: each entry lands on the source line its attribute
//! or child was written on, and the whole emission spans exactly the lines the
//! LuauX did (PLAN.md §5.5).

use super::writer::Writer;
use super::{Backend, EmitContext, EmitError};
use crate::markup::*;
use crate::resolve::Resolution;
use crate::roblox;

// The element factory is configurable (`[factory] create`) and reaches the backend
// through EmitContext, so there is no constant for it here.

/// Merge helper for spread attributes. Inlined into the output rather than
/// required, so luaux has no runtime dependency (see `crate::imports`).
const MERGE_PROPS: &str = crate::imports::MERGE_HELPER;

/// Reads a possibly-source value. Inlined rather than taken from Vide, so
/// interpolated text costs no dependency (see `crate::imports`).
const READ: &str = crate::imports::READ_HELPER;

pub struct Vide;

impl Backend for Vide {
    fn name(&self) -> &'static str {
        "vide"
    }

    fn emit(&self, node: &Node, context: &EmitContext<'_>) -> Result<String, EmitError> {
        let mut writer = Writer::new(context, node.span().start);
        emit_node(node, context, &mut writer)?;
        Ok(writer.finish())
    }
}

/// One entry in an emitted table.
enum Entry<'a> {
    /// `Name = value`
    Pair { offset: usize, text: String },
    /// A nested element or fragment.
    Node { offset: usize, node: &'a Node },
    /// An expression child, emitted verbatim.
    Expression { offset: usize, expression: &'a str },
    /// A retained comment, already Luau. Carries no value, so it needs its own
    /// separator handling — a comment cannot sit between two commas.
    Comment { offset: usize, luau: &'a str },
}

impl Entry<'_> {
    fn offset(&self) -> usize {
        match self {
            Entry::Pair { offset, .. }
            | Entry::Node { offset, .. }
            | Entry::Expression { offset, .. }
            | Entry::Comment { offset, .. } => *offset,
        }
    }
}

fn emit_node(
    node: &Node,
    context: &EmitContext<'_>,
    writer: &mut Writer<'_>,
) -> Result<(), EmitError> {
    match node {
        Node::Element(element) => emit_element(element, context, writer),
        Node::Fragment(fragment) => {
            // A fragment is a plain table; Vide recurses tables in numeric
            // position, so it needs no runtime representation of its own.
            let entries = child_entries(&fragment.children, false);
            emit_table(&entries, fragment.span, context, writer)
        }
    }
}

fn emit_element(
    element: &Element,
    context: &EmitContext<'_>,
    writer: &mut Writer<'_>,
) -> Result<(), EmitError> {
    // Resolution rejects a name that is neither a Roblox class nor bound in the
    // file, so a typo like `<Frmae/>` fails here with a did-you-mean instead of
    // compiling into a call to an undefined global (PLAN.md §3.1).
    // An unresolved name is emitted as written and its attributes are left
    // alone: with no class there is nothing to check them against, and checking
    // anyway would report one error per attribute, all of them caused by the
    // single mistake already reported on the tag.
    let (intrinsic, resolved) = match context.resolve(&element.name, element.span.start) {
        Resolution::Intrinsic(class) => (Some(class), true),
        Resolution::Component => (None, true),
        Resolution::Unresolved(written) => (Some(written), false),
    };

    // Nothing else about an unresolved element is worth checking. Its text
    // children and its attributes are judged against a class that does not
    // exist, so every complaint would be downstream of the one already recorded
    // on the tag itself.
    let plan = match resolved {
        true => plan_text(element, intrinsic.as_deref(), context)?,
        false => TextPlan::default(),
    };

    match &intrinsic {
        Some(class) => {
            context.used_create();
            writer.push(&format!("{}(\"{class}\")(", context.create()));
        }
        None => writer.push(&format!("{}(", element.name.as_written())),
    }

    // Attributes split into groups at each spread: a run of named attributes
    // becomes one table, each spread contributes its own argument, and
    // `mergeProps` joins them in source order.
    let mut groups: Vec<Vec<Entry>> = vec![Vec::new()];
    let mut spreads: Vec<(usize, &str)> = Vec::new();
    let mut order: Vec<Group> = Vec::new();

    for attribute in &element.attributes {
        match attribute {
            Attribute::Spread { expression, span } => {
                if !groups.last().expect("a group").is_empty() {
                    order.push(Group::Table(groups.len() - 1));
                    groups.push(Vec::new());
                }
                order.push(Group::Spread(spreads.len()));
                spreads.push((span.start, expression));
            }
            Attribute::Named { name, value, span } => {
                // Rule 5: text between the tags overrides a `Text` attribute.
                if plan.text.is_some() && name == "Text" {
                    continue;
                }

                // Aliases resolve to canonical Roblox names here, so emitted
                // code is the same regardless of a project's luaux.toml.
                let key = match (&intrinsic, resolved) {
                    (Some(class), true) => context.resolve_attribute(class, name, span.start),
                    _ => name.clone(),
                };

                groups.last_mut().expect("a group").push(Entry::Pair {
                    offset: span.start,
                    text: format!("{key} = {}", attribute_value(value)),
                });
            }
        }
    }

    let last = groups.len() - 1;
    if let Some(text) = &plan.text {
        groups[last].push(Entry::Pair {
            offset: plan.offset,
            text: format!("Text = {text}"),
        });
    }
    groups[last].extend(child_entries(&element.children, plan.consumed_expressions));

    if !groups[last].is_empty() || order.is_empty() {
        order.push(Group::Table(last));
    }

    let uses_merge = !spreads.is_empty();
    if uses_merge {
        context.used_merge_props();
        writer.push(&format!("{MERGE_PROPS}("));
    }

    for (index, group) in order.iter().enumerate() {
        if index > 0 {
            writer.push(",");
            match group {
                Group::Spread(spread) => writer.break_or_space(spreads[*spread].0),
                Group::Table(table) => {
                    let offset = groups[*table]
                        .first()
                        .map(Entry::offset)
                        .unwrap_or(element.span.start);
                    writer.break_or_space(offset);
                }
            }
        }

        match group {
            Group::Spread(spread) => writer.push(spreads[*spread].1),
            Group::Table(table) => emit_table(&groups[*table], element.span, context, writer)?,
        }
    }

    if uses_merge {
        writer.push(")");
    }

    writer.push(")");
    Ok(())
}

enum Group {
    Table(usize),
    Spread(usize),
}

fn emit_table(
    entries: &[Entry<'_>],
    span: Span,
    context: &EmitContext<'_>,
    writer: &mut Writer<'_>,
) -> Result<(), EmitError> {
    if entries.is_empty() {
        writer.push("{}");
        return Ok(());
    }

    writer.push("{");

    // A comment is not a table field, so it never takes a comma — and the comma
    // has to be written *immediately after its value*, before any comment that
    // follows. Deferring it until the next entry puts it after the comment,
    // which is valid Lua but reads as though the comment owned it, and no
    // formatter will move it back.
    let last_value = entries
        .iter()
        .rposition(|entry| !matches!(entry, Entry::Comment { .. }));

    for (index, entry) in entries.iter().enumerate() {
        if let Entry::Comment { offset, luau } = entry {
            writer.break_or_space(*offset);
            writer.push(luau);
            continue;
        }

        writer.break_or_space(entry.offset());

        match entry {
            Entry::Comment { .. } => unreachable!("handled above"),
            Entry::Pair { text, .. } => writer.push(text),
            Entry::Node { node, .. } => emit_node(node, context, writer)?,
            // Emitted bare. An earlier design wrapped these in a one-element
            // table to stop a nil leaving a hole in the array part — but Vide
            // iterates children with generalised `for k, v in t`, which skips
            // absent keys rather than stopping at them. `ipairs` would truncate;
            // Vide does not use it. Verified by tests/runtime.
            Entry::Expression { expression, .. } => writer.push(expression),
        }

        if Some(index) != last_value {
            writer.push(",");
        }
    }

    // The closing brace sits on the line of the closing tag, so the emission
    // spans exactly the lines the LuauX did.
    let close = span.end.saturating_sub(1);

    if writer.will_break(close) {
        // Trailing comma goes before the break, per Luau style — but only if a
        // value was written, since a comment cannot carry one.
        if last_value.is_some() {
            writer.push(",");
        }
        writer.to(close);
    } else {
        writer.push(" ");
    }

    writer.push("}");
    Ok(())
}

fn attribute_value(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Expression(expression) => expression.clone(),
        AttributeValue::StringLiteral(literal) => literal.clone(),
        AttributeValue::Boolean => "true".to_string(),
    }
}

fn child_entries<'a>(children: &'a [Child], expressions_are_text: bool) -> Vec<Entry<'a>> {
    let mut entries = Vec::new();

    for child in children {
        match child {
            Child::Node(node) => entries.push(Entry::Node {
                offset: node.span().start,
                node,
            }),
            // Text is always folded into the `Text` property by plan_text.
            Child::Text { .. } => {}
            Child::Comment { luau, span } => entries.push(Entry::Comment {
                offset: span.start,
                luau,
            }),
            Child::Expression { .. } if expressions_are_text => {}
            Child::Expression { expression, span } => entries.push(Entry::Expression {
                offset: span.start,
                expression,
            }),
        }
    }

    entries
}

/// How an element's children divide between the `Text` property and Vide's
/// numeric child slots.
#[derive(Default)]
struct TextPlan {
    /// Encoded Luau value for the `Text` property, if any.
    text: Option<String>,
    /// Whether expression children were folded into `text` rather than left as
    /// children.
    consumed_expressions: bool,
    /// Source offset of the first text part, so `Text = …` lands on its line.
    offset: usize,
}

/// Text and expression children that lower to the `Text` property (PLAN.md §6.2).
///
/// Vide's numeric slots take Instances, tables, and functions — not strings — so
/// bare text has to become a property at compile time.
fn plan_text(
    element: &Element,
    intrinsic: Option<&str>,
    context: &EmitContext<'_>,
) -> Result<TextPlan, EmitError> {
    let has_text_literal = element
        .children
        .iter()
        .any(|child| matches!(child, Child::Text { .. }));
    let has_nodes = element
        .children
        .iter()
        .any(|child| matches!(child, Child::Node(_)));
    let has_expressions = element
        .children
        .iter()
        .any(|child| matches!(child, Child::Expression { .. }));

    if !has_text_literal && !has_expressions {
        return Ok(TextPlan::default());
    }

    let Some(class) = intrinsic else {
        // Components take children, not text. Nothing here knows their props.
        if has_text_literal {
            return Err(EmitError::new(
                format!(
                    "<{}> is a component, so it cannot take bare text",
                    element.name.as_written()
                ),
                element.span.start,
                element.name.as_written().len() + 1,
            )
            .with_help("pass the text as a prop instead"));
        }
        return Ok(TextPlan::default());
    };

    if !roblox::has_text_property(class) {
        if has_text_literal {
            return Err(EmitError::new(
                format!("<{class}> has no Text property"),
                element.span.start,
                class.len() + 1,
            )
            .with_help("wrap the text in a <TextLabel>"));
        }
        // Expressions are ordinary children on a class with no text.
        return Ok(TextPlan::default());
    }

    // `<TextButton>{label}<UICorner/></TextButton>` is genuinely ambiguous: the
    // expression could be the button's text or another child, and nothing here
    // can tell. Emitting a guess produces code that fails inside Vide at
    // runtime, so refuse and ask for the explicit form.
    if has_expressions && has_nodes {
        return Err(EmitError::new(
            format!(
                "<{class}> has both an expression child and element children, so it is unclear \
                 whether the expression is text or a child"
            ),
            element.span.start,
            class.len() + 1,
        )
        .with_help("write it as Text={...} instead"));
    }

    let mut parts = Vec::new();
    let mut offset = element.span.start;

    for (index, child) in element.children.iter().enumerate() {
        match child {
            Child::Text { text, span } => {
                if index == 0 || parts.is_empty() {
                    offset = span.start;
                }
                parts.push(TextPart::Literal(text.clone()));
            }
            Child::Expression { expression, span } => {
                if parts.is_empty() {
                    offset = span.start;
                }
                parts.push(TextPart::Expression(expression.clone()));
            }
            Child::Node(_) | Child::Comment { .. } => {}
        }
    }

    let text = encode_text(&parts);
    if text.contains(&format!("{READ}(")) {
        context.used_read();
    }

    Ok(TextPlan {
        text: Some(text),
        consumed_expressions: has_expressions,
        offset,
    })
}

enum TextPart {
    Literal(String),
    Expression(String),
}

/// Encodes text parts as the value of the `Text` property.
///
/// Three shapes, and the choice is what makes `<TextLabel>Clicked {count}
/// times</TextLabel>` reactive:
///
/// * **Literals only** — a plain quoted string. Nothing to track.
/// * **A single expression** — emitted bare. Vide already treats a function
///   value on a property key as a source and creates an effect, so `Text = label`
///   is correct whether `label` is a source or a plain string.
/// * **Mixed** — a thunk: `function() return `…{read(x)}…` end`. Interpolation
///   builds a string, which would stringify a source to `function: 0x…`, so each
///   expression is read and the whole thing is wrapped so Vide re-runs it when
///   any source inside changes.
fn encode_text(parts: &[TextPart]) -> String {
    let expressions = parts
        .iter()
        .filter(|part| matches!(part, TextPart::Expression(_)))
        .count();

    if expressions == 0 {
        return encode_plain(parts);
    }

    if let [TextPart::Expression(expression)] = parts {
        return expression.clone();
    }

    format!("function() return {} end", encode_interpolated(parts))
}

/// Double quotes, matching Luau convention and stylua's default. Attribute
/// literals are captured verbatim and keep whatever the author wrote; this
/// governs only strings luaux itself builds, which is text children.
fn encode_plain(parts: &[TextPart]) -> String {
    let mut out = String::from("\"");

    for part in parts {
        if let TextPart::Literal(text) = part {
            for character in text.chars() {
                match character {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    _ => out.push(character),
                }
            }
        }
    }

    out.push('"');
    out
}

fn encode_interpolated(parts: &[TextPart]) -> String {
    let mut out = String::from("`");

    for part in parts {
        match part {
            TextPart::Literal(text) => {
                for character in text.chars() {
                    match character {
                        '\\' => out.push_str("\\\\"),
                        '`' => out.push_str("\\`"),
                        '{' => out.push_str("\\{"),
                        '}' => out.push_str("\\}"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        _ => out.push(character),
                    }
                }
            }
            TextPart::Expression(expression) => {
                out.push_str(&format!("{{{READ}({expression})}}"));
            }
        }
    }

    out.push('`');
    out
}
