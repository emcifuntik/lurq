---
title: Modals
description: Declaring render-flow modal overlays with scoped targets.
---

# Modals

## Declaring A Modal

Use `Modal` as a normal child in your render tree. The modal declaration is layout-neutral, so it does not change parent layout, and its content is layered over the selected target when the open state is `true`.

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Button, Column, Modal, Root, Text},
  core::Signal,
  node::Element,
};

struct App {
  open: Signal<bool>,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self { open: ctx.signal(false) }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let open = self.open.clone();

    Column::new()
      .child(Button::new("Open modal").on_click({
        let open = open.clone();
        move |_| open.set(true)
      }))
      .child(
        Modal::new(
          Column::new()
            .child(Text::new("Modal content"))
            .child(Button::new("Close").on_click({
              let open = open.clone();
              move |_| open.set(false)
            })),
        )
        .open(self.open.clone())
        .target(Root),
      )
  }
}
```

## Targets

`Modal::target(...)` accepts `Parent`, `Root`, or an `ElementRef`.

```rust
Modal::new(content).open(open.clone()).target(Parent);
Modal::new(content).open(open.clone()).target(Root);
Modal::new(content).open(open.clone()).target(panel_ref);
```

- `Parent` covers the declaring parent bounds and is the default target.
- `Root` covers the viewport.
- `ElementRef` covers that element's bounds.

## Behavior

- When the signal is `false`, the modal declaration remains layout-neutral and no modal layer is rendered.
- When the signal is `true`, the modal content is layered above its target.
- Setting the signal back to `false` removes the modal on the next render pass. Content with an exit animation stays
  on screen, taking no input, until the animation ends: see
  [Presence](../animation-transforms/#presence-entering-and-leaving-layers).
- Multiple render-flow modals stack in declaration/layer order.
- Signal-backed modals close on `Escape` by default.
- An open modal traps Tab and Shift+Tab: they cycle the topmost modal's stops (elements with `tab_index(0)` or higher, and form controls) and never reach the page behind it, with or without a form. A form inside the modal cycles inside it. See [Modal Focus Trap](../focus-navigation/#modal-focus-trap).

## Sheets

Added in **lurq 0.61.0**. A `Sheet` is content that comes up from the bottom of the window over everything in
it, as a phone's apps show a choice or a short task. It is a component of its own, mounted anywhere in the tree
and taking no room there, and it is up while its signal is set:

```rust
use lurq::components::{Sheet, SheetDetent, SheetProps, SheetStyle};

let sheet = Sheet::mount(
  ctx,
  SheetProps::new(self.open.clone())
    .detents([SheetDetent::Medium, SheetDetent::Large])
    .style(SheetStyle::new().grabber(true)),
  content,
);
```

- **It comes up and goes down with a move**, a spring, and the window behind dims. It stays in the tree until it
  has gone down.
- **It is put away** by clearing the signal, by Escape, by a press beside it, and by a finger that drags it down
  far enough or throws it down. `SheetProps::dismissable(false)` leaves only the signal.
- **It stands at its detents**: `Content` (as high as its content, the default), `Medium` (half the window),
  `Large` (the whole window under the status bar), `Fraction(share)` and `Height(pixels)`. It comes up at the
  first, a finger moves it between them, and it goes to the nearest to where the finger threw it.
  `SheetProps::detent(signal)` says which it is at, by its place in the list, and sends it to another.
- **Content that scrolls** is named with `SheetProps::scroll(state)`, the `ScrollState` of its list: a finger
  going up moves the sheet to its highest before the list scrolls, and one going down takes the sheet only when
  the list is at its top.
- **The content is clear of the home indicator and the keyboard**: it is padded by the safe area, and the
  sheet rises by the keyboard's height, as far as its highest place.
- Like a `Modal`, it traps Tab.

Several sheets in one component are told apart with `Sheet::mount_keyed`.

### A stack of screens in a sheet

`Sheet::mount_with(ctx, props, |ctx| ..)` makes the sheet's content in the sheet, as a route's is made in its
outlet. What is mounted there is the sheet's own: `ctx.safe_area()` is nothing inside it, since the sheet
stands clear of the status bar and pads its content by the home indicator and the keyboard. That is what puts
a `StackOutlet`'s bar at the sheet's top:

```rust
let router = self.sheet_router.clone();
Sheet::mount_with(
  ctx,
  SheetProps::new(self.open.clone()).detents([SheetDetent::Large]),
  move |ctx| Router::mount(ctx, router.clone()),
)
```

The router is the sheet's own, made once with `ctx.router(..)`, with a `StackOutlet` as its routes' layout.

`SheetStyle` is how it looks and moves; `SheetStyle::dark()` has the platform's dark colors.

| Field | Starts as | |
| --- | --- | --- |
| `background` | white | The sheet's own color |
| `corner_radius` | 20 | Of its top corners |
| `backdrop`, `dim` | black, 0.3 | What is drawn over the window behind, and how much of it shows |
| `grabber` | `false` | The bar at the top that says it can be dragged |
| `grabber_color`, `grabber_width`, `grabber_height`, `grabber_top` | 36 by 5, 5 under the edge | |
| `top_margin` | 10 | How far under the status bar the highest sheet stops |
| `max_width` | none | In a wider window the sheet stands in the middle |
| `duration` | 500 ms | Of every move |
| `throw` | 0.15 | How many seconds of a finger's speed ahead the sheet is taken to be going |

### The window behind

Behind a sheet at its full height a phone's apps draw back: the app gets smaller, its top goes under the status
bar's room and its corners round. `SheetHost` around the whole of the app's content does that, for every sheet
mounted in the content. It makes its content itself, as a route's is made in its outlet:

```rust
fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  let router = self.router.clone();
  SheetHost::mount(ctx, move |ctx| Router::mount(ctx, router.clone()))
}
```

The app draws back as far as the sheet has come from its next height down, with the sheet and with the finger
that has it. A sheet that never takes all the room only dims the app, and so does every sheet without a host.
`SheetHost::mount_with` takes a `SheetHostStyle`: `scale` (0.92), `corner_radius` (12) and the `background`
seen around the app (black).

A sheet that draws all of itself has a clear `background`, no `corner_radius` and no grabber: the content is
then the whole sheet, and the moves, the heights and the finger are still the component's.

## Popups And Outside Presses

`Popup` (and its alias `Popover`) layers content next to an anchor element. With a signal-backed open state it closes on `Escape` and on a left press outside it:

```rust
use lurq::components::{OutsidePress, Placement, Popup};

Popup::new(anchor_ref.clone(), menu_content)
  .open(open.clone())
  .placement(Placement::BottomStart);

// A light-dismiss popover: the closing press also reaches the element under it.
Popup::new(anchor_ref, hint_content)
  .open(open.clone())
  .outside_press(OutsidePress::PassThrough);
```

A popup or overlay is placed against its anchor's layout in every frame, so it moves with the anchor when a list scrolls or the page lays out again. While its anchor is scrolled or clipped wholly out of view (above a scroll container's viewport, under the header over a list, outside a `clip()`ped parent) it is not drawn, and it comes back with the anchor: it never floats over whatever covers the anchor. A `Select`'s menu does the same with its select, and a `Modal` targeting an element with that element. The open state does not change meanwhile, and a hidden popup is dismissed like a CSS popover that is out of view:

- A left press anywhere closes it (with `dismiss_on_outside_click`, a `Popup`'s default). The press is never consumed by a hidden popup, whatever its `outside_press`: it also reaches the element it lands on, since nothing on screen was in its way.
- Escape closes it (with `dismiss_on_escape`).
- Keyboard focus inside it goes back to its anchor (the trigger) or the first focusable element inside the anchor; if the anchor holds none, to the nearest focusable ancestor of the anchor.

A tooltip without an open signal, such as a Markdown link's, just stays hidden until its anchor is back in view.

A press is outside a popup when it lands neither on the popup, nor on its anchor, nor on a layer opened above it, such as a select menu or a popup opened from inside it. By default (`OutsidePress::Consume`) such a press only closes the popup, like a native menu: the element under the pointer receives no press, release or click, and focus does not move. The next press reaches it. This keeps a press meant to close a menu from also activating whatever lies under it, for example a tab, a list row or a button. `OutsidePress::PassThrough` closes the popup and delivers the press as usual, like a light-dismiss web popover; a handler that prevents the default of that press keeps the popup open.

`Select` menus follow the same rule, set with `Select::outside_press(...)`. `Overlay::outside_press(...)` applies to overlays with `dismiss_on_outside_click(true)`. A popup with `dismiss_on_outside_click(false)` or a static open state never closes on a press and never consumes one. Only left presses close popups; other buttons pass through and leave them open. When one press is outside several popups, it closes all of them, and it is consumed if any of them consumes it.

A press on another dropdown's trigger is never consumed, like in a menu bar: it closes the open popup or select menu and goes on to that trigger, so one press opens the other dropdown and moves focus to its trigger. The first interactive element under the pointer (a button, a select, anything that takes focus) decides: it is a trigger when it is a `Select`, or the anchor itself of a popup (or overlay) with `dismiss_on_outside_click(true)` and a signal-backed open state, open or closed. Anything else inside an anchor is ordinary content: the other half of a split button anchored as a row, the items of a status bar that anchors a popover, and the bar's empty space only close the open dropdown. The press closes the consuming popups and the open selects before it reaches the trigger, so a trigger that prevents the default of its press still switches. A press on a scrollbar over a trigger is not on the trigger. A press on the open popup's own anchor is not outside it, so the anchor's own handler decides (a toggle closes it, and a select menu open inside the popup closes too). Set `switch_on_anchor_press(false)` on a popup whose anchor a left press does not open, such as a context menu on a list row or an autocomplete list on a field. lurq only knows the anchor of a popup it can see: declare the popup always and drive it with its open signal, rather than adding it to the tree only while it is open, or a press on its anchor while another dropdown is open only closes that dropdown.
