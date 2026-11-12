use std::sync::Arc;

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, InputToken, InputTokenStyle, InputTokens, TextInput, TextInputController},
  core::Signal,
  layout::text_style::TextStyle,
  node::{Element, color::Color},
};

use crate::support::{RenderSnapshot, render_pass_with_app};

const RAW: &str = "é\r\n{{task.title}}\n🙂\r\n\r\n";
const TOKEN: &str = "{{task.title}}";
const CARET: Color = Color::new(250, 12, 34, 255);
const IDS: [&str; 2] = ["mode-primary", "mode-ordinary"];

struct State {
  sources: [Signal<String>; 2],
  controllers: [TextInputController; 2],
  multiline: Signal<bool>,
  limits: Signal<Option<(usize, usize)>>,
  unrelated: Signal<u64>,
  width: Signal<f32>,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Props(#[devtools_ignore] Arc<State>);

impl PartialEq for Props {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

struct ModeEditor;

impl Component for ModeEditor {
  type Props = Props;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let state = ctx.props::<Self::Props>().0.clone();
    let _revision = state.unrelated.get();
    Column::new()
      .spacing(12.0)
      .child(field(&state, 0))
      .child(field(&state, 1))
  }
}

fn field(state: &State, index: usize) -> TextInput {
  let raw = state.sources[index].get();
  let mut input = TextInput::styled(
    state.sources[index].clone(),
    TextStyle {
      font_size: 13.0,
      line_height: 1.4,
      ..TextStyle::default()
    },
  )
  .id(IDS[index])
  .key(IDS[index])
  .controller(state.controllers[index].clone())
  .width(state.width.get())
  .padding(6.0)
  .caret_color(CARET);
  if state.multiline.get() {
    input = input.multiline();
    if let Some((min, max)) = state.limits.get() {
      input = input.rows(min, max);
    }
  }
  if index == 0
    && let Some(start) = raw.find(TOKEN)
  {
    let token = InputToken::new(
      start..start + TOKEN.len(),
      InputTokenStyle::new(Color::new(3, 160, 70, 255), Color::new(12, 44, 88, 255)).padding(3.0),
    );
    input = input.tokens(InputTokens::new(&raw, vec![token]).unwrap());
  }
  input
}

struct Editor {
  tree: Tree,
  app: App,
  state: Arc<State>,
}

impl Editor {
  fn new(raw: &str) -> Self {
    let state = Arc::new(State {
      sources: std::array::from_fn(|_| Signal::new(raw.to_owned())),
      controllers: std::array::from_fn(|_| TextInputController::new()),
      multiline: Signal::new(false),
      limits: Signal::new(None),
      unrelated: Signal::new(0),
      width: Signal::new(90.0),
    });
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(800, 1600);
    tree.mount_root::<ModeEditor>(&mut app, Props(state.clone()));
    let mut editor = Self { tree, app, state };
    editor.render();
    editor
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  fn height(&mut self, index: usize) -> f32 {
    self
      .tree
      .get_element_by_id_mut(IDS[index])
      .unwrap()
      .bounds()
      .unwrap()
      .height
  }

  fn focus(&mut self, index: usize) {
    self.tree.get_element_by_id_mut(IDS[index]).unwrap().focus();
  }

  fn key(&mut self, key: &str, command: bool) {
    self.tree.key_down_with_meta(
      key.into(),
      key.into(),
      false,
      command && cfg!(not(target_vendor = "apple")),
      false,
      command && cfg!(target_vendor = "apple"),
    );
  }

  fn visible_caret(&mut self) -> f32 {
    let snapshot = self.render();
    let caret = snapshot
      .rects
      .iter()
      .find(|rect| rect.color == CARET)
      .unwrap_or_else(|| {
        panic!(
          "focused caret absent: multiline={} focused=[{},{}] raw=[{:?},{:?}]",
          self.state.multiline.get(),
          self.state.controllers[0].element_ref().focused(),
          self.state.controllers[1].element_ref().focused(),
          self.state.sources[0].get(),
          self.state.sources[1].get()
        )
      });
    assert!(caret.clip.active);
    assert!(caret.x >= caret.clip.x - 0.01);
    assert!(caret.x + caret.width <= caret.clip.x + caret.clip.width + 0.01);
    assert!(caret.y >= caret.clip.y - 0.01);
    assert!(
      caret.y + caret.height <= caret.clip.y + caret.clip.height + 0.01,
      "caret {}..{} must fit clip {}..{}",
      caret.y,
      caret.y + caret.height,
      caret.clip.y,
      caret.clip.y + caret.clip.height
    );
    caret.y - caret.clip.y
  }

  fn update(&mut self, multiline: bool, limits: Option<(usize, usize)>) {
    self.state.multiline.set(multiline);
    self.state.limits.set(limits);
    self.render();
  }

  fn glyph_row_span(&mut self, index: usize) -> f32 {
    let snapshot = self.render();
    let bounds = self.tree.get_element_by_id_mut(IDS[index]).unwrap().bounds().unwrap();
    let rows: Vec<_> = snapshot
      .glyphs
      .iter()
      .filter(|glyph| {
        glyph.y >= bounds.y
          && glyph.y < bounds.y + bounds.height
          && glyph.x >= bounds.x
          && glyph.x < bounds.x + bounds.width
      })
      .map(|glyph| glyph.y)
      .collect();
    let top = rows.iter().copied().fold(f32::INFINITY, f32::min);
    let bottom = rows.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(top.is_finite() && bottom.is_finite());
    bottom - top
  }
}

#[test]
fn input_mode_crlf_same_key_original_p2_has_exactly_five_rows_and_raw_ending_edits() {
  let mut editor = Editor::new(RAW);
  editor.state.width.set(400.0);
  editor.render();
  let pitch = editor.height(0) - 12.0;
  editor.update(true, Some((1, 12)));
  for index in 0..2 {
    assert!((editor.height(index) - (5.0 * pitch + 12.0)).abs() < 0.01);
    editor.focus(index);
    editor.state.controllers[index].select(0, 0).unwrap();
    let first_y = editor.visible_caret();
    editor.state.controllers[index].select(RAW.len(), RAW.len()).unwrap();
    assert!((editor.visible_caret() - first_y - 4.0 * pitch).abs() < 0.01);
    assert_eq!(editor.state.sources[index].get(), RAW);
  }
  editor.focus(0);
  for byte in [2, 3, 4] {
    editor.state.controllers[0].select(byte, byte).unwrap();
    editor.visible_caret();
    assert!(editor.state.controllers[0].replace_selection("!").unwrap());
    let mut expected = RAW.to_owned();
    expected.insert(byte, '!');
    assert_eq!(editor.state.sources[0].get(), expected);
    editor.focus(0);
    editor.key("z", true);
    assert_eq!(editor.state.sources[0].get(), RAW);
    assert_eq!(editor.state.controllers[0].selection().unwrap().caret, byte);
  }
  editor.state.controllers[0].select(2, 4).unwrap();
  assert!(editor.state.controllers[0].replace_selection("X").unwrap());
  editor.key("z", true);
  assert_eq!(editor.state.sources[0].get(), RAW);
  assert_eq!(editor.state.controllers[0].selection().unwrap().range(), 2..4);
  editor.key("y", true);
  assert_eq!(editor.state.sources[0].get(), RAW.replacen("\r\n", "X", 1));
  editor.key("z", true);
  assert_eq!(editor.state.sources[0].get(), RAW);
}

#[test]
fn input_mode_crlf_ascii_token_raw_start_pointer_insertion_and_undo() {
  use lurq::app::events::MouseButton;

  use crate::support::pointer_click;
  let raw = "a\r\n{{task.title}}";
  let mut editor = Editor::new(raw);
  editor.state.width.set(400.0);
  editor.update(true, None);
  editor.focus(0);
  editor.state.controllers[0].select(3, 3).unwrap();
  let snapshot = editor.render();
  let caret = snapshot.rects.iter().find(|rect| rect.color == CARET).unwrap();
  let pill = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == Color::new(12, 44, 88, 255))
    .unwrap();
  assert!((caret.x - pill.x - 3.0).abs() < 0.01);
  let point = (caret.x, caret.y + caret.height * 0.5);
  editor.state.controllers[0].select(0, 0).unwrap();
  editor.render();
  pointer_click(&mut editor.tree, point.0, point.1, MouseButton::Left);
  assert_eq!(editor.state.controllers[0].selection().unwrap().caret, 3);
  editor.key("X", false);
  assert_eq!(editor.state.sources[0].get(), "a\r\nX{{task.title}}");
  editor.key("z", true);
  assert_eq!(editor.state.sources[0].get(), raw);
}

#[test]
fn input_mode_rerender_applies_original_p2_hard_rows_without_remount() {
  let mut editor = Editor::new(RAW);
  let single = [editor.height(0), editor.height(1)];
  editor.update(true, Some((1, 12)));
  for (index, initial) in single.into_iter().enumerate() {
    assert!(
      editor.height(index) > initial * 2.0,
      "{} must apply multiline props with the same key",
      IDS[index]
    );
    assert_eq!(editor.state.sources[index].get(), RAW);
    assert!(
      editor.glyph_row_span(index) > initial,
      "actual glyphs occupy distinct hard rows"
    );
    editor.focus(index);
    editor.state.controllers[index].select(RAW.len(), RAW.len()).unwrap();
    let final_y = editor.visible_caret();
    assert!(final_y > initial);
    editor.state.unrelated.update(|value| *value += 1);
    assert_eq!(editor.visible_caret(), final_y);
  }
}

#[test]
fn input_mode_rerender_retains_crlf_history_and_visible_caret_on_default_scroll_return() {
  for index in 0..2 {
    let mut editor = Editor::new(RAW);
    editor.focus(index);
    editor.state.controllers[index].select(2, 4).unwrap();
    assert!(editor.state.controllers[index].replace_selection("X").unwrap());
    editor.key("z", true);
    assert_eq!(editor.state.sources[index].get(), RAW);
    assert_eq!(editor.state.controllers[index].selection().unwrap().range(), 2..4);
    let single = editor.height(index);
    editor.update(true, Some((1, 12)));
    assert_eq!(editor.state.controllers[index].selection().unwrap().range(), 2..4);
    editor.state.controllers[index].select(RAW.len(), RAW.len()).unwrap();
    editor.visible_caret();
    editor.update(false, None);
    assert!((editor.height(index) - single).abs() < 0.01);
    editor.visible_caret();
    assert_eq!(editor.state.sources[index].get(), RAW);
    assert_eq!(editor.state.controllers[index].selection().unwrap().caret, RAW.len());
    editor.state.unrelated.update(|value| *value += 1);
    editor.visible_caret();
    editor.key("y", true);
    assert_eq!(editor.state.sources[index].get(), RAW.replacen("\r\n", "X", 1));
    editor.key("z", true);
    assert_eq!(editor.state.sources[index].get(), RAW);
    assert_eq!(editor.state.controllers[index].selection().unwrap().caret, RAW.len());
    editor.state.controllers[index].select(2, 4).unwrap();
    assert!(editor.state.controllers[index].replace_selection("Y").unwrap());
    assert_eq!(editor.state.sources[index].get(), RAW.replacen("\r\n", "Y", 1));
    editor.key("z", true);
    assert_eq!(editor.state.sources[index].get(), RAW);
  }
}

#[test]
fn input_mode_rerender_applies_normalized_row_limits_and_omitted_defaults() {
  let raw = "{{task.title}}\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12";
  let mut editor = Editor::new(raw);
  let single = [editor.height(0), editor.height(1)];
  editor.update(true, Some((1, 2)));
  let capped = [editor.height(0), editor.height(1)];
  editor.update(true, Some((5, 3)));
  let normalized = [editor.height(0), editor.height(1)];
  editor.update(true, None);
  for index in 0..2 {
    let intrinsic = editor.height(index);
    assert!(normalized[index] > capped[index] * 1.8);
    assert!(intrinsic > normalized[index] * 1.8);
    editor.focus(index);
    editor.state.controllers[index].select(raw.len(), raw.len()).unwrap();
    editor.visible_caret();
    editor.state.unrelated.update(|value| *value += 1);
    editor.visible_caret();
    assert!((editor.height(index) - intrinsic).abs() < 0.01);
  }
  editor.update(true, Some((1, 2)));
  for (index, height) in capped.into_iter().enumerate() {
    assert!((editor.height(index) - height).abs() < 0.01);
    editor.focus(index);
    editor.visible_caret();
  }
  editor.update(false, None);
  for (index, height) in single.into_iter().enumerate() {
    assert!((editor.height(index) - height).abs() < 0.01);
    assert_eq!(editor.state.sources[index].get(), raw);
    editor.focus(index);
    editor.visible_caret();
  }
}
