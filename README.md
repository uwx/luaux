<div align="center">

# LuauX

**JSX Syntax in Luau**

A compiler that turns `.luaux` — Luau with JSX syntax — into plain `.luau`,
targeting [Vide](https://centau.github.io/vide/).

[![CI](https://github.com/luau-xml/luaux/actions/workflows/ci.yml/badge.svg)](https://github.com/luau-xml/luaux/actions/workflows/ci.yml)

</div>

## Overview

With LuauX, you can write:

```luau
return function ()
  local count = source(0)

  return (
    <Frame Size={UDim2.fromScale(1, 1)}>
      <TextLabel>Clicked {count} times</TextLabel>
      <TextButton Text="Click me" Activated={function () count(count() + 1) end} />
    </Frame>
  )
end
```

Which compiles to exactly the Vide you would have written:

```luau
return function ()
  local count = source(0)

  return (
    create("Frame")({ Size = UDim2.fromScale(1, 1),
      create("TextLabel")({ Text = function() return `Clicked {__luaux_read(count)} times` end }),
      create("TextButton")({ Text = "Click me", Activated = function() count(count() + 1) end }),
    })
  )
end
```

Same lines. Same semantics. No framework in between. 310 characters turned into 244. That's **21% less code** in just that small example. Across all of our [examples](/examples/), that's an average of **22% less code** with the highest being **32.2% less code** in [`HUD.luaux`](/examples/Hud.luaux).
> This is minus comments and whitespace (just code). Benched with [`measure_examples.py`](/scripts/measure_examples.py).

## Why LuauX

**Creating UI in Luau is flat.** The code you write is at the same indentation, and the tree has no shape. It only exists in your `Parent` assignments.

```luau
local frame = Instance.new("Frame")
frame.BackgroundColor3 = Color3.new(0, 0 ,0)
local label = Instance.new("TextLabel")
label.TextColor3 = Color3.new(1, 1, 1)
label.Parent = frame
```

A deeply nested UI and a shallow one look the exact same in code. **Vide fixed the shape.**

```luau
local frame = create "Frame", {
  BackgroundColor3 = Color3.new(0, 0 ,0),

  create "TextLabel" {
    TextColor3 = Color3.new(1, 1, 1)
  },
}
```

Nesting `create` calls inside of each other gave shape back to the tree. **But its still tables.** Properties and children share a single table, told apart only by whether the value has a key or not. Every element carries `create "..."` scaffolding. And six levels down, a column of closing braces tells you nothing about what each one closes. **LuauX gives it syntax.**

```lua
local frame = (
  <Frame
    BackgroundColor3={Color3.new(0, 0, 0)}
  >
    <TextLabel TextColor3={Color3.new(1, 1, 1)} />
  </Frame>
)
```

Tags name themselves, and name themselves again on the way out. Attributes are visibly attributes; children are visibly children. Fragments return siblings without inventing a wrapper Frame. Spread a table of shared props and override what differs. It compiles to Vide with the same reactive graph, same `source`, same `show`, nothing new to learn underneath.

## What is LuauX

LuauX is a compile step, not a library.

- **No runtime.** Vide already handles parenting, fragments, events, and reactive props and children. LuauX inlines two small helpers into the files that use them and emits no `require` at all.
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
$ mise use github:luau-xml/luaux@0.1.0
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

In a project that already uses Vide:

- `luaux init` — Scaffold a `luaux.toml`.
- `luaux build src build` — Compile `src/*.luaux` into `build/*.luau`.
- `luaux watch` — Rebuild on change, beside `rojo serve`.

Point Rojo at `build/`, keep editing `src/`, and carry on.

| Command                   | Description                     |
| ------------------------- | ------------------------------- |
| `luaux build [src] [out]` | Compile `.luaux` to `.luau`.    |
| `luaux check [src]`       | Compile without writing.        |
| `luaux watch [src] [out]` | Rebuild on change.              |
| `luaux init [dir]`        | Scaffold a `luaux.toml`.        |
| `luaux scan <path>...`    | Report where LuauX is detected. |

Paths default to `[build] in` and `out` in `luaux.toml`, so a bare `luaux build` works. Every non-source file is copied into the output unless `exclude` or `include` are specified in `luaux.toml`.

## Syntax

### Elements

Tags are the real Roblox class names, since `<div>` would mean nothing here.

| Form             | Meaning                                                      |
| ---------------- | ------------------------------------------------------------ |
| `<Frame>`        | A Roblox class, or a component bound in the file.            |
| `Size={expr}`    | Attribute taking any Luau expression.                        |
| `Name="literal"` | Attribute taking a string.                                   |
| `Visible`        | Shorthand for `Visible={true}`.                              |
| `{props}`        | Spread, in attribute position.                               |
| `Hello`          | Text child, folded into the `Text` property.                 |
| `Hi {name}`      | Interpolated text, which stays reactive.                     |
| `{items}`        | Any Luau expression, as a child.                             |
| `<>...</>`       | Fragment.                                                    |
| `<!-- ... -->`   | Comment between tags.                                        |
| `{--[[ ... ]]}`  | Comment where a child would go, as JSX writes `{/* ... */}`. |

> [!WARNING]
> `--` does **not** start a comment inside markup, where it is ordinary text.
> <br/>
> Use `<!-- ... -->` between tags, or a Luau comment inside a `{ }` hole.

```lua
<Frame {props} Size={expr} Name="literal" Visible>
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

### Components

A component is any function taking one table. Children arrive as numeric keys,
exactly as they do for a built-in class, so components and intrinsics are
interchangeable at the call site.

```lua
local function Card (props)
  return (
    <Frame BackgroundColor3={props.Color}>
      <UICorner CornerRadius={UDim.new(0, 8)} />

      {props}
    </Frame>
  )
end

local card = <Card Color={c}><TextLabel>Body</TextLabel></Card>
```

`{props}` in child position hands Vide the numeric keys, the children the caller
passed, while `props.Color` reads a named one.

### Reactivity

Reactivity is Vide's, unchanged. `Size={size}` is reactive when `size` is a source, because Vide already treats a function on a property key as one.

The single exception is interpolated text. `<TextLabel>Hi {name}</TextLabel>` becomes a thunk, since building a string from a source would otherwise stringify the function itself.

## Configuration

`luaux.toml` is optional. Every setting has a default.

```toml
[build]
in = "src"
out = "build"
exclude = ["**/*.spec.luaux"]
clean = true                  # delete outputs whose source is gone

[factory]
create = "vide.create"        # default: bare `create`

[elements]
all = "flatcase "             # PascalCase | camelCase | snake_case | flatcase
TextLabel = "text"            # an explicit entry always beats `all`

[properties]
all = "camelCase"
TextColor3 = "textColor"      # rename a property everywhere

[properties.Frame]
BackgroundColor3 = "bgColor"  # or for one class, beating the table above

[lints]
static_conditional_child = "warn"
```

> [!IMPORTANT]
> Renames are **exclusive**. Once `TextLabel = "text"` is declared, `<text>` is the spelling and `<TextLabel>` is an error. `all` behaves the same way, retiring every canonical spelling at once. One vocabulary, not two.

Roblox ships deprecated names beside modern ones that differ only by case, such
as `childAdded` beside `ChildAdded`, so a casing scheme merges those pairs. The
modern name wins. See [ADR-0001](docs/adr/0001-casing-key.md).

Keys on the left are Roblox's own spelling, while keys naming a luaux setting
follow TOML convention. See [ADR-0002](docs/adr/0002-config-keys-use-roblox-spelling.md).

## Status

Early, but real. The compiler works end to end and is covered by 176 tests
across Linux, macOS, and Windows. Two further suites run on every CI build:
golden tests that *run* the generated code against real Vide under Lune, and a
corpus sweep over the Luau and Vide sources confirming that ordinary `.luau` is
never mistaken for markup.

Expect the syntax to be stable and the tooling to keep moving.

## Credits

Built on [Vide](https://github.com/centau/vide) by centau, whose `create` sits so
close to a compile target that LuauX's entire runtime is two helper functions.

This project is not affiliated with or endorsed by Vide.

## License

MIT.
