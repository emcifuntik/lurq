---
title: Blur
description: Blurring an element with its content, and frosted glass that blurs what is behind an element.
---

# Blur

lurq has two blurs, and they answer different questions:

| Modifier | What is blurred | CSS counterpart | Typical use |
|---|---|---|---|
| `.blur(radius)` | The element and everything inside it | `filter: blur()` | A panel going out of focus, a placeholder, a transition |
| `.backdrop_blur(radius)` | What is painted behind the element, inside its box | `backdrop-filter: blur()` | Frosted glass: toolbars, sheets, a scrim under a modal |

Both work on every node and component, on the wgpu renderer and the native DX12 one, and need no feature flag.

## The Radius

`radius` is in logical pixels and means what a shadow's blur radius means: the Gaussian's standard deviation is half of
it. `blur(8.0)` softens an edge as `BoxShadow::new(0.0, 0.0, 8.0, ..)` softens a shadow's, and as the canvas's
`Filter::Blur(8.0)` does.

- CSS's `blur(4px)` names the deviation itself, so it is `blur(8.0)` here. A design tool's layer blur of 8 is
  `blur(8.0)`.
- `0` (the default) is no blur, and costs nothing: no layer, no extra pass.
- The radius is clamped to `layout::opacity_layer::MAX_BLUR_RADIUS` (256). Negative and non-finite radii are no blur.
- The radius is in the element's own units. Under a `.transform(...)` that scales the element, its own or an
  ancestor's, the blur scales with it: `blur(8.0)` on an element scaled by 2 looks like `blur(16.0)` unscaled. A
  transform that scales the two axes differently blurs by their geometric mean, the same along both.

## Blurring An Element

`.blur(radius)` paints the element's subtree into an offscreen layer and draws the layer through a Gaussian blur. What
is behind the element is not touched.

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

- The blur spreads past the element's box by three deviations, which is `1.5 * radius`. The element's layout box does
  not change: neighbours do not move, and the spread paints over them.
- Containers clip by default, so a blur is cut at its parent's edge like any child that paints outside its parent.
  Give it room with padding, or call `.overflow_visible()` on the containers it should escape.
- Where the element is cut by a clip or by the window's edge, the part of it just past the cut still bleeds into what
  shows. A blurred element scrolled half out of a list, or a blurred page that fills the window, does not fade out
  along the cut. A rounded clip cuts the blur with its corners.
- `.blur(...)` and `.opacity(...)` on one element share a layer: the blurred result is faded.
- Blur changes painting only. Layout, scrolling and hit testing are the same at any radius: a blurred button still
  takes clicks.

## Frosted Glass

`.backdrop_blur(radius)` blurs what is already painted behind the element, inside the element's box, and the element
then paints itself over the result. The blur shows wherever the element is translucent, so give it a translucent
background; an opaque one hides it.

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

- The blur fills the element's box with its corner radii and does not spread past it. It is cut by the clips the
  element is under, their rounded corners included.
- Under a `.transform(...)` the blur fills the transformed box: turned, scaled or skewed as the element is.
- At the window's edge the blur reads the edge pixels as if they went on, so glass against an edge does not darken
  there.
- The element's own `.opacity(...)` fades the blur with it: at `0.5` the blurred backdrop is mixed half and half with
  the sharp one.
- `Transition::backdrop_blur()` animates the radius.

### What Counts As Behind

"Behind" is everything painted before the element: earlier siblings, the backgrounds of its ancestors, the page under
an overlay. Content painted after the element, its own children included, is painted over the glass and stays sharp.

Inside an ancestor with `.opacity(...)` below 1 or a `.blur(...)`, behind is only what that ancestor has painted so far,
as in CSS: the ancestor is drawn into a layer of its own, and the glass does not see through the layer to the window.
A glass panel that is the only thing inside a faded wrapper therefore blurs nothing. Put the opacity on the glass
element itself, or keep the content it should blur inside the same wrapper.

## Animating

Both radii animate like any other style value, in transitions and in keyframe animations:

| Transition | Property |
|---|---|
| `Transition::blur()` | `AnimatableProperty::Blur` |
| `Transition::backdrop_blur()` | `AnimatableProperty::BackdropBlur` |
| `Transition::all()` | Both, with everything else |

Like opacity, an animating blur changes painting only: the element keeps its layout cache while the radius runs, so a
blur transition lays nothing out again.

## Cost

- `.blur(...)`: the cost follows the layer's pixels, not the radius. A wide blur runs on a reduced copy of the layer,
  so `blur(64.0)` is not slower than `blur(8.0)`. A blurred layer takes three or four pooled textures for its frame
  and is painted for `1.5 * radius` more on each side than it shows, which is what the radius limit bounds.
- `.backdrop_blur(...)`: each one stops the frame's drawing, copies the pixels around the box and blurs the copy. That
  is a few passes and pooled textures of about the box's size. A handful of glass panels is cheap. A long list in
  which every row is glass is not what it is for: blur the list's backdrop once with one element instead.
- Textures are pooled and reused across frames, and released after a while without use. A frame without blurs
  creates none.

## Checking The Result

- A devtools node screenshot draws both blurs on the CPU. Its capture is the node's own size, so what lies outside it
  does not bleed in at the capture's edges.
- `lurq_screenshot` over MCP and frame captures read the rendered frame back, blurs included.
- `cargo run -p lurq --example layer_blur_capture_check --features screenshot,wgpu,dx12` renders a scene of blurred
  nodes and glass through both renderers and compares the captures.

## For Render Engines

A custom `RenderEngine` sees the blurs in the `RenderList`:

- A blurred element is a `LayerCmd` with `blur: Some(LayerBlur)`. `LayerBlur` has the deviation in physical pixels, the
  rectangle the layer's content is painted for (`LayerCmd::painted()`; larger than the `bounds` it shows in, and
  possibly reaching outside the window), and the rounded clip to cut the result to, if the element is under one.
- A backdrop blur is a `BackdropCmd` in `RenderList::backdrops`: the box, its radii, transform and clip, the deviation
  in physical pixels, and the render order it happens at. No draw has that order: the blur comes after every draw of a
  lower order and before every draw of a higher one, in the target those draws are in.
- `RoundedBox` (`layout::opacity_layer`) describes a rounded box that may be turned or scaled, with its bounds and its
  coverage of a pixel.

An engine that ignores `LayerCmd::blur` draws the layer unblurred, and one that ignores `RenderList::backdrops` draws
the frame without its glass.
