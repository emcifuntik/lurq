//! Typing, deleting, styling and undo move the spans with the text.
use lurq::{
  app::events::{ImeEvent, MouseButton},
  components::{MarkPresence, SpanStyle, StyledEdit, StyledText, TextMark},
};

use super::{Editor, ID, bold, span};
use crate::support::pointer_click;

#[test]
fn styled_typing_takes_the_style_of_the_text_before_the_caret() {
  let mut editor = Editor::new("ab cd", vec![span(0..2, bold())]);
  editor.select(2, 2);
  editor.type_text("X");
  assert_eq!(editor.value.get(), "abX cd");
  assert_eq!(editor.spans(), vec![span(0..3, bold())]);
  // After the plain space the typed text is plain.
  editor.select(4, 4);
  editor.type_text("Y");
  assert_eq!(editor.spans(), vec![span(0..3, bold())]);
  // At the very start the next character lends its marks.
  editor.select(0, 0);
  editor.type_text("Z");
  assert_eq!(editor.spans(), vec![span(0..4, bold())]);
  assert_eq!(editor.spans.get(), editor.spans(), "the input writes the spans signal");
}

#[test]
fn styled_typing_inside_a_link_extends_it_but_typing_at_its_end_does_not() {
  let link = SpanStyle::new().link("https://example.com");
  let mut editor = Editor::new("see docs", vec![span(4..8, link.clone())]);
  editor.select(6, 6);
  editor.type_text("x");
  assert_eq!(editor.spans(), vec![span(4..9, link.clone())]);
  editor.select(9, 9);
  editor.type_text("!");
  assert_eq!(editor.value.get(), "see doxcs!");
  assert_eq!(editor.spans(), vec![span(4..9, link)]);
}

#[test]
fn styled_backspace_delete_and_selection_deletion_cross_span_boundaries() {
  let mut editor = Editor::new("aaBBcc", vec![span(2..4, bold())]);
  editor.select(4, 4);
  editor.key("Backspace");
  assert_eq!(
    (editor.value.get(), editor.spans()),
    ("aaBcc".into(), vec![span(2..3, bold())])
  );
  editor.key("Delete");
  assert_eq!(
    (editor.value.get(), editor.spans()),
    ("aaBc".into(), vec![span(2..3, bold())])
  );
  editor.select(1, 3);
  editor.key("Backspace");
  assert_eq!((editor.value.get(), editor.spans()), ("ac".into(), vec![]));
}

#[test]
fn styled_typing_over_a_selection_takes_its_first_character_style() {
  let mut editor = Editor::new("one two", vec![span(4..7, bold())]);
  editor.select(4, 7);
  editor.type_text("2");
  assert_eq!(
    (editor.value.get(), editor.spans()),
    ("one 2".into(), vec![span(4..5, bold())])
  );
}

#[test]
fn styled_toggle_mark_on_a_selection_is_one_undo_step() {
  let mut editor = Editor::new("word text", vec![]);
  editor.select(0, 4);
  assert!(editor.controller.toggle_mark(TextMark::Bold).unwrap());
  assert_eq!(editor.spans(), vec![span(0..4, bold())]);
  let style = editor.controller.selection_style().unwrap().unwrap();
  assert_eq!(style.bold, MarkPresence::All);
  // A selection only partly bold turns bold everywhere.
  editor.select(2, 7);
  assert_eq!(
    editor.controller.selection_style().unwrap().unwrap().bold,
    MarkPresence::Some
  );
  editor.controller.toggle_mark(TextMark::Bold).unwrap();
  assert_eq!(editor.spans(), vec![span(0..7, bold())]);
  editor.shortcut("z", "KeyZ", false);
  assert_eq!(editor.spans(), vec![span(0..4, bold())]);
  editor.shortcut("z", "KeyZ", false);
  assert_eq!(editor.spans(), vec![]);
  editor.shortcut("z", "KeyZ", true);
  assert_eq!(editor.spans(), vec![span(0..4, bold())]);
  assert_eq!(editor.value.get(), "word text");
}

#[test]
fn styled_toggling_with_nothing_selected_styles_the_next_typed_text() {
  let mut editor = Editor::new("plain", vec![]);
  assert!(editor.controller.toggle_mark(TextMark::Bold).unwrap());
  assert_eq!(
    editor.controller.selection_style().unwrap().unwrap().bold,
    MarkPresence::All
  );
  assert_eq!(editor.spans(), vec![], "nothing is styled until text is typed");
  editor.type_text("BC");
  assert_eq!(editor.spans(), vec![span(5..7, bold())]);
  editor.controller.toggle_mark(TextMark::Bold).unwrap();
  editor.type_text("d");
  assert_eq!(editor.spans(), vec![span(5..7, bold())]);
  // A caret moved away drops the pending style: the text before it decides again.
  editor.controller.toggle_mark(TextMark::Italic).unwrap();
  editor.key("ArrowLeft");
  editor.type_text("e");
  assert_eq!(editor.value.get(), "plainBCed");
  assert_eq!(editor.spans(), vec![span(5..8, bold())]);
}

#[test]
fn styled_set_link_links_a_selection_and_unlinks_the_link_at_the_caret() {
  let mut editor = Editor::new("open the docs", vec![span(9..13, bold())]);
  editor.select(5, 13);
  assert!(editor.controller.set_link(Some("doc:intro")).unwrap());
  let link = SpanStyle::new().link("doc:intro");
  assert_eq!(
    editor.spans(),
    vec![span(5..9, link.clone()), span(9..13, bold().link("doc:intro"))]
  );
  let style = editor.controller.selection_style().unwrap().unwrap();
  assert_eq!(style.link.as_deref(), Some("doc:intro"));
  // With the caret inside the link, unlinking removes all of it.
  editor.select(7, 7);
  assert!(editor.controller.set_link(None).unwrap());
  assert_eq!(editor.spans(), vec![span(9..13, bold())]);
}

#[test]
fn styled_every_change_is_reported_with_the_text_and_its_spans() {
  let mut editor = Editor::new("ab", vec![span(0..2, bold())]);
  editor.type_text("c");
  editor.key("Backspace");
  editor.select(0, 1);
  editor.controller.toggle_mark(TextMark::Italic).unwrap();
  editor.shortcut("z", "KeyZ", false);
  let changes = editor.changes.lock().unwrap().clone();
  assert_eq!(changes.len(), 4, "type, delete, style, undo: {changes:?}");
  assert_eq!(changes[0].0, StyledText::new("abc", vec![span(0..3, bold())]));
  assert_eq!(
    changes[0].1,
    StyledEdit::Text {
      replaced: 2..2,
      inserted: 1
    }
  );
  assert_eq!(
    changes[1].1,
    StyledEdit::Text {
      replaced: 2..3,
      inserted: 0
    }
  );
  assert_eq!(
    changes[2].0.spans,
    vec![span(0..1, bold().italic()), span(1..2, bold())]
  );
  assert_eq!(changes[2].1, StyledEdit::Style { range: 0..1 });
  assert_eq!(changes[3].1, StyledEdit::Replaced);
  assert_eq!(changes[3].0, editor.styled());
}

#[test]
fn styled_an_app_write_of_both_signals_loads_a_new_model_without_reporting_it() {
  let mut editor = Editor::new("old", vec![]);
  editor.value.set("new text".to_owned());
  editor.spans.set(vec![span(0..3, bold())]);
  editor.render();
  assert_eq!(editor.styled(), StyledText::new("new text", vec![span(0..3, bold())]));
  assert!(editor.changes.lock().unwrap().is_empty());
  editor.select(3, 3);
  editor.type_text("s");
  assert_eq!(editor.spans(), vec![span(0..4, bold())]);
}

#[test]
fn styled_ime_composition_commits_in_the_typing_style() {
  let mut editor = Editor::new("ab", vec![span(0..2, bold())]);
  editor.tree.ime(ImeEvent::Preedit {
    text: "かな".into(),
    cursor: Some((0, 6)),
  });
  editor.render();
  assert_eq!(
    editor.spans.get(),
    vec![span(0..2, bold())],
    "a composition is not text yet"
  );
  editor.tree.ime(ImeEvent::Commit("かな".into()));
  assert_eq!(editor.value.get(), "abかな");
  assert_eq!(editor.spans(), vec![span(0..8, bold())]);
}

#[test]
fn styled_word_and_line_moves_and_double_click_ignore_span_boundaries() {
  let mut editor = Editor::new("hello world", vec![span(0..3, bold())]);
  editor.select(0, 0);
  let (ctrl, alt) = if cfg!(target_os = "macos") {
    (false, true)
  } else {
    (true, false)
  };
  editor
    .tree
    .key_down("ArrowRight".into(), "ArrowRight".into(), true, ctrl, alt);
  // A word move stops at the start of the next word, crossing the bold "hel".
  assert_eq!(editor.controller.selection().unwrap().range(), 0..6);
  editor.key("End");
  assert_eq!(editor.controller.selection().unwrap().caret, 11);
  let rect = editor.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
  let (x, y) = (rect.x + 12.0, rect.y + rect.height / 2.0);
  pointer_click(&mut editor.tree, x, y, MouseButton::Left);
  pointer_click(&mut editor.tree, x, y, MouseButton::Left);
  let selected = editor
    .tree
    .get_element_by_id_mut(ID)
    .unwrap()
    .as_text_input()
    .unwrap()
    .selected_text();
  assert_eq!(selected.as_deref(), Some("hello"));
}

#[test]
fn styled_masked_input_refuses_styled_commands() {
  let editor = Editor::with("secret", vec![span(0..2, bold())], |input| input.mask());
  assert!(editor.controller.styled_text().is_err());
}
