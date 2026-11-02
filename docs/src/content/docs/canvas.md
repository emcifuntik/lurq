---
title: Canvas 2D
description: Persistent drawing through existing element refs, with paths, gradients, effects, layers, text, and images.
---

# Canvas 2D

Enable `canvas` alongside your window and renderer features:

```toml
lurq = { version = "0.53.0", features = ["canvas", "winit", "wgpu"] }
```

Canvas is available starting in **lurq 0.19.0**. DX12 supports the same drawing API. `canvas` enables raw image transport, path geometry, tessellation, and the CPU reference renderer; add `image` for PNG/JPEG/WebP/GIF/BMP/TIFF decoding and `resources` for resource loading.

## Use the existing ref

```rust
use lurq::{components::Canvas, core::ElementRef};

let reference = ElementRef::new(); // In a component: ctx.element_ref().
let element = Canvas::new()
  .ref_element(reference.clone())
  .width(480.0)
  .height(240.0);

// After layout, for example in after_layout() or an input handler:
if let Some(canvas) = reference.as_canvas() {
  let draw = canvas.context_2d();
  draw.set_fill_style("#60a5fa");
  draw.fill_rect(20.0, 20.0, 100.0, 60.0);
}
```

`core::ElementRef`, `core::ElementRefMut`, and the borrowed `node::ElementRef` returned by tree inspection all expose `as_canvas()`. It returns `None` before the first committed layout, for another node kind, or after removal. A zero-sized attached canvas still returns a handle.

The returned `CanvasHandle` and `Context2D` are owned, cloneable, `Send + Sync` handles. Contexts from one canvas share their drawing state, current path, save stack, and pixels. You can retain a context in application state and draw from input handlers, timers, or workers. There is no required `on_draw` callback.

Do not attach one ref to multiple live nodes. A conflicting canvas binding is rejected with a diagnostic. Clone a ref to access the same node; clone an `Element` to create an independent node and surface.

## First paint and resize

Store the ref when the component is created, attach it in `render`, and acquire the handle in `after_layout`. For a scene that should redraw after a resize, register one metrics observer and retain its subscription:

```rust
use std::sync::Mutex;
use lurq::{
  app::{component::Component, ctx::Ctx},
  canvas::CanvasObserver,
  components::Canvas,
  core::ElementRef,
  node::Element,
};

struct Chart {
  reference: ElementRef,
  subscription: Mutex<Option<CanvasObserver>>,
}

impl Component for Chart {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self { reference: ctx.element_ref(), subscription: Mutex::new(None) }
  }

  fn render(&self, _: &mut Ctx) -> impl Into<Element> {
    Canvas::new().ref_element(self.reference.clone()).width(480.0).height(240.0)
  }

  fn after_layout(&self) {
    let mut subscription = self.subscription.lock().unwrap();
    if subscription.is_some() { return; }
    let canvas = self.reference.as_canvas().unwrap();
    let draw = canvas.context_2d();
    *subscription = Some(canvas.observe_metrics(move |metrics| {
      draw.reset();
      draw.set_fill_style("#60a5fa");
      draw.fill_rect(12.0, 12.0, (metrics.size.width - 24.0).max(0.0), 40.0);
    }));
  }
}
```

An observer runs immediately with current metrics, then after committed logical-size or backing-scale changes. It runs outside canvas locks. Dropping `CanvasObserver` unsubscribes; the surface keeps a weak callback, so a callback may capture its context without creating a surface ownership cycle. The example intentionally redraws its scene on both kinds of change.

`Canvas::new()` has a 300 by 150 logical-pixel intrinsic size, subject to normal layout constraints. Width and height size the node. Padding reduces the drawable content rectangle. Bitmap dimensions are `ceil(content_size * display_scale)` on each axis. Drawing does not change layout size.

| Change | Result |
| --- | --- |
| Compatible rerender at the same size | Preserves surface identity, pixels, state, and path. |
| Keyed sibling reorder | Preserves the matching surface. |
| Changed key, incompatible kind, removal, or remount | A replacement receives a new surface; old handles stay with the old surface. |
| Logical content resize | Clears pixels, state, save stack, and path; keeps surface identity. |
| Display-scale change only | Resamples pixels and saved clips, preserving logical state. Redraw for sharper detail if needed. |
| Node opacity, background, transform, or ordinary window redraw | Changes presentation without reapplying drawing operations. |

State defaults are black fill and stroke, alpha 1, no shadow, no filter, the
`Normal` blend mode, identity transform, line width 1, butt caps, miter joins with limit 10, no dash or clip, smoothing enabled, left text alignment, and alphabetic baseline. The default font snapshots the theme's default typography at first readiness. `reset()` and logical resize restore that initial font snapshot.

## Drawing operations

All drawing coordinates and stroke widths use canvas-local logical pixels. Rotation and arc angles use radians. Solid paint setters accept lurq `Color` or checked `#RGB`, `#RGBA`, `#RRGGBB`, and `#RRGGBBAA` strings. Invalid style strings and non-finite numeric assignments leave the old value unchanged. Alpha must be between 0 and 1; line width and miter limit must be positive.

| Area | Methods and types |
| --- | --- |
| State | `save`, `restore`, `reset`; fill/stroke style, global alpha, line width/cap/join/miter, dash/offset, smoothing getters and setters. |
| Rectangles | `fill_rect`, `stroke_rect`, `clear_rect`, and whole-surface `clear`. |
| Transforms | `get_transform`, `set_transform`, `reset_transform`, `transform`, `translate`, `scale`, `rotate`; uses `Transform2D`. |
| Geometry | `begin_path`, `close_path`, `move_to`, `line_to`, `quadratic_curve_to`, `bezier_curve_to`, `rect`, uniform-radius `round_rect`, `arc`, `arc_to`, `ellipse`. |
| Path painting | `fill`, `fill_with_rule`, `stroke`, `fill_path`, `stroke_path`; `FillRule::{NonZero, EvenOdd}`. |
| Clipping | `clip`, `clip_with_rule`, `clip_path`. Clips intersect; `restore` restores the saved clip. |
| Hit tests | `is_point_in_path`, `is_point_in_path2d`, `is_point_in_stroke`, `is_point_in_stroke_path`. |
| Paints | `set_fill_style`, `set_stroke_style` take a `Color`, a colour string or a `Paint`; `Gradient`, `GradientKind::{Linear, Radial, Angular}`. |
| Effects | `set_shadow`, `shadow`, `set_filter`, `filter`; `Shadow`, `Filter::{None, Blur}`. |
| Compositing | `set_global_composite_operation`, `global_composite_operation`, `begin_layer`, `end_layer`; `BlendMode`. |
| Text | `set_font`, `fill_text`, `measure_text`, alignment/baseline setters; `CanvasFont`, `TextMetrics`, `TextAlign`, `TextBaseline`. |
| Images | `draw_image`, `draw_image_scaled`, `draw_image_region`. |
| Inspection | Handle `surface_id`, `size`, `pixel_size`, `scale_factor`, `metrics`, `status`, `is_attached`, `snapshot`. |

`clear_rect` erases through the current transform and clip, independently of global alpha. `clear` erases the entire bitmap while keeping state and path. `reset` also restores defaults and discards the save stack and path. Saving and restoring state does not restore the current path or undo pixels.

The current path captures the transform when geometry is added. A separate `Path2D` stores reusable geometry and applies the context's transform when filled, stroked, clipped, or hit-tested. `Path2D::add_path` accepts an explicit transform. Hit-test points are canvas-local, unaffected by the current drawing transform or clip. Curve hit testing uses flattened vector geometry, not pixel alpha.

Text uses lurq's font database and aliases with cosmic-text shaping and Swash rasterization. Register fonts on the app before layout. Set a typed `CanvasFont` with family, logical size, weight, style, letter spacing (logical pixels after every glyph, scaled with the transform like the size), and OpenType font features ([Font Features](../theme/#font-features)); an empty family selects the sans-serif fallback, and a CSS generic family (`"monospace"`, `"serif"`, …) the platform's font for it ([Generic Families](../theme/#generic-families)). Text is a single line: newlines and tabs become spaces. It is neither selectable nor part of layout. `measure_text` returns advance width and ink/font bounds relative to the selected alignment and baseline. Text methods return `Result` for unavailable services or oversized work. `stroke_text`, CSS font strings, wrapping, and `max_width` are not implemented.

Image sources must be immutable, nonempty CPU RGBA8 `ImageData`. Source regions use image pixels; destinations use logical canvas units. `draw_image_region` takes `[x, y, width, height]` for each region. Negative sizes extend the region in the opposite direction without mirroring; out-of-bounds source crops shrink the destination proportionally. The call retains the immutable source through a shared reference. The renderer uploads and premultiplies a source once, then reuses its cached GPU texture. Animated, streaming, native GPU, and video sources return `UnsupportedImage`.

## Paints

A fill or a stroke can use a solid colour or a gradient. `set_fill_style` and
`set_stroke_style` accept a `Color`, a checked colour string, or a `Paint`:

```rust
use lurq::canvas::{Gradient, GradientKind};

draw.set_fill_style(
  Gradient::linear()
    .stop(0.0, "#2563eb")
    .stop(1.0, "#f43f5e")
    .rotation(std::f32::consts::FRAC_PI_4)
    .in_box(20.0, 20.0, 200.0, 120.0),
);
draw.fill_rect(20.0, 20.0, 200.0, 120.0);
```

Since 0.20.0, `fill_style()` and `stroke_style()` return `Paint` instead of `Color`. Use `paint.color()` to obtain `Some(Color)` for a solid paint; gradients return `None`. Existing solid-color setter calls continue to work.

A gradient is written the way an authoring tool writes one. `in_box(x, y, w, h)`
gives it a box in the **user space in force when the paint is used**. The paint
does not capture the context transform when it is constructed. Inside that box:

| Field | Meaning |
| --- | --- |
| `center(x, y)` | Fractions of the box; `(0.5, 0.5)` is its centre. |
| `size(w, h)` | Fractions of the box; `(1.0, 1.0)` spans it. |
| `rotation(radians)` | Clockwise about the centre. |
| `stop(offset, colour)` | 2 to 16 stops, offsets 0..=1 and not decreasing. |

The box is normalised first, then rotated, then divided by `size`, so a rotation
means the same thing in a wide box as in a square one. `GradientKind::Linear`
runs the ramp along the frame's x axis, `Radial` outwards from the centre to the
box's ellipse, and `Angular` around the centre, starting along +x and increasing
clockwise. Colour and alpha are interpolated separately and premultiplied
afterwards, which is what the software backend's own gradients do.

A gradient that cannot paint — fewer than two stops, offsets that are not
ascending, a collapsed box — **draws nothing**, rather than a colour it picked.
`Gradient::is_valid` reports the same answer before the draw. A gradient set as
the fill is refused for `fill_text` and `measure_text` with `UnsupportedPaint`:
shaped text is rasterised with one colour, and this release does not stretch a
ramp across it.

## Shadows, blur and blend modes

```rust
use lurq::canvas::{BlendMode, Filter, Shadow};
use lurq::node::color::Color;

draw.set_shadow(Some(Shadow::new(Color::new(0, 0, 0, 160)).offset(0.0, 8.0).blur(24.0).spread(2.0)));
draw.round_rect(24.0, 24.0, 240.0, 120.0, 12.0)?;
draw.fill();
draw.set_shadow(None);

draw.set_filter(Filter::Blur(12.0));        // blurs what the next draws paint
draw.set_global_composite_operation(BlendMode::Multiply);
```

`Shadow` carries a colour, an offset, a blur radius, a spread and an `inset`
flag. An outer shadow draws behind the shape and an inner one only inside it;
both take the current clip, transform and global alpha, and both apply to fills,
strokes and `fill_text`. Unlike HTML Canvas, where shadows ignore the transform,
**offset, blur and spread are in the current user space** and therefore scale and
rotate with it — a design tool's shadows belong to the node, so they have to
follow it when the view zooms. Spread has no meaning for a raster and is not
applied to `fill_text`. Shadows and filters are not applied to `draw_image_*`; a
shadow for an image box is the shadow of its rectangle.

`set_filter(Filter::Blur(radius))` blurs what the following draws paint, shape
and paint together. Backdrop blur — blurring what is already **under** a
shape — is not in this release; it needs to read the surface it writes, and that
ordering is not yet stated. See [lurq#17](https://github.com/ivanzaida/lurq/issues/17).

`set_global_composite_operation` supports eighteen blend modes: `Normal`, `Darken`, `Multiply`, `LinearBurn`, `ColorBurn`,
`Lighten`, `Screen`, `LinearDodge`, `ColorDodge`, `Overlay`, `SoftLight`,
`HardLight`, `Difference`, `Exclusion`, `Hue`, `Saturation`, `Color` and
`Luminosity`. Non-separable modes operate on non-premultiplied colours, as the
specification requires. A draw with a mode other than `Normal` is isolated
before it blends, so its own shadow does not blend with the shape that casts it.

Shadows, filters, blend modes and the paint are all part of the saved drawing
state: `save` and `restore` carry them.

## Isolated layers

`global_alpha` multiplies each draw's own alpha, so a group of overlapping
shapes drawn at 0.5 shows its own overlaps through itself. A layer composites
the group once instead:

```rust
draw.begin_layer(0.5, BlendMode::Normal)?;
draw.fill_rect(20.0, 20.0, 80.0, 80.0);
draw.fill_rect(60.0, 60.0, 80.0, 80.0);   // the overlap is not darker
draw.end_layer()?;
```

Draws between the two calls composite into a target of their own using the
current drawing state, including `global_alpha`. `end_layer` then composites
that target onto the parent once, with the layer's `alpha` and `blend`. Keep
`global_alpha` at 1 when only the group's opacity should change. Layers nest
to `MAX_LAYER_DEPTH` (8) and are **not** the save stack:
`restore` does not close one, `end_layer` without a matching `begin_layer` is
`UnbalancedLayer`, and a layer still open when the canvas is reset, resized or
detached is discarded with its contents. A layer's commands are held on the
surface until it closes, so a batch handed to the renderer never carries half a
layer — and an unclosed layer's drawing is not presented.

## What each new operation costs

The bounds below are the ones a consumer that charges its own per-pass budget
has to keep charging.

| Operation | Commands | Vertices | Queue bytes | Other |
| --- | --- | --- | --- | --- |
| Gradient fill or stroke | 1, as the solid one | The same mesh as the solid one; the paint is written into the `uv` the vertex already carries | The path, plus 1 KiB for the ramp the first time that stop list is used | The ramp is one 256 × 1 asset per distinct stop list, shared by every draw |
| Shadow (outer or inner) | 1 image | 6 | The rasterised image, `width * height * 4` | One CPU rasterisation and blur, cached by content |
| `Filter::Blur` | 1 image, replacing the draw | 6 | The rasterised image | As above |
| Blend mode on one draw | 2 (`BeginLayer`, `EndLayer`) plus the draw | 0 extra | 2 command headers | One extra tile resolve and two whole-tile quads per tile it touches |
| `begin_layer`/`end_layer` | 2 plus its contents | 0 extra | 2 command headers | As above, plus two TILE × TILE textures per open depth in the renderer, allocated on first use |

The **mesh-cache key is unchanged**: the path, the fill rule and the curve
flattening scale bucket. A paint is applied while the cached triangles are
copied out, exactly as the colour always was, so a gradient does not multiply a
page's cache entries and the per-batch `MAX_VERTICES` charge is what it was.

A shadow or a blur is rasterised on the CPU, in device pixels, and drawn as an
ordinary premultiplied image; one implementation therefore serves the software
backend and both native ones. Three things bound it: the raster is **reduced**
when the blur is wide (a wide blur has no detail to lose), it is **clipped to
the surface**, because nothing outside the canvas can be seen, and whatever
remains is capped at `MAX_EFFECT_PIXELS` (4 194 304 device pixels) — over that,
the draw reports `StateLimit` rather than rasterising. `MAX_SHADOW_BLUR`,
`MAX_BLUR_RADIUS` and `MAX_SHADOW_SPREAD` are 512 user units each; a value
outside the range leaves the state alone rather than being clamped.
Rasterisations are cached across frames, keyed by their own content, in a
process-wide cache bounded at 32 MiB and 512 entries with random eviction, the
same policy as the mesh cache. The cache places a blurred raster on whole device
pixels, so a shadow can sit up to half a pixel from where a sub-pixel pan would
put it; that is what lets a pan reuse it.

An isolated layer costs the renderer two TILE × TILE RGBA textures (1 MiB each)
per open depth, allocated the first time a layer reaches that depth and shared
by every canvas of one renderer — not a surface-sized target per layer. Per tile
the layer touches, it costs one extra multisample resolve and two whole-tile
quads. A blend mode other than `Normal` reads the saved tile as its backdrop and
writes the finished composite itself, which is why it needs no fixed-function
blending and why it costs the same split.

## Pointer input

Mouse event coordinates are already window-logical. Convert them to the content rectangle, including ancestor transforms and padding:

```rust
let reference = canvas_ref.clone();
Canvas::new().ref_element(canvas_ref).on_click(move |event: lurq::app::events::MouseEvent| {
  let Some(canvas) = reference.as_canvas() else { return; };
  let Some((x, y)) = canvas.point_from_window(event.x, event.y) else { return; };
  canvas.context_2d().fill_rect(x - 2.0, y - 2.0, 4.0, 4.0);
});
```

Conversion returns `None` when detached or when the presentation transform cannot be inverted. It does not invert the drawing transform or clamp to canvas bounds. Individual painted shapes have no automatic event targets; describe the ones that matter as [items](#describing-what-you-drew) and hit-test those, use geometry hit tests, or overlay normal controls. Keep controls that need focus and keyboard input in ordinary UI nodes.

## Describing what you drew

Pixels carry no structure. Register one `CanvasItem` per drawn thing that matters (a bar, a point, a series, an axis label) with a stable id, a role, an optional label and value text, and its shape in content coordinates, the space `point_from_window` returns:

```rust
use lurq::canvas::{CanvasHandle, CanvasItem};

fn draw(canvas: &CanvasHandle, runs: &[(&str, &str, f32)]) {
  let draw = canvas.context_2d();
  draw.reset();
  draw.set_fill_style("#60a5fa");
  let mut items = Vec::new();
  for (index, (id, label, value)) in runs.iter().enumerate() {
    let (x, height) = (20.0 + index as f32 * 70.0, value * 8.0);
    draw.fill_rect(x, 170.0 - height, 50.0, height);
    items.push(
      CanvasItem::rect(*id, "bar", x, 170.0 - height, 50.0, height)
        .label(*label)
        .value(format!("{value} runs")),
    );
  }
  canvas.set_items(items); // replaces the whole set
}
```

| API | Does |
| --- | --- |
| `CanvasItem::rect(id, role, x, y, width, height)` | A rectangle; negative extents are normalized. |
| `CanvasItem::point(id, role, x, y, radius)` | A point that hits within `radius`. |
| `.label(text)` / `.value(text)` | Accessible name (`"Tue"`) and displayed value (`"18 runs"`). The fields are public to read; build items with the constructors, as more fields may follow. |
| `canvas.set_items(items)` | Replace the canvas's items. Cheap; it does not repaint. Ids are unique per canvas: of items sharing an id, the last is kept, and a warning is logged once per set of duplicated ids. |
| `canvas.items()` | The current items, shared (`Arc<[CanvasItem]>`). |
| `canvas.item_at(x, y)` | The last registered item containing a content point, so later items count as drawn on top. |
| `canvas.item_window_bounds(id)` | Window-logical `(x, y, width, height)`, like `ElementRef::rect`, clipped to the canvas, for anchoring a tooltip; `None` when the item lies outside the canvas. |

Call `set_items` from the same code that draws, so items and pixels change together. A logical resize discards the items with the pixels; the redraw that follows registers them again. A display-scale change keeps both. Item coordinates ignore the drawing transform (`translate`, `scale` on the context), like `point_from_window`; register them in the space you hit-test in. Window bounds include padding, scrolling and ancestor transforms; a rotated placement yields the enclosing box.

Hover tooltips hit-test the same items:

```rust
let reference = canvas_ref.clone();
Canvas::new().ref_element(canvas_ref).on_mouse_move(move |event: lurq::app::events::MouseEvent| {
  let Some(canvas) = reference.as_canvas() else { return; };
  let item = canvas.point_from_window(event.x, event.y).and_then(|(x, y)| canvas.item_at(x, y));
  hovered.set(item.map(|item| item.id));
});
```

With the `mcp` feature, agents see the items as children of the canvas in `lurq_read_tree` and `lurq_inspect`, and can hover, click and screenshot them by ref; see [Canvas content](../mcp/#canvas-content). Item ids, roles, labels and values are shown to any MCP client with the observe scope, unmasked; do not put secrets in them. Items are plain data on the canvas handle and cost nothing else when MCP is off.

## Persistence, scheduling, and cost

`Canvas::new()` draws into a persistent GPU texture on both WGPU and native DX12. Calls record ordered work; paths are tessellated on the CPU, then rasterized and blended on the GPU. Text uses cached CPU shaping/glyph rasterization and GPU image drawing. The default canvas has no full-size CPU bitmap. A new blank canvas defers its backing allocation until drawing or readback needs it.

GPU commands keep geometry separate from its drawing transform. Both GPU backends cache model-space triangles across frames, keyed by geometry content, fill rule, and curve flattening scale. Reusing a `Path2D` also reuses its immutable recorded snapshot; editing it invalidates that snapshot without affecting prior drawings or clones. Identical rebuilt paths can reuse triangles, but still pay construction and hashing costs. For camera movement, retain the document's `Path2D` objects, clear the surface, set the camera transform, and draw them again. Transforming and uploading the prepared vertices still costs work each frame; this is not a retained scene or a camera uniform API.

Polygon meshes survive arbitrary zoom. Curves use power-of-two scale buckets based on the transform's maximum stretch, including DPI, skew, and nonuniform scale. Flattening tolerance is at most 0.1 physical pixels; entering a finer bucket tessellates once per uncached path. Stroke outlines and dashes retain their existing model-space resolution of 1.0 and are still computed when recording a stroke. The full transform scales the outline, including its width; the resulting outline's triangles are cached. Zoom does not introduce a new stroke-outline approximation policy.

The renderer processes only new commands. A shared 512 × 512 tile surface provides 4-sample antialiasing; touched tiles are seeded from the existing texture, drawn, resolved, and copied back on the GPU. A small edit does not upload, convert, or copy the whole canvas. Full clears discard obsolete queued drawing while preserving resize and snapshot barriers. Idle surfaces retain pixels without replaying history or requesting continuous frames.

Internal source-over blending uses premultiplied sRGB channel values. Image sources are premultiplied before filtering. Window composition passes the result through the existing image pipeline, including node backgrounds, borders, clipping, radius, and ancestor opacity, which blends it over the window on sRGB-encoded channels like every other translucent color.

Drawing increments the content revision, wakes the window, and coalesces presentation. It does not dirty reactive state or layout. Winit installs the waker automatically. Custom hosts must install `Tree::set_canvas_waker` and schedule a pass; custom renderer wrappers must forward `RenderEngine::prepare_canvases`, including surfaces culled from the visible image list.

Methods serialize through the surface lock. A set-style plus draw sequence is not an atomic transaction across threads; coordinate multi-call sequences in the application. Callbacks and wakeups run outside internal locks.

### Explicit snapshots

`snapshot()` returns a `CanvasReadback` ticket. It captures commands before that call, excluding later drawing, node styling, and window composition. Poll from the UI thread:

```rust
let readback = canvas.snapshot();
// After the host has rendered, in a later event/tick:
if let Some(result) = readback.try_take() {
  let snapshot = result?; // width, height, straight-alpha sRGB rgba, revision
}
```

A worker can use `wait_timeout(Duration)` while the UI continues rendering. Never wait for a queued GPU readback on the rendering thread. Readbacks are bounded to two outstanding requests per canvas and eight process-wide, including in-flight GPU copies. Dropping a queued request, detaching, resizing, or losing the renderer completes its ticket with an error. A full `clear()` preserves prior snapshot requests. Submitted pixels are never reapplied following a failed window presentation.

### Lifetime and resource limits

Removing the node invalidates the ref and rejects further GPU drawing with `Detached`. Old handles retain their identity and never target a replacement. The renderer releases detached backing textures after pending GPU use. Device loss reports `RendererLost`; there is no retained CPU checkpoint or complete drawing history, so applications must redraw after recovery.

Use `Canvas::new().software()` explicitly for the synchronous tiny-skia reference renderer, tests, or a host without GPU canvas support. Its readback ticket is ready immediately. Software mode retains the earlier detached-bitmap behavior and full-bitmap upload costs. A renderer without GPU canvas support reports `UnsupportedBackend` for the default canvas.

Limits include 16,384 pixels per backing dimension (also subject to device limits), 16,777,216 backing pixels, 128 saved states/clip levels, 65,536 input path segments, and 8 MiB of distinct retained vector clips. Queued plus encoding work is charged against 64 MiB per canvas and 8,192 commands. Source/clip references are conservatively charged per queued draw. Overflow reports `QueueFull` and rejects that operation; render pending work before continuing, or clear/reset obsolete work. Tessellation expansion is capped at 1,048,576 output vertices. GPU image/text textures are kept per renderer within a `CanvasAssetBudget` of charged bytes, 64 MiB by default, with at least 64 KiB charged per texture to bound small-texture overhead; set it with `with_canvas_asset_budget` on `WgpuRenderEngine` or `Dx12RenderEngine` (at most 1 GiB). Shaped text has an 8 MiB app cache in addition to the bounded glyph cache. Software clips retain their separate 64 MiB limit.

Both caches are refilled in the same order every frame, so neither evicts what the current or the previous frame drew. When those two frames alone need more than the budget, the cache keeps them, up to twice the budget; a texture or shaped text beyond that is drawn but not kept past its frame, so the rest stays cached instead of every entry being evicted and rebuilt each frame. Once frames need less, the cache shrinks back to its budget, evicting what neither of the last two frames used, oldest first. A frame ends when a GPU renderer encodes the canvas, except while a replacement presentation is drawn in batches over several encodes: those encodes are one frame, which ends at the encode that commits, aborts or supersedes the replacement, or resizes or detaches its canvas. Passes that draw no canvas, and passes that only reproject retained artwork, do not age entries. The completed front and captured artwork are render targets of their own, outside both caches, so evicting a texture never changes what is shown. A text engine that no GPU renderer has encoded yet, such as one used only by software canvases, keeps its shaped text least recently used within the 8 MiB. Loading, installing or registering a font, or a scale override, replaces the app's text engine. When a GPU renderer has ended frames of the engine it replaces, the replacement is framed from its first frame, so the first frame after a font change can keep a page over the budget whole and the next frame shapes nothing again.

`status()` exposes attachment, metrics, content revision, errors, charged pending bytes, backing GPU bytes, and cumulative submitted batches, vertices, tiles, and source-upload bytes. A 3840 × 2160 backing needs 33,177,600 color bytes. Antialiasing scratch is shared across canvases and fixed in size: approximately 9 MiB with D24S8, with WGPU depth/stencil allocation depending on the backend. Queues, geometry buffers, source caches, explicit readbacks, and resources awaiting GPU fences add to those figures; these limits are not a global application memory cap.

The CPU mesh cache is shared across canvases within each renderer and capped at 32 MiB of charged source geometry, triangle storage, and a metadata allowance, with an independent 32,768-entry limit. It uses random eviction and bypasses retention for oversized entries. Random victims prevent repeated ordered scans above capacity from evicting every next-needed mesh, while keeping hits and individual evictions constant-time. It does not guarantee a particular hit ratio for every workload. These bounds accommodate thousands of ordinary paths at several zoom levels and bound metadata for tiny paths. Clears and resizes keep reusable meshes; renderer destruction releases the cache. Mesh-cache memory is separate from `status().gpu_bytes`, which reports backing textures. `status().gpu` exposes cumulative `mesh_cache_hits`, `mesh_cache_misses`, and `mesh_cache_evictions` attributed to this canvas during CPU preparation, including batches that later fail to submit. `mesh_cache_entries` and `mesh_cache_bytes` are shared-renderer occupancy snapshots at that canvas's last preparation. Charged bytes include source geometry and metadata as well as triangle positions; multiplying submitted vertices by eight does not measure cache occupancy. Curveless paths use one scale bucket but still consume cache entries and bytes.

Patterns, backdrop blur, canvas-to-canvas drawing, pixel upload, and automatic animation callbacks remain outside the supported subset. Gradients, shadows, layer blur, the eighteen blend modes and isolated layers are covered above, from 0.20.0.

## Examples and checks

```text
cargo run -p lurq --example canvas --features canvas,winit,wgpu
cargo run -p lurq --example canvas_chart --features canvas,winit,wgpu,mcp
cargo test -p lurq --features canvas --test canvas_tests
cargo test -p lurq --features canvas,mcp --test mcp_canvas_tests
cargo test -p lurq --features canvas,wgpu --lib gpu_canvas_pixels -- --ignored
cargo test -p lurq --features canvas,wgpu --lib gpu_canvas_effects -- --ignored
```

The effects suite draws one fixture — gradients, shadows, a layer blur, every
blend mode and a nested isolated layer — on the native backend and on the
software backend, and compares them.

On Windows, the hidden-window capture harness checks actual composition, tile updates, ordered GPU readbacks, culled drawing, and scale changes on either backend:

```text
cargo run -p lurq --example canvas_capture_check --features canvas,screenshot,wgpu -- wgpu
cargo run -p lurq --example canvas_capture_check --features canvas,screenshot,dx12 -- dx12
```

The vocabulary follows the [HTML Canvas specification](https://html.spec.whatwg.org/multipage/canvas.html), with deliberate lurq choices for typed refs, sizing, display scale, defaults, and the supported subset. This is not a claim of full browser conformance.

## Complete presentation transactions

Available since **lurq 0.38.0**. `begin_presentation()` isolates a replacement across bounded drawing batches. Capture artwork with `capture_artwork()` before transient overlays, then publish only the matching token with `commit_presentation(token)`. `abort_presentation(token)` keeps the completed front. Superseded tokens cannot publish, and snapshots taken while a replacement is pending describe the completed front and its revision.

`draw_retained_artwork(matrix)` reprojects completed artwork on the GPU without CPU readback; it does not make old editor geometry authoritative. `forget_artwork()` releases that visual ownership. `PresentationBusy` is temporary leased-target pressure: preserve the front and retry after submission completion. Target ownership has a separate 256MiB allowance; it is not the uploaded-asset budget. WGPU accounting reports logical texture extents, not opaque physical driver heaps. Renderer-front commit and CPU present counters do not prove physical display scanout.
