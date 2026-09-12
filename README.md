<div align="center">

# LuauX

**JSX Syntax in Luau**

A compiler that turns `.luaux` — Luau with JSX syntax — into plain `.luau`,
targeting [React](https://github.com/jsdotlua/react-lua),
[Vide](https://centau.github.io/vide/),
[Fluid](https://github.com/ffrostfall/fluid),
[Fusion](https://github.com/dphfox/Fusion), and anything shaped like them.

[![CI](https://github.com/luau-xml/luaux/actions/workflows/ci.yml/badge.svg)](https://github.com/luau-xml/luaux/actions/workflows/ci.yml)

</div>

## Overview

With LuauX, you can write:

```luau
return function ()
  local count, setCount = React.useState(0)

  return (
    <Frame Size={UDim2.fromScale(1, 1)}>
      <TextLabel>Clicked {count} times</TextLabel>
      <TextButton Text="Click me" Activated={function () setCount(count + 1) end} />
    </Frame>
  )
end
```

Which compiles to exactly the React you would have written:

```luau
return function ()
  local count, setCount = React.useState(0)

  return (
    React.createElement("Frame", { Size = UDim2.fromScale(1, 1) }, {
      React.createElement("TextLabel", { Text = `Clicked {count} times` }),
      React.createElement("TextButton", { Text = "Click me", [React.Event.Activated] = function () setCount(count + 1) end }),
    })
  )
end
```

Same lines. Same semantics. No framework in between.

The same source compiles to Vide or Fusion by changing five lines of
`luaux.toml` — see [Targets](#targets). Across our
[examples](/examples/), which target Vide, LuauX is an average of **22% less
code**, the highest being **32.2% less** in [`Hud.luaux`](/examples/Hud.luaux).

> Minus comments and whitespace. Benched with
> [`measure_examples.py`](/scripts/measure_examples.py).

## Why LuauX

**Creating UI in Luau is flat.** The code you write is at the same indentation, and the tree has no shape. It only exists in your `Parent` assignments.

```luau
local frame = Instance.new("Frame")
frame.BackgroundColor3 = Color3.new(0, 0 ,0)
local label = Instance.new("TextLabel")
label.TextColor3 = Color3.new(1, 1, 1)
label.Parent = frame
```

A deeply nested UI and a shallow one look the exact same in code. **Every modern
Roblox UI library fixed the shape.**

```luau
local frame = create "Frame" {
  BackgroundColor3 = Color3.new(0, 0 ,0),

  create "TextLabel" {
    TextColor3 = Color3.new(1, 1, 1)
  },
}
```

Nesting constructor calls inside each other gave shape back to the tree. **But
it's still tables and calls.** Properties and children share a single table, told
apart only by whether the value has a key or not — or they don't, and children go
under a special key you have to remember. Every element carries `create "..."`
scaffolding. And six levels down, a column of closing braces tells you nothing
about what each one closes. **LuauX gives it syntax.**

```lua
local frame = (
  <Frame
    BackgroundColor3={Color3.new(0, 0, 0)}
  >
    <TextLabel TextColor3={Color3.new(1, 1, 1)} />
  </Frame>
)
```

Tags name themselves, and name themselves again on the way out. Attributes are visibly attributes; children are visibly children. Fragments return siblings without inventing a wrapper Frame. Spread a table of shared props and override what differs. It compiles to your library's own idiom — same reactive graph, same hooks or sources, nothing new to learn underneath.

## What is LuauX

LuauX is a compile step, not a library.

- **No runtime.** Your UI library already handles parenting, fragments, events, and reactivity. LuauX inlines two one-line helpers into the files that use them and emits no `require` at all.
- **Output you can read.** Every construct lands on the line it came from, so a stack trace on `build/App.luau:42` really is about `src/App.luaux:42`.
- **It knows Roblox.** Tags check against the 356 creatable classes, attributes against the inheritance chain of 899. Mistakes surface before rojo syncs.

A typo fails the build rather than the game:

```
  × Frame has no property or event named BackgroundColour3
   ╭─[src/App.luaux:7:5]
 7 │     BackgroundColour3={Color3.new()}
   ·     ────────┬────────
   ·             ╰── did you mean BackgroundColor3?
   ╰────
```

## Installation

You can install LuauX from (1) [LPM](https://luaupm.com), (2) [Rokit](https://github.com/rojo-rbx/rokit), (3) [Mise](https://mise.jdx.dev/), (4) [Nixpkgs](https://nixos.org/manual/nixpkgs/stable/), or (5) build it from source.

1. Install with [LPM](https://luaupm.com):

```console
$ lpm tool add luaux
```

2. Install with [Rokit](https://github.com/rojo-rbx/rokit) (a popular toolchain manager):

```console
$ rokit add luau-xml/luaux
$ rokit install
```

3. Install with [Mise](https://mise.jdx.dev/):

```console
$ mise use github:luau-xml/luaux@0.2.0
```

4. Install with [Nixpkgs](https://nixos.org/manual/nixpkgs/stable/):

```console
$ nix profile add github:luau-xml/luaux
```

5. Build from source:

```sh
git clone https://github.com/luau-xml/luaux
cd luaux
cargo build --release
```

## Usage

- `luaux init --library react` — Scaffold a `luaux.toml` for your library.
- `luaux build src build` — Compile `src/*.luaux` into `build/*.luau`.
- `luaux watch` — Rebuild on change, beside `rojo serve`.

Point Rojo at `build/`, keep editing `src/`, and carry on.

| Command                        | Description                     |
| ------------------------------ | ------------------------------- |
| `luaux build [src] [out]`      | Compile `.luaux` to `.luau`.    |
| `luaux check [src]`            | Compile without writing.        |
| `luaux watch [src] [out]`      | Rebuild on change.              |
| `luaux init [dir] [--library]` | Scaffold a `luaux.toml`.        |
| `luaux scan <path>...`         | Report where LuauX is detected. |

`--library` takes `react`, `vide`, `fluid`, or `fusion`, and writes the matching
`[factory]` block. Paths default to `[build] in` and `out` in `luaux.toml`, so a
bare `luaux build` works. Every non-source file is copied into the output unless
`exclude` or `include` are specified.

## Targets

LuauX does not know about any UI library. It knows three **arrangements** — how
a constructor call is shaped — and a handful of settings for what goes inside
one. Between them they cover the libraries people actually use.

| Library    | Arrangement                 | `luaux init --library` |
| ---------- | --------------------------- | ---------------------- |
| **React**  | `F(class, props, children)` | `react` *(default)*    |
| **Vide**   | `F(class)(propsAndChildren)`| `vide`                 |
| **Fluid**  | `F(class)(propsAndChildren)`| `fluid`                |
| **Fusion** | `F(class)(propsAndChildren)`| `fusion`               |

A fourth arrangement, `curried`, is the same curried shape as Vide, Fluid, and
Fusion, but a component curries through the factory too —
`F(Component)(propsAndChildren)` instead of `Component(propsAndChildren)` —
for a library that treats components and intrinsics identically at the call
site. It has no `luaux init --library` preset of its own (see ADR-0004); write
its `[factory]` block by hand.

The same `.luaux` file compiles under all three, with the same line count:

```lua
<TextLabel TextSize={16}>HP {health} / {maxHealth}</TextLabel>
<TextButton {Row} Activated={fire}>Fire</TextButton>
```

```luau
-- React
React.createElement("TextLabel", { TextSize = 16, Text = `HP {health} / {maxHealth}` })
React.createElement("TextButton", __luaux_merge(Row, { [React.Event.Activated] = fire, Text = "Fire" }))

-- Vide
create("TextLabel")({ TextSize = 16, Text = function() return `HP {__luaux_read(health)} / {__luaux_read(maxHealth)}` end })
create("TextButton")(__luaux_merge(Row, { Activated = fire, Text = "Fire" }))

-- Fusion
scope:New("TextLabel")({ TextSize = 16, Text = scope:Computed(function(use) return `HP {use(health)} / {use(maxHealth)}` end) })
scope:New("TextButton")(__luaux_merge(Row, { [OnEvent("Activated")] = fire, Text = "Fire" }))
```

The markup ports. The reactivity is yours and always was — `health` is a source
under Vide, a `StateObject` under Fusion, and a plain value under React, and
LuauX never touches which. See
[ADR-0003](docs/adr/0003-backends-own-arrangement.md) for where the line sits.

A library not listed here needs no code, only a `[factory]` block that describes
its shape. There are no presets baked into the compiler, so nothing is
second-class.

## Syntax

### Elements

Tags are the real Roblox class names, since `<div>` would mean nothing here.

| Form             | Meaning                                                      |
| ---------------- | ------------------------------------------------------------ |
| `<Frame>`        | A Roblox class, or a component bound in the file.            |
| `Size={expr}`    | Attribute taking any Luau expression.                        |
| `Name="literal"` | Attribute taking a string.                                   |
| `Visible`        | Shorthand for `Visible={true}`.                              |
| `={props.Size}`  | Shorthand for `Size={props.Size}` — the name is inferred.    |
| `{props}`        | Spread, in attribute position.                               |
| `Hello`          | Text child, folded into the `Text` property.                 |
| `Hi {name}`      | Interpolated text.                                           |
| `{items}`        | Any Luau expression, as a child.                             |
| `<>...</>`       | Fragment.                                                    |
| `<!-- ... -->`   | Comment between tags.                                        |
| `{--[[ ... ]]}`  | Comment where a child would go, as JSX writes `{/* ... */}`. |

> [!WARNING]
> `--` does **not** start a comment inside markup, where it is ordinary text.
> <br/>
> Use `<!-- ... -->` between tags, or a Luau comment inside a `{ }` hole.

```lua
<Frame {props} Size={expr} Name="literal" Visible ={props.Position}>
  <TextLabel>Hello</TextLabel>
  <TextLabel>Hi {name}, you have {n} messages</TextLabel>

  {items}

  <>
    <UICorner CornerRadius={UDim.new(0, 8)} />
  </>

  <!-- a comment between tags -->
  {--[[ or a Luau one, in a hole ]]}
</Frame>
```

A name resolves to a Roblox class first, then to anything bound in the file, and
otherwise errors with a suggestion. Dotted names such as `<Foo.Bar/>` are always
components.

### Inferred property names

When the value already says which property it is, `={...}` says it once:

```lua
<Frame ={props.Size} ={props.Visible}>
  <TextLabel ={Text} />
</Frame>
```

```luau
create("Frame")({ Size = props.Size, Visible = props.Visible,
  create("TextLabel")({ Text = Text }),
})
```

The expression has to **be** a name: an identifier, or a dotted path of them,
whose last segment is the property. Anything else — `={f().Size}`, `={t[1]}`,
`={a or b}` — is an error telling you to write the property out, because the rule
is about names rather than about punctuation, and a shorthand you cannot predict
is worse than one you type in full.

It is spelled `=` rather than a bare `{Size}` because a bare hole in attribute
position already means a spread.

Inference decides *which* name and nothing else. The name it produces is then an
ordinary attribute: `luaux.toml` renames and casing apply to it, the class has to
actually have it, an event still becomes an event key, and text between the tags
still overrides an inferred `Text`.

### Components

A component is any function taking one table, and components and intrinsics are
interchangeable at the call site.

```lua
local function Card (props)
  return (
    <Frame BackgroundColor3={props.Color}>
      <UICorner CornerRadius={UDim.new(0, 8)} />

      {props.children}
    </Frame>
  )
end

local card = <Card Color={c}><TextLabel>Body</TextLabel></Card>
```

**Forwarding children is the one place the target shows through.** LuauX
generates the code that *passes* children to a component, so where they land is
configurable. It never generates the code that *reads* them back out — that is
ordinary Luau you write, and luaux passes your expressions through untouched. So
this is a line to type, not a setting to choose:

| Target | Children arrive as | Forward them with |
| ------ | ------------------ | ----------------- |
| React  | `props.children`   | `{props.children}` |
| Vide   | numeric keys of the props table | `{props}` |
| Fluid  | numeric keys of the props table | `{props}` |
| Fusion | the key `[factory] children` names | `{props[Children]}` |

The example above is React's. Porting a component the wrong way is loud rather
than subtle — React tries to render the props table itself and fails on the first
render, rather than quietly showing nothing.

### Reactivity

Reactivity is your library's, unchanged. LuauX generates it in exactly one
place — **interpolated text** — because building a string from a reactive value
would otherwise stringify the value itself.

| Target | `<TextLabel>Hi {name}</TextLabel>` becomes            |
| ------ | ----------------------------------------------------- |
| React  | `` Text = `Hi {name}` ``                              |
| Vide   | ``Text = function() return `Hi {__luaux_read(name)}` end`` |
| Fusion | ``Text = scope:Computed(function(use) return `Hi {use(name)}` end)`` |

A single expression — `<TextLabel>{label}</TextLabel>` — is emitted bare in every
target, because a source, a `StateObject`, and a string are all correct there.

Everything else is yours. `Size={size}` is reactive under Vide because Vide
treats a function on a property key as a source; under React it is a `UDim2`,
because React re-runs the whole component instead.

> [!WARNING]
> **Do not interpolate a React binding.** `<TextLabel>Hi {binding}</TextLabel>`
> builds a string, so the binding is stringified — the label reads
> `Hi RoactBinding(one)` and never updates. Pass it as a whole prop
> (`Text={binding}`) or map it (`Text={binding:map(f)}`). LuauX cannot tell a
> binding from a string, so this is yours to avoid; it is at least loud, since
> the wrong text is on screen.

## Configuration

`luaux.toml` is optional. With no `[factory]` block LuauX targets React.

```toml
[build]
in = "src"
out = "build"
exclude = ["**/*.spec.luaux"]
clean = true                  # delete outputs whose source is gone

# Pick ONE arrangement. Keys belonging to the other are rejected rather than
# ignored, so this block is per-target — `luaux init --library <name>` writes it.

[factory]
backend = "element"           # F(class, props, children)
create = "React.createElement"
fragment = "React.Fragment"   # required here; a plain table is not an element
event = "React.Event."        # trailing dot indexes: [React.Event.Activated]

[elements]
all = "flatcase"              # PascalCase | camelCase | snake_case | flatcase
TextLabel = "text"            # an explicit entry always beats `all`

[properties]
all = "camelCase"
TextColor3 = "textColor"      # rename a property everywhere

[properties.Frame]
BackgroundColor3 = "bgColor"  # or for one class, beating the table above

[lints]
static_conditional_child = "warn"
```

The one-table arrangement instead, with everything it can move:

```toml
[factory]
backend = "table"             # F(class)(propsAndChildren)
create = "scope:New"
children = "Children"         # [Children] = { ... }; omit for Vide and Fluid
event = "OnEvent"             # no trailing dot calls: [OnEvent("Activated")]
compute = "scope:Computed"    # wrapper for interpolated text
use = "use"                   # the reader inside compute's callback
merge = "mergeProps"          # replaces the inlined spread helper
```

The `curried` arrangement is the same as `table` above — same keys, same
defaults — except a component curries through `create` too instead of being
called directly:

```toml
[factory]
backend = "curried"           # F(class)(propsAndChildren), components curry too
create = "create"
```

`interpolate` follows the arrangement — `plain` under `element`, `wrap` under
`table` and `curried` — and is there to override when a library does not match
its shape.

> [!IMPORTANT]
> **Writing a `[factory]` block turns every default off**, and `backend` becomes
> required. Without that rule, a Vide project that set only `create` would
> inherit React's arrangement, pass the in-scope check — its own name really is
> in scope — and emit the wrong shape in silence. One explicit line beats output
> that compiles and is wrong.

Every name in `[factory]` has to be in scope in each file that uses it. LuauX
never emits a `require`: a require is location-dependent, and your own import
always works.

> [!IMPORTANT]
> Renames are **exclusive**. Once `TextLabel = "text"` is declared, `<text>` is the spelling and `<TextLabel>` is an error. `all` behaves the same way, retiring every canonical spelling at once. One vocabulary, not two.

Roblox ships deprecated names beside modern ones that differ only by case, such
as `childAdded` beside `ChildAdded`, so a casing scheme merges those pairs. The
modern name wins. See [ADR-0001](docs/adr/0001-casing-key.md).

Keys on the left are Roblox's own spelling, while keys naming a luaux setting
follow TOML convention. See [ADR-0002](docs/adr/0002-config-keys-use-roblox-spelling.md).

`static_conditional_child` defaults to `off` under the element backend, where the
component body re-runs and `{isOpen and <Panel/> or nil}` is ordinary. Under the
table backend it defaults to `warn`, because there such a child really is built
once.

## Status

Early, but real. The compiler works end to end and is covered by 274 tests
across Linux, macOS, and Windows. Three further suites run on every CI build:
golden tests that *run* the generated code against real Vide **and real React**
under Lune, and a corpus sweep over the Luau, Vide, and react-lua sources
confirming that ordinary `.luau` is never mistaken for markup.

The golden suites are the ones that earn their keep. The React suite caught a
compaction step that would have remounted every sibling below a toggled
conditional child — a bug that every other test passed.

Expect the syntax to be stable and the tooling to keep moving.

## Credits

Built on the shoulders of [Vide](https://github.com/centau/vide) by centau,
whose `create` sits so close to a compile target that LuauX's first backend was
two helper functions, and [React Lua](https://github.com/jsdotlua/react-lua),
whose `createElement` gave the syntax its original meaning back.

This project is not affiliated with or endorsed by either.

## License

MIT.
