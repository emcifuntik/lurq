use lurq::{
  app::{App, Tree, events::MouseButton},
  components::{InputToken, InputTokenStyle, InputTokens, TextInput, TextInputController, TextInputOverflowAnchor},
  core::Signal,
  layout::{quad::QuadContent, text_style::TextAlign},
  node::color::Color,
};

use crate::support::{RenderSnapshot, pointer_click, render_pass_with_app};

const TOKEN: &str = "{{task.title}}";
const PILL: Color = Color::new(12, 44, 88, 255);
const CARET: Color = Color::new(250, 12, 34, 255);

struct Editor {
  tree: Tree,
  app: App,
  source: Signal<String>,
  controller: TextInputController,
}

impl Editor {
  fn new(raw: &str, width: f32, align: TextAlign, anchor: TextInputOverflowAnchor) -> Self {
    let source = Signal::new(raw.to_owned());
    let controller = TextInputController::new();
    let mut tree = Tree::new();
    let start = raw.find(TOKEN).unwrap();
    let tokens = InputTokens::new(
      raw,
      vec![InputToken::new(
        start..start + TOKEN.len(),
        InputTokenStyle::new(Color::new(3, 160, 70, 255), PILL).padding(3.0),
      )],
    )
    .unwrap();
    tree.set_root(
      TextInput::new(source.clone())
        .id("editor")
        .controller(controller.clone())
        .tokens(tokens)
        .caret_color(CARET)
        .width(width)
        .padding(6.0)
        .text_align(align)
        .unfocused_overflow_anchor(anchor),
    );
    let mut editor = Self {
      tree,
      app: App::new(),
      source,
      controller,
    };
    editor.render();
    editor
  }
  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }
  fn focus(&mut self) {
    self.tree.get_element_by_id_mut("editor").unwrap().focus();
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
  fn assert_caret_visible(&mut self) -> (f32, f32) {
    let snapshot = self.render();
    let caret = snapshot.rects.iter().find(|rect| rect.color == CARET).unwrap();
    assert!(caret.clip.active);
    assert!(
      caret.x >= caret.clip.x - 0.01 && caret.x + caret.width <= caret.clip.x + caret.clip.width + 0.01,
      "caret {}..{} must fit clip {}..{}",
      caret.x,
      caret.x + caret.width,
      caret.clip.x,
      caret.clip.x + caret.clip.width
    );
    assert!(caret.y >= caret.clip.y - 0.01 && caret.y + caret.height <= caret.clip.y + caret.clip.height + 0.01);
    (caret.x, caret.y + caret.height * 0.5)
  }
  fn glyph_quad_x(&self) -> f32 {
    self
      .tree
      .resolve_quads(self.tree.last_layout().unwrap())
      .into_iter()
      .find(|quad| matches!(quad.content, QuadContent::DecoratedInput { .. }))
      .unwrap()
      .x
  }
}

#[test]
fn input_tokens_scroll_lf_crlf_before_after_token_are_one_visible_raw_mapped_row() {
  for raw in [
    "before\n{{task.title}} after",
    "{{task.title}}\r\nafter",
    "before\r\n{{task.title}}\r\nafter",
  ] {
    let mut editor = Editor::new(raw, 500.0, TextAlign::Left, TextInputOverflowAnchor::Start);
    editor.focus();
    editor.controller.select(raw.len(), raw.len()).unwrap();
    let (_, end_y) = editor.assert_caret_visible();
    let snapshot = editor.render();
    let pill = snapshot.rects.iter().find(|rect| rect.color == PILL).unwrap();
    assert_eq!(snapshot.rects.iter().filter(|rect| rect.color == PILL).count(), 1);
    assert!(end_y >= pill.y && end_y <= pill.y + pill.height);
    assert!(snapshot.glyphs.iter().all(|glyph| glyph.y < pill.y + pill.height));
    pointer_click(
      &mut editor.tree,
      pill.x + pill.width * 0.5,
      pill.y + pill.height * 0.5,
      MouseButton::Left,
    );
    let byte = editor.controller.selection().unwrap().caret;
    let start = raw.find(TOKEN).unwrap();
    assert!(byte > start && byte < start + TOKEN.len());
    editor.controller.replace_selection("!").unwrap();
    let mut expected = raw.to_owned();
    expected.insert(byte, '!');
    assert_eq!(editor.source.get(), expected);
    editor.key("z", true);
    assert_eq!(editor.source.get(), raw);
  }
}

#[test]
fn input_tokens_scroll_break_selection_controller_insert_and_end_navigation_stay_raw() {
  let raw = "before\r\n{{task.title}}\nafter";
  let mut editor = Editor::new(raw, 120.0, TextAlign::Right, TextInputOverflowAnchor::Start);
  editor.focus();
  editor.controller.select(6, 8).unwrap();
  assert_eq!(editor.controller.selection().unwrap().range(), 6..8);
  editor.controller.replace_selection("X").unwrap();
  assert_eq!(editor.source.get(), "beforeX{{task.title}}\nafter");
  editor.key("z", true);
  assert_eq!(editor.source.get(), raw);
  editor.key("Home", false);
  assert_eq!(editor.controller.selection().unwrap().caret, 0);
  editor.assert_caret_visible();
  editor.key("End", false);
  assert_eq!(editor.controller.selection().unwrap().caret, raw.len());
  editor.assert_caret_visible();
}

#[test]
fn input_tokens_scroll_overwide_rtl_all_alignments_reveal_and_edit_both_raw_ends() {
  let raw = "שלום שלום {{task.title}} עולם עולם";
  for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right, TextAlign::End] {
    let mut editor = Editor::new(raw, 90.0, align, TextInputOverflowAnchor::Start);
    editor.focus();
    for byte in [0, raw.len()] {
      editor.controller.select(byte, byte).unwrap();
      let (x, y) = editor.assert_caret_visible();
      pointer_click(&mut editor.tree, x + 0.05, y, MouseButton::Left);
      assert_eq!(editor.controller.selection().unwrap().caret, byte);
      editor.controller.replace_selection("!").unwrap();
      let mut expected = raw.to_owned();
      expected.insert(byte, '!');
      assert_eq!(editor.source.get(), expected);
      editor.key("z", true);
      assert_eq!(editor.source.get(), raw);
    }
    editor.key("Home", false);
    assert_eq!(editor.controller.selection().unwrap().caret, 0);
    editor.assert_caret_visible();
    editor.key("End", false);
    assert_eq!(editor.controller.selection().unwrap().caret, raw.len());
    editor.assert_caret_visible();
  }
}

#[test]
fn input_tokens_scroll_overwide_rtl_unfocused_visual_edge_anchors_are_coherent() {
  let raw = "שלום שלום {{task.title}} עולם עולם";
  for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right, TextAlign::End] {
    let start = Editor::new(raw, 90.0, align, TextInputOverflowAnchor::Start);
    let end = Editor::new(raw, 90.0, align, TextInputOverflowAnchor::End);
    assert!((start.glyph_quad_x() - 6.0).abs() < 0.01);
    assert!(end.glyph_quad_x() < start.glyph_quad_x() - 1.0);
    assert_eq!(start.source.get(), raw);
    assert_eq!(end.source.get(), raw);
  }
}

#[test]
fn input_tokens_scroll_default_width_matches_ordinary_input_even_when_rtl_overflows() {
  let raw = "שלום שלום שלום {{task.title}} עולם עולם עולם";
  let source = Signal::new(raw.to_owned());
  let controller = TextInputController::new();
  let start = raw.find(TOKEN).unwrap();
  let tokens = InputTokens::new(
    raw,
    vec![InputToken::new(
      start..start + TOKEN.len(),
      InputTokenStyle::new(CARET, PILL).padding(3.0),
    )],
  )
  .unwrap();
  let mut plain = Tree::new();
  plain.set_root(TextInput::new(source.clone()));
  let mut app = App::new();
  render_pass_with_app(&mut plain, &mut app);
  let expected = plain.find_element(|_| true).unwrap().bounds().width;
  let mut tree = Tree::new();
  tree.set_root(
    TextInput::new(source)
      .id("editor")
      .tokens(tokens)
      .controller(controller.clone())
      .caret_color(CARET),
  );
  render_pass_with_app(&mut tree, &mut app);
  assert_eq!(tree.find_element(|_| true).unwrap().bounds().width, expected);
  tree.get_element_by_id_mut("editor").unwrap().focus();
  controller.select(raw.len(), raw.len()).unwrap();
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let caret = snapshot.rects.iter().find(|rect| rect.color == CARET).unwrap();
  assert!(caret.x >= caret.clip.x && caret.x + caret.width <= caret.clip.x + caret.clip.width + 0.01);
}
