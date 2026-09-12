//! The curried-components backend (ADR-0004) — `F(class)(propsAndChildren)`,
//! where a user component curries through the factory the same as an
//! intrinsic does.
//!
//! Identical to [`super::table::Table`] in every respect but one: under
//! `Table`, a component is called directly (`Component(props)`) because the
//! library is assumed to treat components and intrinsics differently. Here,
//! both go through the factory (`create(Component)(props)`,
//! `create("Frame")(props)`), differing only in whether the first argument is
//! quoted — the same distinction `Element` (backend-plan.md) draws for its own
//! three-argument arrangement, but kept on the curried, single-table shape.
//!
//! Output is line-preserving on the same terms as every other backend
//! (PLAN.md §5.5).

use super::common::{child_entries, emit_table, plan_text, Entry, Props, TextPlan};
use super::writer::Writer;
use super::{Backend, EmitContext, EmitError};
use crate::markup::*;
use crate::resolve::Resolution;

// The element factory is configurable (`[factory] create`) and reaches the
// backend through EmitContext, so there is no constant for it here.

pub struct Curried;

impl Backend for Curried {
    fn name(&self) -> &'static str {
        "curried"
    }

    fn emit(&self, node: &Node, context: &EmitContext<'_>) -> Result<String, EmitError> {
        let mut writer = Writer::new(context, node.span().start);
        emit_node(node, context, &mut writer)?;
        Ok(writer.finish())
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
            // A fragment is a plain table; the recursing libraries this
            // arrangement targets walk tables positionally, so a fragment
            // needs no runtime representation of its own.
            let entries = child_entries(&fragment.children, false);
            emit_table(
                &entries,
                Some(fragment.span.end.saturating_sub(1)),
                context,
                writer,
                emit_node,
            )
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

    context.used_create();
    match &intrinsic {
        Some(class) => writer.push(&format!("{}(\"{class}\")(", context.create())),
        // Unlike `Table`, a component curries through the factory too — this
        // backend's whole reason to exist.
        None => writer.push(&format!(
            "{}({})(",
            context.create(),
            element.name.as_written()
        )),
    }

    let mut props = Props::build(element, &plan, intrinsic.as_deref(), resolved, context);
    let children = child_entries(&element.children, plan.consumed_expressions);

    match context.children() {
        // Children are the array part of the props table.
        None => props.extend_last(children),
        // Children go under a key, and the entry is omitted entirely when there
        // are none, so `<Frame/>` stays `F("Frame")({})` rather than gaining an
        // empty `[Children] = {}` (factory-plan.md §3.1).
        //
        // Applies to components as well as intrinsics: the two are
        // interchangeable at the call site, and splitting them here would
        // break that over a guess about what a component expects.
        Some(key) => {
            if !children.is_empty() {
                context.used_children();
                props.push_last(Entry::Children {
                    offset: element.span.start,
                    key: key.to_string(),
                    entries: children,
                });
            }
        }
    }

    props.emit(
        element,
        Some(element.span.end.saturating_sub(1)),
        context,
        writer,
        emit_node,
    )?;

    // The closing parenthesis sits on the line of the closing tag, so the
    // element spans exactly the lines the LuauX did.
    //
    // `emit_table` already does this for the table it emits, which covers most
    // elements — but an element whose attributes are *all* spreads emits no
    // table at all (the trailing group is empty and is dropped above), and then
    // nothing else would. `<Component {props} />` written across lines is the
    // ordinary way to forward props, and it was losing every line it spanned.
    writer.to(element.span.end.saturating_sub(1));

    writer.push(")");
    Ok(())
}
