---
title: Animation And Transforms
description: Transitions, keyframe animations, easing, animatable properties, and 2D transform behavior.
---

# Animation And Transforms

`lurq` has two timeline systems:

- transitions, which animate a property when its target value changes,
- keyframe animations, which play a registered sequence on a node.

Both systems are owned by `Tree` and tick during the normal runtime pass. While either engine has active timelines, the shell asks for a frame each time they next draw something else (see [Frame Pacing](#frame-pacing)).

## Transitions

Attach transitions to an element with `.transition(...)`. A transition watches the selected animatable property. When the target value changes, the transition engine interpolates from the previous visual value to the new target.

```rust
use lurq::{
  animation::{Easing, Transition},
  node::{CursorIcon, color::Color},
};

lurq::components::Rect::new(80.0, 40.0)
  .background("#3b82f6")
  .rounded(4.0)
  .border_inside(1.0, Color::from_hex("#1d4ed8"))
  .transition(Transition::background_color().duration_ms(250))
  .transition(Transition::all().duration_ms(350).easing(Easing::EASE_OUT))
  .hovered(|style| {
    style
      .background("#ef4444")
      .rounded(18.0)
      .border_inside(2.0, Color::from_hex("#f8fafc"))
  })
  .cursor(CursorIcon::Pointer)
```

Use a property-specific transition when only one value should animate. Use `Transition::all()` when several style, size, offset, opacity, or transform values can change together.

Transition builders include:

| Builder | Purpose |
|---------|---------|
| `Transition::all()` | Animate every supported property that changes |
| `Transition::background_color()` | Animate fill/background color |
| `Transition::border_color()` | Animate border color |
| `Transition::border_width_top()` and related sides | Animate border widths |
| `Transition::border_radius_top_left()` and related corners | Animate corner radii |
| `Transition::offset_x()` / `Transition::offset_y()` | Animate relative offset |
| `Transition::width()` / `Transition::height()` | Animate frame dimensions |
| `Transition::opacity()` | Animate opacity |
| `Transition::blur()` | Animate the [blur](../blur/) radius |
| `Transition::backdrop_blur()` | Animate the [backdrop blur](../blur/#frosted-glass) radius |
| `Transition::transform()` | Animate `Transform2D` |
| `Transition::foreground()` | Animate the [foreground](../styling-events/#foreground): the color of the text in the element and under it |
| `Transition::outline()` | Animate the [outline](../styling-events/#outline): its width, offset and color together |
| `Transition::box_shadow()` | Animate the [box shadows](../styling-events/#box-shadows) |

Colors, border widths and shadows given as theme roles (`PaletteColor`, `BorderSize`, `ShadowStyle`) are read against
the theme, so they animate like concrete values; a role whose value changes, when the theme changes, runs the
transition too.

- The foreground, the outline and the box shadow are painted only: a transition of them lays nothing out.
- An outline that appears grows in from no width and no opacity at its own offset and color, and one that goes away
  fades back the same way, so a focus ring with `Transition::outline()` comes and goes smoothly even though the element
  has no outline at rest.
- Box shadows animate shadow by shadow, as in CSS: one with no partner in the other list comes from, or goes to,
  nothing where it is. A list in which a shadow turns from outer to inset changes halfway, and a list of more than four
  shadows changes at once.

`duration(...)`, `duration_ms(...)`, `delay(...)`, `delay_ms(...)`, `easing(...)`, and `linear()` configure timing.

## Reduced Motion

A user can ask every app for less motion in the system's accessibility settings. lurq reads the setting on Windows
(Settings › Accessibility › Visual effects › Animation effects), macOS (System Settings › Accessibility › Display ›
Reduce motion) and iOS (Settings › Accessibility › Motion › Reduce Motion), when a window opens and each time one takes
focus, so a change made in the settings shows when the user comes back.

```rust
// What is in effect: the app's decision, or else the system's setting.
let reduced = ctx.reduced_motion();

// The app decides, whatever the system says; `None` follows the system again.
app.theme().set_reduced_motion(Some(true));
```

- While motion is reduced, a transition of where an element is or how large it is (`transform`, `offset_x`, `offset_y`,
  `width`, `height`) ends at once: the element is at its new place and size in the next frame. Sheets and stack
  navigation, which move with such transitions, switch without the slide.
- A fade or a change of color is not motion and still runs: opacity, colors, the foreground, the outline, box shadows,
  blur, radii and border widths.
- Keyframe animations are the app's and are not changed. Read `ctx.reduced_motion()`, which renders the component again
  when the setting changes, to hold back one that moves or to show a fade in its place.
- Linux and other platforms have no setting every desktop shares, so lurq reads none there: motion is reduced only when
  the app says so with `set_reduced_motion`.
- A tree without a window, as in a test, never reads the system: its motion does not depend on the machine.

`Easing::Spring { velocity }` eases as a spring that comes to rest without going past its end: quick at first,
settling slowly. `velocity` is the speed the value already has when the transition starts, as a share of the
way per duration, and 0 from rest; something a finger lets go of goes on at the finger's speed with it.

## Keyframe Animations

Register keyframes on the tree before nodes reference them:

```rust
use lurq::{
  animation::{AnimatableProperty, Animation, Easing, Keyframes, KeyframesId},
  node::color::Color,
};

let mut tree = lurq::app::Tree::new();
const COLOR_CYCLE: KeyframesId = KeyframesId::new(1);

tree.register_keyframes(
  Keyframes::new(COLOR_CYCLE)
    .frame(0.0, |f| {
      f.set(AnimatableProperty::BackgroundColor, Color::from_hex("#3b82f6"));
    })
    .frame(0.5, |f| {
      f.set(AnimatableProperty::BackgroundColor, Color::from_hex("#8b5cf6"));
      f.easing(Easing::EASE_IN_OUT);
    })
    .frame(1.0, |f| {
      f.set(AnimatableProperty::BackgroundColor, Color::from_hex("#3b82f6"));
    }),
);
```

Then attach the animation by ID:

```rust
lurq::components::Rect::new(64.0, 64.0)
  .background("#3b82f6")
  .animation(Animation::new(COLOR_CYCLE).duration_ms(1600).linear().infinite())
```

Keyframe offsets are normalized from `0.0` to `1.0`. The engine finds the surrounding frames for the current progress, applies the frame easing when present, and interpolates properties that exist in both frames.

`Animation` supports:

| Method | Purpose |
|--------|---------|
| `duration(...)` / `duration_ms(...)` | Total duration for one iteration |
| `delay(...)` / `delay_ms(...)` | Delay before playback starts |
| `easing(...)` / `linear()` | Default easing for frame spans |
| `direction(...)` / `alternate()` | Normal, reverse, alternate, or alternate-reverse playback |
| `fill_mode(...)` | None, forwards, backwards, or both; forwards holds the pose the run stops on, part way into an iteration for a fractional count |
| `iteration_count(...)` / `infinite()` | Finite or infinite playback |

## Frame Pacing

Each pass that ticks the timelines also works out when they next draw something else, and the winit shell asks for the next frame then, at most one frame per display refresh:

- a span eased by `Easing::Steps { count, .. }` holds its value between steps, so it gets one frame per step;
- a delay draws nothing new until it ends;
- a span that changes continuously (`Easing::Linear`, the cubic Bézier easings) gets a frame every display refresh.

Between frames the event loop sleeps until the next one is due. For a continuous animation the shell wakes half a refresh after the last present and the pass then waits in the swapchain for the next vblank, which keeps one frame per vblank; input, timers, MCP calls and wakes from other threads that arrive during that wait are handled after it, up to about half a refresh later. A `Steps` animation draws long before its frame could wait, so during it they are handled as they arrive.

Each frame lays out and draws the whole window, so a continuous animation costs a full frame at the display's rate (60, 120 or 144 times a second) for as long as it runs. For an indicator that turns or pulses for a long time, such as a loading spinner, give it steps: 12 to 15 steps a turn look smooth for a small icon and cost a fifth of the frames at 60 Hz, and less on faster displays.

```rust
use lurq::animation::{Animation, Easing, StepPosition};

Animation::new(SPIN)
  .duration_ms(1000)
  .easing(Easing::Steps { count: 12, position: StepPosition::JumpEnd })
  .infinite()
```

There is no frame-rate cap per animation: steps are the cap, and they put every frame on an even pose.

Each node's run starts when the node first ticks, so spinners that mount at different moments step at different moments, and every distinct phase adds its own frames: four runs' spinners mounted one after another draw four times as many frames as one. Register the keyframes with `Tree::register_keyframes_in_phase` instead of `Tree::register_keyframes` to play every run on the process's animation clock: a run starts at the last whole number of its periods since the clock's origin (`lurq::animation::clock_origin()`), so a spinner that mounts late shows the same pose as the others and they all step together, one frame per step however many are shown. The clock is one per process, so spinners in different windows are in phase too.

- **Direction.** The period is the animation's duration for `Normal` and `Reverse`, and twice the duration for `Alternate` and `AlternateReverse`, so every run plays the same iterations forward and the same ones backward and shows the same pose in every direction.
- **Iteration count.** A late run starts part-way through an iteration, as a CSS animation with a negative delay does, so a finite iteration count ends on the clock rather than a whole count after the node mounted. A run that mounts after that end in its period does not play.
- **Delay.** The delay counts from the run's start on the clock, not from the mount: every run plays from a whole number of periods after the origin plus the delay. A node that mounts within the delay of its period waits only for the rest of it, showing nothing (or the first pose with a backwards fill) while nodes that mounted earlier already play; one that mounts later plays at once. Runs of the same animation show the same pose whenever they all play.

```rust
tree.register_keyframes_in_phase(spin_keyframes());
```

A minimized window, or one that macOS reports occluded, draws no animation frames. Animations keep their clock while it is hidden: when the window shows again it draws one frame at once, with each animation where its time puts it, and paces the frames after it as before. Windows reports a minimized window but not one covered by other windows.

A shell that drives `Tree::pass` itself does the same with `Tree::next_timeline_change()`: after a pass that only animated, `Tree::needs_redraw()` is false, and the next frame is due at that time.

## Animatable Properties

The animation engine supports color, float, and decomposed transform values:

```rust
use lurq::{
  animation::{AnimatableProperty, AnimatableValue},
  node::{color::Color, transform::Decomposed},
};

AnimatableValue::Color(Color::from_hex("#22c55e"));
AnimatableValue::Float(0.5);
AnimatableValue::Transform(Decomposed::IDENTITY.with_scale(1.2, 1.2));
```

The supported properties are:

- `BackgroundColor`
- `BorderColor`
- `BorderWidthTop`, `BorderWidthRight`, `BorderWidthBottom`, `BorderWidthLeft`
- `BorderRadiusTopLeft`, `BorderRadiusTopRight`, `BorderRadiusBottomRight`, `BorderRadiusBottomLeft`
- `OffsetX`, `OffsetY`
- `Width`, `Height`
- `Opacity`
- `Transform`

`Width`, `Height`, `OffsetX`, and `OffsetY` can invalidate layout while they animate. `Opacity` and `Transform` are paint-only in normal use. While an element's opacity is below 1, a subtree that paints more than one primitive is drawn into an offscreen layer every frame (see [Opacity](../styling-events/#opacity)); fading a large panel costs one layer of its size.

## Transforms

Use `.transform(Transform2D)` for a visual 2D transform around the element center:

```rust
use lurq::node::transform::Transform2D;

lurq::components::Rect::new(80.0, 48.0)
  .background("#3b82f6")
  .transform(Transform2D::rotate_deg(12.0))
```

`Transform2D` has helpers for:

| Helper | Purpose |
|--------|---------|
| `Transform2D::translate(x, y)` | Translate in paint space |
| `Transform2D::scale(sx, sy)` | Scale independently on x and y |
| `Transform2D::scale_uniform(s)` | Scale both axes |
| `Transform2D::rotate(radians)` | Rotate in radians |
| `Transform2D::rotate_deg(degrees)` | Rotate in degrees |
| `Transform2D::skew(ax, ay)` | Skew by radians |
| `.then(&other)` | Compose transforms |

Composition is applied right-to-left. In this example, the scale is applied first, then the rotation:

```rust
lurq::components::Rect::new(80.0, 48.0)
  .transform(Transform2D::rotate_deg(20.0).then(&Transform2D::scale(1.2, 0.9)))
```

Transforms are visual-only for parent layout. The element keeps its measured layout size, siblings are not moved by the transform, and the transformed subtree paints from that layout slot. Hit testing uses transformed coordinates, so pointer events, text selection, and text input carets follow the painted position.

## Transform Animation

Transform interpolation uses `Decomposed` values: translation, scale, rotation, and skew are interpolated independently, then recomposed into a matrix.

```rust
use lurq::{
  animation::{AnimatableProperty, Animation, Keyframes, KeyframesId},
  node::transform::Decomposed,
};

const SPIN: KeyframesId = KeyframesId::new(14);

tree.register_keyframes(
  Keyframes::new(SPIN)
    .frame(0.0, |f| {
      f.set(AnimatableProperty::Transform, Decomposed::IDENTITY.with_rotate(0.0));
    })
    .frame(1.0, |f| {
      f.set(
        AnimatableProperty::Transform,
        Decomposed::IDENTITY.with_rotate(std::f32::consts::TAU),
      );
    }),
);

lurq::components::Rect::new(48.0, 48.0)
  .animation(Animation::new(SPIN).duration_ms(1200).linear().infinite())
```

Use `Decomposed` directly when the path matters. For example, `Transform2D::rotate_deg(360.0)` is visually the identity matrix, so it cannot express a full spin by itself.

## Text Under Transforms

Text defaults to `TextTransformMode::Bitmap`. It rasterizes glyphs normally and applies the transform to glyph quads at render time. This is the best mode for animated transforms because changing the angle does not create a new glyph atlas entry for every frame.

Use `TextTransformMode::Rasterized` for static transformed text that needs sharper rotated glyph edges:

```rust
use lurq::node::{TextTransformMode, transform::Transform2D};

lurq::components::Text::new("Static label")
  .text_transform_mode(TextTransformMode::Rasterized)
  .transform(Transform2D::rotate_deg(-8.0))
```

Because the transform matrix is part of the rasterized glyph cache key, continuously animated text should usually stay on `Bitmap`.
