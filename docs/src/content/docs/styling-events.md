---
title: Styling And Events
description: Visual modifiers, state styles, cursors, inputs, event handlers, scroll, and drag and drop.
---

# Styling And Events

Most visual and input behavior is expressed as chainable modifiers on typed components.

Use [Theme](../theme/) roles for shared app semantics such as palette colors, text variants, radii, spacing, border sizes, shadows, and compound form controls. Use concrete values for isolated one-off visuals.

## Visual Modifiers

```rust
use lurq::{components::Rect, node::color::Color};

Rect::new(120.0, 40.0)
  .background("#2563eb")
  .rounded(8.0)
  .border_inside(1.0, Color::from_hex("#1d4ed8"))
```

Common visual modifiers:

| Modifier | Purpose |
| --- | --- |
| `.background(color)` | Background color from a concrete color or `PaletteColor`. |
| `.background_gradient(gradient)` | Linear, radial, or conic gradient fill. See [Gradients](#gradients). |
| `.rounded(radius)` | Uniform corner radius from `f32` or `RadiusSize`. |
| `.corner_radius_*` | Per-corner radius from `f32` or `RadiusSize`. |
| `.border_inside(width, color)` | Border inside the element bounds from a concrete width or `BorderSize`. |
| `.border_center(width, color)` | Border centered on the element edge from a concrete width or `BorderSize`. |
| `.border_outside(width, color)` | Border outside the element bounds from a concrete width or `BorderSize`. |
| `.box_shadow(shadow)` | Drop or inset shadows from a `ShadowStyle` role, a `BoxShadow`, or a list. See [Box Shadows](#box-shadows). |
| `.opacity(value)` | Fade the element and its subtree as one group. See [Opacity](#opacity). |
| `.blur(radius)` | Blur the element and its subtree as one layer. See [Blur](#blur). |
| `.backdrop_blur(radius)` | Blur what is painted behind the element: frosted glass. See [Backdrop Blur](#backdrop-blur). |
| `.clip()` | Clip descendants to this element. |
| `.overflow_visible()` | Allow descendants to paint outside this element. |

Translucent colors (`"#000000a6"`, `.opacity(...)`, anti-aliased edges, text, images) blend on the sRGB-encoded
channels, as CSS and design tools do: a `#000000a6` scrim over `#eeeeee` shows `#535353`. Both native backends render
through a non-sRGB target to get this; the devtools screenshot renderer blends the same way.

## Opacity

`.opacity(value)` fades an element together with everything inside it, as CSS `opacity`, Figma and Pencil do: the
subtree is painted into an offscreen layer as if it were opaque, and the layer is then blended over what is behind it
once, at `value`. Content inside the group blends only with other content of the group. A dark label on a light fill
at `opacity(0.4)` keeps its contrast against the faded fill, and a border drawn over the fill's edge adds no lighter
ring:

```rust
use lurq::components::{Stack, Text};

// Disabled primary button over a #1c1c1c window: the fill shows #6c6c6c and the
// label #1a1a1a, the colours a design tool shows for the same group.
Stack::new()
  .size(100.0, 36.0)
  .background("#e4e4e4")
  .border_inside(1.0, "#e4e4e4")
  .rounded(6.0)
  .opacity(0.4)
  .child(Text::new("Save").color("#171717"))
```

- Nested opacities compose: a group at `0.5` inside a group at `0.5` shows its content at `0.25` where it covers
  nothing else of the outer group.
- A group that paints a single primitive (a fill without a border, one text run without a shadow, an image, a
  rasterized SVG, a box shadow) looks the same either way, so it fades that primitive directly and needs no layer.
  Opacity `1` (the default) never makes a layer.
- A layer covers the group's painted pixels in whole physical pixels, within its clips and the window, and maps one to
  one onto the window's pixels: it is never resampled, so it stays sharp at fractional scale factors such as 1.25 and
  1.5. Layer textures are pooled and reused across frames; a group that is clipped away or at `opacity(0.0)` paints
  nothing.
- Opacity changes painting only. Layout, clipping, scrolling, transforms and hit testing are the same at any opacity: a
  faded element still takes clicks unless it is disabled some other way.
- `RenderList::layers` lists the layers of a frame (`LayerCmd`: the render orders a layer groups, its opacity and pixel
  bounds). A custom `RenderEngine` composites each layer's draws into its own target, or draws them without a layer
  and loses the flattening.

## Blur

`.blur(radius)` blurs an element together with everything inside it, as CSS `filter: blur()` and a design tool's layer
blur do: the subtree is painted into an offscreen layer, and the layer is drawn through a Gaussian blur. What is
behind the element is not blurred.

```rust
use lurq::{animation::Transition, components::{Column, Text}};

// A panel that goes out of focus when `inactive`, over 200 ms.
Column::new()
  .padding(16.0)
  .background("#ffffff")
  .child(Text::new("Drafts"))
  .blur(if inactive { 6.0 } else { 0.0 })
  .transition(Transition::blur().duration_ms(200))
```

- `radius` is in logical pixels and means what a shadow's blur radius means: the Gaussian's standard deviation is half
  of it, so `blur(8.0)` softens an edge like `BoxShadow::new(0.0, 0.0, 8.0, ..)`. CSS's `blur(4px)` names the
  deviation itself and is `blur(8.0)` here. `0` (the default) is no blur and no layer; the radius is clamped to
  `layout::opacity_layer::MAX_BLUR_RADIUS` (256).
- The blur spreads past the element's box, by three deviations (`1.5 * radius`). It is cut by the clips the element is
  under and by the window, as the element's own pixels are; inside them, content of the element that lies just outside
  the clip still bleeds in, so a blurred element cut by a scroll container or the window's edge does not fade out
  there. Containers clip by default: give a blur room with padding, or call `.overflow_visible()` on the containers it
  should escape. A rounded clip cuts a blurred element to its box, without the corners.
- `.blur(...)` and `.opacity(...)` on one element share a layer: the blurred result is faded.
- Blur changes painting only. Layout, scrolling and hit testing are the same at any radius, and the radius does not
  scale with a `.transform(...)`.
- The cost is the layer's pixels, not the radius: a wide blur runs on a reduced copy of the layer. A blurred layer
  takes two or three pooled textures for its frame.
- A blurred `LayerCmd` carries a `LayerBlur`: the deviation in physical pixels and the rectangle the layer is painted
  for, which is larger than the `bounds` it shows in and may reach outside the window. A custom `RenderEngine` that
  ignores it draws the layer unblurred.

## Backdrop Blur

`.backdrop_blur(radius)` blurs what is painted behind an element, inside the element's box, as CSS
`backdrop-filter: blur()` does: frosted glass. The element then paints itself over the result, so the blur shows
wherever the element is translucent. Give it a translucent background; an opaque one hides the blur.

```rust
use lurq::components::{Column, Text};

// A glass toolbar over whatever scrolls under it.
Column::new()
  .padding(12.0)
  .background("#ffffff40")
  .border_inside(1.0, "#ffffff99")
  .rounded(12.0)
  .backdrop_blur(16.0)
  .child(Text::new("Toolbar"))
```

- `radius` means what `.blur(radius)`'s means and has the same limit; `0` (the default) is no blur.
- The blur fills the element's box with its corner radii, inside the clips the element is under. It does not spread
  past the box. A rounded clip of an ancestor cuts it to the clip's box, without the corners. Under a `.transform(...)`
  that scales or flips, the box is the scaled box; under a rotation or a skew it is the bounding box of the turned
  one, without corner radii.
- "Behind" is everything painted before the element: earlier siblings, ancestors' backgrounds, anything under an
  overlay. Inside an ancestor with `.opacity(...)` below 1 or a `.blur(...)`, it is only what that ancestor's layer
  holds, as in CSS: the blur does not see through the layer to the window.
- At the window's edge (or its layer's) the blur reads the edge pixels as if they went on, so glass against an edge
  does not darken there.
- The element's own `.opacity(...)` fades the blur with it: at `0.5` the blurred backdrop is mixed half and half with
  the sharp one.
- `Transition::backdrop_blur()` animates the radius.
- Each backdrop blur stops the frame's drawing, copies the pixels around the box and blurs the copy, so it costs a
  few passes and pooled textures of about the box's size. A handful of glass panels is cheap; a long list of glass
  rows is not what it is for.
- `RenderList::backdrops` lists the frame's backdrop blurs (`BackdropCmd`: the box, its radii and clip, the deviation
  in physical pixels, and the render order the blur happens at). A custom `RenderEngine` that ignores them draws the
  frame without the blurs.

## Gradients

`.background_gradient(...)` fills an element with a CSS-like gradient. It is separate from `.background(color)`; if both are set, the gradient paints the fill. Gradients respect the element's rounded corners, clipping, and `.opacity(...)` just like a solid background.

```rust
use lurq::{components::Rect, node::{Gradient, GradientStop}};

Rect::new(240.0, 120.0)
  .rounded(12.0)
  .background_gradient(Gradient::linear(135.0, ["#ff0080", "#7928ca"]))
```

Three kinds are supported on both the wgpu and dx12 backends:

| Constructor | Description |
| --- | --- |
| `Gradient::linear(angle_deg, stops)` | Linear gradient. `angle_deg` follows CSS: `0` points up, increasing clockwise (`90` is to the right). The line is sized so `0%`/`100%` reach the box corners. |
| `Gradient::radial(stops)` | Radial gradient, farthest-corner. Defaults to an ellipse fitted to the box; call `.circle()` for a circle. |
| `Gradient::conic(from_deg, stops)` | Conic gradient sweeping clockwise from `from_deg` at the top. |

### Color Stops

Stops accept anything that converts into a color (hex strings, `Color`, or a `PaletteColor`), so theme palette colors work inside gradients. A bare color is auto-positioned; use `GradientStop::at(color, position)` for an explicit position in `0.0..=1.0`.

```rust
use lurq::node::{Gradient, GradientStop};

// Auto-spaced: first at 0.0, last at 1.0, middle evenly distributed.
Gradient::linear(90.0, ["#f00", "#0f0", "#00f"]);

// Explicit positions and a palette color (lurq::app::theme::PaletteColor).
Gradient::linear(90.0, [
  GradientStop::at("#000", 0.0),
  GradientStop::at(PaletteColor::Accent, 0.4),
  GradientStop::at("#fff", 1.0),
]);
```

Omitted positions follow the CSS rules: the first defaults to `0.0`, the last to `1.0`, and runs of omitted stops are spread evenly between their defined neighbors. Colors are interpolated in linear space (CSS interpolates in sRGB, so midpoints are lighter than a browser's); the result then blends over what is below like any other translucent color.

### Center And Shape

```rust
use lurq::node::Gradient;

// Radial circle centered in the top-left quadrant.
Gradient::radial(["#fff", "#1e293b"]).circle().center(0.25, 0.25);

// Conic starting from 45 degrees, centered.
Gradient::conic(45.0, ["#f43f5e", "#8b5cf6", "#06b6d4", "#f43f5e"]);
```

`.center(x, y)` moves the radial/conic origin; coordinates are normalized `0.0..=1.0` within the element (default `(0.5, 0.5)`).

## Box Shadows

`.box_shadow(...)` gives an element CSS-like `box-shadow`s. It takes a `ShadowStyle` [theme role](../theme/#shadow), which is what app UI should use, or concrete `BoxShadow` values for one-off visuals:

```rust
use lurq::{
  app::theme::ShadowStyle,
  components::Rect,
  node::BoxShadow,
};

// A theme elevation.
Rect::new(240.0, 120.0).background("#ffffff").rounded(12.0).box_shadow(ShadowStyle::Md);

// offset_x, offset_y, blur, color; spread and inset are optional.
Rect::new(240.0, 120.0)
  .background("#ffffff")
  .rounded(12.0)
  .box_shadow([
    BoxShadow::new(0.0, 1.0, 2.0, "#0f172a1f"),
    BoxShadow::new(0.0, 12.0, 32.0, "#0f172a40").spread(-4.0),
  ]);

// An inset well.
Rect::new(240.0, 40.0)
  .background("#ffffff")
  .box_shadow(BoxShadow::new(0.0, 2.0, 6.0, "#0000004d").inset());
```

A `BoxShadow` follows CSS and design tools such as Figma and Pencil:

| Field | Meaning |
| --- | --- |
| `offset_x`, `offset_y` | Moves the shadow, in logical pixels. |
| `blur` | CSS blur radius: a Gaussian with a standard deviation of `blur / 2`. `0` is a hard edge. |
| `spread` | Grows the shadow shape (negative shrinks it) before the blur. Corner radii grow and shrink with it as CSS specifies, so square corners stay square. |
| `color` | A `Color`, hex string, or `PaletteColor` role. |
| `inset` | Paints inside the element instead of beneath it (`.inset()`). |

How shadows paint:

- A list paints first on top. Outer shadows paint beneath the element's background; inset shadows above the background and below the border and the children, inside the padding box (within an inside or centered border).
- An outer shadow follows the element's corner radii and paints only outside its box, so it never shows through a translucent background.
- Shadows never change layout and are not hit-tested: a click on a shadow goes to whatever is under it.
- Shadows take the element's `.opacity(...)` and transform, and its clip: an ancestor that clips its children clips their shadows too. Containers clip by default, so give a shadow room with padding, or call `.overflow_visible()` on the containers it should escape (as with any child that paints outside its parent).
- Values scale with the display like every other length.
- Hover, active, and focus styles can change the shadow, for example to raise a card on hover with `.hovered(|style| style.box_shadow(ShadowStyle::Lg))`; `BoxShadowValue::none()` removes it.

Both native backends evaluate the blurred rounded rect analytically in the quad shader (a closed-form `erf` along one axis, eight samples along the other), so a shadow is one more instance in the quad pipeline: no offscreen pass and no blur texture. wgpu and DX12 render the same pixels; the devtools screenshot renderer uses the same formula on the CPU.

## Hover, Active, And Focus Styles

State styles merge into the base style while the node is hovered, active, focused, or focus-visible.

```rust
use lurq::{components::Text, node::CursorIcon};

Text::new("Save")
  .padding_horizontal(12.0)
  .padding_vertical(8.0)
  .background("#2563eb")
  .rounded(6.0)
  .cursor(CursorIcon::Pointer)
  .hovered(|style| style.background("#3b82f6"))
  .active(|style| style.background("#1d4ed8"))
  .focus_visible(|style| style.border_inside(1.0, "#93c5fd".into()))
```

The focused style (`.focused(...)`) shows whenever the node has focus, whether a click, Tab, or a focus request put it there. The focus-visible style (`.focus_visible(...)`) shows only for keyboard focus, like CSS `:focus-visible`: Tab or a key press shows it, a click does not, and a text input shows it for any focus. Use it for focus rings. Checkboxes, sliders and selects also have part-level focus styles, which follow focus-visible; see [Focus-Visible](../focus-navigation/#focus-visible) and [Focused Styles](../focus-navigation/#focused-styles).

A fifth state style, `.drag_over(...)` (`drag_over_style`), applies while files from the OS are dragged over the node; see [File Drag And Drop](#file-drag-and-drop).

State styles can affect layout if they change frame, padding, or flex. That is supported, but it can force relayout when interaction state changes.

Use `ctx.interaction()` when component code needs to read the current interaction state:

```rust
let interaction = ctx.interaction();
let hovered = interaction.is_hovered();
```

Attach the state to an element with `.interactive(interaction)` if you need to observe that element's state from component code.

## Mouse Events

```rust
use lurq::app::events::MouseEvent;

Rect::new(100.0, 40.0)
  .on_mouse_down(|event: MouseEvent| println!("down {:?}", event.button))
  .on_mouse_up(|event: MouseEvent| println!("up at {}, {}", event.x, event.y))
  .on_click(|event: MouseEvent| println!("click target {:?}", event.target_id))
  .on_dblclick(|event: MouseEvent| println!("double click {:?}", event.target_id))
  .on_mouse_move(|event: MouseEvent| println!("move {}, {}", event.x, event.y))
  .on_mouse_enter(|| println!("enter"))
  .on_mouse_leave(|| println!("leave"))
```

Hover follows the content under a pointer that holds still: after a layout that moved anything (a wheel scroll, a list that grew, a stick-to-bottom chat taking a new message), lurq hit-tests the hover again at the pointer's last position, as a browser does. The node that moved away gets `on_mouse_leave` and loses its `hovered` style, the node now under the pointer gets `on_mouse_enter`, and the frame that shows the new layout already shows the result, so a hover tooltip never stays behind its row. Nothing is hovered again while the pointer is outside the window or a drag holds it. The handlers run with the source of the input that last moved the pointer (see [Synthetic Input](../ctx/#synthetic-input)), so an agent's pointer stays synthetic. A hover style that moves its own node off a still pointer (a `hovered` size, offset or transform) unhovers it, which moves it back, which hovers it again: like CSS `:hover`, such a node flickers, so keep hover styles from moving the node away from the pointer.

`MouseEvent` includes `x`, `y`, `button`, `kind`, `target_id`, the modifiers `shift`, `ctrl`, `alt` and `meta` (Cmd on macOS, the Windows key on Windows), and `from_keyboard`: a click that Enter on a focused link, or Enter or Space on a focused button, made, with the key's modifiers, `button` `Left` and `x`, `y` at 0. A shell that drives a `Tree` itself passes Meta with `Tree::mouse_move_with_meta`, `mouse_down_with_meta` and `mouse_up_with_meta`. See [Event Control](#event-control) for `prevent_default()` and propagation methods.

Each `on_*` modifier appends a handler for that rendered node, so a node can have multiple handlers for the same event. Inline closures are render output: when the node is rendered again, the rendered handler list should replace the previous list.

Use a stable `EventHandler` when you need to remove the exact handler later:

```rust
use lurq::{app::events::MouseEvent, node::EventHandler};

let handler = EventHandler::new(|event: &MouseEvent| {
  println!("click target {:?}", event.target_id);
});

let node = Rect::new(100.0, 40.0)
  .on_click(handler.clone())
  .off_click(handler);
```

Use `ctx.on_click_outside` with an element ref when a component needs to react to clicks outside one of its own nodes:

```rust
let menu_ref = ctx.element_ref();
let open = self.open.clone();
ctx.on_click_outside(menu_ref.clone(), move |_| open.set(false));

Column::new()
  .ref_element(menu_ref)
  .child(Text::new("Menu"))
```

The hook listens for left clicks outside the referenced element's measured bounds. It is removed automatically when the component stops calling it during render.

## Keyboard And Focus

Keyboard events go to the focused node. A press where nothing can take focus blurs it; a press on a `focusable(false)` element keeps it. Which elements take focus, how Tab and Shift+Tab move between them, modal focus traps, and scrolling focus into view are described in [Focus And Keyboard Navigation](../focus-navigation/).

Inside a component, request focus with `ctx.focus(&field_ref)`, where `field_ref` is a retained `core::ElementRef` attached through `.ref_element(field_ref.clone())`. The request is applied after the render is reconciled, including when a newly mounted route creates the field, and the field is then scrolled into view. The last request wins; a ref absent from the resulting tree is ignored. `field_ref.focused()` subscribes the rendering component to focus changes; `field_ref.focus_signal()` exposes the same state for observation. `field_ref.focus_visible()` and `field_ref.focus_visible_signal()` do the same for [focus-visible](../focus-navigation/#focus-visible), the keyboard-only part of focus.

Retained input and select value signals, element refs, explicit IDs, keys and component slots keep focus attached to the same control across sibling insertion/reordering. Removing the focused control emits its `on_blur` callbacks and clears its ref, including when the whole tree is dropped. Use explicit keys or IDs for otherwise anonymous reorderable controls.

```rust
use lurq::app::events::KeyboardEvent;

Text::new("Focusable")
  .on_focus(|| println!("focused"))
  .on_blur(|| println!("blurred"))
  .on_key_down(|event: KeyboardEvent| {
    println!("key={} code={} shift={}", event.key, event.code, event.shift);
  })
```

`KeyboardEvent` includes `key`, `code`, `shift`, `ctrl`, `alt`, `meta`, `target_id`, `text_input_focused` and `composing`. The keys an input method takes never reach `on_key_down` handlers or lurq's own key handling, so an app that sends a message on `Enter` does not send when `Enter` confirms a composition; a press that arrives while a composition is shown has `composing` set, and lurq's own defaults ignore an `Enter` so marked. See [Input Methods](#input-methods).

User `on_key_down` handlers run before built-in keyboard defaults, so they can block text editing, focused-button activation, select navigation, modal or popup Escape dismissal, and similar defaults:

```rust
use lurq::app::events::KeyboardEvent;

TextInput::new(value)
  .on_key_down(|event: KeyboardEvent| {
    if event.key == "Tab" {
      event.prevent_default();
    }
  })
```

## Text Selection

Plain text is not selectable by default. Opt in with `.selectable(true)`:

```rust
lurq::components::Text::new("Drag, double-click, or triple-click this text")
  .selectable(true)
```

Selectable text supports pointer drag ranges, double-click word selection, and triple-click line selection. A Shift press extends the selection from where it started (the pressed point of the last plain press, or the start of a double- or triple-click's word or line) to the pressed point, across texts too, and a drag with Shift held goes on from there; a Ctrl press starts another selection and keeps the ones in other texts. Escape clears every selection. Selection is visual-coordinate aware, so text inside transformed parents can still be selected from the painted position.

The selection follows the glyphs on screen: hit-testing, the highlight and the copied text index the layout the text is drawn with, span by span for Markdown, and the text's place in a box taller than it. Multiline and wrapped text render one selection highlight per selected row, covering the advances of the selected glyphs from the caret before the first to the caret after the last. On a row that the text wrapped, the whitespace and invisible marks at its end hang past it and draw nothing, so they are not highlighted; a row that ends with a line break or with the text keeps its trailing spaces highlighted. There is no marker for a line break.

`Tree::selected_text()` returns the selection as copying gives it: the texts drawn side by side on one line (the cells of a table row) in x order, texts stacked below each other one line each, and what a Markdown paragraph draws as presentation (a link's isolate marks and trailing runs) left out. With the `clipboard` feature enabled, `Ctrl+C` and `Ctrl+Insert` copy that text to the system clipboard.

## Sensitive Text

Text the user must read but tooling must not, such as a one-time password, is marked with `.sensitive()`:

```rust
use lurq::{components::Text, core::Sensitive};

let code = Sensitive::new(one_time_code); // a signal of Sensitive<String> stays redacted too
Text::new(code.expose()).sensitive()
```

It is laid out and painted on screen like any text. Every inspector shows `•••` in its place (always three dots, whatever the length) with `sensitive=true`: the MCP tools (`lurq_read_tree`, `lurq_inspect`, `lurq_find_by_id`, `lurq_find_by_class`, `lurq_find`, and a button or other element named after it) and DevTools (element tree, inspector, snapshots). The app still reads the real text through its element handles.

Screenshots taken for an inspector cover the text: in `lurq_screenshot` (of a window, a region or an element) and in a DevTools node screenshot, every pixel its glyphs can paint, text shadow included, is an opaque grey bar, at the place and size the text has on screen, so the layout around it is unchanged. The bar is painted over the captured frame, so it also covers whatever the app draws over the text there. The window itself keeps showing the text. The app's own captures (`WindowHandle::screenshot`, `screenshot_region`, `screenshot_node`) and `Tree::painted_quads` show the text, as they show the screen.

Selection follows `.selectable(...)` as for any text and is off by default. A selectable sensitive text can be selected and copied with `Ctrl+C`, which no inspector sees; to let the user copy without selecting, give them a copy button that writes the value to the clipboard itself.

`.sensitive()` covers the element tree only. Keep the value in a `lurq::core::Sensitive<T>` wherever it is held: its `Debug` output and its DevTools rendering are `•••`, so a signal, store or component props holding it shows `•••` in DevTools' signal values and history and in the props inspector, and a `{:?}` log line does not reveal it. `Sensitive` has no `Display`; `expose()` returns the value where the app uses it.

## Scroll

Wrap content in one of the scroll components:

```rust
use lurq::{
  app::events::ScrollEvent,
  components::{Column, ScrollVertical, Text},
};

ScrollVertical::new(
  Column::new()
    .spacing(8.0)
    .child(Text::new("Row 1"))
    .child(Text::new("Row 2")),
)
.on_scroll(|event: ScrollEvent| println!("delta: {}, {}", event.delta_x, event.delta_y))
```

Set the default scrollbar style on the theme:

```rust
use lurq::{layout::scrollbar::{ScrollBarStyle, ScrollBarVisibility}, node::color::Color};

app.theme().set_scrollbar(ScrollBarStyle {
  visible: ScrollBarVisibility::Auto,
  width: 7.0,
  thumb_color: Color::from_hex("#64748b"),
  thumb_radius: 4.0,
  ..ScrollBarStyle::default()
});
```

Override the scrollbar on a specific scroll component:

```rust
use lurq::{layout::scrollbar::{ScrollBarStyle, ScrollBarVisibility}, node::color::Color};

ScrollVertical::new(content)
  .scrollbar(ScrollBarStyle {
    visible: ScrollBarVisibility::Auto,
    width: 7.0,
    thumb_color: Color::from_hex("#64748b"),
    thumb_radius: 4.0,
    ..ScrollBarStyle::default()
  })
  .scrollbar_hovered(|style| style.with_thumb_color(Color::from_hex("#94a3b8")))
```

`padding` (default 2) is the gap around the bar. `edge_inset` overrides the gap between the bar and the edge it runs along (right for a vertical bar, bottom for a horizontal one), and `end_inset` the gap between each end of the track and the container's edge; `.insets(edge, end)` sets both. Both are measured from the scroll container's outer bounds, border included, so a thumb clears an 11 px rounded corner with `end_inset` 11, and the track is the container's length minus twice `end_inset`. The thumb is `track × visible / content` long, at least `min_thumb_length`. A `Reserved` gutter is `width` plus twice the edge inset.

`visible` says when the bar shows. `ScrollBarVisibility::Auto` (the default) shows it while the content overflows, `Always` also while it fits, and `Never` (`ScrollBarStyle::hidden()`) paints no bar and takes no pointer for one, while the wheel, the keyboard and scroll requests still scroll. `ScrollBarVisibility::OnHover` (`ScrollBarStyle::on_hover()`) keeps the bar of overflowing content hidden until it is needed: it shows while the pointer is anywhere over the scroll container, while the content scrolls (the wheel, a scroll request, a text input bringing its caret into view) and while its thumb is dragged. Once none of these holds it stays for `fade_delay` (default 600 ms) and fades out over `fade_duration` (default 200 ms) along `fade_easing` (default `Easing::EASE_OUT`); `.fade(delay, duration)` sets both times, and a zero `fade_duration` hides it at once. A hidden bar is not painted and takes no pointer, so what lies under its place gets the hover and the press; a shown one, fading too, behaves as an `Auto` bar. The pointer coming straight onto a hidden bar's place shows the bar, which takes the pointer from its next move. `OnHover` works the same for scroll containers, `VirtualizedList` and a multiline `TextInput`; with `ScrollBarPlacement::Reserved` the gutter stays reserved while the bar is hidden. lurq does not read the system's reduced-motion setting: an app that follows it sets `fade_duration` to zero.

`.scrollbar_hovered(...)` receives the effective style, so it applies to either the theme default or the component override. It applies while the pointer is over the area where the scrollbar takes the pointer, described below.

Where a scrollbar takes the pointer depends on whether it visibly owns its lane. A `Reserved` gutter, or an overlay bar with a painted track (a `track_color` that is not fully transparent), takes the whole lane: the track along its length and, across it, the bar plus 4 px on each side (or the edge inset, if larger), so it covers the gutter and reaches the container's edge. A plain overlay bar, with the default transparent track, takes only its thumb plus 4 px around it on every side; the rest of the lane belongs to the content, so a trailing button or a menu option next to the bar keeps its clicks and hover. A bar shown by `ScrollBarVisibility::Always` while the content fits has nothing to scroll and takes no pointer.

Where a scrollbar takes the pointer, a press reaches no content underneath: none of its handlers, buttons, inputs or sliders, and no text selection. The scroll container itself and its ancestors still receive the press. A press on the thumb drags it. A press on the track of a bar that owns its lane pages one viewport toward the press, once per press (holding the button does not repeat), on Windows and macOS alike. Either way the scrollbar holds the pointer until the release, which clicks nothing. Where two bars overlap, the one painted on top wins: an outer scroll container's bar over the bars of the containers inside it, and in the corner of a two-axis scroller the horizontal bar over the vertical one. The mouse wheel and Shift+wheel are unaffected.

`ScrollEvent` includes `x`, `y`, `delta_x`, `delta_y`, `phase`, and `target_id`.

Scroll handlers run before the default scroll movement. Call `prevent_default()` to observe a wheel/scroll event without moving the scroll container:

```rust
use lurq::app::events::ScrollEvent;

ScrollVertical::new(content)
  .on_scroll(|event: ScrollEvent| {
    event.prevent_default();
  })
```

## Event Control

`MouseEvent`, `KeyboardEvent`, and `ScrollEvent` share the same control methods:

| Method | Effect |
| --- | --- |
| `event.prevent_default()` | Blocks runtime default behavior for that event. |
| `event.default_prevented()` | Returns whether a handler already prevented the default. |
| `event.stop_propagation()` | Stops later handlers for the same dispatched event path. |
| `event.propagation_stopped()` | Returns whether propagation has been stopped. |
| `event.stop_immediate_propagation()` | Stops later handlers on the current node and later nodes. |
| `event.immediate_propagation_stopped()` | Returns whether immediate propagation has been stopped. |

Propagation control and default-action control are separate. Use `stop_propagation()` when another handler should not see the event. Use `prevent_default()` when handlers may still run, but the runtime should not perform the event's built-in action.

```rust
use lurq::app::events::MouseEvent;

Rect::new(100.0, 40.0)
  .on_click(|event: MouseEvent| {
    event.stop_propagation();
  })
```

Common defaults that can be prevented include:

- focusing an input from mouse down,
- text input editing from key down,
- focused button activation from `Enter` or `Space`,
- single-line text input submit or blur on `Enter`,
- select keyboard navigation,
- modal or popup Escape dismissal,
- form submit from buttons or keyboard,
- popup outside-click dismissal,
- scroll container movement.

Public capture-phase handlers are not part of the general event API yet. The current model keeps dispatch simple: handlers receive the event, can stop later dispatch with propagation methods, and can block runtime defaults with `prevent_default()`.

## Inputs

Inputs are controlled by signals.

```rust
let checked = ctx.signal(false);
let volume = ctx.signal(50);
let name = ctx.signal(String::new());

Column::new()
  .child(lurq::components::Checkbox::new(checked.clone()))
  .child(lurq::components::Slider::new(volume.clone()).range(0, 100))
  .child(lurq::components::TextInput::new(name.clone()).placeholder("Name"))
```

Input updates write back to their signals, which rerenders the owning component.

`TextInput::on_input` runs before a built-in text edit is applied. The event carries the input's `Signal<String>` as `event.value` and the key that caused the edit as `event.keyboard`. Mutate the signal directly for custom input behavior, and call `event.prevent_default()` to cancel the built-in edit for that action:

```rust
use lurq::app::events::TextInputEvent;

TextInput::new(command.clone())
  .on_input(|event: TextInputEvent| {
    if event.keyboard.key == "Tab" {
      event.value.set("/play ".to_owned());
      event.prevent_default();
    }
  })
```

### Checkbox Styling

Checkboxes accept normal element modifiers such as `.size()`, `.background()`, `.border_inside()`, `.rounded()`, `.cursor()`, `.hovered()`, and `.focused()`. Generic `.background()` styles the unchecked box. Checked visuals use checkbox-specific styles so the checked state can have its own color or indicator. `.box_focused(...)` styles the box while the checkbox has keyboard focus ([focus-visible](../focus-navigation/#focus-visible): Tab or a key press, not a click), checked or not; it changes paint only.

A checkbox the app has not styled takes its fill from the theme: the box is `SurfaceInput` and the checked box `Accent`. A part the app styles without a fill paints none, like a button or text input without a background: `.box_part(...)` without `background` leaves the unchecked box unfilled, and `.checked_box(...)` without one the checked box (its border and indicator still paint). A checked box whose `checked_box` part the app did not set keeps the unchecked part's fill, or the theme's `Accent`. Generic `.background()` still fills the unchecked box.

```rust
use lurq::{components::Checkbox, core::Signal, node::color::Color};

let enabled = Signal::new(true);

Checkbox::new(enabled)
  .size(20.0, 20.0)
  .background("#ffffff")
  .border_inside(1.0, Color::from_hex("#94a3b8"))
  .rounded(4.0)
  .checked_box(|style| {
    style
      .background("#2563eb")
      .border_inside(1.0, Color::from_hex("#1d4ed8"))
      .rounded(4.0)
  })
  .box_hovered(|style| style.border_inside(1.0, Color::from_hex("#38bdf8")))
  .checked_box_hovered(|style| style.background("#1d4ed8"))
```

With the `image` feature enabled, checked boxes can render an indicator image centered inside the box:

```rust
use lurq::{components::Checkbox, images::ImageData};

let check = ImageData::from_file("assets/check.png").unwrap();

Checkbox::new(enabled)
  .checked_box(|style| {
    style
      .background("#16a34a")
      .indicator_image(check)
      .indicator_size(12.0, 12.0)
      .indicator_contain()
  })
```

With `image` and `resources`, the indicator can come from the app resource loader:

```rust
Checkbox::new(enabled)
  .checked_box(|style| style.indicator_image("ui/check.png").indicator_size(12.0, 12.0))
```

### Text Input Editing

Plain `Text` can align content inside its own box:

```rust
use lurq::{layout::Alignment, node::dimension::Dimension};

Text::new("No endpoints yet")
  .width(Dimension::Pct(100.0))
  .text_align(Alignment::Center)
```

`TextInput` keeps editing state internally while the string value remains signal-owned. Clicking focuses the input and places the caret. Dragging selects a range; double-click selects a word; triple-click selects a line. Multiline inputs support vertical caret movement and per-row selection highlights. `ArrowUp` and `ArrowDown` move the caret one laid-out row, so a soft-wrapped paragraph is crossed a wrapped line at a time; above the first row the caret goes to the start, below the last to the end. A run of them keeps the column it started from, so a short row on the way does not pull the caret to its end. After a value that ends with a line break, the end is the empty row below the last laid-out row.

A multiline input whose text needs more rows than its `rows(min, max)` allow scrolls its own text. Its caret's line stays in view while the person types or moves the caret. The wheel, a trackpad and its scrollbar move the text within it, and that scroll stays until the next edit or caret move brings the caret back; deleting lines brings the text back to its first line once it fits. While the text overflows, the input shows a vertical scrollbar along its right edge, styled and placed like a scroll container's: the theme's `ScrollBarStyle`, or the input's own `.scrollbar(...)` and `.scrollbar_hovered(...)`, with `edge_inset` and `end_inset` measured from the input's outer bounds, whatever its padding. The padding is the text's inset and scrolls with the text, as in a browser's `textarea`: scrolled down, the text runs through the top padding, and the bottom padding shows under the last line at the end; the caret's line is kept in view inside the padding. Its thumb drags and a press on its track pages, and the pointer over it shows the default cursor, whatever cursor the input sets. A bar placed `ScrollBarPlacement::Overlay` (the default) runs over the text; one placed `ScrollBarPlacement::Reserved` gets a lane of its own at the input's right edge, as wide as the bar and its edge inset on both sides (the wider of its resting and hovered styles), and the text wraps short of it, the right padding between them. Like a reserved scroll container's gutter, the lane stays while the text fits, so the text does not re-wrap when it starts or stops overflowing. A wheel over an input whose text fits, or one already at the end the wheel moves to, passes on to the scroll containers around it.

Lines, the caret and selections follow the font's metrics, not the ink of the glyphs shown: a multi-line input's lines sit in line boxes of the style's `line_height` from the top of the content box (`VerticalAlign::LineBox`), so a line does not move when a taller glyph is typed. The caret is `font_size` tall and a selection covers the font's ascent and descent, both centered in the line box, so the leading of a relaxed `line_height` stays unpainted. The caret starts at its insertion point; at the start of a left-aligned line it ends there instead, so it never covers the first glyph of the value or the placeholder (unless an ancestor clips right at that edge). A focused input's caret blinks every 530 ms unless its `caret_mode` or the theme's is `CaretMode::Persistent`; an idle window wakes only at each toggle.

Single-line inputs can align value and placeholder text inside their content box:

```rust
use lurq::layout::text_style::TextAlign;

TextInput::new(endpoint.clone())
  .placeholder("Connect to an endpoint to get started.")
  .single_line()
  .text_align(TextAlign::Center)
```

Password inputs can hide their contents with `.mask()`, which renders a bullet (`•`, U+2022) for each character instead of the typed text. Use `.mask_char(...)` for a custom mask character, such as `.mask_char('\u{25cf}')` for a WinUI-style heavy dot (`●`) or `.mask_char('*')` for the previous default. Use `.unmask()` to clear masking:

```rust
TextInput::new(password.clone())
  .placeholder("Password")
  .single_line()
  .mask()

TextInput::new(pin.clone())
  .single_line()
  .mask_char('#')

TextInput::new(visible_secret.clone())
  .mask()
  .unmask()
```

Masking changes displayed text and built-in node inspection. The signal value, clipboard copy/cut, and caret and selection behavior all operate on the real text.

Built-in MCP and DevTools inspection returns the displayed mask and `masked=true`, including tree text, lookup/find data, shape details and set-value replies. Underlying editing/form values remain intact. Typed `TextInputHandle::mask()` and `is_masked()` expose the configuration; direct application access to `value()` still returns the real value. An empty masked field can still display its ordinary placeholder.

Mask glyphs use the normal text shaping and font fallback chain. If the selected font lacks U+2022, the shaper searches the loaded and system fallback fonts for the bullet. If no available fallback contains it, the font's missing-glyph marker may appear; Lurq does not substitute `*`. Load a font containing U+2022 or choose a supported custom mask in that case.

Keyboard editing supports character insertion, `Backspace`, `Delete`, arrow keys, `Home`, `End`, `Ctrl+A`, `Ctrl+Z`, `Ctrl+Y`, and `Ctrl+Shift+Z`. Hold `Shift` with movement keys to extend the selection; hold `Ctrl` with horizontal movement to jump by words.

With the `clipboard` feature enabled, text inputs also support `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+Insert`, `Shift+Insert`, and `Shift+Delete` (`Cmd` for `Ctrl` on macOS). Without `clipboard`, those shortcuts do not read or write the system clipboard.

#### Paste

A paste into a text input delivers one `PasteEvent` before the text is inserted: `Ctrl+V`, `Shift+Insert`, a `WindowHandle::paste_from_clipboard()` request (for an app's own Paste menu item or context menu) and `Tree::paste(content)` all go through it. The event carries everything the clipboard holds, as [`lurq::clipboard::read_clipboard`](../resources-media/#clipboard) reads it: text, an RGBA image (a screenshot) and file paths (files copied in Explorer or Finder), any of them at once. Handlers run on the input first and bubble to its ancestors (`stop_propagation()` ends that); unless one calls `prevent_default()`, the text is inserted at the caret. An image or files without a handler do nothing.

```rust
use lurq::{app::events::PasteEvent, components::TextInput};

TextInput::new(draft.clone())
  .multiline()
  .on_paste(move |event: PasteEvent| {
    if let Some(image) = event.image() {
      attachments.update(|list| list.push(Attachment::png(image.to_png().unwrap())));
      event.prevent_default(); // no text goes into the input
    } else if !event.files().is_empty() {
      attachments.update(|list| list.extend(event.files().iter().cloned().map(Attachment::file)));
      event.prevent_default();
    } else if event.text().is_some_and(|text| text.len() > 4_000) {
      attachments.update(|list| list.push(Attachment::text(event.text().unwrap())));
      event.prevent_default();
    }
    // plain text pastes as always
  })
```

`PasteEvent::kinds()` tells which of `text`, `image` and `files` the clipboard holds; `content()` gives the whole `ClipboardContent`. The handler runs inside the input's delivery, so `is_synthetic_input()` tells an MCP client's paste from a person's. A synthetic `Ctrl+V` (`lurq_interact` `key`, an applied `SyntheticInput::key_down`) never reads the OS clipboard: it is refused, and a synthetic paste carries its own content instead (`SyntheticInput::paste(content)`, the MCP `paste` action). See [Testing](../testing/#pastes-file-drags-and-runtime-images).

#### Input Methods

Text inputs take input method (IME) composition on Windows and macOS: Japanese, Chinese and Korean input, dead keys and the macOS accent menu. While a text input that is not masked has focus, the winit shell lets the window's input method compose and places its candidate window at the input's caret; password fields (`.mask()`) take no composition, like the platforms' native ones.

- While composing, the input shows the composition text at the caret, in place of the selection, underlined in the text colour, with the caret where the input method puts it. The value does not change and `on_input` does not fire until the composition is committed.
- The commit inserts the text like typing: `on_input` handlers run first, with a `KeyboardEvent` whose `key` is the committed text, whose `code` is empty and whose `composing` is set, and can prevent it. A cancelled composition leaves the value as it was. Moving focus away cancels a composition.
- The input method takes the keys it uses, including the `Enter` that confirms a composition: Windows reports each as a press of the key `"Process"` with the physical key as its `code`, macOS does not report it at all. These presses reach neither `on_key_down` handlers nor lurq's defaults (newline, form submit, button activation); their releases reach `on_key_up` handlers with `KeyboardEvent::composing` set. A key the input method passes on while a composition is shown, such as a space after a Korean syllable, arrives as usual with `composing` set and is handled like any key, so nothing is lost and a composition the platform never ends holds no key back. The exception is `Enter`: one that arrives marked `composing` reaches `on_key_down` handlers, but no lurq default acts on it (form submit, button or select activation, newline, blur), as on the web, where apps ignore an `Enter` with `isComposing`. A form exposes no key handler, so it could not tell such an `Enter` apart itself; once the composition is committed, `Enter` submits as usual. A new input method session (`ImeEvent::Enabled`) or a focus change ends a composition left behind.

```rust
use lurq::app::events::KeyboardEvent;

TextInput::new(draft.clone())
  .multiline()
  .on_key_down(move |event: KeyboardEvent| {
    // The confirming Enter never arrives; a press passed on while composing is marked.
    if event.key == "Enter" && !event.shift && !event.composing {
      event.prevent_default();
      send();
    }
  })
  .on_key_up(|event: KeyboardEvent| {
    if event.composing {
      return;
    }
    // ...
  })
```

`Tree::is_composing()` and `TextInputHandle::composition()` report a composition in progress. A shell other than winit forwards its platform's input method events through `Tree::ime(ImeEvent)` and asks `Tree::ime_allowed()` and `Tree::ime_cursor_area()` (logical pixels) whether, and where, the input method should compose; tests drive composition the same way (see [Testing](../testing/#input-methods)).

### Slider Styling

`Slider::new` uses `Signal<i32>`. Pointer input maps the track position into the range, and the default keyboard step is `1`. Use `Slider::new_f32` with `Signal<f32>` and `.range_f32(min, max)` for fractional values; `.step(value)` controls snapping and keyboard increments.

An unstyled slider takes its fills from the theme: the track `Border`, the thumb `Accent` (generic `.background()` fills the track). A `.track(...)` or `.thumb(...)` part without a `background` paints no fill for that part.

```rust
let gain = lurq::core::Signal::new(0.5_f32);
lurq::components::Slider::new_f32(gain).range_f32(0.0, 1.0).step(0.05);
```

The slider frame still accepts normal modifiers like `.width()`, `.height()`, `.cursor()`, and `.focused()`. Track and thumb visuals are styled separately with `SliderPartStyle`; `.thumb_focused(...)` styles the thumb while the slider has keyboard focus ([focus-visible](../focus-navigation/#focus-visible): Tab or a key press, not a click or drag) and changes paint only.

```rust
use lurq::{components::Slider, core::Signal, node::color::Color};

let value = Signal::new(68);

Slider::new(value)
  .range(0, 100)
  .width(260.0)
  .height(34.0)
  .track(|style| {
    style
      .size(220.0, 2.0)
      .background("#334155")
      .rounded(1.0)
      .border_center(1.0, Color::from_hex("#64748b"))
  })
  .track_hovered(|style| {
    style
      .height(4.0)
      .background("#475569")
      .border_center(1.0, Color::from_hex("#93c5fd"))
  })
  .thumb(|style| {
    style
      .size(12.0, 12.0)
      .background("#f97316")
      .rounded(6.0)
      .border_inside(2.0, Color::from_hex("#0f172a"))
  })
  .thumb_hovered(|style| {
    style
      .size(14.0, 14.0)
      .background("#fb923c")
      .rounded(7.0)
      .border_inside(2.0, Color::from_hex("#f8fafc"))
  })
```

The track and thumb support width, height, background color, border, corner radius, image backgrounds, and hover overrides. Corner radius accepts `f32` or `RadiusSize`. Hover dimensions are included in the slider's preferred size, so a larger hover thumb does not resize surrounding layout when the pointer enters.

The thumb is centered on the track line, not on the slider frame. A `2px` track with a `10px` or `14px` thumb keeps the thumb vertically centered on that thin track.

Image-backed slider parts use the same `image` feature as node background images:

```rust
use lurq::{components::Slider, images::ImageData};

let track = ImageData::from_file("assets/track.png").unwrap();
let thumb = ImageData::from_file("assets/thumb.png").unwrap();

Slider::new(value)
  .track(|style| style.height(2.0).background_image(track).background_cover())
  .thumb(|style| style.size(16.0, 16.0).background_image(thumb).background_cover())
```

With `image` and `resources`, pass resource paths instead:

```rust
Slider::new(value)
  .track(|style| style.background_image("ui/slider-track.png").background_cover())
  .thumb(|style| style.background_image("ui/slider-thumb.png").background_cover())
```

### Select

`Select::new` binds a `Signal<T>` for one value and `Select::multiple` a `Signal<Vec<T>>`. Options are `(value, label)` tuples or `SelectOption`s, which add a detail line under the label and a disabled state:

```rust
use lurq::components::{Select, SelectOption};

Select::new(plan.clone())
  .placeholder("Plan")
  .options([
    SelectOption::new(Plan::Free, "Free"),
    SelectOption::new(Plan::Team, "Team"),
    SelectOption::new(Plan::Enterprise, "Enterprise")
      .detail("Contact sales to enable")
      .disabled(true),
  ])
```

A disabled option cannot be chosen by pointer or keyboard, never takes the hover or keyboard highlight, and is skipped by arrow keys, `Home`/`End` and type-ahead.

A pointer press on the trigger opens the menu without a highlight. On the focused select:

| Key | Closed | Open |
| --- | --- | --- |
| `ArrowDown`, `ArrowUp` (with or without `Alt`) | Open with the selected option highlighted, else the first enabled one | Move the highlight over enabled options, stopping at the ends. After a pointer open, the first arrow highlights the selected option, else the first enabled one |
| `Home`, `End` | | Highlight the first or last enabled option |
| `Enter`, `Space` | Open like the arrows | Choose the highlighted option: single-select closes, multi-select toggles it and stays open. With no highlight the menu closes without a change |
| Letters | | Type-ahead: highlight the next enabled option whose label starts with the typed text. Repeating one letter cycles through its options; typing within a second extends the text, `Space` included |
| `Escape`, `Tab` | | Close without a change |

The highlighted option scrolls into view, and opening scrolls the selected option into view. Focus stays on the select throughout. A press outside closes the menu without a change. By default that is all it does, like `Escape`: the select keeps focus, and the element under the pointer receives no press or click until the next press. A press on another dropdown's trigger (another select, or the anchor of a dismissible popup) is the exception: it closes the menu and opens that dropdown in one press. `Select::outside_press(OutsidePress::PassThrough)` delivers the closing press as well; it then follows the usual press rule: it blurs the select, or focuses the pressed element if that can take focus. See [Popups And Outside Presses](../modals/#popups-and-outside-presses).

A select is identified across re-renders by the signal it binds, like a `TextInput`: its open menu, keyboard highlight and focus stay with it when siblings are inserted, removed or reordered, and never pass to another select. Create the signal once (in `create` or as a field), not per render; a new signal each render is a new select, so its menu closes on every re-render. The highlight survives a re-render only while its row shows the same enabled option; when options are replaced it is cleared, so `Enter` closes without a change.

`SelectStyle` styles the trigger, the menu and the options with `SelectPartStyle`s. Colours, border sizes, radii, spacing, typography and shadows accept theme roles:

```rust
use lurq::{
  app::theme::{PaletteColor, ShadowStyle, TypographyStyle},
  node::{BoxShadow, SelectCheckmarkPosition, SelectIcon, SelectPartStyle, SelectStyle, padding::Padding},
};

// "icon" is an app typography role that selects the icon font at 14 px.
let icon = |glyph: &str| SelectIcon::glyph(glyph, TypographyStyle::extra("icon"));
let ring = BoxShadow::new(0.0, 0.0, 0.0, PaletteColor::BorderFocus).spread(1.0).inset();

SelectStyle::new()
  .trigger_open(SelectPartStyle::new().border_inside(1.0, PaletteColor::BorderFocus))
  .chevron(icon("\u{e06d}"))
  .chevron_open(icon("\u{e070}"))
  .chevron_color(PaletteColor::TextMuted)
  .menu(
    SelectPartStyle::new()
      .background(PaletteColor::SurfaceRaised)
      .border_inside(1.0, PaletteColor::Border)
      .rounded(11.0)
      .padding(Padding::all(4.0))
      .box_shadow(ShadowStyle::Lg),
  )
  .max_menu_height(266.0)
  .option(
    SelectPartStyle::new()
      .min_height(32.0)
      .padding(Padding::symmetric(8.0, 0.0))
      .rounded(7.0)
      .typography(TypographyStyle::Body),
  )
  .option_hovered(SelectPartStyle::new().background(PaletteColor::SurfacePanel))
  .option_selected(SelectPartStyle::new())
  .option_selected_hovered(SelectPartStyle::new().background(PaletteColor::SurfacePanel))
  .option_highlighted(SelectPartStyle::new().box_shadow(ring))
  .option_disabled(SelectPartStyle::new().text_color(PaletteColor::TextMuted))
  .single_checkmark(true)
  .checkmark(icon("\u{e06c}"))
  .checkmark_size(14.0)
  .checkmark_position(SelectCheckmarkPosition::Trailing)
  .checkmark_color(PaletteColor::Accent)
```

- An option row merges `option`, then `option_selected`, the pointer hover (`option_hovered`), the keyboard highlight, `option_selected_hovered`, the highlighted-and-selected part, and `option_disabled` last. Without `option_highlighted`, the keyboard highlight looks like the hover (`option_hovered`, `option_selected_hovered`). With it, the highlight has its own look, such as an inset ring from an inset `box_shadow`, which composes with the hover fill; `option_selected_highlighted` then styles a highlighted selected option. The pointer hover changes a row's fill, border and shadow.
- `SelectPartStyle` has `background`, borders, `rounded`, `padding`, `min_width`, `min_height`, `box_shadow`, `opacity` (menu and options), and text as a `typography` role, an explicit `text` style and a `text_color`. The default `option_disabled` is `opacity(0.45)`. `option_detail` styles detail lines; the default is `Caption` in `TextMuted`.
- The menu opens below the trigger at the trigger's width, `menu_gap` (default 4) away, or above it when there is no room below. `min_menu_width(260.0)` lets a menu grow wider than a narrow trigger; it is still capped by the viewport width and repositioned to fit. `max_menu_height` (default 240) limits its height; the options scroll inside. `menu_scrollbar` replaces the theme scrollbar; its insets are measured from the menu's outer bounds, border included (see [Scroll](#scroll) for `edge_inset`/`end_inset`). The menu's padding insets the options and scrolls with them.
- Option rows are at least 34 px tall unless the option part sets `min_height`. The default option part pads labels by 10 px horizontally, like the trigger.
- The chevron defaults to the glyph `▾` at `chevron_size` (default 10) in the trigger's text style. `chevron` replaces it with a `SelectIcon`: `SelectIcon::text` (a glyph in the part's explicit text style), `SelectIcon::glyph` (a glyph in a typography role, such as an icon font) or `SelectIcon::element` (an app-built element; the supplier receives the configured colour). `chevron_open` is drawn instead while the menu is open.
- Multi-select marks chosen options with a checkmark. `single_checkmark(true)` also shows it on a single-select's selected option. It is off by default: a single-select marks its selection with `option_selected` only. When checkmarks are shown, every option reserves the checkmark slot (`checkmark_size`, default 16 wide), so labels do not shift. `checkmark_position` puts the slot before or after the label, `checkmark_gap` (default 6) separates them, and `checkmark` replaces the `✓` glyph with a `SelectIcon`.
- `Select::trigger(|state| ...)` replaces the trigger content. It receives the label, placeholder and selection.
- The trigger's fill is its part's `background`: the theme's `SurfaceInput` in the default `SelectStyle`. A trigger part without a `background`, as in `SelectStyle::unstyled()` or a part passed to `.trigger(...)` that sets none, paints no fill, like a button or text input without a background, so an app that styles the trigger itself (or puts it on a surface of its own) sees that surface. Its border, radius and shadows still paint. `Tree::painted_quads()` shows the fill in tests (see [Testing](../testing/#painted-output-without-a-window)).

## Programmatic Interaction

Nodes tagged with `.id("...")` can be driven imperatively from integration code and tests, browser-DOM style. Look the node up on `Tree` and use the handle's universal actions or a typed downcast:

```rust
Column::new()
  .child(TextInput::new(email.clone()).id("email"))
  .child(Checkbox::new(agree.clone()).id("agree"))
  .child(Button::new("Save").id("save").on_click(on_save))
```

```rust
// DOM el.click(): fires the node's own on_click at its bounds center
// without hit-testing, focuses focusable nodes, submits for submit buttons.
tree.get_element_by_id_mut("save").unwrap().click();

// DOM el.value = x: writes the backing signal and clamps the caret,
// but does NOT fire on_input handlers.
tree.get_element_by_id_mut("email").unwrap()
  .as_text_input().unwrap()
  .set_value("ada@example.com");

// Widget default actions live on the typed handles.
tree.get_element_by_id_mut("agree").unwrap().as_checkbox().unwrap().toggle();
```

`focus()` and `blur()` route through the tree's focus machinery and fire the node's own `on_focus`/`on_blur` handlers. Typed handles exist for `TextInput`, `Checkbox`, `Slider`, and `Select`; downcasting a different node kind returns `None`.

These operations write signal-backed widget state, so they behave like real user input from the app's perspective — minus the event side effects called out above. For pointer-fidelity interaction (hover, capture, hit testing), drive `tree.mouse_down` / `tree.mouse_up` instead, composing coordinates from the handle's `bounds().center()`. See [Runtime And Retained Tree](../retained_nodes/#ids-and-classes) for the lookup and mutation contract.

## Drag And Drop

Use high-level DnD components when you want draggable nodes and drop zones.

```rust
use lurq::components::{
  DragContainer, DragContainerProps, Draggable, DraggableProps, DropZone, DropZoneProps, Rect, Stack,
};

let card = Draggable::mount(
  ctx,
  DraggableProps::new().on_drag_end(|event| {
    println!("drop result: {:?}", event.drop_result);
  }),
  Rect::new(64.0, 64.0)
    .background("#2563eb")
    .absolute_position(24.0, 24.0),
);

let zone = DropZone::mount(
  ctx,
  DropZoneProps::new().on_drop(|event| {
    println!("source {:?} dropped on {:?}", event.source_id, event.target_id);
  }),
  Rect::new(160.0, 100.0)
    .background("#16a34a33")
    .absolute_position(180.0, 80.0),
);

DragContainer::mount(
  ctx,
  DragContainerProps::new(),
  Stack::new()
    .size(420.0, 240.0)
    .child(zone)
    .child(card),
)
```

`DragContainerProps::new()` bounds descendant draggables to the container surface. Use `DragBounds::None` when the draggable should not be constrained.

Low-level node drag handlers are also available: `.on_drag_start`, `.on_drag_move`, `.on_drag_end`, and `.on_drop`.

## File Drag And Drop

Files dragged into the window from the OS (Explorer, Finder, another program) reach the element under the pointer, with the window's overlays and modals hit-tested as for the mouse, so a drag over a popup reaches the popup:

```rust
use lurq::{app::events::{FileDragEvent, FileDropEvent}, components::Column};

Column::new()
  .background("#1f2937")
  .drag_over(|style| style.background("#1e3a8a").border_inside(1.0, "#60a5fa".into()))
  .on_file_drag_enter(|event: FileDragEvent| {
    if event.paths.has_folders() {
      event.refuse(); // folders are not attachments
    }
  })
  .on_file_drop(move |event: FileDropEvent| {
    for path in event.paths.files() {
      attachments.update(|list| list.push(Attachment::file(path.to_path_buf())));
    }
  })
```

- `on_file_drag_enter(FileDragEvent)` fires once when the drag comes over the element, `on_file_drag_over(FileDragEvent)` on every move over it (with `x`, `y` in logical pixels, as on a `MouseEvent`), `on_file_drag_leave()` when it moves off the element or leaves the window without a drop, and `on_file_drop(FileDropEvent)` on the drop. Enter, over and drop bubble from the element under the pointer to the root; `stop_propagation()` ends that.
- `event.paths` is a `DraggedPaths`, given once for the whole drag however many files it holds: `paths()`, `files()`, `folders()`, `has_folders()` and `items()` (each a `DroppedPath` with `is_folder()`, looked up once with `Path::is_dir` on the shell's thread when the drag enters; a path on a slow network drive makes that look-up slow). Only files and folders arrive: text or an image dragged from a browser is ignored, since winit reports nothing for it.
- While the drag is over an element, its `.drag_over(...)` style applies, like `hovered` under the pointer, and `InteractionState::is_drag_over()` reports it. `event.refuse()` (or `prevent_default()`) on an enter or over event refuses the drag at the element whose handler called it (not at its ancestors or siblings) for as long as the drag stays over that element: no element shows the style and `Tree::file_drag_accepted()` reports `Some(false)` while it does; the drag leaving the element and entering it again asks again. The OS cursor is not changed by a refusal: winit answers the OS with a copy effect before lurq sees the drag, on Windows and macOS alike. The drop is still delivered to `on_file_drop`, whose handler ignores what it refused (the folders in the example).
- A drop ends the drag: the `drag_over` style goes without an `on_file_drag_leave`.

The winit shell collects the OS's burst of one event per file into one session on Windows and macOS, and reads the pointer position from the OS while the drag hovers (the OS owns the pointer during a drag, so no `CursorMoved` arrives), every 16 ms, delivering `on_file_drag_over` when it moved. A hover ends as a leave when no mouse button has been pressed for 250 ms of polls (the OS drag ended without winit telling; read from the OS on Windows and macOS — the grace period covers the release, which the OS reports before its drop callback runs, so a drop within it continues the same session without a leave, and it covers drags whose button state is reported differently, a touch or pen drag promoted to a mouse drag, ClickLock, macOS drag lock, which end after the grace period at most and have not been checked), or when it neither moved nor got a drop or a cancel for 60 s, since its source is gone (a program that died mid-drag) and polling would never stop; a drag held still that long is ended too, and a drop that comes later still arrives. On Linux the last pointer position the window saw is used. A shell that drives a `Tree` itself calls `Tree::file_drag_enter(paths, x, y)`, `file_drag_over(x, y)`, `file_drag_leave()` and `file_drop(paths, x, y)` with physical pixels.
