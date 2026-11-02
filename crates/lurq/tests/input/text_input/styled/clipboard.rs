//! Copy gives the app's form of the styled selection; paste takes plain
//! text in the typing style, or styled spans from the app; a link clicked
//! with the shortcut modifier is reported.
use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    events::{ClipboardContent, MouseButton, PasteEvent},
    theme::{TextMarkStyle, ThemeTextMarks},
  },
  components::{LinkClick, SpanStyle, StyledText},
  node::color::Color,
};

use super::{Editor, ID, bold, span};

/// A tiny Markdown writer for bold runs, as an app's copy format.
fn markdown(text: &StyledText) -> String {
  text
    .runs()
    .into_iter()
    .map(|(run, style)| {
      if style.bold {
        format!("**{run}**")
      } else {
        run.to_owned()
      }
    })
    .collect()
}

#[test]
fn styled_copy_puts_the_apps_format_of_the_selection_on_the_clipboard() {
  let mut editor = Editor::with("plain bold end", vec![span(6..10, bold())], |input| {
    input.copy_format(markdown)
  });
  editor.select(3, 12);
  let copied = editor
    .tree
    .get_element_by_id_mut(ID)
    .unwrap()
    .as_text_input()
    .unwrap()
    .copied_text();
  assert_eq!(copied.as_deref(), Some("in **bold** e"));
  let selected = editor.controller.selected_styled_text().unwrap().unwrap();
  assert_eq!(selected, StyledText::new("in bold e", vec![span(3..7, bold())]));
}

#[test]
fn styled_a_plain_paste_takes_the_style_typing_would() {
  let mut editor = Editor::new("bold", vec![span(0..4, bold())]);
  assert!(editor.tree.paste(ClipboardContent::text("er")));
  assert_eq!(
    (editor.value.get(), editor.spans()),
    ("bolder".into(), vec![span(0..6, bold())])
  );
}

#[test]
fn styled_a_paste_handler_inserts_the_apps_styled_spans_as_one_undo_step() {
  let controller = Arc::new(Mutex::new(None::<lurq::components::TextInputController>));
  let slot = controller.clone();
  let mut editor = Editor::with("a  z", vec![], move |input| {
    let slot = slot.clone();
    input.on_paste(move |event: PasteEvent| {
      let Some(text) = event.text() else {
        return;
      };
      // The app parses "**B** c" from Markdown.
      let parsed = StyledText::new(text.replace("**", ""), vec![span(0..1, SpanStyle::new().bold())]);
      if let Some(controller) = slot.lock().unwrap().as_ref() {
        controller.replace_selection_styled(&parsed).unwrap();
        event.prevent_default();
      }
    })
  });
  *controller.lock().unwrap() = Some(editor.controller.clone());
  editor.select(2, 2);
  assert!(editor.tree.paste(ClipboardContent::text("**B** c")));
  assert_eq!(editor.value.get(), "a B c z");
  assert_eq!(editor.spans(), vec![span(2..3, bold())]);
  editor.shortcut("z", "KeyZ", false);
  assert_eq!((editor.value.get(), editor.spans()), ("a  z".into(), vec![]));
}

#[test]
fn styled_a_modifier_click_on_a_link_reports_its_target_and_range() {
  const LINK: Color = Color::new(200, 30, 160, 255);
  let clicks = Arc::new(Mutex::new(Vec::<LinkClick>::new()));
  let log = clicks.clone();
  let link = SpanStyle::new().link("ORC-123");
  let mut editor = Editor::with(
    "see ORC-123 now",
    vec![span(4..7, link.clone()), span(7..11, link.clone().bold())],
    move |input| {
      let log = log.clone();
      input.on_link_click(move |click: &LinkClick| log.lock().unwrap().push(click.clone()))
    },
  );
  editor.app.theme().set_text_marks(ThemeTextMarks {
    link: TextMarkStyle {
      color: Some(LINK.into()),
      ..TextMarkStyle::default()
    },
    ..ThemeTextMarks::default()
  });
  let snapshot = editor.render();
  let glyph = snapshot
    .glyphs
    .iter()
    .find(|glyph| glyph.color == LINK.to_linear_f32_array())
    .expect("a link glyph");
  let (x, y) = (glyph.x + glyph.width / 2.0, glyph.y + glyph.height / 2.0);
  let (ctrl, meta) = (cfg!(not(target_os = "macos")), cfg!(target_os = "macos"));

  // A plain click places the caret and reports nothing.
  editor.tree.mouse_down(x, y, MouseButton::Left);
  editor.tree.mouse_up(x, y, MouseButton::Left);
  assert!(clicks.lock().unwrap().is_empty());

  editor
    .tree
    .mouse_down_with_meta(x, y, MouseButton::Left, false, ctrl, false, meta);
  editor
    .tree
    .mouse_up_with_meta(x, y, MouseButton::Left, false, ctrl, false, meta);
  assert_eq!(
    clicks.lock().unwrap().as_slice(),
    &[LinkClick {
      target: "ORC-123".into(),
      range: 4..11,
    }]
  );
}
