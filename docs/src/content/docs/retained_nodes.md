---
title: Runtime And Retained Tree
description: Retained tree behavior, dirty tracking, layout caching, element lookup, and redraw flow.
---

# Runtime And Retained Tree

This page is the lower-level reference for `Tree`. Start with [Mental Model](../mental-model/) and [App Runtime](../app-runtime/) first.

## Public Types

`lurq` uses these runtime-facing types:

- `App`: shared fonts, theme, resources, storage, menus, and optional Tokio runtime.
- `Tree`: retained UI tree, components, layout, input, render engine, DevTools, profiling.
- `Element`: public erased UI value returned from components.
- `Node`: crate-private retained layout/render/input node.
- `Ctx`: per-component retained context.

There is no public `Runtime` type. Older notes may use that name; read it as `Tree`.

## Root Modes

Static root:

```rust
let mut tree = lurq::app::Tree::new();
tree.set_root(lurq::components::Text::new("static"));
```

Component root:

```rust
tree.mount_root::<RootComponent>(&mut app, RootProps);
```

Root props can be updated without replacing the component type:

```rust
tree.update_root_props::<RootComponent>(RootProps { enabled: true });
```

Call `tree.rebuild()` when code needs to force a root component render.

## Render Engine Ownership

Use `set_render_engine_factory`.

```rust
tree.set_render_engine_factory(|| Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()));
```

The factory creates the render engine for the main tree and any secondary trees. This is required because each OS window needs its own renderer state.

## Dirty Tracking

Dirty tracking happens at component context boundaries.

State changes mark the owning context dirty:

- `Signal::set` and `Signal::update`,
- `Store::set` and `Store::update`,
- `Lens::set` and `Lens::update`,
- memo output changes,
- reactive context changes,
- imperative element-ref layout mutations.

Before layout, rendering, hit testing, or by-ID/by-class lookup, `Tree` rebuilds dirty component subtrees. Clean component subtrees keep their previous retained output. Predicate-based `find_element` and `find_element_mut` use the last completed layout instead; run a pass before relying on updated geometry.

Dirty tracking does not happen at individual element-builder calls. If a component rerenders, all plain elements built by that component's `render` function are rebuilt as fresh `Node` descriptions. Runtime state, node IDs, and layout caches are then preserved where the new and old nodes match.

Mounted child components are different: each mount has a retained component slot. A dirty parent still calls the mount site, but the child component's `render` is skipped when its identity, props, context, slot children, and own dirty state are unchanged. In that case the slot returns the previous rendered output cloned for reuse.

## Prop Reconciliation

Mounted components are reused when identity matches.

For unkeyed mounts:

```rust
ctx.mount::<Child>(props)
```

Identity is component type plus slot position.

For keyed mounts:

```rust
ctx.mount_keyed::<Child>("stable-key", props)
```

Identity is component type plus key. Use keyed mounts for lists that can reorder.

If new props are unequal to stored props, the child context is marked dirty.

## Retained Node IDs

Every retained node has a `NodeId`. IDs are assigned by the tree's monotonic generator and are never recycled after removal. Compatible nodes preserve their IDs across reconciliation; keys, component slots, explicit IDs, retained refs, and input and select value signals help match controls across sibling changes.

Node IDs are used for:

- event targets,
- hit testing,
- element lookup,
- DevTools tree rows,
- overlay selection,
- drag/drop source and target IDs.

## Layout Cache

Layout is cached on retained nodes. Cache invalidation happens when layout-affecting state changes:

- size/frame constraints,
- padding,
- flex parameters,
- scroll state,
- text input value,
- text selection/caret runtime state,
- style state that affects layout,
- element-ref rect overrides,
- root resize or scale changes.

Smart relayout should stop as soon as an ancestor can still contain the changed child size. Full-tree invalidation should be reserved for changes that can affect ancestors, hit testing, or global viewport assumptions.

## Element Lookup

`find_element` gives read access to the current tree plus computed bounds.

```rust
let found = tree.find_element(|element| {
  element.text_content() == Some("Submit")
});

if let Some(found) = found {
  let rect = found.bounds();
  println!("{}x{}", rect.width, rect.height);
}
```

`find_element_mut` gives a mutable element ref for imperative rect overrides:

```rust
let handle = tree
  .find_element_mut(|element| element.text_content() == Some("Panel"))
  .unwrap();

handle.set_relative_bounds(12.0, 24.0, 300.0, 180.0);
```

Use this sparingly. Declarative component state should remain the default way to move UI.

### Ids and Classes

Every builder accepts HTML-like `id` and `class` attributes for lookup. Explicit IDs also participate in reconciliation and state preservation: keep them stable for controls that move among siblings. Classes do not participate in identity. Neither attribute applies styling; there is no selector engine.

```rust
Column::new()
  .child(Text::new("Title").id("headline"))
  .child(Rect::new(24.0, 24.0).class("icon").classes(["muted", "small"]))
```

Browser-style lookup on `Tree`:

```rust
// First match in tree order (duplicate ids warn in debug builds).
let headline = tree.get_element_by_id("headline");

// All matches in tree order.
let icons = tree.get_elements_by_class_name("icon");

// Accessors also work inside find_element predicates.
let found = tree.find_element(|element| element.has_class("muted"));
```

Unlike `find_element`, the by-id/by-class lookups walk the live tree directly and work before the first layout pass. Pending component re-renders are flushed first, so results reflect the latest state.

### Mutation Handles

`get_element_by_id_mut` returns a short-lived `ElementHandle` for direct mutation:

```rust
let mut card = tree.get_element_by_id_mut("card").unwrap();
card.add_class("selected");
card.set_background("#ef4444");
card.set_opacity(0.5);
let center = card.bounds().unwrap().center();
```

Mutations write directly into the live node. In trees with a static root they are permanent; in component trees they last until the owning component re-renders, which rebuilds the node from its declarative description. Durable state belongs in signals.

Handles are meant to be re-resolved per lookup — nodes are replaced wholesale on re-render, so storing a handle across passes is not supported (and the borrow checker enforces the short lifetime).

### Typed Interaction

DOM-downcast style: widgets expose their signal-backed operations through typed sub-handles. Because these write the same signals the app holds, they *do* survive re-renders.

```rust
// Text inputs: DOM `el.value = x` semantics — writes the signal and clamps
// the caret, but does not fire on_input handlers.
tree.get_element_by_id_mut("email").unwrap()
  .as_text_input().unwrap()
  .set_value("ada@example.com");

// Checkboxes, sliders, selects.
tree.get_element_by_id_mut("agree").unwrap().as_checkbox().unwrap().toggle();
tree.get_element_by_id_mut("volume").unwrap().as_slider().unwrap().set_from_ratio(0.5);
tree.get_element_by_id_mut("country").unwrap().as_select().unwrap().commit(2);
```

Universal actions live on the handle itself:

```rust
let mut save = tree.get_element_by_id_mut("save").unwrap();
// DOM el.click(): fires the node's own on_click handlers at its bounds
// center without hit-testing (works when occluded), focuses focusable
// nodes, and submits for submit buttons.
save.click();
// Focuses and, in the next pass, scrolls the node into view.
save.focus();
save.blur();
// Scrolls the node into view in the next pass without focusing it.
save.scroll_into_view();
```

For pointer-fidelity interaction (hit testing, hover, capture) keep using `tree.mouse_down` / `tree.mouse_up`, composing coordinates from `bounds().center()`.

## Input Dispatch

The shell forwards input into `Tree`:

```rust
tree.mouse_move(x, y);
tree.mouse_down(x, y, MouseButton::Left);
tree.mouse_up(x, y, MouseButton::Left);
tree.scroll(x, y, delta_x, delta_y, ScrollPhase::Scroll);
tree.key_down(key, code, shift, ctrl, alt);
```

The tree resolves target nodes from the latest layout and updates hover, active, focus, drag, scroll, cursor, text selection, and text input editing state. Click events are synthesized from matching pointer down/up events. Hit testing uses visual coordinates, so transformed text and transformed parents can still receive pointer selection from their painted position.

## Redraw

`Tree::needs_redraw()` reports whether the shell should request a frame. `WinitWindow` handles this automatically.

A pointer move that only moves the hover between elements that show no hover asks for no frame: `needs_redraw()` stays false (see [Mouse Events](../styling-events/#mouse-events) for the hovers that are drawn). The cursor is no part of a frame: a manual shell reads `Tree::cursor()` after each pointer event, and after each pass, which moves the hover when the content moved under a pointer that held still.

Animations and transitions do not set `needs_redraw`: after a pass, `Tree::next_timeline_change()` says when they next draw something else. A continuous animation returns the time of the last pass, so its next frame is due at once. Manual shells pace these frames as `WinitWindow` does: the frame for a change at `changes_at` is due at `max(changes_at, presented + T/2, started + 3T/4)`, where `T` is the display's refresh interval and `started` and `presented` are when the last frame's pass started and when its present returned.

```rust
if tree.needs_redraw() {
  window.request_redraw();
} else if let Some(changes_at) = tree.next_timeline_change() {
  let due = changes_at
    .max(last_presented_at + refresh_interval / 2)
    .max(last_started_at + refresh_interval * 3 / 4);
  schedule_redraw_at(due);
}
```

With a FIFO (vsync) swapchain, a pass woken half a refresh after the last present waits in its acquire for the next vblank, so each vblank shows a new frame as long as a frame's work stays under about half a refresh. Waking a whole refresh after the present instead adds the frame's work and the wake latency to every period: frames drift and skip vblanks, and continuous animations judder. The `started + 3T/4` floor caps a present mode that does not wait for the display (`Mailbox`, `Immediate`) at four frames in three refreshes.

During redraw, `tree.pass(&mut app, &surface)` draws the frame; the pass clears `needs_redraw` itself.

## Last Layout And Profiles

Use `last_layout()` to inspect the last computed layout.

```rust
if let Some(layout) = tree.last_layout() {
  println!("root size: {}x{}", layout.size.width, layout.size.height);
}
```

With `perf_profile` enabled, use `last_profile()` for frame timings and memory counters.

```rust
let profile = tree.last_profile();
println!("layout: {:?}", profile.layout);
```

## Secondary Windows

Secondary windows are owned by the main `Tree`. DevTools uses this path, but the concept is generic inside the runtime.

The important rule: secondary trees should render with the same render engine factory, not share the same render engine instance. Renderer instances are window/surface specific.
