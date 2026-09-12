# ADR-0004 — A third arrangement: components curry too

## Status

Accepted.

## Context

ADR-0003 settled on two arrangements — `table` (`F(class)(props)`, a
component called directly) and `element` (`F(class, props, children)`, a
component through the factory) — and framed them as covering "the Roblox UI
libraries." That framing held only because every library considered so far
happened to fall into one bucket or the other by both its arity *and* whether
a component goes through the factory.

A library that curries a single props table the way Vide, Fluid, and Fusion
do, but treats a component exactly like an intrinsic at the call site —
`create(Component)(props)`, not `Component(props)` — does not fit `table`
(which special-cases the component branch to call it directly) and has no
reason to adopt `element`'s three-argument, positional-children shape just to
get that one property. ADR-0003's own test for what needs a backend —
"anything that changes the arity or arrangement of the constructor call" —
is met here too: which argument position a component's *call* sits in is
part of the arrangement, not a `[factory]` variable, even though the number
of arguments to the outer call (one, curried) does not change.

## Decision

Add `backend::curried::Curried`, selected by `[factory] backend = "curried"`.

It is `table` with exactly one difference: the `None` (component) branch of
the opening-call match curries through `create` instead of calling the
component directly. Everything else — props table layout, the array-part or
`[factory] children`-key placement of children, fragments as plain tables,
`interpolate`/`compute`/`use`/`merge`/`event` semantics, and the
`arrangement_defaults` and cross-key validation rules — is identical to
`table`, including the rule that a `fragment` key is rejected (a fragment is
still a plain table; nothing about currying components changes what a
fragment is).

No `luaux init --library` preset was added for it. Unlike `react`, `vide`,
`fluid`, and `fusion`, no specific library was the reason this arrangement
was requested — it exists for a project whose library curries components,
described by hand with a `[factory]` block, the same way any library without
a preset is (README.md, "Targets").

## Consequences

**"Two arrangements cover the Roblox UI libraries" (ADR-0003) is no longer
exactly true**, and that ADR is left as written rather than edited — it
recorded the reasoning that was sufficient at the time, and a third
arrangement showing up later does not make the first two wrong, only
incomplete. This document is where "three" is recorded instead.

**The seam ADR-0003 drew still holds.** Nothing about `curried` needed a new
kind of thing to exist alongside `Backend` and `[factory]` — it needed a new
value of the kind that already existed. `backend/common.rs`'s shared
emission machinery (table layout, comments, spreads, text folding) covers it
unchanged, which is the same claim ADR-0003 made about `element` costing "a
few hundred lines rather than a fork."

**A future arrangement is not assumed to be the last one either.** The
pattern here — copy the nearest existing backend file, change the one branch
that differs, let `common.rs` carry the rest — is what a fourth arrangement
would do again, and there is no reason to believe three is where library
shapes stop varying.

## See also

- `docs/adr/0003-backends-own-arrangement.md` — the seam this arrangement
  fits into without changing.
