---
title: App Runtime
description: App, Tree, render engine factories, windows, frame flow, profiling, and element lookup.
---

# App Runtime

The public runtime surface is split between `App`, `Tree`, and the shell.

## App

`App` is a cloneable handle to shared application services. Mounted component contexts retain those services, so timer, future and input-event renders can use `ctx.app_ref()` and `ctx.app_ref_mut()` after the caller moves or drops its handle. Clones observe changes to fonts, scale, resource configuration and storage-backend selection. `persistent_storage()` returns an owned clone of the currently selected backend handle; an already returned handle continues to refer to that backend if the App later selects another.

`App` stores services shared by a tree pass:

- glyph engine and loaded fonts,
- theme,
- optional scale override,
- optional resource loader and decoded image/SVG caches,
- menus, secondary-window requests, optional storage and Tokio runtime handles.

```rust
let mut app = lurq::app::App::new();

app.load_font_file(std::path::Path::new("assets/Inter.ttf"));
app.register_font("ui", "Inter");

#[cfg(feature = "resources")]
app.set_resource_root(std::path::PathBuf::from("assets"));
```

## Tree

`Tree` stores the retained UI state. It owns:

- root component or static root element,
- component contexts and reactive subscriptions,
- retained nodes,
- layout state and layout cache,
- render engine instance and render engine factory,
- input state,
- animation and transition engines,
- perf overlay state,
- secondary windows and optional DevTools metadata.

```rust
let mut tree = lurq::app::Tree::new();
tree.mount_root::<RootComponent>(&mut app, RootProps::default());
```

For static UI without a component root:

```rust
tree.set_root(lurq::components::Text::new("Hello"));
```

## Render Engine Factory

Use a factory, not a prebuilt render engine. Secondary windows, including DevTools, can inherit the same renderer choice by asking the factory for their own engine instance.

```rust
tree.set_render_engine_factory(|| {
  Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new())
});
```

WGPU creates its instance on the first frame. On Windows it defaults to DX12, avoiding the reported Vulkan-loader race when independent windows render and tear down concurrently. Other platforms retain the full backend selection. Select backends before the first frame if your host needs a different configuration:

```rust
use lurq::app::wgpu_render::{WgpuBackends, WgpuRenderEngine};
let engine = WgpuRenderEngine::new().with_backends(WgpuBackends::DX12);
```

Excluded backend loaders are never initialized by that engine. Explicitly opting into Vulkan on Windows also opts back into its platform loader behavior.

On Windows with `dx12`:

```rust
tree.set_render_engine_factory(|| {
  Box::new(lurq::app::dx12_render::Dx12RenderEngine::new())
});
```

## Winit Shell

`WinitWindow` creates the OS window, forwards events to `Tree`, and calls `Tree::pass`.

```rust
WinitWindow::new(app, tree)
  .with_title("lurq app")
  .with_size(1200, 800)
  .with_min_size(800, 500)
  .with_title_bar_color(lurq::node::color::Color::from_hex("#101215"))
  .with_icon(lurq::app::WindowIcon::from_rgba(vec![255, 0, 0, 255], 1, 1))
  .with_corner_radius(lurq::app::WindowCornerRadius::RoundedSmall)
  .with_decorations(false)
  .run();
```

`with_icon` sets the window icon: on Windows the title bar's small icon and the taskbar's and Alt+Tab's big icon, on X11 the window icon; macOS shows the application bundle's icon instead and ignores it.

The shell drives timers, async work, animation deadlines, and requested redraws. It waits when idle; continuous video or an installed `on_tick` callback can keep the loop polling. Use `on_tick` only when the app needs continuous custom work.

Runtime window commands requested through `ctx.window()` are applied by the winit shell. This includes closing, minimizing, fullscreen toggles, decoration toggles, the window title, native title bar color, native corner radius, window icon, moving, resizing, native platform window drag or resize requests for custom chrome, synthetic input injection, and — with the `screenshot` feature — full-window, region, and node-scoped frame capture (see [Ctx § Window](../ctx/#window)). `start_drag()` asks the shell to begin an OS-level window move, and `start_resize(direction)` asks it to begin an OS-level edge or corner resize. `stop_drag()` is available for portable shells that track drag state manually.

## Frame And Redraw Flow

`Tree` sets `needs_redraw` when state changes:

- signal/store/memo dependency updates,
- input, hover, active, focus, scroll, drag, or cursor changes,
- layout-affecting ref mutation,
- perf overlay updates,
- DevTools pick/overlay state.

The shell observes `needs_redraw`, requests a redraw, then calls `Tree::pass(app, window)`.

Animations and transitions do not set `needs_redraw`. A pass that ticks them records when they next draw something else, `Tree::next_timeline_change()`: the next step of an `Easing::Steps` span, the end of a delay, or at once for a span that changes continuously. The shell schedules the next frame for then, at most one per display refresh, and sleeps until it is due: for a continuous animation it wakes half a refresh after the last present, and the swapchain puts the frame on the next vblank. A minimized or occluded window gets no such frames until it shows again (see [Frame Pacing](../animation-transforms/#frame-pacing)).

`Tree::pass(...)` returns a `PassReport` describing whether the pass was required, whether it rendered, whether it reused a cached render list, whether layout updated or recalculated, and which runtime reasons contributed to the pass. `WinitWindow::on_paint` receives the same report after a presented frame:

```rust
WinitWindow::new(app, tree)
  .on_paint(|tree, delta, report| {
    if report.layout_recalculated {
      eprintln!("layout recalculated after {:?}", delta);
    }

    let _ = tree.frame_count();
  })
  .run();
```

## Input Dispatch

Pointer input is resolved against the latest layout and hit-tested in visual coordinates. A press, a release or a wheel that comes before any frame laid out what an earlier event rebuilt (a hover that opened a box or pushed a row away) lays it out first, so it lands on what the next frame shows. The runtime tracks hover, active, focus, drag, scroll, cursor, text selection, and text click counts across retained nodes. Mouse handlers run before pointer defaults such as input focus, text selection, form submit, and outside-click overlay dismissal, so handlers can call `event.prevent_default()` to block those defaults.

User keyboard handlers run before built-in keyboard defaults. Built-in text behavior handles caret movement, selection replacement, undo/redo, and clipboard shortcuts when the `clipboard` feature is enabled. Call `event.prevent_default()` from an `on_key_down` handler to block those built-in defaults for that key. `TextInput::on_input` runs inside the text-edit default, before the edit is applied, and can mutate the input signal or prevent that edit. Scroll handlers also run before default scroll movement.

## Sizing And Scale

The shell updates the tree from the window before each pass:

```rust
tree.set_scale_factor(window.scale_factor() as f32);
tree.resize(size.width, size.height);
```

Tests can override layout constraints directly:

```rust
tree.set_layout_constraints_override(Some(lurq::layout::Constraints::tight(
  lurq::layout::Size::new(800.0, 600.0),
)));
```

## Element Lookup

Use `find_element` when integration code or tests need a computed rect.

Predicate-based `find_element` and `find_element_mut` use the last completed layout and do not flush pending component renders. Run a pass first when state has changed. The by-ID/by-class lookups below flush dirty subtrees themselves.

```rust
let found = tree.find_element(|el| el.text_content() == Some("Save"));

if let Some(found) = found {
  let bounds = found.bounds();
  println!("{}x{} at {}, {}", bounds.width, bounds.height, bounds.x, bounds.y);
}
```

Use `find_element_mut` only for imperative layout overrides. Declarative component state is the normal path.

Nodes tagged with `.id("...")` / `.class("...")` support browser-style lookup, mutation, and typed interaction:

```rust
let save = tree.get_element_by_id("save");                 // first match in tree order
let rows = tree.get_elements_by_class_name("row");         // all matches in tree order

let mut handle = tree.get_element_by_id_mut("save").unwrap();
handle.click();                                            // DOM el.click() semantics
handle.set_background("#ef4444");                          // transient direct mutation

tree.get_element_by_id_mut("email").unwrap()
  .as_text_input().unwrap()
  .set_value("ada@example.com");                           // signal-backed, no on_input
```

See [Retained Nodes](../retained_nodes/#ids-and-classes) for the full contract (transiency, duplicate ids, pre-layout behavior).

## Perf Overlay

The runtime has a built-in frame perf overlay.

```rust
tree.draw_perf_overlay();
```

Enable the `perf_profile` Cargo feature for frame timing and memory instrumentation. It is independent of `devtools`; there is no runtime `App::set_profiling_enabled` switch. With that feature, profiling data is available through:

```rust
let profile = tree.last_profile();
```

The profile records high-level timings such as layout, resolve, glyph, upload, encode, submit, present, and memory counters used by DevTools.

## Performance Notes

The plain-text pipeline reuses shaped paragraphs, wrapped layouts, and caret geometry. The current cache has a 48 MiB accounted budget per glyph engine and a 64-entry limit. See [Text Pipeline Optimization](../text-pipeline-optimization/) for implementation details and dated measurements; profile the current app and renderer before choosing the next optimization.

## DevTools Window

With `devtools` enabled:

```rust
lurq::app::devtools::load_fonts(&mut app);
tree.mount_devtools(&mut app);
```

DevTools is represented as a secondary tree. The shell does not need special inspector logic; it only manages secondary windows and renders each tree.
