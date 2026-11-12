---
title: Focus And Keyboard Navigation
description: Which elements take focus, how Tab moves between them, modal focus traps, focused and focus-visible styles, and testing focus.
---

# Focus And Keyboard Navigation

One element has focus at a time. It receives keyboard events, shows its focused style, and is where Tab and Shift+Tab start. It is focused whatever moved focus there: a click, Tab, `ctx.focus(&element_ref)`, or `ElementHandle::focus()`. Whether it also shows a focus ring for keyboard users is a second state, [focus-visible](#focus-visible), like CSS `:focus-visible`.

## Focusable Elements

Buttons, text inputs, checkboxes, sliders, selects, and any element with a `tab_index` are focusable. A click on one focuses it, and a click on its content (a button's label) focuses the nearest focusable ancestor. A press where nothing can take focus (empty space, a plain row, a label) blurs the focused element, button or input, like HTML.

- `.focusable(true)` makes any element focusable by click and by request. It does not put the element in the Tab order outside a form; add `.tab_index(0)` for that.
- `.focusable(false)` keeps an element from ever taking focus: not by click, not by Tab, not by `ctx.focus`. A click on it does not change focus either: whatever had focus keeps it. Its click handlers still run, and a checkbox still toggles. Use it for toolbar buttons that must leave focus in an editor.

```rust
use lurq::components::Button;

// Clicking it runs the handler; focus stays in the text editor.
Button::new("Bold").focusable(false).on_click(|_| toggle_bold());
```

## Tab Order

`tab_index` follows HTML `tabindex`:

| Element location | `tab_index` not set | `tab_index(0)` | `tab_index(n > 0)` | `tab_index(-1)` |
| --- | --- | --- | --- | --- |
| Inside a form | in Tab order | in layout order | first, by number | skipped |
| Outside any form | not in Tab order | in layout order | first, by number | skipped |

- Positive values come first, in ascending order; equal values keep tree order. Then come `tab_index(0)` elements (and, inside forms, controls without a tab index) in tree order.
- `tab_index(-1)` only removes an element from the Tab order. A click still focuses it, as in HTML.
- A button is one stop: its content is never a separate stop.
- Setting a tab index makes the element focusable, unless it is `focusable(false)`.

Outside forms, only elements that opt in with `tab_index(0)` or higher are stops. Give toolbar, sidebar, and list buttons `tab_index(0)` to make them reachable by keyboard:

```rust
use lurq::components::{Button, Row};

Row::new()
  .child(Button::new("New").tab_index(0).on_click(|_| new_file()))
  .child(Button::new("Open").tab_index(0).on_click(|_| open_file()));
```

## Tab Scope

Tab and Shift+Tab move through one scope and wrap around at its ends:

1. **A modal is open**: the topmost open modal (see [Modal Focus Trap](#modal-focus-trap)).
2. **Otherwise**: the whole window, including open popups and overlays after the page, in tree order.

`WindowChrome`'s title bar and resize zones are a layer over the page, but they are window decoration, not a modal: they never become the Tab scope, and stops placed in the title bar follow the page's stops in the window order. A `Modal` declared in the chrome's content or passed to `WindowChrome::overlay` is a real modal and traps Tab as usual.

Forms do not have a scope of their own. A form's controls are stops without a tab index, in tree order among the scope's other stops, and the form is passed through like in a browser: Tab from a toolbar button reaches the form's first field, Tab from its last control moves on to the next stop after the form, and Shift+Tab from its first control goes back to the stop before it. A form cycles only when it is all its scope contains, for example the only content of a modal.

When the focused element is not a stop itself (a clicked button without a tab index, or a text input outside a form), Tab continues from its place in the tree, like a browser: Tab goes to the next stop after it, Shift+Tab to the previous one. With nothing focused, Tab goes to the first stop and Shift+Tab to the last.

Tab with no stops in scope outside a modal is not handled, so it reaches other keyboard defaults. An `on_key_down` handler that calls `prevent_default()` on Tab replaces traversal entirely (see [Keyboard And Focus](../styling-events/#keyboard-and-focus)).

Tab traversal does not need the `form` feature; without it there are no forms, and only elements with a tab index are stops.

## Modal Focus Trap

An open `Modal` confines Tab and Shift+Tab to itself, whether or not it contains a form:

- Focus behind the modal is never a stop. When focus is still on the page behind it (the button that opened it), the next Tab moves into the modal.
- Stops inside the modal follow the same rules: `tab_index(0)` or higher, and form controls inside a form in the modal. A form in a modal cycles inside the modal, together with the modal's other stops.
- A modal without stops consumes Tab and keeps focus where it is, so Tab never reaches the page behind it.
- With nested or stacked modals, the topmost one traps.
- When the modal closes, the window scope applies again.

Pointer input is not trapped; a click on the page behind a `Parent`- or element-targeted modal still focuses what it hits.

## Scrolling Focus Into View

When Tab or Shift+Tab, `ctx.focus(&element_ref)` or `ElementHandle::focus()` moves focus to an element inside a scroll container, every scroll container around it scrolls by the smallest amount that shows the element, innermost first, on each axis the container scrolls, like a browser's `element.focus()`. An element already in view does not move anything; one taller or wider than the viewport is aligned to the viewport start. A focus request scrolls in the pass that applies it, so the frame drawn next already shows the element. Focus moved by a click does not scroll: what was clicked is already on screen, and moving it under the pointer would be a jump.

To scroll an element into view without focusing it, use `ctx.scroll_into_view(&element_ref)` in a component, or `ElementHandle::scroll_into_view()` on a handle from `Tree::get_element_by_id_mut` (see [Scroll](../layout/#scroll)).

## Focus-Visible

Focus-visible is the keyboard-only part of focus, like CSS `:focus-visible`: a button focused by Tab shows a focus ring, the same button focused by a click does not. The focused element is focus-visible when the last input that could move focus was the keyboard:

- **Keyboard**: Tab and Shift+Tab, and any key press other than a bare modifier (Shift, Control, Alt, Super/Command) made without Control, Alt or Super/Command held. Arrow keys, Space, Enter, Escape and letters count, so pressing a key on a clicked button shows its ring, as in a browser; a shortcut such as Ctrl+C or Cmd+C does not.
- **Pointer**: any mouse button press in the window, including one that only closes a popup, and `ElementHandle::click()`. It hides the ring of the element that keeps focus.
- **Focus requests** (`ctx.focus(&element_ref)`, `ElementHandle::focus()`) inherit the last input's modality: a request made from a key handler shows the ring, one made from a click handler does not. Before any input the modality is pointer, so an app that focuses an element at startup shows no ring until the user touches the keyboard.
- **Text inputs** are focus-visible whenever they have focus, clicked or not, as browsers treat text fields: the caret alone does not show which field takes the keys.

Synthetic input (`lurq::app::synthetic_input`, the MCP `lurq_interact` tool) goes through the same mouse and key entry points and follows the same rules. The rules are the same on Windows and macOS.

Read the state with `InteractionState::is_focus_visible()` (on a node given `.interactive(state)`), `core::ElementRef::focus_visible()` and its reactive `focus_visible_signal()`, or `Tree::focus_visible()` for the focused element. The focused state (`is_focused()`, `focused()`, `focus_signal()`) is unchanged and stays true for any focus.

## Focused Styles

Every element accepts a focused state style, merged over the base style while it has focus, and a focus-visible style, merged over the focused style while it is focus-visible. Both sit under the hovered and active styles, so a hovered border paints over a focus ring:

```rust
use lurq::{
  app::theme::{BorderSize, PaletteColor},
  components::Button,
};

// A ring for keyboard focus only; a click focuses the button without it.
Button::new("Save")
  .tab_index(0)
  .border_inside(BorderSize::Sm, PaletteColor::Border)
  .focus_visible(|style| style.border_inside(BorderSize::Sm, PaletteColor::BorderFocus));
```

Use `.focused(...)` / `.focused_style(...)` for a style on any focus, and `.focus_visible(...)` / `.focus_visible_style(...)` for a focus ring. A ring that sits outside the control and moves nothing is an [outline](../styling-events/#outline): `.focus_visible(|style| style.outline(Outline::new(2.0, PaletteColor::BorderFocus).offset(2.0)))`.

Controls whose visuals are drawn from part styles have part-level focus styles, layered like their hovered styles. They show while the control is focus-visible, like the native focus ring of a browser's checkbox, range input and select: a click or a drag does not show them, Tab or a key press does. They change paint only; a width or height in them is ignored so focus never moves layout:

| Control | Focus ring (focus-visible) |
| --- | --- |
| Button, text input, any element | `.focus_visible(...)` / `.focus_visible_style(...)` |
| `Checkbox` | `.box_focused(...)` / `.box_focused_style(...)` |
| `Slider` | `.thumb_focused(...)` / `.thumb_focused_style(...)` |
| `Select` | `SelectStyle::trigger_focused(...)` |

For a checkbox, slider or select that should mark every focus, put a `.focused(...)` style on the control itself.

The compound form controls take their focus border from the form theme: `form.input.border_focus`, `form.checkbox.border_focus`, `form.slider.thumb_border_focus`, and `border_focus` on both `form.button` roles, all `PaletteColor::BorderFocus` by default. The buttons, the checkbox and the slider draw it while focus-visible; the text input draws it for any focus. See [Form Theme](../theme/#form-theme).

## Testing Focus

`Tree::focused_element()` returns the element that has focus, like `document.activeElement`: the control itself, not the wrapper that owns its `on_focus` handler. `Tree::focus_visible()` tells whether it is [focus-visible](#focus-visible). The MCP `lurq_read_tree` tool lists a focus-visible node's states as `focused,focus-visible`, and `lurq_inspect` reports `"focus_visible": true`.

`Tree::pass_headless(&mut app)` runs a pass without a window: it rebuilds components and lays out the tree, overlays and modals included, so hit testing, focus, and Tab work, but it draws nothing and never calls the render engine. It needs no window handle and no `unsafe`:

```rust
use lurq::{
  app::{App, Tree},
  components::{Button, Column, Modal, Root},
};

let mut tree = Tree::new();
tree.set_root(
  Column::new()
    .child(Button::new("Toolbar").id("toolbar").tab_index(0))
    .child(
      Modal::new(
        Column::new()
          .child(Button::new("Cancel").id("cancel").tab_index(0))
          .child(Button::new("Confirm").id("confirm").tab_index(0)),
      )
      .target(Root),
    ),
);
tree.pass_headless(&mut App::new());

tree.key_down("Tab".into(), "Tab".into(), false, false, false);
assert_eq!(tree.focused_element().and_then(|element| element.id()), Some("cancel"));
```

Call `tree.resize(width, height)` before the pass to choose the viewport (800×600 by default). Run another `pass_headless` after state changes that re-render (opening a signal-backed modal) and after keyboard scrolling, before reading bounds. See [Testing](../testing/).

## Giving The Focus Up

`ctx.blur()` takes the focus from whatever has it after the current render: a text input being typed into is
left, and a phone's keyboard goes away. A request to focus made in the same render wins.
