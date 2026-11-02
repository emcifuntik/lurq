---
title: Styled text inputs
description: Bold, italic, inline code, strikethrough and links edited in place in a TextInput.
---

A `TextInput` bound to a spans signal edits styled text: the value stays the plain `Signal<String>`, and a
`Signal<Vec<StyledSpan>>` holds the styled runs over it as UTF-8 byte ranges. The input draws each run with the
theme's text marks, moves the spans with every edit, keeps them in its undo history and writes them back. The app
owns the model: it loads one by setting both signals and hears every change the input makes. A rich editor (a
block editor that stores Markdown, for example) builds on this; lurq parses and writes no Markdown itself.

Styled inputs come with the `markdown` feature. Without it none of this API exists and a `TextInput` does no span
work at all.

## Binding the spans

```rust
use lurq::{
  components::{SpanStyle, StyledChange, StyledEdit, StyledSpan, TextInput, TextInputController},
  core::Signal,
};

fn paragraph(value: Signal<String>, spans: Signal<Vec<StyledSpan>>, controller: TextInputController) -> TextInput {
  TextInput::new(value)
    .spans(spans)
    .controller(controller)
    .multiline()
    .on_styled_change(|change: &StyledChange| {
      // What changed, and the spans the input now holds.
      if let StyledEdit::Text { replaced, .. } = change.edit() {
        let _ = (replaced, change.inserted_text());
      }
      let _ = change.spans();
    })
}

let value = Signal::new("Read the docs".to_owned());
let spans = Signal::new(vec![
  StyledSpan::new(0..4, SpanStyle::new().bold()),
  StyledSpan::new(9..13, SpanStyle::new().link("https://example.com")),
]);
```

A `SpanStyle` has `bold`, `italic`, `code`, `strikethrough` and an optional `link` target. Spans the input reports
are canonical: sorted, disjoint, styled only (plain text has no span), never splitting a character, with equal
neighbours merged. `StyledText::new` puts any spans in that form, and `StyledText::runs` walks the whole text as
runs of one style, plain runs included, which is the walk a Markdown writer needs.

`on_styled_change` runs after every change the input makes with a `StyledChange`: the edit (`StyledEdit::Text`
with the replaced range and the inserted length, `Style` with the restyled range, or `Replaced` for undo and redo),
the spans the input now holds, shared rather than copied, and the text on demand (`with_text`, `inserted_text`,
or `to_styled_text` for an owned copy). Nothing of the text is copied unless the handler asks, so an app can keep
its own model in step with an edit at a time.

To load a model, set the value and the spans signals; the input lays out again and does not report the write. A
spans signal that does not match the value is clamped to it.

## Typing and editing

Caret moves, selection by pointer and keyboard, word and line moves, double and triple click, deleting and input
method composition work as in a plain input; styles do not change them. Typed text takes the style of the text
before the caret, with two exceptions: a link does not grow past its end, and at the start of a line the character
after the caret lends its marks (not its link). Typing over a selection takes the style of its first character. A
composition is drawn in the style it will commit in.

## Styling commands

The controller carries what a toolbar or a shortcut calls; each is one undo step with the text edits around it:

- `toggle_mark(TextMark::Bold)` turns a mark on over the selection unless all of it has it, then off. With nothing
  selected it toggles the mark of the next typed text, which is how Ctrl+B works while typing. `set_mark` sets it.
- `set_link(Some(target))` links the selection; `set_link(None)` unlinks it. With nothing selected it changes the
  whole link the caret is in.
- `set_pending_style(Some(style))` sets the whole style of the next typed text at the caret.
- `selection_style()` reports, per mark, whether all, some or none of the selection has it (`MarkPresence`), and
  the link target when the selection is one link; with nothing selected, the style typing would give.
- `styled_text()` and `selected_styled_text()` read the model and the selection with its spans.

lurq binds no shortcut to these: handle Ctrl+B in the input's `on_key_down` and call `prevent_default()`.

```rust
use lurq::{app::events::KeyboardEvent, components::TextMark};

let shortcuts = controller.clone();
input.on_key_down(move |event: KeyboardEvent| {
  let command = if cfg!(target_os = "macos") { event.meta } else { event.ctrl };
  if command && event.key.eq_ignore_ascii_case("b") {
    let _ = shortcuts.toggle_mark(TextMark::Bold);
    event.prevent_default();
  }
})
```

A toolbar button that should leave the keyboard focus in the input is built with `focusable(false)`. The
controller keeps the selection when focus moves anyway.

## Copy, paste and links

- `copy_format(f)` sets what Ctrl+C and Ctrl+X put on the clipboard: `f` gets the styled selection and returns the
  text, such as Markdown. Without it the plain text is copied.
- A paste inserts the clipboard's text in the typing style. To paste styled text, handle `on_paste`, parse the
  text, call `controller.replace_selection_styled(&styled)` and `prevent_default()`.
- `on_link_click(f)` runs when a link is clicked with Ctrl (Cmd on macOS) held, with its target and whole range;
  the app opens it. A plain click places the caret.

## Look

The theme's `ThemeTextMarks` (`theme.set_text_marks`) holds one `TextMarkStyle` per mark: font family, size scale,
weight, style, features, a color and a background fill (palette roles or colors), and an underline or a line
through. A run with several marks applies code, bold, italic, strikethrough, then link. The defaults are a bold
weight, an italic style, the `monospace` family at 0.92 over the `SurfaceRaised` role, a line through, and the
`Accent` role underlined.

## Limits

- A masked input is plain, and a styled input ignores `tokens`.
- Code fills have no horizontal padding.
- A multiline input lays out a paragraph at a time and keeps each one, so an edit lays out only the paragraph it
  changed. A long left-to-right paragraph is also shaped in chunks of a few hundred bytes, cut after spaces where
  its own words choose, so an edit shapes only the chunk it changed; its rows are still laid out again. Paragraphs
  with right-to-left text or tabs are shaped whole. `benches/styled_input.rs` measures typing, caret moves,
  selection drags and style toggles on 4,000 characters as one paragraph and as 50.
- Span edits copy the input's span list (linear in its spans, not in its text).
