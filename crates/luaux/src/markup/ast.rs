//! LuauX syntax tree.
//!
//! Luau expressions are held as **raw source slices**, never parsed. LuauX lowers
//! to an ordinary Luau expression and everything else passes through unchanged,
//! so the compiler never needs to understand what is inside `{...}` — only where
//! it ends (PLAN.md §5.1).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Fragment(Fragment),
}

impl Node {
    pub fn span(&self) -> Span {
        match self {
            Node::Element(element) => element.span,
            Node::Fragment(fragment) => fragment.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: ElementName,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Child>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub children: Vec<Child>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementName {
    /// `<Frame>` — resolved to an intrinsic or a component later.
    Simple(String),
    /// `<Foo.Bar>` — always a component.
    Member(Vec<String>),
}

impl ElementName {
    pub fn as_written(&self) -> String {
        match self {
            ElementName::Simple(name) => name.clone(),
            ElementName::Member(parts) => parts.join("."),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribute {
    Named {
        name: String,
        value: AttributeValue,
        span: Span,
    },
    /// `{props}` in attribute position.
    Spread { expression: String, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    /// `Size={expr}` — raw Luau, verbatim.
    Expression(String),
    /// `Name="literal"` — the raw literal *including* its quotes.
    ///
    /// Attribute strings are ordinary Luau strings, so they are captured and
    /// re-emitted byte for byte. Decoding luaux's `\{` escapes here would
    /// corrupt Luau escapes like `\n`; those escapes apply to text *children*,
    /// which are a different lexical mode.
    StringLiteral(String),
    /// Shorthand `Visible`, meaning `true`.
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Node(Node),
    /// `{expr}` — raw Luau, verbatim.
    Expression {
        expression: String,
        span: Span,
    },
    /// Literal text with escapes already decoded and whitespace normalised.
    Text {
        text: String,
        span: Span,
    },
    /// A comment, stored as **Luau source ready to emit**.
    ///
    /// `<!-- … -->` is wrapped into a block comment here rather than at emit
    /// time; a hole like `{--[[ … ]]}` is already Luau and is kept verbatim.
    /// Wrapping twice would produce `--[[ --[[ … ]] ]]`, and Lua block comments
    /// do not nest — the inner `]]` closes the outer one.
    Comment {
        luau: String,
        span: Span,
    },
}

impl Child {
    /// Where this child began in the source. Codegen positions each emitted
    /// entry on its original line (PLAN.md §5.5).
    pub fn span(&self) -> Span {
        match self {
            Child::Node(node) => node.span(),
            Child::Expression { span, .. }
            | Child::Text { span, .. }
            | Child::Comment { span, .. } => *span,
        }
    }
}
