use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  core::Signal,
  node::{Element, color::Color},
};

use crate::support::{render_pass, run_pass};

fn pointer_click(runtime: &mut Tree, x: f32, y: f32) {
  runtime.mouse_down(x, y, MouseButton::Left);
  runtime.mouse_up(x, y, MouseButton::Left);
}

fn shortcut_key_down(runtime: &mut Tree, key: &str, code: &str, shift: bool) {
  let (ctrl, meta) = if cfg!(target_vendor = "apple") {
    (false, true)
  } else {
    (true, false)
  };
  runtime.key_down_with_meta(key.to_owned(), code.to_owned(), shift, ctrl, false, meta);
}

fn word_nav_key_down(runtime: &mut Tree, key: &str, code: &str, shift: bool) {
  let (ctrl, alt) = if cfg!(target_vendor = "apple") {
    (false, true)
  } else {
    (true, false)
  };
  runtime.key_down(key.to_owned(), code.to_owned(), shift, ctrl, alt);
}

#[test]
fn typing_into_focused_text_input_appends_to_existing_value() {
  let value = Signal::new("A".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("B".to_owned(), "KeyB".to_owned(), false, false, false);

  assert_eq!(value.get(), "AB");
}

#[test]
fn clicking_text_input_padding_places_caret_before_text() {
  let value = Signal::new("AB".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(
    lurq::components::TextInput::new(value.clone())
      .width(120.0)
      .height(40.0)
      .padding_horizontal(10.0),
  );
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();

  pointer_click(&mut runtime, rect.x + 5.0, rect.y + rect.height / 2.0);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "XAB");
}

#[test]
fn backspace_removes_character_before_caret() {
  let value = Signal::new("AB".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("Backspace".to_owned(), "Backspace".to_owned(), false, false, false);

  assert_eq!(value.get(), "A");
}

#[test]
fn arrow_left_moves_caret_before_inserted_text() {
  let value = Signal::new("AB".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("ArrowLeft".to_owned(), "ArrowLeft".to_owned(), false, false, false);
  runtime.key_down("C".to_owned(), "KeyC".to_owned(), false, false, false);

  assert_eq!(value.get(), "ACB");
}

#[test]
fn ctrl_arrow_left_moves_caret_to_previous_word() {
  let value = Signal::new("Hello world".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("End".to_owned(), "End".to_owned(), false, false, false);
  word_nav_key_down(&mut runtime, "ArrowLeft", "ArrowLeft", false);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "Hello Xworld");
}

#[test]
fn arrow_up_moves_caret_to_previous_multiline_row() {
  let value = Signal::new("abc\ndef".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("End".to_owned(), "End".to_owned(), false, false, false);
  runtime.key_down("ArrowUp".to_owned(), "ArrowUp".to_owned(), false, false, false);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "abcX\ndef");
}

#[test]
fn arrow_down_moves_caret_to_next_multiline_row() {
  let value = Signal::new("abc\ndef".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("Home".to_owned(), "Home".to_owned(), false, false, false);
  runtime.key_down("ArrowDown".to_owned(), "ArrowDown".to_owned(), false, false, false);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "abc\nXdef");
}

#[test]
fn clicking_multiline_row_places_caret_on_that_row() {
  let value = Signal::new("one\ntwo\nthree".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let line_height = 19.2;

  pointer_click(&mut runtime, rect.x + 1.0, rect.y + line_height * 2.0 + 1.0);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "one\ntwo\nXthree");
}

#[test]
fn dragging_multiline_row_selects_text_on_that_row() {
  let value = Signal::new("one\ntwo\nthree".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let line_height = 19.2;
  let y = rect.y + line_height * 2.0 + 1.0;

  runtime.mouse_down(rect.x + 1.0, y, MouseButton::Left);
  runtime.mouse_move(rect.x + rect.width, y);
  runtime.mouse_up(rect.x + rect.width, y, MouseButton::Left);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "one\ntwo\nX");
}

#[test]
fn backspace_removes_previous_unicode_character() {
  let value = Signal::new("Aé".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("Backspace".to_owned(), "Backspace".to_owned(), false, false, false);

  assert_eq!(value.get(), "A");
}

#[test]
fn shift_arrow_selection_is_replaced_by_inserted_text() {
  let value = Signal::new("AB".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("ArrowLeft".to_owned(), "ArrowLeft".to_owned(), true, false, false);
  runtime.key_down("C".to_owned(), "KeyC".to_owned(), false, false, false);

  assert_eq!(value.get(), "AC");
}

#[test]
fn ctrl_a_selects_all_text_for_replacement() {
  let value = Signal::new("Hello".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  shortcut_key_down(&mut runtime, "a", "KeyA", false);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "X");
}

#[test]
fn ctrl_z_undoes_last_text_input_edit() {
  let value = Signal::new("A".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("B".to_owned(), "KeyB".to_owned(), false, false, false);
  shortcut_key_down(&mut runtime, "z", "KeyZ", false);

  assert_eq!(value.get(), "A");
}

#[test]
fn ctrl_y_and_ctrl_shift_z_redo_text_input_edits() {
  let value = Signal::new("A".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("B".to_owned(), "KeyB".to_owned(), false, false, false);
  shortcut_key_down(&mut runtime, "z", "KeyZ", false);
  shortcut_key_down(&mut runtime, "y", "KeyY", false);
  assert_eq!(value.get(), "AB");

  shortcut_key_down(&mut runtime, "z", "KeyZ", false);
  shortcut_key_down(&mut runtime, "Z", "KeyZ", true);
  assert_eq!(value.get(), "AB");
}

#[test]
fn new_text_input_edit_clears_redo_history() {
  let value = Signal::new("A".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("B".to_owned(), "KeyB".to_owned(), false, false, false);
  shortcut_key_down(&mut runtime, "z", "KeyZ", false);
  runtime.key_down("C".to_owned(), "KeyC".to_owned(), false, false, false);
  shortcut_key_down(&mut runtime, "y", "KeyY", false);

  assert_eq!(value.get(), "AC");
}

#[test]
fn enter_is_ignored_for_scroll_overflow_text_input() {
  let value = Signal::new("A".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  runtime.key_down("Enter".to_owned(), "Enter".to_owned(), false, false, false);

  assert_eq!(value.get(), "A");
}

#[test]
fn mouse_drag_selection_renders_and_replaces_selected_text() {
  let value = Signal::new("Hello".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let y = rect.y + rect.height / 2.0;

  runtime.mouse_down(rect.x, y, MouseButton::Left);
  runtime.mouse_move(rect.x + rect.width, y);
  runtime.mouse_up(rect.x + rect.width, y, MouseButton::Left);

  let snapshot = render_pass(&mut runtime);
  assert!(
    snapshot
      .rects
      .iter()
      .any(|rect| { rect.color == Color::from_hex("#bfdbfe") && rect.width > 1.0 && rect.height > 0.0 })
  );

  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "X");
}

struct RebuildingTextInput {
  value: Signal<String>,
}

impl Component for RebuildingTextInput {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      value: ctx.signal("Hello".to_owned()),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    lurq::components::TextInput::new(self.value.clone())
  }
}

#[test]
fn mouse_drag_selection_survives_component_rebuild() {
  let mut runtime = Tree::new();

  runtime.mount_root::<RebuildingTextInput>(&mut App::new(), ());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let y = rect.y + rect.height / 2.0;

  runtime.mouse_down(rect.x, y, MouseButton::Left);
  runtime.rebuild();
  runtime.mouse_move(rect.x + rect.width, y);
  runtime.mouse_up(rect.x + rect.width, y, MouseButton::Left);

  let snapshot = render_pass(&mut runtime);
  assert!(
    snapshot
      .rects
      .iter()
      .any(|rect| { rect.color == Color::from_hex("#bfdbfe") && rect.width > 1.0 && rect.height > 0.0 })
  );

  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert!(
    runtime
      .find_element(|element| element.text_content() == Some("X"))
      .is_some()
  );
}

#[test]
fn multiline_selection_renders_a_rect_for_each_selected_row() {
  let value = Signal::new("one\ntwo\nthree".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let (x, y) = rect.center();

  pointer_click(&mut runtime, x, y);
  shortcut_key_down(&mut runtime, "a", "KeyA", false);

  let snapshot = render_pass(&mut runtime);
  let selection_rects = snapshot
    .rects
    .iter()
    .filter(|rect| rect.color == Color::from_hex("#bfdbfe") && rect.width > 1.0 && rect.height > 0.0)
    .count();

  assert_eq!(
    selection_rects, 3,
    "multiline selection should render one highlight rect per selected row"
  );
}

#[test]
fn double_click_text_input_selects_clicked_word() {
  let value = Signal::new("one two".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let y = rect.y + rect.height / 2.0;
  let x = rect.x + 46.0;

  pointer_click(&mut runtime, x, y);
  pointer_click(&mut runtime, x, y);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "one X");
}

#[test]
fn triple_click_single_line_text_input_selects_whole_line() {
  let value = Signal::new("one two".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let y = rect.y + rect.height / 2.0;
  let x = rect.x + 46.0;

  pointer_click(&mut runtime, x, y);
  pointer_click(&mut runtime, x, y);
  pointer_click(&mut runtime, x, y);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "X");
}

#[test]
fn triple_click_multiline_text_input_selects_clicked_line() {
  let value = Signal::new("one\ntwo words\nthree".to_owned());
  let mut runtime = Tree::new();

  runtime.set_root(lurq::components::TextInput::new(value.clone()).multiline());
  run_pass(&mut runtime);
  let rect = runtime.find_element(|_| true).unwrap().bounds();
  let line_height = 19.2;
  let y = rect.y + line_height + 1.0;
  let x = rect.x + 46.0;

  pointer_click(&mut runtime, x, y);
  pointer_click(&mut runtime, x, y);
  pointer_click(&mut runtime, x, y);
  runtime.key_down("X".to_owned(), "KeyX".to_owned(), false, false, false);

  assert_eq!(value.get(), "one\nX\nthree");
}
