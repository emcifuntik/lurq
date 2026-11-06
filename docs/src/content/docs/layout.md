---
title: Layout
description: Constraints, containers, modifiers, alignment, flex, scroll, and text layout.
---

# Layout

## Core Idea

Layout is constraints-based and compositional. Public UI code builds typed components such as `Row`, `Column`, `Text`,
and `Rect`, then converts them into the erased `Element` type at runtime/component boundaries. Internally, each
`Element` wraps a crate-private node tree made of containers, leaves, and modifier nodes.

A plain empty component has no intrinsic size. Size, padding, alignment, offsets, visuals, and scroll behavior are added
by wrapping the component in modifiers.

## Modifiers

Modifiers are chainable and wrap the current element.

```rust
lurq::components::Rect::new(80.0, 40.0)
  .padding(12.0)
  .background("#3b82f6")
  .rounded(8.0)
```

Common modifiers:

| Modifier                                                                | Purpose                                                                |
|-------------------------------------------------------------------------|------------------------------------------------------------------------|
| `.size(width, height)`                                                  | Force width and height                                                 |
| `.width(width)`                                                         | Force width                                                            |
| `.height(height)`                                                       | Force height                                                           |
| `.min_size(width, height)` / `.max_size(width, height)`                 | Set minimum or maximum width and height                                |
| `.min_width(width)` / `.max_width(width)`                               | Set a width bound                                                      |
| `.min_height(height)` / `.max_height(height)`                           | Set a height bound                                                     |
| `.padding(...)` / `.padding_horizontal(...)` / `.padding_vertical(...)` | Add insets around the child from a concrete dimension or `SpacingSize` |
| `.background(color)`                                                    | Fill the element background from a concrete color or `PaletteColor`    |
| `.rounded(radius)`                                                      | Set border radius from `f32` or `RadiusSize`                           |
| `.border_inside(width, color)`                                          | Draw an inside border from a concrete width or `BorderSize`            |
| `.offset(x, y)`                                                         | Shift visually without changing parent layout                          |
| `.relative(x, y)`                                                       | Alias for `.offset(x, y)`                                              |
| `.absolute(x, y, width, height)`                                        | Absolute stack positioning with forced size                            |
| `.absolute_position(x, y)`                                              | Absolute stack positioning with measured size                          |
| `.transform(Transform2D)`                                               | Apply a visual 2D transform around the element center                  |
| `.align(Alignment)`                                                     | Override alignment within parent container                             |
| `.flex(factor)`                                                         | Participate in row/column flex distribution                            |
| `.flex_shrink(factor)`                                                  | Give up main-axis space when a row/column overflows                    |
| `.shrink_order(order)` / `.shrink_limit(ShrinkLimit)`                   | When and how far a shrinking child gives way                           |
| `.shrink_drop_below(size)`                                              | Shrink a child down to `size`, then drop it                            |

Sizing modifiers accept `Dimension` values. Passing a plain `f32` is shorthand for `Dimension::Px(value)`. Min/max
sizing clamps a child's measured size without forcing both bounds. Padding accepts `f32`, `Dimension`, or `SpacingSize`.

```rust
use lurq::node::dimension::Dimension;

lurq::components::Spacer::new().width(120.0)
lurq::components::Spacer::new().width(Dimension::Pct(50.0))
lurq::components::Spacer::new().width(Dimension::Auto)
lurq::components::Spacer::new().min_width(160.0).max_height(240.0)
lurq::components::Spacer::new().min_size(120.0, 80.0).max_size(320.0, 180.0)
```

## Constraints Model

Layout follows the same high-level model as Flutter:

1. Parent passes `Constraints` down to each child.
2. Child picks a concrete `Size` within those constraints.
3. Parent positions each child with an offset.

```rust
pub struct Constraints {
  pub min_width: f32,
  pub max_width: f32,
  pub min_height: f32,
  pub max_height: f32,
}
```

Constraint kinds:

- Tight: `min == max`; forces an exact size.
- Loose: `min == 0`; child can choose any size up to max.
- Unbounded: `max == f32::INFINITY`; used by scroll containers on the scroll axis.

Application code normally does not call layout directly. Runtime computes layout when rendering, dispatching input, or
looking up elements.

## Containers

### Column

`lurq::components::Column::new()` arranges children top-to-bottom.

```rust
lurq::components::Column::new()
  .spacing(8.0)
  .align_items(Alignment::Center)
  .child(lurq::components::Text::new("A"))
  .child(lurq::components::Text::new("B"))
```

Column layout:

1. Lays out non-flex children with loosened vertical constraints.
2. Sums child heights plus spacing.
3. Uses the max child width.
4. Positions children vertically and applies cross-axis alignment.
5. Distributes remaining height to flex children when present.

`.spacing(...)` accepts either a plain pixel value or a `SpacingSize` from the active theme.

### Row

`lurq::components::Row::new()` arranges children left-to-right.

```rust
lurq::components::Row::new()
  .spacing(8.0)
  .align_items(Alignment::Center)
  .child(lurq::components::Text::new("A"))
  .child(lurq::components::Text::new("B"))
```

Row layout is the horizontal equivalent of column layout.

`.spacing(...)` accepts either a plain pixel value or a `SpacingSize` from the active theme.

### Stack

`lurq::components::Stack::new()` overlays children. Later children paint on top of earlier children.

```rust
lurq::components::Stack::new()
  .stack_align(StackAlignment::Center)
  .child(lurq::components::Rect::new(200.0, 120.0))
  .child(lurq::components::Rect::new(40.0, 40.0).background("#ef4444"))
```

Stack layout:

1. Lays out all children with the same constraints.
2. Sizes itself to the max width/height of non-absolute children.
3. Positions normal children using stack alignment or per-child `.align(...)`.
4. Positions absolute children at their explicit `(x, y)` offset.

Absolute children do not contribute to stack size.

## Relative Positioning

```rust
lurq::components::Rect::new(50.0, 50.0).relative(10.0, 20.0)
```

Relative positioning is an offset. It moves the child visually but the parent still reserves space as if the offset were
zero. Siblings are not moved by the offset.

## Absolute Positioning

Absolute positioning is intentionally scoped to `Stack`.

```rust
lurq::components::Stack::new()
  .child(lurq::components::Rect::new(300.0, 120.0).background("#f8fafc"))
  .child(
    lurq::components::Rect::new(80.0, 32.0)
      .background("#f97316")
      .absolute(190.0, 24.0, 80.0, 32.0),
  )
```

Use:

- `.absolute(x, y, width, height)` when the positioned child should have a forced size.
- `.absolute_position(x, y)` when the positioned child should keep its measured size.

There is no z-index. Rendering order is structural: later stack children paint above earlier children.

## Alignment

### Row And Column Alignment

```rust
lurq::components::Column::new()
  .align_items(Alignment::Center)
  .child(lurq::components::Text::new("centered"))
```

```rust
pub enum Alignment {
  Start,
  Center,
  End,
  Stretch,
}
```

### Per-Child Override

```rust
lurq::components::Column::new()
  .align_items(Alignment::Start)
  .child(lurq::components::Text::new("left"))
  .child(lurq::components::Text::new("right").align(Alignment::End))
```

### Stack Alignment

```rust
lurq::components::Stack::new()
  .stack_align(StackAlignment::BottomEnd)
  .child(lurq::components::Rect::new(40.0, 40.0))
```

```rust
pub enum StackAlignment {
  TopStart,
  TopCenter,
  TopEnd,
  CenterStart,
  Center,
  CenterEnd,
  BottomStart,
  BottomCenter,
  BottomEnd,
}
```

## Flex

Children inside `Row` or `Column` can consume remaining space with `.flex(factor)`.

```rust
lurq::components::Row::new()
  .child(lurq::components::Rect::new(100.0, 50.0))
  .child(lurq::components::Spacer::new().flex(1.0))
  .child(lurq::components::Rect::new(100.0, 50.0))
```

Flex layout:

1. Lay out non-flex children first.
2. Subtract fixed child sizes and spacing from available space.
3. Divide remaining space by flex factor.
4. Lay out flex children with tight constraints for their assigned size.

In an unbounded main axis (for example a column inside `ScrollVertical`, or a column measured by its own content) there
is no remaining space: a flex child gets its `basis`, or its natural size when it has none.

### Shrink

`.flex_shrink(factor)` (or `.flex_full(grow, shrink, basis)`) lets a child give up main-axis space when the children
overflow the row or column. The overflow is split by shrink factor, never below a child's `min_width`/`min_height`.
Each shrunk child is then laid out again with its shrunk size as a tight constraint, so its content matches the box it
got: a shrunk scroll container reports the shrunk viewport and scrolls to its last row, and a shrunk column distributes
its own flex children inside the smaller height.

```rust
lurq::components::Column::new()
  .height(300.0)
  .child(header)
  .child(lurq::components::ScrollVertical::new(rows).flex_shrink(1.0))
  .child(footer)
```

The scroll area takes its content height while that fits, and the space between header and footer otherwise.

### Give-way order and limits

By default every shrinking child gives way at once. `.shrink_order(order)` makes them give way in turn: the children
with the lowest order absorb the overflow, down to their limit, before any child of the next order shrinks at all.
Children of one order share their part by shrink factor, as above. The order is an `i32` and defaults to 0, so a child
without one gives way before children with a positive order. Both modifiers apply only to a child with a `flex_shrink`
factor in a single-line `Row` or `Column`.

`.shrink_limit(ShrinkLimit)` sets how far a child gives way:

| `ShrinkLimit` | The child shrinks                                                                                     |
|---------------|-------------------------------------------------------------------------------------------------------|
| `MinSize`     | Down to its `min_width`/`min_height`, 0 without one (the default)                                     |
| `Content`     | Down to its content minimum, never below an explicit `min_width`/`min_height`                         |
| `Drop`        | Not at all: it keeps its natural size, or is dropped (zero size, no spacing, not drawn and not hit)   |

The content minimum of a `Row` in a row (or a `Column` in a column) is its padding and spacing, the natural size of
every child that does not shrink, and the limit of every child that does (a droppable child counts as gone). Anything
else (text, rects, stacks, scroll containers, wrapping rows, lines on the other axis) keeps its natural size, like a
flex item with CSS `min-width: auto`. Within one order, droppable children are dropped, last first, only while the
shrinking children of that order cannot absorb the rest of the overflow. This holds for the line that gives way, not
inside a shrinking item of it: an item whose row holds droppable children drops them all at once when it collapses
(see below). A dropped child stays mounted and keeps its
state, and it comes back once the line has room for it. While it is dropped, nothing inside it is a Tab stop or takes
focus (`focus()` and `ctx.focus` requests for it are refused, keys never activate it), focus inside it moves off (the
focused node is blurred), and the element refs and bounds of everything inside it report a
zero-size rect at its place in the line, where the next child starts. Devtools and the MCP tree mark it `dropped`.

What gives way whole only accumulates as a line narrows: a child dropped at one width is dropped at every narrower
one. A dropped child frees its whole size and spacing, which can be more than the line needed; the rest goes back to
children that only trimmed (labels, spacers), never to a child that dropped, so the line stays filled without bringing
anything back. The same holds one line down. A shrinking item whose own row holds droppable children (a `Drop` or
`shrink_drop_below` child, or such an item nested in it) keeps its natural size until its line needs it to give way.
It then collapses, after the children of its order that drop, as a whole step like a drop: its row drops every one of
them (and collapses every such item in it) at once, so it holds its padding, its other children at their natural size
and the spacing between them, and from there it shrinks on toward its floor like any other child, never growing back.
The line decides this and the item's own layout follows it, so the two cannot disagree. Inside such an item, children
therefore give way only once it collapses: a trimming child does not trim while a droppable child beside it stays,
whatever their orders. To let a label trim before a time drops, make them separate children of the line that gives
way, the time in a later order than the label. An ellipsizing text with `Drop` and a shrink factor is
measured at its full width, so it drops rather than ellipsizes.

`.shrink_drop_below(size)` combines both: the child first shrinks like its limit, but no further than `size`, and when
its order still cannot absorb the overflow with every shrinking child at its floor, it drops (as with `Drop`) before
the next order shrinks at all. A child already narrower than `size` drops without shrinking; a negative or NaN `size`
counts as 0. Within one order, children with `Drop` are dropped first, then those with `shrink_drop_below`, each last
first, and either kind counts as gone in an ancestor's content minimum. With `ShrinkLimit::Drop` the size has no
effect.

```rust
// The request words trim to 100 px, then drop so the count stays whole.
Row::new()
  .child(label("Designer asks to use pencil").flex_shrink(1.0).shrink_order(1).shrink_drop_below(100.0))
  .child(label("1 approval waiting").flex_shrink(1.0).shrink_order(2))
```

A line that uses orders or limits also shares in whole pixels: within one order every child loses a whole number of
pixels except the one with the largest shrink factor, which takes the fraction. A sliver of overflow therefore
truncates one label instead of putting a stray ellipsis on several, and as the line narrows each child gives up a
pixel at a time. If that child reaches its floor, the fraction it cannot take goes to a child that already loses a
pixel or more, and only when there is none to one that loses nothing. A line without orders or limits shares exactly by
factor.

Use orders, not factors, to say which child gives way first: shares are computed in `f32`, so keep the factors within
one order of similar magnitude (a factor thousands of times smaller than its neighbours' is lost in their sum).

```rust
use lurq::{
  components::{Rect, Row, Text, TextOverflow},
  layout::layout_kind::ShrinkLimit,
};

let label = |text: &str| Text::new(text).nowrap().text_overflow(TextOverflow::Elipsis);

// Icon and count stay whole; the detail inside the item gives way first.
let approvals = Row::new()
  .child(Rect::new(14.0, 14.0))
  .child(label("1 approval waiting"))
  .child(label("Designer asks to use pencil").flex_shrink(1.0))
  .flex_shrink(1.0)
  .shrink_order(2)
  .shrink_limit(ShrinkLimit::Content);

Row::new()
  .spacing(8.0)
  .child(label("15:32").flex_shrink(1.0).shrink_order(1).shrink_limit(ShrinkLimit::Drop))
  .child(approvals)
  .child(label("Run 3").flex_shrink(1.0).shrink_order(3))
```

As the row narrows, the time is dropped first, then the approval detail is ellipsized down to the icon and count, and
only then does the run label shrink.

## Scroll

```rust
lurq::components::ScrollVertical::new(
  lurq::components::Column::new()
    .spacing(4.0)
    .with_children(items),
)
.size(300.0, 180.0)
```

Scroll containers give their child unbounded constraints on the scroll axis and apply scroll offsets during
layout/rendering. Without a fixed size on the scroll axis, a scroll container takes its content's size within its
constraints, so `.max_height(...)` makes it grow with its content up to the cap and scroll beyond it.

To bring an element into view, request it from a component with `ctx.scroll_into_view(&element_ref)` (the ref attached
with `.ref_element(...)`), or call `scroll_into_view()` on the `ElementHandle` from `tree.get_element_by_id_mut(id)`.
Every scroll container around the element, nearest first, scrolls by the smallest amount that shows it, on each axis it
scrolls, like the web's `scrollIntoView({ block: "nearest", inline: "nearest" })`: an element already in view does not
move anything, and one larger than a viewport is aligned to the viewport start. The request is resolved after the next
layout, and the frame drawn after it already shows the element. Focusing an element by request or with Tab scrolls it
into view the same way (see [Focus And Keyboard Navigation](../focus-navigation/#scrolling-focus-into-view)).

```rust
// `details` is a ref retained by the component and attached with `.ref_element(details.clone())`.
ctx.scroll_into_view(&details);
```

`VirtualizedList` sizes the same way: `.height(...)` or `.flex(...)` gives it a fixed viewport, while `.max_height(...)`
alone lets a short list take its content height and a long one stop at the cap.

```rust
VirtualizedList::new(ctx, items)
  .max_height(240.0)
  .mount_keyed::<ItemRow, _, _, _>(|item| item.id, |item| item.clone())
```

`.reveal_key(Some(key))` scrolls the row with that key into view, for example to show the selection again after
navigating back to a list. The reveal fires on the first render and again each time the key changes, never on other
re-renders, so it does not fight the user's scrolling. A row that is already fully visible does not move; any other row
is scrolled to the top of the viewport. The row does not need to be mounted: the list places it from its measured and
estimated row heights and settles on the exact offset over the next few frames as the rows around it are measured. A
key that is not among the items is dropped, except while the list has no items yet, so a list mounted before its data
loads still opens on the row.

When the items change so that the row keys differ (a row added, removed or moved), the list keeps the first visible row
where it was, and a list at its end stays at its end. Rows added or removed below that row (a message appended to a
chat, a trailing row dropped) cost no more than the rows in view. A change above it measures the rows not measured yet
over the next frames to restore the exact offset. A scroll asked for in the same update wins over that: a pending
scroll queued on the list's `ScrollState` (`scroll_to_bottom_pending`, `scroll_to_top_pending`, `set_scroll_pending`),
`set_scroll`, a `reveal_key` or a scroll into view, also when it lands while the list is still measuring its rows to
keep that row in place.

Use `reveal_key` for rows of a `VirtualizedList`: only the rows in and near the viewport are mounted, so an element of
any other row does not exist to be scrolled to. `ctx.scroll_into_view` does reach an element inside a mounted row, for
example a field in the row being edited.

Before the list has measured a row it cannot size its window (the viewport is not laid out yet), so its first pass
renders a fixed number of bootstrap rows, 16 by default or `.bootstrap_rows(n)`, whose heights seed the estimate the
next pass sizes the window with. They come from where the list opens: the last rows when `scroll_to_bottom_pending` was
queued on its `ScrollState` (a chat), the rows from the `reveal_key`'s row, or else the first rows. A chat of 300 long
replies opened at its end builds and lays out its last 16 only, however long it is; its first rows are built when they
are scrolled to. A list whose rows are tall passes fewer; one whose rows are short can pass about a viewport's worth so
its first frame is full. When the items are swapped for others whose keys all differ (another conversation under the
same list), the measured heights go but their average stays as the estimate, so the new rows are windowed where the
list opens, about a viewport's worth, instead of bootstrapped again. A row whose key stays but whose item changed (the
same line number in another file) loses its measured height too and counts as the estimate until it is measured again.

Rows the list has not measured count as the average of the rows it has. When measured heights change, because a row
grew (the person opened it) or the rows scrolled to measure other than their estimate, the first row in view stays
where it is on screen: the offset moves with the content above it. A list at its end stays at its end instead, and a
scroll asked for (`set_scroll`, a pending scroll, a reveal) lands where it asked. In a list whose rows differ much in
height, one tall row measured among few pulls the average up for every row not measured yet; `.estimate_height(|item|
...)` gives the height of a row not measured yet instead, by kind for example, so the estimate does not move with the
rows measured, and the list is sized from its first pass without bootstrap rows.

```rust
VirtualizedList::new(ctx, entries)
  .estimate_height(|entry| if entry.is_output() { 240.0 } else { 56.0 })
  .mount_keyed::<EntryRow, _, _, _>(|entry| entry.id, Clone::clone)
```

### Middle-button autoscroll

A scroll container can scroll the way browsers do while the middle button is held: `.middle_button_autoscroll(true)` on
`ScrollVertical`, `ScrollHorizontal`, `ScrollBoth` or `VirtualizedList` turns it on for that container; it is off by
default, so an app picks the containers that have it (a long chat, not a code editor, for example).

```rust
VirtualizedList::new(ctx, entries)
  .middle_button_autoscroll(true)
  .flex(1.0)
  .mount_keyed::<Entry, _, _, _>(|entry| entry.id, |entry| entry.clone())
```

A middle press over the container draws a mark where the button went down. While the button is held, the content
scrolls toward the pointer, faster the farther the pointer is from the mark, on the axes the container scrolls; within
about 12 logical pixels of the mark it stands still. Releasing the button, pressing Esc or the pointer leaving the window
ends it, and that release clicks nothing. The container's `on_scroll` handlers (and its scroll-reach handlers) hear each
step, so a `VirtualizedList` mounts the rows it scrolls to.

The innermost container under the pointer that has it on and has content to scroll takes the press. A press on an
element that takes the middle button itself is left to it: an `on_mouse_click(MouseButton::Middle, ...)` handler, a drag
that starts with the middle button, or a `on_mouse_down` handler that calls `prevent_default()` (as a Markdown link does
when the app takes middle clicks). The wheel, the scrollbar and text selection are unchanged.

### Row state in a virtualized list

A row that scrolls out of the rendered window (the viewport and `overscan_px` beyond it) is unmounted, and it is created
again from its props when it scrolls back. Everything the row component holds itself starts over: fields set in
`create`, signals from `ctx.signal`, element refs, the value signal of a text input the row created, and focus inside
the row. Keep state that must survive scrolling above the list, keyed by the row key:

- data belongs in the items themselves;
- view state (expanded, a draft being edited) goes in a signal or store the screen owns, such as a
  `Signal<HashMap<Key, RowState>>`, or a `HashMap<Key, Signal<String>>` of text-input values. Hand it to the rows with
  `ctx.provide` in the screen and `ctx.use_context` in the row, or through the row props. A row that reads its entry
  during render re-renders when the entry changes.

Remove entries whose keys left the items when the items change. `ctx.persistent_value` is for values kept across app
restarts, not for this.

```rust
// Screen `create`: one map for all rows, keyed like the rows.
let expanded: Signal<HashMap<TaskId, bool>> = ctx.signal(HashMap::new());
ctx.provide(expanded);

// Row `create` keeps the map; `render` reads the row's entry.
let expanded = ctx.use_context::<Signal<HashMap<TaskId, bool>>>().expect("provided by the screen");
let open = expanded.with(|rows| rows.get(&task.id).copied().unwrap_or(false));
// On toggle:
expanded.update(|rows| {
  rows.insert(task.id, !open);
});
```

```rust
VirtualizedList::new(ctx, tasks)
  .flex(1.0)
  .reveal_key(selected.get().map(|id| id.to_string()))
  .mount_keyed::<TaskRow, _, _, _>(|task| task.id, |task| task.clone())
```

## Text

Text is measured by the glyph engine and wraps within its width constraint.

```rust
lurq::components::Text::styled("hello", TextStyle {
  font_size: 18.0,
  ..TextStyle::default()
})
```

The glyph engine also records caret positions for text selection, one before and one after every glyph of the layout
the text is drawn with (a right-to-left glyph's caret before it is at its right edge). `Text::selectable(true)` uses
those positions for drag ranges, double-click word selection, and triple-click line selection. Wrapped or multiline
selections render separate highlight rectangles for each selected row.

Transforms are visual-only for layout size, but render output and hit testing use the transformed coordinates. Text
selection and text input carets therefore follow transformed text, including text inside a transformed parent.

See [Animation And Transforms](../animation-transforms/) for transform composition, keyframe animation, and transition
details.

### Text Transform Modes

`TextTransformMode::Bitmap` is the default. It rasterizes glyphs in their normal orientation and transforms those glyph
quads during rendering. This path preserves float placement and is best for animated transforms because changing the
transform does not create a new glyph atlas entry for every angle.

`TextTransformMode::Rasterized` is for static transformed text. It bakes the transform into the glyph mask, uses float
screen-space placement for the baked mask, and emits identity-transform glyph quads. This produces sharper rotated edges
than GPU-transforming the normal glyph bitmap, but the transform matrix is part of the glyph cache key, so continuously
animated angles can grow the atlas.

```rust
use lurq::node::{TextTransformMode, transform::Transform2D};

lurq::components::Text::new("Static rotated text")
  .text_transform_mode(TextTransformMode::Rasterized)
  .transform(Transform2D::rotate_deg(-8.0))
```
