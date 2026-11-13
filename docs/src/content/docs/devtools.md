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
| Profiler | A recording of every pass of the inspected windows between Start and Stop: a frame timeline with counter tracks (FPS, nodes, draw calls, memory, allocations) and input and UI-update markers, an opt-in filmstrip of frame screenshots, FPS, and each frame's phases, counters and full frame profile. |
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
lurq = { version = "0.61.0", features = ["winit", "wgpu", "devtools"] }
```

The Profiler tab records on the inspected tree's [profiling session service](../profiling/), the one `Tree::profiling_handle()` returns:

The tab's buttons are in a toolbar of its own under the tab bar; it wraps onto more rows in a narrow window, and the recording's status and the last error are lines under it.

1. **Start recording** starts a [recording session](../profiling/#two-session-modes) on it. Nothing is recorded before you press it, and the tab copies nothing while DevTools renders.
2. While it runs, the line under the tab's toolbar shows how long it has run, the memory its samples hold and how many it took, reread four times a second. A recording keeps every sample; at its 1 GiB ceiling it stops taking more and says so.
3. **Stop recording** ends the session. The recording holds every pass of the inspected windows (the main window and its other secondary windows) that began after Start and finished before Stop, each as its own frame: none is skipped, merged with an identical one, or capped to a fixed count. Passes already running at Start are left out and counted.

DevTools' own window is not recorded. Turn on **Include DevTools** before Start to record it too. Turn on **Components** before Start for a [detailed recording](../profiling/#detailed-recordings): each component render and `after_layout` hook becomes a span named by the component's type, each pass counts the nodes it laid out again per component, and each UI update keeps what made its components render. **Clear** forgets the recording, and ends one still running.

Turn on **Screenshots** before Start to record [screenshots of presented frames](../profiling/#frame-screenshots) as well: at most four a second, each fitted into 320 x 200 pixels, with [sensitive text](../styling-events/#sensitive-text) covered by a grey bar as in a node screenshot. It is off by default, and then no frame is read back. Each screenshot copies its frame on the GPU; the readback and scaling run off the frame's thread, but the frame that made the copy is slower, so it is marked in the timeline. Screenshots count toward the 1 GiB ceiling (about 19 minutes of a 16:9 window rendering at the defaults, less the samples' share), and the status line counts them.

With a recording shown, **Save** writes it to a lurq recording file and **Export for Firefox Profiler** to a profile that [profiler.firefox.com](https://profiler.firefox.com) opens with its call tree, flame graph and frame/input markers. **Load** opens a file Save wrote in place of the recording, with the same timeline, flame chart and tables; a file it cannot read leaves the recording shown and says why in a line under the toolbar. Each asks for its file in the system's file dialog, one dialog at a time; iOS has none, so there the tab has no file buttons and an app can call the [recording file functions](../profiling/#recording-files) itself.

After Stop the tab shows:

- **Recording**: the whole recording in equal slices, each drawn as its longest pass. Press and drag over it, or shift-click to extend from the last press, to select a time range; **Whole recording** selects all of it. A recording of more than 600 passes opens on its last 600.
- **Frames**: the selected range on a time axis. Each pass is a bar at its start time, as tall as its duration, against a line at the 16.7 ms frame budget (one 60 Hz refresh): green within the budget, yellow over it, red over twice it, faded when the pass presented no frame. The header counts the passes over budget and the frames that missed a vsync. With screenshots, a **filmstrip** above the bars splits the range into 16 equal parts on the same time axis, each showing the first screenshot whose frame's pass starts in it; hovering a thumbnail previews it in the frame details, a click selects its pass. The rows below the bars mark, at the same times:
  - **missed vsync**, in red: the frames that came a refresh or more late inside a run of frames (an animation, a drag, a scroll). A frame after the app idled is never marked: the renderer draws on demand, so a gap is not a dropped frame. See [present timing](../profiling/#input-latency-presented-frames-and-pass-reasons) for how a run is told apart and what the backends report (CPU present times only).
  - **captured**, in teal, with screenshots: the frames that copied a screenshot. Their render timings include the copy; do not read them as the app's.
  - **GPU**: an orange bar at each pass that has [GPU time](../profiling/#coverage-and-units) (its UI and Canvas passes, measured with wgpu timestamp queries), scaled to one frame budget or its longest bar; the label counts the passes that have one. When none was measured, it says why (the adapter has no timestamp queries, the native DX12 renderer, or no result arrived).
  - **Counter tracks**: one bar per pass at its start, scaled to the range's highest value, for **fps** (from the interval since its window's previous presented frame, up to 250 ms), **nodes**, **draw calls** (of a presented frame's render list), **memory** (the runtime estimate taken at the end of the pass) and **allocs** (allocations per pass). Each label shows the selected pass's value, or the range's highest. A click on a counter bar selects its pass, as on the frame bars. The allocation track says **unavailable** unless the app installs [`CountingAllocator`](../profiling/#counting-allocations).
  - **input**: where the shell received each input event, coloured by its input-to-present latency (from receiving the event to the present of the first frame after it): green within 50 ms, yellow within 100 ms, red above; blue for input that no frame presented (it changed nothing on screen). The label gives the count and the longest latency.
  - **ui update**: where root rebuilds and subtree refreshes began.

  A range with more than 600 passes asks for a narrower one instead of drawing them.
- **FPS**: from the intervals between frames presented in the range, for the window that presented most. Intervals over 250 ms are the app idling and are left out, so FPS is blank (`–`) when the app was idle, and does not drop because the app paused between bursts.
- **Frame details** for the selected bar (the slowest pass of the range until you click one): with screenshots, first a preview of the thumbnail under the pointer or else of what the pass's window showed (its frame's screenshot, or the window's last one before it); its start, duration, GPU time, why the pass ran (its reasons, such as `redraw_requested` or `timeline_active`), window (with why a presented frame has no GPU time), whether it copied a screenshot and, for a late frame, the vsyncs it missed; its phases from the recorded spans, nested by depth and placed on the pass's own time axis; the inputs this frame presented, each with its interaction id, queue delay, dispatch time and input-to-present latency; and everything its sample holds: the pass timings and flags, its GPU time split into UI and Canvas, its present timing (interval, refresh interval, whether it continued a run), its counters (nodes, memory, cache sizes and allocations), Canvas text work, and its frame profile's timings, counts, render (draw calls included), Canvas, Canvas asset upload, text and memory figures. GPU time arrives one or more frames after its pass, so the last frames before Stop may have none. In a detailed recording the phases include the components by name, **Render causes** lists what made each component render in the UI updates that led to the pass (those of its window since its previous pass, during input handling or inside the pass), such as `signal u32`, `props` or `memo u64 via signal u64`, taken from the notifications as they happened, and **Component layouts** the nodes the pass laid out again per component and reason.

Under the timeline, three views analyse the passes that start in the selected range (**Range**), or the selected pass alone (**Selected frame**):

- **Flame chart**: every span to scale on a time axis. The passes' toolkit phases are on the UI track, each pass a root with its phases nested below it; [application scopes](../profiling/#synchronous-application-scopes) are on a track per lane (UI lane, worker lane), nested by their parent. The mouse wheel zooms around the pointer, a horizontal wheel or a drag pans, **Reset zoom** shows the whole range again. Hovering a span shows its name, total and self time and the pass it belongs to; clicking selects it (and its pass in the timeline), and **Zoom to selection** fits the view to it. Spans narrower than 3 px are merged into grey blocks until you zoom in, so a range of thousands of passes draws a few hundred elements. A recording keeps its last 128 application scopes; the bar says how many earlier ones it no longer holds.
- **Call tree**: the spans of one track (Frames, or an application lane) merged by their path from the root, top-down, with total time, self time (total less its children), count, and both times as a percent of the track's total. For Frames that total is the pass time of the passes analysed. Rows open to the passes' phases; click a row to open or close it, click a column title to sort by it.
- **Bottom-up**: a row per span name with its self time and its total time, and under it the names that called it, outward. A phase inside another of its name counts its total once. The rows' self times add up to the same total as the call tree.

In a detailed recording, components show in all three views by their type name: under `ui_rebuild` each component that rendered (with the components it mounted below it), and under `component_after_layout` each component's hook (with its descendants' hooks below it), so the call tree merges the renders of one component type on one path.

Phases recorded on another thread can overlap: each span is trimmed to its parent and to the end of the sibling before it, so no time counts twice. The aggregation is `lurq::app::profiler::SpanForest` (`forest_of`, `call_tree`, `bottom_up`), plain data that runs on any `ProfileReport`.

Measured on a synthetic recording of 2000 passes with 62,000 spans and the analysis over all of it (`a_recording_of_2000_frames_stays_interactive`, Windows, headless pass of the whole tab, no GPU): opening it takes about 75 ms; in a release build, a hover takes about 1 ms (at most 3 ms), a zoom step 0.7 ms (at most 10 ms), a pan step 0.2 ms, and the first call tree or bottom-up table 8–9 ms, then under 1 ms; a debug build takes 3, 1.6 and 1 ms per hover, zoom and pan step.

The recording holds only phase names, times and counts, and in a detailed recording the static type names of components and of the signals, memos, stores and contexts that made them render: type names, never values. No text, prop or signal values, keys, element or signal ids, input values, pointer positions or paths. With **Screenshots** on it also holds the pixels of the frames it took, as the window showed them except for sensitive text, which is covered. The Signals tab records signal changes.

`Tree::last_profile()` returns the latest low-level frame profile for custom tooling. In a build with neither `devtools` nor `perf_profile` it holds default values.

Profiling costs a DevTools build something on every pass, even with no session open: the pass timers run and each coarse phase is published so a session started during a slow pass can see it. Measured with `cargo bench -p lurq --bench profiler_idle` (Windows, release, headless pass): a pass over 40 unchanged rows takes about 0.31–0.34 µs without `perf_profile` and 0.52–0.61 µs with it and no session, so about 0.2–0.35 µs more per pass. While a session is recording, each captured pass also gathers its spans and counters: about 3.6 µs more on that scene, and about 19 µs more on a pass that rebuilds a component and lays out the rows again (31 µs without `perf_profile`, 34 µs with it and no session, 53 µs with a session). See [profiling](../profiling/#overhead) for details.

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
