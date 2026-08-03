# ADR-0002: Config keys use canonical Roblox spelling

Date: 2026-07-31

## Status

Accepted

## Context

TOML files conventionally use snake_case keys, and `luaux.toml` follows that for
everything luaux itself defines:

```toml
[build]
in = "src"
out = "build"
clean = true

[factory]
create = "vide.create"

[lints]
static_conditional_child = "warn"
```

But `[elements]` and `[properties]` are different. Their keys are not luaux
options; they are names borrowed from the Roblox API:

```toml
[elements]
TextLabel = "text"

[properties]
BackgroundColor3 = "bgColor"
```

Read quickly, the file looks inconsistent, and the obvious tidy-up is to
snake_case those too:

```toml
[elements]
text_label = "text"
```

The question is whether the convention applies to keys that name something
another system already named.

## Decision

**Keys that name a luaux option follow TOML convention. Keys that name an
external identifier keep that identifier's spelling.**

So `[build] in`, `[lints] static_conditional_child`, and `[elements] all` are
lowercase, while `[elements] TextLabel` and `[properties] BackgroundColor3` are
PascalCase, because that is what Roblox calls them.

This is the same rule Cargo follows. `[dependencies] serde` is spelled as the
crate is published, not normalised to the manifest's own style, while
`codegen-units` is a Cargo option and takes Cargo's convention. `.luaurc`
treats its alias keys the same way.

There is also a structural reason specific to `[elements]` and `[properties]`:
these tables declare a **mapping** from a Roblox name to a project name. A
mapping needs one fixed end. If the left side were also transformed, the table
would relate two project-chosen spellings and nothing would anchor it to the
API. ADR-0001 makes this concrete, since `all = "snake_case"` already lets a
project write snake_case in its `.luaux` files. The config key is what says
*which Roblox thing* is being renamed, so it has to be the Roblox name.

## Consequences

### Positive

- The config, the compiler's diagnostics, and the Roblox API all use one
  spelling for a given class or property. A message reading
  `Frame has no property named BackgroundColour3` matches what the config would
  say.
- Names copy directly from the Roblox API reference, the Creator Hub, or
  existing Luau, with no mental transform.
- A did-you-mean suggestion is usable verbatim as a config key.
- No lossy transform runs at parse time, so there is no question of how
  `UIAspectRatioConstraint` splits into words before it can be looked up.

### Negative

- The file mixes two conventions, which looks like an oversight until the rule
  is known. This ADR exists mostly to answer that.
- A contributor accustomed to snake_case TOML has to learn the distinction, and
  the error for getting it wrong is a "not a creatable Roblox class" message
  rather than something that names the convention.
- The rule needs restating whenever a new table is added, since "is this an
  option or an identifier" is a judgement call rather than a mechanical test.

## Alternatives considered

**snake_case keys everywhere.** Uniform at a glance, and technically workable:
transforming the 356 class names forward produces no collisions, as measured in
ADR-0001. It was rejected because the config would then disagree with every
diagnostic luaux prints and with Roblox's own documentation, and because a
project that renames `TextLabel` would have to write `text_label` on the left to
say so, which asserts a name Roblox does not use.

**PascalCase keys everywhere**, including luaux's own options. Uniform in the
other direction, but it would spell luaux's settings in a style neither TOML nor
the wider Rust ecosystem uses, and those settings genuinely are luaux's to name.
