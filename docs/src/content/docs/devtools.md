---
title: DevTools
description: Enabling DevTools, inspecting components, selecting nodes, profiling renders, and signal debugging.
---

# DevTools

DevTools is feature-gated. Without the `devtools` Cargo feature, the app does not store extra prop trees, signal values, memo values, effect metadata, or profiler snapshots for inspection.

## Enable It

Build with `lurq/devtools` and mount DevTools from the main tree. For the WGPU desktop example, also enable `winit` and `wgpu`. `devtools` enables `perf_profile`, so a DevTools build has frame timings, memory counters and [profiling sessions](../profiling/), and pays the profiler's idle cost on every pass (see [Profiler](#profiler)).

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
| Profiler | A recording of every pass of the inspected windows between Start and Stop: a frame timeline with input and UI-update markers, FPS, and each frame's phases and full frame profile. |
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

`devtools` turns on frame profiling (`perf_profile`) at compile time:

```toml
lurq = { version = "0.58.5", features = ["winit", "wgpu", "devtools"] }
```

The Profiler tab records on the inspected tree's [profiling session service](../profiling/), the one `Tree::profiling_handle()` returns:

1. **Start recording** starts a [recording session](../profiling/#two-session-modes) on it. Nothing is recorded before you press it, and the tab copies nothing while DevTools renders.
2. While it runs, the top bar shows how long it has run, the memory its samples hold and how many it took, reread four times a second. A recording keeps every sample; at its 1 GiB ceiling it stops taking more and says so.
3. **Stop recording** ends the session. The recording holds every pass of the inspected windows (the main window and its other secondary windows) that began after Start and finished before Stop, each as its own frame: none is skipped, merged with an identical one, or capped to a fixed count. Passes already running at Start are left out and counted.

DevTools' own window is not recorded. Turn on **Include DevTools** before Start to record it too. **Clear** forgets the recording, and ends one still running.

After Stop the tab shows:

- **Recording**: the whole recording in equal slices, each drawn as its longest pass. Press and drag over it, or shift-click to extend from the last press, to select a time range; **Whole recording** selects all of it. A recording of more than 600 passes opens on its last 600.
- **Frames**: the selected range on a time axis. Each pass is a bar at its start time, as tall as its duration, against a line at the 16.7 ms frame budget (one 60 Hz refresh): green within the budget, yellow over it, red over twice it, faded when the pass presented no frame. Below it, the **GPU** track draws an orange bar at each pass that has [GPU time](../profiling/#coverage-and-units) (its UI and Canvas passes, measured with wgpu timestamp queries), scaled to one frame budget or its longest bar, and counts the passes that have one; when none was measured, it says why (the adapter has no timestamp queries, the native DX12 renderer, or no result arrived). Two lanes below mark where input dispatches and UI updates (root rebuilds and subtree refreshes) began. A range with more than 600 passes asks for a narrower one instead of drawing them.
- **FPS**: from the intervals between frames presented in the range, for the window that presented most. Intervals over 250 ms are the app idling and are left out, so FPS is blank (`–`) when the app was idle, and does not drop because the app paused between bursts.
- **Frame details** for the selected bar (the slowest pass of the range until you click one): its start, duration, GPU time and window (with why a presented frame has no GPU time); its phases from the recorded spans, nested by depth and placed on the pass's own time axis; and everything its sample holds: the pass timings and flags, its GPU time split into UI and Canvas, Canvas text work, and its frame profile's timings, counts, render, Canvas, Canvas asset upload, text and memory figures. GPU time arrives one or more frames after its pass, so the last frames before Stop may have none.

The recording holds only phase names, times and counts: no text, prop or signal values, input values or paths. The Signals tab records signal changes.

`Tree::last_profile()` returns the latest low-level frame profile for custom tooling. In a build with neither `devtools` nor `perf_profile` it holds default values.

Profiling costs a DevTools build something on every pass, even with no session open: the pass timers run and each coarse phase is published so a session started during a slow pass can see it. Measured with `cargo bench -p lurq --bench profiler_idle` (Windows, release, headless pass): a pass over 40 unchanged rows takes about 0.31–0.34 µs without `perf_profile` and 0.52–0.61 µs with it and no session, so about 0.2 µs more per pass; an open session adds about 0.15–0.2 µs more. A pass that rebuilds a component and lays out the rows again takes 20–28 µs in every configuration, where the profiler's share is within the run-to-run noise. See [profiling](../profiling/#overhead) for details.

The same service also gives independent bounded captures through `Tree::profiling_handle()` and MCP's `lurq_profile_start`/`lurq_profile_read`/`lurq_profile_end`, which run alongside a DevTools recording. GPU time is measured on the wgpu renderer only, while some session is active; the native DX12 renderer reports it unavailable.

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
