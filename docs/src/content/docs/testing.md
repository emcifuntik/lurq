---
title: Testing
description: How lurq tests layout, rendering, input, reactivity, resources, and DevTools behavior.
---

# Testing

The repo has focused Rust tests under `crates/lurq/tests`. They are the best reference for expected behavior when changing runtime internals.

## Run Tests

```powershell
cargo test -p lurq --features resources
```

With feature combinations:

```powershell
cargo test -p lurq --features "image svg resources devtools"
cargo test -p lurq --features "winit wgpu image svg resources devtools clipboard"
```

The combinations above do not enable every subsystem. Check feature-specific behavior explicitly:

```powershell
cargo test -p lurq --features query,tokio --test query_tests
cargo test -p lurq --features form,router,persistent_storage,i18n --lib --tests
cargo test -p lurq --all-features --doc
cargo test -p lurq --features canvas --test canvas_tests
cargo check --workspace --all-features --all-targets --locked
```

The publish workflow in `.github/workflows/publish-crates.yml` defines release validation. Native GPU and window checks need the matching platform and a usable graphics environment; see [Canvas 2D](../canvas/#examples-and-checks) and [Window lifecycle](../window-lifecycle-menus/#mcp-and-verification). Ignored hardware tests are not run by an ordinary `cargo test`.

On Windows, the render readback tests draw into a hidden window of their own and compare both native backends with CSS blending and with the box-shadow formula; the box-shadow capture check runs a themed scene through layout and both backends and requires identical captures:

```powershell
cargo test -p lurq --features wgpu,dx12,raster,screenshot --lib readback -- --ignored --test-threads=1
cargo run -p lurq --example box_shadow_capture_check --features screenshot,wgpu,dx12
```

Run one area:

```powershell
cargo test -p lurq layout::padding
cargo test -p lurq reactivity::signal
cargo test -p lurq dnd::target_tracking
```

## Layout Tests

Layout tests usually create a `Tree`, install a static root, and run a layout pass with explicit constraints.

```rust
use lurq::{
  app::Tree,
  layout::{Constraints, Size},
};

let mut tree = Tree::new();
tree.set_root(lurq::components::Spacer::new().size(100.0, 50.0));

let result = tree
  .pass_layout(Constraints::loose(Size::new(400.0, 400.0)))
  .unwrap();

assert_eq!(result.size.width, 100.0);
assert_eq!(result.size.height, 50.0);
```

This snippet runs inside the layout test modules, where `use super::PassLayoutExt;` imports the helper from `tests/layout/mod.rs`. `pass_layout` is not a public `Tree` method. Production app code normally lets `Tree::pass` drive layout.

## Render Snapshot Tests

`tests/support.rs` defines a capturing render engine that records the generated render list.

Use this style when testing visual output without opening a real GPU window:

```rust
let mut tree = Tree::new();
tree.set_root(lurq::components::Rect::new(100.0, 50.0).background("#22c55e"));

let snapshot = support::render_pass(&mut tree);
assert_eq!(snapshot.rects.len(), 1);
assert_eq!(snapshot.rects[0].width, 100.0);
```

This is useful for border, radius, opacity, image order, SVG order, and render-list regressions. `snapshot.layers` lists the frame's opacity layers (see [Opacity](../styling-events/#opacity)); rects and glyphs record their `order`, so a test can tell which layer holds them. The pixels of layers are checked through both render engines by the `opacity_layer_readback` tests:

```powershell
cargo test -p lurq --features wgpu,dx12,screenshot --lib opacity_layer_readback -- --ignored --test-threads=1
```

## Reactivity Tests

Reactivity tests validate state containers independently and through component dirty tracking.

```rust
let signal = lurq::core::Signal::new(0);
signal.update(|value| *value += 1);
assert_eq!(signal.get(), 1);
```

Dirty tracking tests verify that:

- child signal updates do not rerender clean parents,
- parent signal updates do not rerender clean children unnecessarily,
- passed signals mark children dirty when the child reads them,
- prop changes rerender the affected child.

## Input And DnD Tests

Input tests drive the tree directly:

```rust
tree.mouse_move(20.0, 20.0);
tree.mouse_down(20.0, 20.0, MouseButton::Left);
tree.mouse_up(20.0, 20.0, MouseButton::Left);
tree.scroll(20.0, 20.0, 0.0, -120.0, ScrollPhase::Scroll);
tree.key_down("a".into(), "KeyA".into(), false, false, false);
```

Click handlers run from a matching pointer down/up pair; tests should not inject a separate click event.

Use direct tree input for deterministic hover, active, focus, scroll, text input, selectable text, slider, checkbox, drag, and drop behavior.

Text input tests cover caret placement, Unicode-safe deletion, keyboard selection, multiline movement, undo/redo, and double/triple-click selection. Selectable text tests cover drag ranges, word and line selection, and transformed visual-coordinate hit testing.

## Pastes, File Drags And Runtime Images

A test pastes without the OS clipboard: `tree.paste(content)` delivers the `PasteEvent` and inserts the text, and `synthetic_input::apply(&mut tree, &SyntheticInput::paste(content))` does the same marked as synthetic input, as an MCP client's `paste` action does. The content is built in the test:

```rust
use lurq::app::{
  events::{ClipboardContent, ClipboardImage},
  synthetic_input::{self, SyntheticInput},
};

let image = ClipboardImage::from_rgba(2, 1, vec![255; 8]).unwrap();
synthetic_input::apply(
  &mut tree,
  &SyntheticInput::paste(ClipboardContent::image(image).with_files([PathBuf::from("C:/shots/a.png")])),
);
```

A synthetic `Ctrl+V` (`SyntheticInput::key_down("v")` with `ctrl`) is refused: it never reads the person's clipboard, and no paste event fires. A test of a person's `Ctrl+V` installs a clipboard of its own with `tree.set_clipboard_reader(Some(Arc::new(reader)))`, a `lurq::app::ClipboardReader` with `read_text()` (what a paste no handler looks at reads) and `read()` (everything, for a paste an `on_paste` handler looks at); the OS clipboard is never touched.

Files are dragged with `tree.file_drag_enter(paths, x, y)`, `file_drag_over(x, y)`, `file_drag_leave()` and `file_drop(paths, x, y)` (physical pixels, as the pointer methods take), or their synthetic forms `SyntheticInput::file_drag_enter(paths, x, y)`, `file_drag_over(x, y)`, `file_drag_leave()` and `file_drop(paths, x, y)`. `DraggedPaths::from_paths(paths)` looks each path up once to tell folders from files; `DraggedPaths::new([DroppedPath::with_kind(path, true)])` marks a folder that does not exist. A drop without an enter first enters at the same point.

```rust
use lurq::app::events::DraggedPaths;

let (x, y) = tree.get_element_by_id_mut("composer").unwrap().bounds().unwrap().center();
tree.file_drag_enter(DraggedPaths::from_paths([PathBuf::from("C:/shots/a.png")]), x, y);
assert_eq!(tree.file_drag_accepted(), Some(true));
assert!(tree.file_drop(DraggedPaths::from_paths([PathBuf::from("C:/shots/a.png")]), x, y));
```

A runtime image (`Image::from_bytes`) decodes on a worker thread: the first pass shows the placeholder, `lurq::images::wait_for_decoded_images(timeout)` blocks until no decode is pending, and the next pass shows the image. The cache is process-wide, so tests in one binary share it: give each test its own key (`cache_key`) when it asserts on `decoded_images().completed()`, and use `DecodedImages::new()` for a cache of the test's own.

## Headless Passes And Focus

`tree.pass_headless(&mut app)` lays out the tree, overlays and modals included, without a window handle or render engine, so input, hit testing, focus, and Tab work in tests without the `unsafe` test surface. `tree.focused_element()` returns the focused control:

```rust
tree.set_root(Column::new().child(Button::new("Save").id("save").tab_index(0)));
tree.pass_headless(&mut App::new());
tree.key_down("Tab".into(), "Tab".into(), false, false, false);
assert_eq!(tree.focused_element().and_then(|element| element.id()), Some("save"));
```

Use the render snapshot helpers when a test asserts on drawn output. See [Testing Focus](../focus-navigation/#testing-focus).

## Painted Output Without A Window

Some of what an element paints is not in the element tree: a control's own fill, border or shadow, a text input's caret and selection. After any pass, `tree.painted_quads()` returns what the last layout paints, in paint order, as `lurq::layout::quad::Quad`s at absolute logical positions, without a window or render engine, so an application's own tests can check it after `pass_headless`:

```rust
use lurq::{
  app::{App, Tree, theme::PaletteColor},
  components::Select,
  core::{ElementRef, Signal},
  layout::quad::QuadContent,
  node::color::Color,
};

let surface = Color::from_hex("#1e293b");
let select = ElementRef::new();
let mut app = App::new();
app.theme().set_palette_color(PaletteColor::SurfaceInput, surface);
let mut tree = Tree::new();
tree.set_root(Select::new(Signal::new(1)).options([(1, "One")]).ref_element(select.clone()));
tree.pass_headless(&mut app);

let bounds = select.bounds();
let trigger_fill = tree.painted_quads().into_iter().find_map(|quad| match quad.content {
  QuadContent::Rect { color, .. } if (quad.x, quad.y, quad.width) == (bounds.x, bounds.y, bounds.width) => Some(color),
  _ => None,
});
assert_eq!(trigger_fill, Some(surface));
```

Quads are the draw list before text shaping and batching; the render snapshot helpers above show the batched rects and glyphs a render engine receives.

## Input Methods

Drive input method composition through `Tree::ime`, as the winit shell does with the platform's events, and check the result through the focused input:

```rust
use lurq::app::events::ImeEvent;

tree.get_element_by_id_mut("draft").unwrap().focus();
tree.ime(ImeEvent::Preedit { text: "にほん".into(), cursor: Some((9, 9)) });
assert!(tree.is_composing());
let draft = tree.get_element_by_id_mut("draft").and_then(|element| element.as_text_input()).unwrap();
assert_eq!(draft.composition().as_deref(), Some("にほん"));

// Windows reports the Enter that confirms as a "Process" press; it reaches no handler.
tree.key_down("Process".into(), "Enter".into(), false, false, false);
tree.ime(ImeEvent::Preedit { text: String::new(), cursor: None });
tree.ime(ImeEvent::Commit("日本".into()));
```

`tests/input/text_input/ime.rs` covers the preedit display, commits, cancellation, the `Process` keys withheld and the keys passed on during a composition on both platforms' event orders, stale compositions, masked inputs and the candidate window area. What an input method does with the keys is up to the platform and is not exercised headlessly.

## Element Lookup And Typed Interaction

Tag nodes with `.id("...")` in the tree under test, then address them directly instead of writing predicates:

```rust
tree.set_root(
  Column::new()
    .child(TextInput::new(value.clone()).id("email"))
    .child(Button::new("Save").id("save").on_click(on_save)),
);
run_pass(&mut tree);

// Signal-backed value write; does NOT fire on_input (DOM `el.value = x`).
tree.get_element_by_id_mut("email").unwrap()
  .as_text_input().unwrap()
  .set_value("ada@example.com");

// DOM el.click(): fires the node's own on_click without hit-testing.
tree.get_element_by_id_mut("save").unwrap().click();
```

`click()` works even when the node is occluded. For pointer-fidelity coverage (hover, capture, hit testing) keep driving `tree.mouse_down` / `tree.mouse_up`, composing coordinates from the handle's `bounds().center()`.

## DevTools Tests

DevTools tests construct snapshots from the tree and assert collected metadata:

- tag names for built-ins and user components,
- recursive props from `DevtoolsInspectable`,
- signal/memo value history,
- effect metadata,
- context metadata,
- overlay selection behavior,
- pick mode and scroll-into behavior.

Run them with:

```powershell
cargo test -p lurq --features devtools
```

If a test only fails with `devtools`, check trait bounds first. Props, signal values, stores, and memo outputs may need `DevtoolsInspectable`.

## Benchmarks

Benchmarks live in `crates/lurq/benches`:

```powershell
cargo bench -p lurq
```

Current benches cover layout, tree build, render-list generation, and the Markdown-backed text pipeline. The text benchmark requires its feature explicitly:

```powershell
cargo bench -p lurq --bench text_pipeline --features markdown
```

Its fixtures include the root `README.md`, so documentation changes can alter workload size. Compare the same fixture and source/toolchain metadata before interpreting historical numbers. Use benchmarks when changing layout caching, smart relayout, retained-node reconciliation, render command generation, or text rasterization.

`text_chat` draws a chat screen of short texts (a channel list, a member list, a panel of messages) one scripted frame at a time: cold, idle, hover, scroll, channel switch, resize, typing and a scale factor change. It needs no feature; built with `perf_profile,counting_allocator` and run with `LURQ_CHAT_METRICS=<csv>` it writes each frame's phases, allocations and cache sizes instead, and `examples/text_chat_gpu_probe.rs` runs the same frames through the wgpu and DX12 renderers. See [Chat Screen Text Optimization](/lurq/text-chat-optimization/).

```powershell
cargo bench -p lurq --bench text_chat
```

Text pipeline optimization notes and benchmark history live in [Text Pipeline Optimization](/lurq/text-pipeline-optimization/).
