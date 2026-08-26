# ADR-0003 — A backend owns the arrangement; `[factory]` owns the contents

## Status

Accepted.

## Context

LuauX has to lower to four Roblox UI libraries — React, Vide, Fusion, and
Fluid — without knowing about any of them. Two things vary between them, and
they are not the same kind of thing:

|        | arrangement                  | children              | events               | reactive value |
| ------ | ---------------------------- | --------------------- | -------------------- | -------------- |
| Vide   | `F(class)(props)`            | numeric keys in props | string key           | function       |
| Fluid  | `F(class)(props)`            | numeric keys in props | string key           | function       |
| Fusion | `F(class)(props)`            | `[Children]` in props | `[OnEvent(name)]`    | `StateObject`  |
| React  | `F(class, props, children)`  | third argument        | `[Event.Name]`       | none           |

An earlier proposal was to pick one shape, emit it everywhere, and ship a
per-library runtime adapter that translates. That was rejected, for reasons that
are worth recording because they will come up again:

- **It reintroduces a runtime.** `imports.rs` refuses to resolve module paths
  because a require is location-dependent, and no configured string is correct
  for every file. An adapter is a module every generated file must require.
- **The value type differs and no adapter can hide it.** `<Frame/>` under Vide,
  Fusion, and Fluid *is* an Instance — assignable `.Parent`, indexable `.Size`.
  Under React it is an inert description. An adapter makes the *call* portable
  and leaves the author's surrounding code exactly as unportable as before.
- **Reactivity cannot be adapted.** An adapter handed `Size = function() … end`
  has no scope to build a Fusion `Computed` with and no hook site to register
  React state at.

## Decision

Two seams, and they are not interchangeable:

> A **`[factory]` variable** changes what goes where **inside one props table**
> passed to a curried constructor. Anything that changes the **arity or
> arrangement** of the constructor call needs a **backend**.

Concretely:

- `backend::table::Table` — `F(class)(props)`. Vide, Fluid, Fusion.
- `backend::element::Element` — `F(class, props, children)`. React.
- `[factory]` keys `children`, `event`, `compute`, `use`, `fragment`,
  `interpolate`, and `merge` describe contents, not arrangement.

`children` is deliberately a **key expression** and not a general placement
mechanism. A sentinel like `children = "@arg3"` that moved children out of the
table would change the call's arrangement from inside a string meant to hold a
table key — the kind of overload that makes a setting impossible to describe.
The element backend rejects `children` outright for the same reason.

Backends are named for the shape, never the library. A `preset = "react"` key,
or a backend called `React`, would bake one library into the compiler and make
every library without one second-class. `luaux init --library <name>` writes an
explicit block instead: presets as scaffolding, not as a config key.

## Consequences

**Most of the compiler is untouched by any of this.** Lexer, parser, resolver,
the Roblox class and property tables, alias config, text rules, and lints all sit
above the seam. `backend/common.rs` holds the emission machinery both backends
share — entry layout, line placement, comment and trailing-comma handling, text
folding — so a second backend cost a few hundred lines rather than a fork.

**Two things follow from the element arrangement rather than from React.** A
component goes through the factory (`F(Card, props)`) instead of being called
directly, and a fragment needs a component because a plain table is not an
element. Both are arrangement facts, which is why they live in the backend.

**Line-preservation ownership had to become explicit.** Under the one-table
arrangement the props table is the whole element, so its closing brace lands on
the closing tag's line. Under the element arrangement a children argument follows
it, and a props table that spanned to the close would eat the lines those
children need. `emit_table` therefore takes `close: Option<usize>` — `Some` for
whichever table is genuinely last.

**A nil child needs nothing.** React reconciles an array child list with
`for i = 1, #newChildren`, which reads like a hazard — `#` on a table with a hole
is a border Lua leaves undefined. A table *constructor* presizes its array part,
so the width is the number of expressions written, and React renders nothing for
the nil while its siblings keep their indices.

An earlier version of this backend compacted the children through an inlined
helper on the strength of that reading. It was wrong twice: unnecessary, and
harmful — compacting moves later children up an index, and React keys children by
index, so a conditional that toggled would have remounted every sibling below it.
The golden runtime suite checks that the surviving sibling is the same Instance
across a toggle, which is the assertion that would have caught it.

**The default is React, and writing `[factory]` turns every assumption off.**
With no `[factory]` block luaux picks React, which is the one place it names a
library. The moment a project writes one, `backend` is required and nothing else
is assumed. Without that rule the flip would be silent: a Vide project that set
only `create` would inherit React's arrangement, pass the in-scope check because
its own name really is in scope, and emit the wrong shape without a word.

**Raw `Instance.new` still cannot be a backend.** It is statement-oriented, and
`Backend::emit` requires an expression so LuauX composes in every position an
expression can appear. That constraint is unchanged and is why `DEFER.md`'s
backend has never landed.

## See also

- `backend-plan.md` — the full design, including what React costs per construct.
- `factory-plan.md` — the `[factory]` variables and why Fusion needs no backend.
- ADR-0002 — why config keys naming luaux settings use TOML convention while
  keys naming Roblox members use Roblox spelling.
