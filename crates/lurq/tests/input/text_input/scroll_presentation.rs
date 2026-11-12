use std::sync::Arc;

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{ImeEvent, MouseButton},
  },
  components::{InputEditError, TextInput, TextInputController},
  core::Signal,
  layout::quad::QuadContent,
  node::{Element, color::Color},
};

use crate::support::{RenderSnapshot, pointer_click, render_pass_with_app};

const CARET: Color = Color::new(250, 12, 34, 255);

struct State {
  raw: Signal<String>,
  controller: TextInputController,
  unrelated: Signal<u64>,
  mask: Option<char>,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Props(#[devtools_ignore] Arc<State>);

impl PartialEq for Props {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

struct ScrollEditor;

impl Component for ScrollEditor {
  type Props = Props;
  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }
  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let state = ctx.props::<Self::Props>().0.clone();
    let _revision = state.unrelated.get();
    let input = TextInput::new(state.raw.clone())
      .id("scroll")
      .key("scroll")
      .controller(state.controller.clone())
      .caret_color(CARET)
      .width(500.0)
      .padding(6.0);
    match state.mask {
      Some(mask) => input.mask_char(mask),
      None => input,
    }
  }
}

struct Editor {
  tree: Tree,
  app: App,
  state: Arc<State>,
}

impl Editor {
  fn new(raw: &str, mask: Option<char>) -> Self {
    let state = Arc::new(State {
      raw: Signal::new(raw.to_owned()),
      controller: TextInputController::new(),
      unrelated: Signal::new(0),
      mask,
    });
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.mount_root::<ScrollEditor>(&mut app, Props(state.clone()));
    let mut editor = Self { tree, app, state };
    editor.render();
    editor.tree.get_element_by_id_mut("scroll").unwrap().focus();
    editor
  }
  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }
  fn caret(&mut self) -> (f32, f32) {
    let snapshot = self.render();
    let caret = snapshot.rects.iter().find(|rect| rect.color == CARET).unwrap();
    assert!(caret.clip.active);
    assert!(caret.x >= caret.clip.x && caret.x + caret.width <= caret.clip.x + caret.clip.width + 0.01);
    assert!(caret.y >= caret.clip.y && caret.y + caret.height <= caret.clip.y + caret.clip.height + 0.01);
    (caret.x, caret.y + caret.height * 0.5)
  }
  fn undo(&mut self) {
    self.tree.key_down_with_meta(
      "z".into(),
      "KeyZ".into(),
      false,
      cfg!(not(target_vendor = "apple")),
      false,
      cfg!(target_vendor = "apple"),
    );
  }
}

#[test]
fn input_scroll_presentation_break_bytes_pointer_insert_undo_and_keyed_retention() {
  for raw in [
    "é\r\n{{task.title}}\n🙂\r\n",
    "é\n{{task.title}}\r字\n",
    "é\r{{task.title}}\n字\r",
  ] {
    let mut editor = Editor::new(raw, None);
    editor.state.controller.select(0, 0).unwrap();
    let start_y = editor.caret().1;
    for byte in [2, 3, raw.len()] {
      editor.state.controller.select(byte, byte).unwrap();
      let (x, y) = editor.caret();
      assert!((y - start_y).abs() < 0.01);
      assert_eq!(editor.state.controller.selection().unwrap().caret, byte);
      editor.state.unrelated.update(|value| *value += 1);
      assert_eq!(editor.caret(), (x, y));
    }
    editor.state.controller.select(2, 4).unwrap();
    editor.state.unrelated.update(|value| *value += 1);
    editor.render();
    assert_eq!(editor.state.controller.selection().unwrap().range(), 2..4);
    assert!(editor.state.controller.replace_selection("X").unwrap());
    let mut expected = raw.to_owned();
    expected.replace_range(2..4, "X");
    assert_eq!(editor.state.raw.get(), expected);
    editor.undo();
    assert_eq!(editor.state.raw.get(), raw);
  }
  let raw = "é\r\n{{task.title}}\n🙂\r\n";
  let mut editor = Editor::new(raw, None);
  editor.state.controller.select(3, 3).unwrap();
  let (x, y) = editor.caret();
  editor.state.controller.select(0, 0).unwrap();
  editor.render();
  pointer_click(&mut editor.tree, x, y, MouseButton::Left);
  assert_eq!(editor.state.controller.selection().unwrap().caret, 3);
  assert!(editor.state.controller.replace_selection("!").unwrap());
  assert_eq!(editor.state.raw.get(), "é\r!\n{{task.title}}\n🙂\r\n");
  editor.undo();
  assert_eq!(editor.state.raw.get(), raw);
}

#[test]
fn input_scroll_presentation_preedit_break_cursor_underline_cancel_and_commit() {
  let mut editor = Editor::new("é end", None);
  editor.state.controller.select(2, 2).unwrap();
  let before_y = editor.caret().1;
  let preedit = "字\r\n🙂";
  editor.tree.ime(ImeEvent::Preedit {
    text: preedit.into(),
    cursor: Some((preedit.len(), preedit.len())),
  });
  let snapshot = editor.render();
  assert!(editor.tree.is_composing());
  assert_eq!(editor.state.raw.get(), "é end");
  assert_eq!(
    editor.state.controller.replace_selection("X"),
    Err(InputEditError::Composing)
  );
  assert!((editor.caret().1 - before_y).abs() < 0.01);
  let area = editor.tree.ime_cursor_area().unwrap();
  let bounds = editor.tree.get_element_by_id_mut("scroll").unwrap().bounds().unwrap();
  assert!(area.y >= bounds.y && area.y + area.height <= bounds.y + bounds.height);
  let underlines: Vec<_> = snapshot
    .rects
    .iter()
    .filter(|rect| rect.height == 1.0 && rect.width > 1.0)
    .collect();
  assert_eq!(underlines.len(), 1);
  assert!(underlines[0].y >= bounds.y && underlines[0].y <= bounds.y + bounds.height);
  editor.tree.ime(ImeEvent::Preedit {
    text: String::new(),
    cursor: None,
  });
  editor.tree.ime(ImeEvent::Disabled);
  editor.render();
  assert_eq!(editor.state.raw.get(), "é end");
  assert!(!editor.tree.is_composing());
  editor.tree.ime(ImeEvent::Preedit {
    text: preedit.into(),
    cursor: None,
  });
  editor.tree.ime(ImeEvent::Commit(preedit.into()));
  editor.render();
  assert_eq!(editor.state.raw.get(), "é字\r\n🙂 end");
  editor.caret();
  editor.undo();
  assert_eq!(editor.state.raw.get(), "é end");
}

#[test]
fn input_scroll_presentation_custom_break_mask_keeps_mask_glyph_order() {
  for mask in ['\r', '\n'] {
    let mut editor = Editor::new("aéX", Some(mask));
    editor.render();
    let quads = editor.tree.resolve_quads(editor.tree.last_layout().unwrap());
    assert!(quads.iter().any(|quad| matches!(&quad.content,
      QuadContent::Text { text, .. } if text == &mask.to_string().repeat(3))));
    assert_eq!(
      editor.state.controller.replace_selection("Q"),
      Err(InputEditError::Masked)
    );
    let bounds = editor.tree.get_element_by_id_mut("scroll").unwrap().bounds().unwrap();
    pointer_click(&mut editor.tree, bounds.x + 80.0, bounds.y + 10.0, MouseButton::Left);
    editor.tree.key_down("Q".into(), "KeyQ".into(), false, false, false);
    assert_eq!(
      editor.state.raw.get(),
      "QaéX",
      "break-mask first-row hit stays at the first raw character"
    );
  }
}
