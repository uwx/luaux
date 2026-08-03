# ADR-0001: Add an `all` casing key for elements and properties

Date: 2026-07-31

## Status

Accepted

## Context

Element and property renames were one-at-a-time:

```toml
[elements]
TextLabel = "text"
```

A project wanting a house style across the board would need hundreds of those
lines. An `all` key would let it pick one scheme instead:

```toml
[elements]
all = "camelCase"

[properties]
all = "snake_case"
```

The obstacle is that Roblox keeps deprecated spellings alongside modern ones,
and the pairs frequently differ only by case:

| Modern | Deprecated |
| --- | --- |
| `ChildAdded` | `childAdded` |
| `BrickColor` | `brickColor` |
| `CFrame` | `cframe` |
| `AudioSubType` | `AudioSubtype` |

Any casing transform maps both members of such a pair onto the same key. Written
`child_added`, which member does the compiler mean?

Measured against the API dump:

- **Class names collide zero times** under every scheme. Elements are safe.
- **Members collide 13 times** under `snake_case` and `camelCase`, 14 under
  `flatcase`.
- Because `ChildAdded` and `childAdded` are both declared on `Instance`, the
  collision is inherited by **every creatable class**. There is no subset of
  Roblox where the problem does not arise.

## Decision

Ship `all`, and resolve collisions with a two-step rule.

**1. Prefer the name Roblox does not mark deprecated.** The API dump tags
deprecated members, and the generator emits that set as
`roblox::generated::DEPRECATED`. This resolves 11 of the 13 collisions, always in
favour of the modern spelling.

**2. Where that leaves no single winner, take the byte-order-first name.** Two
pairs are deprecated on *both* sides, `FormFactor`/`formFactor` and
`Part1`/`part1`, so step 1 eliminates every candidate. Sorting puts uppercase
first, which yields the PascalCase spelling. The rule is arbitrary but
deterministic, and since both names are deprecated, a project depending on the
distinction has a larger problem.

### Precedence

An explicit entry always beats `all`.

```toml
[elements]
all = "camelCase"
TextLabel = "text"      # <text>, not <textLabel>
```

Resolution order for a written name:

1. An explicit alias from `[elements]` or `[properties]`
2. The `all` scheme, over names no explicit entry claimed
3. The canonical Roblox name, when no scheme is set

Step 2 skips anything already renamed in step 1, so an override removes its class
from the blanket scheme entirely rather than leaving two ways to write it.

### Retirement

`all` retires the canonical spelling, exactly as a single rename does:

```
× <Frame> was renamed by [elements] all; use <frame>
```

This follows the existing rule that a rename is exclusive. A project has one
vocabulary, not two.

## Consequences

### Positive

- A project picks a naming style once instead of writing hundreds of aliases.
- Word boundaries come from the canonical PascalCase spelling, so `UICorner`
  splits as `UI` + `Corner` and yields `ui_corner` rather than `u_i_corner`.
- Deprecated Roblox names become unreachable under a scheme, since the modern
  spelling always wins the collision.
- The collision policy is derived from the API dump rather than hardcoded, so it
  stays correct as Roblox deprecates more names.

### Negative

- Diagnostics, and any future language server, must render names in the
  project's scheme rather than Roblox's, and every did-you-mean has to transform
  through it.
- Code no longer copies verbatim from Roblox documentation, forum posts, or
  existing Luau. The project takes this cost on knowingly.
- Two collisions resolve by a tiebreak that carries no real meaning, and a
  project that genuinely needs `formFactor` rather than `FormFactor` must write
  an explicit override.
- The collision set comes from a dated API dump, and nothing currently fails when
  a new release introduces a pair. A CI check that regenerates the tables and
  diffs them would catch it; that check does not exist yet.

## Alternatives considered

**Rejecting `all` outright.** Defensible on the grounds that matching Roblox's
own vocabulary is the feature, but it forces hundreds of alias lines on any
project with a house style.

**A hardcoded exception table for the collisions.** Would work today and rot on
the next API dump. Deriving the answer from the dump's own deprecation tags keeps
it current for free.
