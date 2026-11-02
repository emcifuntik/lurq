---
title: DevTools
description: Enabling DevTools, inspecting components, selecting nodes, profiling renders, and signal debugging.
---

# DevTools

DevTools is feature-gated. Without the `devtools` Cargo feature, the app does not store extra prop trees, signal values, memo values, effect metadata, or profiler snapshots for inspection.

## Enable It

Build with `lurq/devtools` and mount DevTools from the main tree. For the WGPU desktop example, also enable `winit` and `wgpu`. Add `perf_profile` for frame timings and memory counters; `devtools` does not enable that feature automatically.

```rust
use lurq::app::{App, Tree};

let mut app = App::new();
let mut tree = Tree::new();

lurq::app::devtools::load_fonts(&mut app);

tree.set_render_engine_factory(|| Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()));
tree.mount_root::<Root>(&mut app, RootProps);
tree.mount_devtools(&mut app);
```

The winit shell sees DevTools as a secondary window. It does not need inspector-specific code.

## What DevTools Shows

The current DevTools UI has three primary tabs:

| Tab | Shows |
| --- | --- |
| Components | Retained component/element tree, selected node details, props, signals, contexts, effects, shape/style rows, and optional overlay. Author-supplied `id`/`class` attributes appear first in each tree row and in the Attributes section, like browser devtools. |
| Profiler | Captured render commits, frame timings, render causes, signal changes, memo recomputes, layout status, and perf overlay stats. |
| Signals | Signal list, value, owner component, subscriber count, dependency graph, and change history. |

## Component Search

The Components tab search box accepts plain text and a small query language.

Plain terms search component/node names, keys, and text values:

```text
button
settings modal
"Save changes"
```

Multiple terms are combined with AND. A row matches only when every positive term matches and no negated term matches. Matching descendants keep their ancestors visible, and collapsed branches are expanded while the search is active.

Field prefixes narrow a term to one part of the node snapshot:

| Query | Matches |
| --- | --- |
| `tag:Button` | Full or short component/node tag. |
| `name:Button` | Alias for `tag:`. |
| `component:Button` | Alias for `tag:`. |
| `key:submit` | Component key. |
| `text:"Save changes"` | Text value. |
| `value:"Save changes"` | Alias for `text:`. |
| `kind:component` | Node kind, `component` or `element`. |
| `state:focused` | Runtime state, `focused`, `hovered`, or `active`. |

Prefix a term with `-` to exclude nodes:

```text
tag:Button -key:secondary
kind:element state:focused
text:"Save changes" -key:secondary
```

Quotes keep spaces inside one term. Backslash escapes work inside quoted terms:

```text
text:"Delete \"draft\""
```

## Inspectable Props

When `devtools` is enabled, component props must implement `DevtoolsInspectable`.

```rust
#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct InfoCardProps {
  title: &'static str,
  body: &'static str,
  accent: &'static str,
  metadata: Metadata,
}

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct Metadata {
  count: i32,
  enabled: bool,
}
```

Nested structs that also derive `DevtoolsInspectable` are shown recursively. Scalar values are shown with type and value. Mark sensitive or noisy fields with `#[devtools_ignore]`.

```rust
#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct Credentials {
  user: String,
  #[devtools_ignore]
  token: String,
}
```

Enums show their current variant.

Masked text inputs expose their displayed mask and `masked=true` in node text and shape snapshots, and [sensitive text](../styling-events/#sensitive-text) (`Text::sensitive()`) shows as `•••` with `sensitive=true`; a node screenshot covers it with a grey bar. This does not redact arbitrary application props, signals, custom annotations, or logs: hold secret values in `lurq::core::Sensitive<T>`, which DevTools shows as `•••` in signal values, history and props, or use `#[devtools_ignore]` for sensitive fields you expose to inspection.

## Inspectable Signals And Memos

With `devtools`, signal and memo values must be inspectable too:

```rust
#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct CounterState {
  count: i32,
}

let state = ctx.signal(CounterState { count: 0 });
let doubled = ctx.memo({
  let state = state.clone();
  move || state.get().count * 2
});
```

DevTools records:

- signal id,
- signal type,
- formatted value,
- owner component,
- subscriber count,
- recent value changes,
- memo recomputes.

## Overlay And Pick Mode

The Components tab can draw an overlay over the selected inspected node. The overlay is controlled from DevTools, while the main tree owns the actual overlay drawing.

Pick mode reverses selection:

1. Click the pick button in DevTools.
2. Click an item in the inspected app.
3. The matching component tree row is selected in DevTools.
4. The tree panel expands ancestors and scrolls the selected row into view.

The main tree exposes this through internal tree methods such as debug overlay selection and node picking; the shell only routes secondary-window pick requests.

## Profiler

Enable frame profiling at compile time:

```toml
lurq = { version = "0.53.0", features = ["winit", "wgpu", "devtools", "perf_profile"] }
```

The profiler tab uses frame snapshots from the tree. A commit records:

- commit index,
- duration,
- number of rendered components/nodes,
- signal count,
- whether layout recalculated,
- human-readable timestamp,
- render triggers such as signal changes and memo recomputes,
- perf overlay timings when available.

With `perf_profile`, `Tree::last_profile()` returns the latest low-level frame profile for custom tooling. Without it, the DevTools frame profile contains default values.

The shared [profiling session service](../profiling/) also exposes independent bounded captures through `Tree::profiling_handle()` and MCP's `lurq_profile_start`/`lurq_profile_read`/`lurq_profile_end`. It records completed operations and unfinished current-phase observations without UI roundtrips. This is the common model/collector for a future DevTools profiling view; the current commit/signal inspector UI does not yet consume that service. GPU timestamp metrics remain unavailable.

## Perf Overlay

The perf overlay is separate from DevTools but feeds data that DevTools can show.

```rust
tree.draw_perf_overlay();
```

The overlay samples FPS once per second. Enable `perf_profile` to include frame stage timings such as layout, resolve, glyph, acquire, upload, encode, submit, and present.

## Common Issues

If a prop type fails to compile only with `devtools`, derive or implement `DevtoolsInspectable`.

If signal values show as unknown, check that the signal type implements `DevtoolsInspectable` and that the value is stored through `ctx.signal`, `ctx.store`, or `ctx.memo`.

If DevTools does not repaint after picking while the app window is focused, check the secondary window redraw path in the shell. Secondary windows must request/present frames even when they are not the active OS window.
