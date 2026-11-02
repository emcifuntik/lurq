---
title: Typed Component API
description: Public typed builders for layout, visuals, events, drag and drop, and components.
---

# Typed Component API

## Import

```rust
use lurq::{
  components::{
    Checkbox, Column, DragContainer, DragContainerProps, Draggable, DraggableProps, DropZone, DropZoneProps, Rect, Row,
    Slider, Spacer, Stack, Text, TextInput,
  },
  layout::{Alignment, StackAlignment},
  node::{CheckboxStyle, Element, TextTransformMode, color::Color},
};
```

Typed components are the public UI builders. `Element` is the erased return/transport type used by the runtime and component system. The internal `Node` type is crate-private.

## Constructors

| Constructor | Description |
|-------------|-------------|
| `Row::new()` | Horizontal container |
| `Column::new()` | Vertical container |
| `Stack::new()` | Overlay container; later children paint on top |
| `Text::new("content")` | Text with default style |
| `Text::styled("content", style)` | Text with custom `TextStyle` |
| `TextInput::new(signal)` | Controlled editable text input |
| `Checkbox::new(signal)` | Controlled boolean checkbox |
| `Slider::new(signal)` | Controlled integer slider |
| `Slider::new_f32(signal)` | Controlled floating-point slider; use `.range_f32(...)` and `.step(...)` |
| `Select::new(signal)` | Controlled single-select dropdown; use `Select::multiple(signal)` for multiple values |
| `Button::new(label)` | Clickable button; form submission requires `form` |
| `Canvas::new()` | Persistent Canvas 2D surface; requires `canvas` |
| `Markdown::mount(ctx, props)` | Markdown component; requires `markdown` |
| `Rect::new(width, height)` | Fixed-size rectangle leaf |
| `Spacer::new()` | Empty leaf, often used with `.flex(1.0)` |
| `ScrollVertical::new(child)` | Vertical scroll container |
| `ScrollHorizontal::new(child)` | Horizontal scroll container |
| `ScrollBoth::new(child)` | Two-axis scroll container |
| `DragContainer::mount(ctx, DragContainerProps::new(), child)` | Drag surface that bounds descendant draggables by default |
| `Draggable::mount(ctx, DraggableProps::new(), child)` | Blank component wrapper that makes its child draggable |
| `DropZone::mount(ctx, DropZoneProps::new(), child)` | Blank component wrapper that makes its child a drop target |

## Children

```rust
lurq::components::Column::new()
  .child(lurq::components::Text::new("one"))
  .child(lurq::components::Text::new("two"))
  .child(lurq::components::Text::new("three"))
```

```rust
let items = vec![lurq::components::Text::new("a"), lurq::components::Text::new("b"), lurq::components::Text::new("c")];
lurq::components::Column::new().with_children(items)
```

## Ids And Classes

Every builder accepts HTML-like `id` and `class` attributes for browser-style lookup (`Tree::get_element_by_id`, `Tree::get_elements_by_class_name`). Explicit IDs also help preserve node identity across sibling changes. Classes are lookup labels only, and neither attribute applies styling.

```rust
lurq::components::Column::new()
  .child(lurq::components::Text::new("Title").id("headline"))
  .child(
    lurq::components::Rect::new(24.0, 24.0)
      .class("icon")
      .classes(["muted", "small"]),
  )
```

`class` appends and deduplicates like the DOM's `classList.add`. Duplicate ids are allowed (easy to produce inside `for_each`); lookup returns the first match in tree order and warns in debug builds.

The same methods exist on `Element` for components not covered by the builder macro:

```rust
let element: Element = my_component.into();
let element = element.id("sidebar").class("panel");
```

## Containers

```rust
lurq::components::Row::new()
  .spacing(12.0)
  .align_items(Alignment::Center)
  .child(lurq::components::Rect::new(40.0, 40.0))
```

```rust
lurq::components::Stack::new()
  .stack_align(StackAlignment::BottomEnd)
  .child(lurq::components::Rect::new(200.0, 120.0))
```

## Sizing

```rust
lurq::components::Spacer::new().size(200.0, 100.0)
lurq::components::Spacer::new().width(200.0)
lurq::components::Spacer::new().height(100.0)
lurq::components::Spacer::new().min_width(160.0).max_width(320.0)
lurq::components::Spacer::new().min_height(80.0).max_height(180.0)
lurq::components::Spacer::new().min_size(120.0, 80.0).max_size(320.0, 180.0)
lurq::components::Rect::new(80.0, 80.0)
```

Plain `f32` values are pixel shorthand. Use `Dimension` directly for non-pixel sizing:

```rust
use lurq::node::dimension::Dimension;

lurq::components::Spacer::new().width(Dimension::Pct(50.0))
lurq::components::Spacer::new().height(Dimension::Auto)
```

## Visuals

```rust
lurq::components::Rect::new(100.0, 50.0)
  .background("#3b82f6")
  .rounded(8.0)
  .border_inside(1.0, Color::from_hex("#1d4ed8"))
```

```rust
use lurq::node::CursorIcon;

lurq::components::Rect::new(100.0, 50.0).cursor(CursorIcon::Pointer)
lurq::components::Rect::new(100.0, 50.0).hovered(|style| style.cursor(CursorIcon::Text))
```

## Padding

```rust
lurq::components::Column::new().padding(16.0)
lurq::components::Column::new().padding_horizontal(16.0).padding_vertical(8.0)
lurq::components::Column::new().padding_left(10.0)
lurq::components::Column::new().padding_right(10.0)
lurq::components::Column::new().padding_top(10.0)
lurq::components::Column::new().padding_bottom(10.0)
```

## Flex

```rust
lurq::components::Row::new()
  .child(lurq::components::Rect::new(100.0, 50.0))
  .child(lurq::components::Spacer::new().flex(1.0))
  .child(lurq::components::Rect::new(100.0, 50.0))
```

Flex applies inside `Row` and `Column`.

## Relative Positioning

```rust
lurq::components::Rect::new(50.0, 50.0).relative(10.0, 20.0)
```

`relative(x, y)` is an alias for `offset(x, y)`. It shifts the element visually without changing the space it takes in parent layout.

## Absolute Positioning

Absolute positioning is supported in `Stack`.

```rust
lurq::components::Stack::new()
  .child(
    lurq::components::Rect::new(300.0, 120.0)
      .background("#f8fafc")
      .rounded(12.0),
  )
  .child(
    lurq::components::Rect::new(86.0, 34.0)
      .background("#f97316")
      .rounded(8.0)
      .absolute(190.0, 24.0, 86.0, 34.0),
  )
  .child(
    lurq::components::Text::new("absolute")
      .absolute_position(201.0, 31.0),
  )
```

Rules:

- `absolute(x, y, width, height)` positions the child at `(x, y)` inside the stack and forces its size.
- `absolute_position(x, y)` positions the child but lets it keep its measured size.
- Absolute children do not affect stack size.
- There is no z-index API. Rendering follows child order; later stack children paint on top.

## Alignment Override

```rust
lurq::components::Column::new()
  .align_items(Alignment::Start)
  .child(lurq::components::Text::new("left"))
  .child(lurq::components::Text::new("right").align(Alignment::End))
```

## Events

```rust
use lurq::app::events::{DragEvent, KeyboardEvent, MouseEvent};

lurq::components::Rect::new(100.0, 40.0)
  .background("#3b82f6")
  .on_click(|e: MouseEvent| println!("clicked at {}, {}", e.x, e.y))
  .on_drag_start(|e: DragEvent| println!("drag started at {}, {}", e.x, e.y))
  .on_drag_move(|e: DragEvent| println!("drag delta: {}, {}", e.delta_x, e.delta_y))
  .on_drag_end(|e: DragEvent| println!("drag ended at {}, {}", e.x, e.y))
  .on_mouse_enter(|| println!("hover in"))
  .on_mouse_leave(|| println!("hover out"))
  .on_key_down(|e: KeyboardEvent| println!("key: {}", e.key))
```

Mouse, keyboard, and scroll events can stop propagation and block built-in defaults:

```rust
use lurq::app::events::KeyboardEvent;

lurq::components::TextInput::new(value)
  .on_key_down(|e: KeyboardEvent| {
    if e.key == "Tab" {
      e.prevent_default();
      e.stop_propagation();
    }
  })
```

Use `prevent_default()` for runtime defaults such as text editing, focus, form submit, overlay dismissal, and scroll movement. Use `stop_propagation()` when later handlers should not receive the same event.

`TextInput::on_input` is the text-edit hook. It runs before the built-in edit, passes the bound `Signal<String>` as `event.value`, and can cancel the built-in edit with `prevent_default()`:

For retained picker insertion and editable source-backed variable pills, see [Source-backed input tokens](../text-input-tokens/). For bold, italic, inline code, strikethrough and links edited in place, see [Styled text inputs](../text-input-styled/).

```rust
use lurq::app::events::TextInputEvent;

lurq::components::TextInput::new(value)
  .on_input(|event: TextInputEvent| {
    if event.keyboard.key == "Tab" {
      event.value.set("/play ".to_owned());
      event.prevent_default();
    }
  })
```

## Drag And Drop

`DragContainer`, `Draggable`, and `DropZone` are components with explicit one-child `mount` helpers. Each requires exactly one child. `Draggable` and `DropZone` are blank wrappers. `DragContainer` uses its child as the drag surface and applies container policy; `DragContainerProps::new()` bounds descendant draggables to that surface.

```rust
fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  let drop_zone = lurq::components::DropZone::mount(
    ctx,
    lurq::components::DropZoneProps::new().on_drop(|event| {
      println!("dropped on {:?}", event.target_id);
    }),
    lurq::components::Rect::new(120.0, 80.0)
      .background("#22c55e33")
      .absolute_position(200.0, 110.0),
  );

  let card = lurq::components::Draggable::mount(
    ctx,
    lurq::components::DraggableProps::new().on_drag_move(|event| {
      println!("move by {}, {}", event.delta_x, event.delta_y);
    }),
    lurq::components::Rect::new(64.0, 64.0)
      .background("#3b82f6")
      .absolute_position(24.0, 24.0),
  );

  lurq::components::DragContainer::mount(
    ctx,
    lurq::components::DragContainerProps::new(),
    lurq::components::Stack::new()
      .size(360.0, 220.0)
      .child(drop_zone)
      .child(card),
  )
}
```

## Text Styling

Theme palettes, typography, radii, and spacing expose named values:

```rust
use lurq::{
  app::theme::{BorderSize, PaletteColor, RadiusSize, SpacingSize, TypographyStyle},
  node::color::Color,
};

app.theme().set_palette_color(PaletteColor::Accent, Color::from_hex("#2563eb"));

app.theme().set_spacing_value(SpacingSize::Sm, 8.0);

lurq::components::Rect::new(120.0, 40.0)
  .background(PaletteColor::Accent)
  .padding(SpacingSize::Sm)
  .border_inside(BorderSize::Sm, PaletteColor::Border)
  .rounded(RadiusSize::Md);

lurq::components::Row::new()
  .spacing(SpacingSize::Sm);
```

Plain text resolves its style from the active theme. `Text::new` uses the theme body text style, and `variant` selects a named typography style.

```rust
use lurq::layout::text_style::{FontWeight, TextStyle};

app.theme().set_typography_style(TypographyStyle::Body, TextStyle {
  font_size: 16.0,
  ..TextStyle::default()
});

app.theme().set_typography_style(
  TypographyStyle::Heading,
  TextStyle {
    font_size: 32.0,
    weight: FontWeight::Bold,
    ..TextStyle::default()
  },
);

lurq::components::Text::new("Headline")
  .variant(TypographyStyle::Heading)
```

The built-in text variants are the `TypographyStyle` names. The built-in radius variants are `RadiusSize::Sm`, `RadiusSize::Md`, and `RadiusSize::Lg`; spacing variants are `SpacingSize::Xs`, `SpacingSize::Sm`, `SpacingSize::Md`, `SpacingSize::Lg`, `SpacingSize::Xl`, and `SpacingSize::Section`; border-size variants are `BorderSize::Sm`, `BorderSize::Md`, and `BorderSize::Lg`. Each enum also has an `Extra` variant for application-defined roles, such as `Text::new("RECENT").variant("overline")`; see [Extra Roles](../theme/#extra-roles).

Use `Text::styled` for a one-off style that should ignore theme typography:

```rust
use lurq::{
  layout::text_style::{FontStyle, FontWeight, TextStyle},
  node::color::Color,
};

lurq::components::Text::styled("Bold title", TextStyle {
  font_size: 24.0,
  weight: FontWeight::Bold,
  style: FontStyle::Normal,
  color: Color::from_hex("#1e293b"),
  ..TextStyle::default()
})
```

Make plain text selectable when users need copyable or inspectable text:

```rust
lurq::components::Text::new("Selectable text")
  .selectable(true)
```

Selectable text supports drag selection, double-click word selection, and triple-click line selection. `TextInput` has the same pointer selection gestures plus caret movement, undo/redo, and signal-backed edits.

Align text within its own box with `TextAlign` or layout `Alignment`:

```rust
use lurq::layout::{Alignment, text_style::TextAlign};

lurq::components::Text::new("No endpoints yet")
  .width(lurq::node::dimension::Dimension::Pct(100.0))
  .text_align(Alignment::Center);

lurq::components::TextInput::new(endpoint.clone())
  .placeholder("Connect to an endpoint to get started.")
  .single_line()
  .text_align(TextAlign::Center)
```

Tighten or loosen tracking with `.letter_spacing(...)` in logical pixels on `Text` and `TextInput`, or with `TextStyle::letter_spacing`; see [Letter Spacing](../theme/#letter-spacing). Turn OpenType features such as ligatures on or off with `.font_features(...)` or `TextStyle::font_features`; see [Font Features](../theme/#font-features).

Use text overflow when a fixed-width label should stay on one line:

```rust
use lurq::components::TextOverflow;

lurq::components::Text::new("Connected to production cluster")
  .width(lurq::node::dimension::Dimension::Px(180.0))
  .text_overflow(TextOverflow::Elipsis)
```

### Transformed Text

Text uses `TextTransformMode::Bitmap` by default. In this mode the glyphs are rasterized normally, then transformed during rendering. This keeps glyph placement in float screen space and is the right default for animated transforms because it does not create a new glyph atlas entry for every angle.

Use `TextTransformMode::Rasterized` for static transformed text when the rotated glyph edges need to stay sharp. This mode bakes the transform into glyph rasterization, disables transform-time hinting for the rotated mask, and emits identity-transform glyph quads. Because the transform is part of the glyph atlas cache key, animated rotations should usually stay on `Bitmap`.

```rust
lurq::components::Text::new("Static rotated label")
  .text_transform_mode(TextTransformMode::Rasterized)
  .transform(lurq::node::transform::Transform2D::rotate_deg(-8.0))
```

## Components

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  core::Signal,
  node::Element,
};

struct Counter {
  count: Signal<i32>,
}

impl Component for Counter {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self { count: ctx.signal(0) }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let count = self.count.clone();
    lurq::components::Column::new()
      .spacing(8.0)
      .child(lurq::components::Text::new(&format!("Count: {}", self.count.get())))
      .child(
        lurq::components::Text::new("Increment")
          .on_click(move |_| count.update(|n| *n += 1)),
      )
  }
}

fn render_parent(ctx: &mut Ctx) -> Element {
  lurq::components::Column::new().child(ctx.mount::<Counter>(())).into()
}
```
