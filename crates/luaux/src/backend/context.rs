//! Source positions available to backends.
//!
//! Backends need line numbers to emit line-preserving output (PLAN.md §5.5):
//! the generated `.luau` has the same line count as the `.luaux` it came from,
//! with each construct on the line its tag was on. Luau has no runtime source
//! map support, so a matching line number is the only thing that makes a stack
//! trace or a luau-lsp diagnostic point at the right place.

use crate::backend::EmitError;
use crate::markup::ElementName;
use crate::resolve::{Resolution, Resolver};
use std::cell::{Cell, RefCell};

/// Which runtime helpers an emission actually referenced.
///
/// Tracked so the compiler only injects a `require` for what a file really uses
/// — a module with no spreads should not gain a `mergeProps` import.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Helpers {
    pub create: bool,
    pub read: bool,
    pub merge_props: bool,
}

/// Byte offsets of the start of each line, plus the file-wide name resolver.
pub struct EmitContext<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
    resolver: &'a Resolver,
    create: Cell<bool>,
    read: Cell<bool>,
    merge_props: Cell<bool>,
    /// Resolution errors recovered from rather than returned.
    ///
    /// A name that does not resolve is a mistake in one tag, and stopping there
    /// costs the author every *other* diagnostic in the file — including
    /// everything a type checker would say about the generated Luau, since a
    /// compile that returns `Err` produces no Luau at all. Recording and
    /// carrying on turns "one error, then nothing" into "every error, and a file
    /// that still checks".
    ///
    /// Parse errors are different and still stop the compile: recovering from
    /// `<Frame` with no `>` means guessing what was meant, and a wrong guess
    /// buries the one real mistake under a page of invented ones.
    errors: RefCell<Vec<EmitError>>,
}

impl<'a> EmitContext<'a> {
    pub fn new(source: &'a str, resolver: &'a Resolver) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );

        Self {
            source,
            line_starts,
            resolver,
            create: Cell::new(false),
            read: Cell::new(false),
            merge_props: Cell::new(false),
            errors: RefCell::default(),
        }
    }

    /// Expression that constructs an element — `[factory] create`.
    pub fn create(&self) -> &str {
        self.resolver.create()
    }

    pub fn used_create(&self) {
        self.create.set(true);
    }

    pub fn used_read(&self) {
        self.read.set(true);
    }

    pub fn used_merge_props(&self) {
        self.merge_props.set(true);
    }

    pub fn helpers(&self) -> Helpers {
        Helpers {
            create: self.create.get(),
            read: self.read.get(),
            merge_props: self.merge_props.get(),
        }
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    /// Whether a tag is a Roblox class or a component (PLAN.md §3.1).
    ///
    /// The resolver is file-wide, so it stays correct for LuauX nested inside a
    /// captured expression even though that expression is compiled on its own.
    ///
    /// A name that resolves to neither is recorded and reported as
    /// [`Resolution::Unresolved`] rather than returned as an error, so the rest
    /// of the file still compiles.
    pub fn resolve(&self, name: &ElementName, offset: usize) -> Resolution {
        match self.resolver.resolve(name, offset) {
            Ok(resolution) => resolution,
            Err(error) => {
                let written = name.as_written();
                self.errors.borrow_mut().push(error);
                Resolution::Unresolved(written)
            }
        }
    }

    /// Canonical Roblox name for a written attribute, applying `luaux.toml`
    /// aliases and rejecting names the class does not have.
    ///
    /// Recovered the same way: the name as written is emitted, which is a key
    /// the class does not have but is still Luau, so one bad attribute does not
    /// cost the file.
    pub fn resolve_attribute(&self, class: &str, written: &str, offset: usize) -> String {
        match self.resolver.resolve_attribute(class, written, offset) {
            Ok(canonical) => canonical,
            Err(error) => {
                self.errors.borrow_mut().push(error);
                written.to_string()
            }
        }
    }

    /// The recovered errors, in the order they were found.
    pub fn take_errors(&self) -> Vec<EmitError> {
        std::mem::take(&mut self.errors.borrow_mut())
    }

    /// Zero-based line containing `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next - 1,
        }
    }

    /// The leading whitespace of `line`, reused so emitted code keeps the shape
    /// of the source it replaced.
    pub fn indent_of(&self, line: usize) -> &'a str {
        let start = self.line_starts.get(line).copied().unwrap_or(0);
        let end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.source.len());

        let text = &self.source[start..end];
        let indent = text.len() - text.trim_start().len();
        &text[..indent]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::Resolver;

    #[test]
    fn maps_offsets_to_lines() {
        let resolver = Resolver::new("", crate::Config::default());
        let context = EmitContext::new("a\nbb\n\nccc", &resolver);
        assert_eq!(context.line_of(0), 0);
        assert_eq!(context.line_of(1), 0);
        assert_eq!(context.line_of(2), 1);
        assert_eq!(context.line_of(4), 1);
        assert_eq!(context.line_of(5), 2);
        assert_eq!(context.line_of(6), 3);
    }

    #[test]
    fn reads_line_indentation() {
        let resolver = Resolver::new("", crate::Config::default());
        let context = EmitContext::new("no\n  two\n\tTab\n", &resolver);
        assert_eq!(context.indent_of(0), "");
        assert_eq!(context.indent_of(1), "  ");
        assert_eq!(context.indent_of(2), "\t");
    }

    #[test]
    fn handles_a_final_line_without_a_newline() {
        let resolver = Resolver::new("", crate::Config::default());
        let context = EmitContext::new("a\n  b", &resolver);
        assert_eq!(context.line_of(4), 1);
        assert_eq!(context.indent_of(1), "  ");
    }
}
