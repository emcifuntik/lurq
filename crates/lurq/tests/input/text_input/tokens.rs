use lurq::{
  app::{
    App, Tree,
    events::{ImeEvent, MouseButton},
  },
  components::{InputToken, InputTokenError, InputTokenStyle, InputTokens, TextInput, TextInputController},
  core::Signal,
  layout::text_style::TextStyle,
  node::color::Color,
};

use crate::support::{RenderSnapshot, pointer_click, render_pass_with_app};

const TOKEN: &str = "{{task.title}}";
const FILL: Color = Color::new(22, 44, 99, 255);
const INK: Color = Color::new(11, 222, 111, 255);
const CARET: Color = Color::new(255, 11, 33, 255);

fn style() -> InputTokenStyle {
  InputTokenStyle::new(INK, FILL).padding(3.0).radius(3.0)
}

fn tokens(source: &str) -> InputTokens {
  let start = source.find(TOKEN).unwrap();
  InputTokens::new(source, vec![InputToken::new(start..start + TOKEN.len(), style())]).unwrap()
}

struct Editor {
  tree: Tree,
  app: App,
  source: Signal<String>,
  controller: TextInputController,
}

impl Editor {
  fn new(source: &str, width: f32) -> Self {
    let source = Signal::new(source.to_owned());
    let controller = TextInputController::new();
    let mut tree = Tree::new();
    tree.set_root(Self::field(source.clone(), controller.clone(), width));
    let mut editor = Self {
      tree,
      app: App::new(),
      source,
      controller,
    };
    editor.render();
    editor.tree.get_element_by_id_mut("editor").unwrap().focus();
    editor
  }

  fn field(source: Signal<String>, controller: TextInputController, width: f32) -> TextInput {
    TextInput::styled(
      source.clone(),
      TextStyle {
        font_size: 13.0,
        line_height: 1.4,
        ..TextStyle::default()
      },
    )
    .id("editor")
    .controller(controller)
    .tokens(tokens(&source.get()))
    .caret_color(CARET)
    .multiline()
    .width(width)
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  fn key(&mut self, key: &str, code: &str, shift: bool, command: bool) {
    self.tree.key_down_with_meta(
      key.into(),
      code.into(),
      shift,
      command && cfg!(not(target_vendor = "apple")),
      false,
      command && cfg!(target_vendor = "apple"),
    );
  }

  fn click(&mut self, x: f32, y: f32) {
    pointer_click(&mut self.tree, x, y, MouseButton::Left);
  }
}

#[test]
fn input_tokens_validate_exact_utf8_nonoverlapping_inline_ranges() {
  let source = "aé {{task.title}}";
  assert_eq!(
    InputTokens::new(source, vec![InputToken::new(2..3, style())]).unwrap_err(),
    InputTokenError::InvalidRange
  );
  assert_eq!(
    InputTokens::new(
      source,
      vec![InputToken::new(1..3, style()), InputToken::new(1..4, style())]
    )
    .unwrap_err(),
    InputTokenError::Overlap
  );
  assert_eq!(
    InputTokens::new("a\nb", vec![InputToken::new(0..3, style())]).unwrap_err(),
    InputTokenError::MultilineToken
  );
  assert!(InputTokens::new(source, vec![InputToken::new(4..source.len(), style())]).is_ok());
  assert!(!format!("{:?}", tokens(source)).contains(source));
}

#[test]
fn input_tokens_paint_measured_padding_and_keep_authoritative_raw_source() {
  let mut editor = Editor::new(TOKEN, 300.0);
  let snapshot = editor.render();
  let pill = snapshot.rects.iter().find(|r| r.color == FILL).unwrap();
  let glyphs: Vec<_> = snapshot
    .glyphs
    .iter()
    .filter(|g| g.color == INK.to_linear_f32_array())
    .collect();
  assert_eq!(glyphs.len(), TOKEN.len());
  assert!(pill.width > 6.0);
  assert!(glyphs.iter().all(|g| g.x >= pill.x + 0.5 && g.x < pill.x + pill.width));
  assert!(glyphs.iter().all(|g| g.order > pill.order));
  assert_eq!(pill.radii, [3.0; 4]);
  assert_eq!(editor.source.get(), TOKEN);
  editor.click(pill.x + pill.width + 0.5, pill.y + 4.0);
  assert_eq!(editor.controller.selection().unwrap().caret, TOKEN.len());
  editor.click(pill.x + 0.5, pill.y + 4.0);
  assert_eq!(editor.controller.selection().unwrap().caret, 0);
}

#[test]
fn input_tokens_click_inside_pill_edits_raw_expression_and_invalidates_old_decoration() {
  let mut editor = Editor::new("left {{task.title}} right", 300.0);
  let snapshot = editor.render();
  let pill = snapshot.rects.iter().find(|r| r.color == FILL).unwrap();
  editor.click(pill.x + pill.width / 2.0, pill.y + 4.0);
  let index = editor.controller.selection().unwrap().caret;
  assert!(index > 5 && index < 5 + TOKEN.len());
  editor.key("X", "KeyX", false, false);
  let mut expected = "left {{task.title}} right".to_owned();
  expected.insert(index, 'X');
  assert_eq!(editor.source.get(), expected);
  assert!(!editor.render().rects.iter().any(|r| r.color == FILL));
  editor.key("z", "KeyZ", false, true);
  assert_eq!(editor.source.get(), "left {{task.title}} right");
  assert!(editor.render().rects.iter().any(|r| r.color == FILL));
  editor.key("z", "KeyZ", true, true);
  assert_eq!(editor.source.get(), expected);
}

#[test]
fn input_tokens_wrap_long_token_and_pointer_rows_share_raw_mapping() {
  let mut editor = Editor::new("before words {{task.title}} after words\nend", 45.0);
  let snapshot = editor.render();
  let pills: Vec<_> = snapshot.rects.iter().filter(|r| r.color == FILL).collect();
  assert!(pills.len() >= 2, "long inline source wraps into measured pill rows");
  let first = pills.first().unwrap();
  let last = pills.last().unwrap();
  editor.click(first.x + 3.0, first.y + 4.0);
  let first_index = editor.controller.selection().unwrap().caret;
  editor.click(last.x + last.width - 3.0, last.y + 4.0);
  let last_index = editor.controller.selection().unwrap().caret;
  assert!(last_index > first_index);
  assert_eq!(last_index, "before words ".len() + TOKEN.len());
  editor.key("!", "Digit1", false, false);
  assert_eq!(editor.source.get(), "before words {{task.title}}! after words\nend");
}

#[test]
fn input_tokens_selection_delete_and_undo_preserve_raw_string() {
  let mut editor = Editor::new("é {{task.title}} 字", 200.0);
  editor.controller.select(3, 3 + TOKEN.len()).unwrap();
  editor.key("Backspace", "Backspace", false, false);
  assert_eq!(editor.source.get(), "é  字");
  editor.key("z", "KeyZ", false, true);
  assert_eq!(editor.source.get(), "é {{task.title}} 字");
  assert_eq!(editor.controller.selection().unwrap().range(), 3..3 + TOKEN.len());
  assert!(editor.render().rects.iter().any(|r| r.color == FILL));
}

#[test]
fn input_tokens_keyed_cached_rebuild_retains_geometry_selection_and_history() {
  let mut editor = Editor::new("left {{task.title}} right", 250.0);
  editor.controller.select(7, 12).unwrap();
  let before = editor.render();
  editor
    .tree
    .set_root(Editor::field(editor.source.clone(), editor.controller.clone(), 250.0));
  let after = editor.render();
  let pill_before = before.rects.iter().find(|r| r.color == FILL).unwrap();
  let pill_after = after.rects.iter().find(|r| r.color == FILL).unwrap();
  assert_eq!(
    (pill_before.x, pill_before.y, pill_before.width),
    (pill_after.x, pill_after.y, pill_after.width)
  );
  assert_eq!(editor.controller.selection().unwrap().range(), 7..12);
  editor.tree.get_element_by_id_mut("editor").unwrap().blur();
  assert!(editor.controller.replace_selection("Q").unwrap());
  assert_eq!(editor.source.get(), "left {{Qtitle}} right");
}

#[test]
fn input_tokens_ime_uses_plain_composition_geometry_and_preserves_preedit() {
  let mut editor = Editor::new(TOKEN, 120.0);
  editor.controller.select(2, 2).unwrap();
  editor.tree.ime(ImeEvent::Preedit {
    text: "かな".into(),
    cursor: Some((0, 6)),
  });
  assert!(!editor.render().rects.iter().any(|r| r.color == FILL));
  assert_eq!(editor.source.get(), TOKEN);
  editor.tree.ime(ImeEvent::Commit("かな".into()));
  assert_eq!(editor.source.get(), "{{かなtask.title}}");
  editor.key("z", "KeyZ", false, true);
  assert_eq!(editor.source.get(), TOKEN);
  assert!(editor.render().rects.iter().any(|r| r.color == FILL));
}

#[test]
fn input_tokens_masked_field_never_paints_source_tokens() {
  let source = Signal::new(TOKEN.to_owned());
  let mut tree = Tree::new();
  tree.set_root(TextInput::new(source).mask().tokens(tokens(TOKEN)).width(150.0));
  let snapshot = render_pass_with_app(&mut tree, &mut App::new());
  assert!(!snapshot.rects.iter().any(|r| r.color == FILL));
  assert!(!snapshot.glyphs.iter().any(|g| g.color == INK.to_linear_f32_array()));
}

#[test]
fn input_tokens_center_and_right_alignment_move_paint_hit_and_caret_together() {
  use lurq::layout::text_style::TextAlign;
  for align in [TextAlign::Center, TextAlign::Right] {
    let mut editor = Editor::new(TOKEN, 200.0);
    editor
      .tree
      .set_root(Editor::field(editor.source.clone(), editor.controller.clone(), 200.0).text_align(align));
    let snapshot = editor.render();
    let pill = snapshot.rects.iter().find(|r| r.color == FILL).unwrap();
    assert!(pill.x > 20.0);
    if align == TextAlign::Right {
      assert!((pill.x + pill.width - 200.0).abs() < 0.1);
    }
    editor.click(pill.x + 1.0, pill.y + 4.0);
    assert_eq!(editor.controller.selection().unwrap().caret, 0);
    editor.click(pill.x + pill.width - 1.0, pill.y + 4.0);
    assert_eq!(editor.controller.selection().unwrap().caret, TOKEN.len());
    let caret = editor.render().rects.into_iter().find(|r| r.color == CARET).unwrap();
    assert!((caret.x - (pill.x + pill.width - 3.0)).abs() < 0.1);
  }
}

#[test]
fn input_tokens_crlf_and_trailing_newline_follow_visual_rows_after_soft_wrap() {
  let mut editor = Editor::new("words words {{task.title}}\r\n", 45.0);
  let snapshot = editor.render();
  let last_pill = snapshot.rects.iter().filter(|r| r.color == FILL).last().unwrap();
  let caret = snapshot.rects.iter().find(|r| r.color == CARET).unwrap();
  assert!(caret.y > last_pill.y);
  assert!(caret.y - last_pill.y < 30.0, "CRLF is one visual break");
  editor.click(caret.x + 1.0, caret.y + 4.0);
  assert_eq!(editor.controller.selection().unwrap().caret, editor.source.get().len());
  editor.key("終", "KeyA", false, false);
  assert_eq!(editor.source.get(), "words words {{task.title}}\r\n終");
}

#[test]
fn input_tokens_combining_utf8_source_is_editable_and_theme_changes_remeasure() {
  use lurq::app::theme::{PaletteColor, RadiusSize, SpacingSize, TypographyStyle};
  let source = Signal::new("前 {{é\u{301}字}} 後".to_owned());
  let controller = TextInputController::new();
  let mut tree = Tree::new();
  let decoration = InputTokenStyle::new(PaletteColor::Accent, PaletteColor::SurfaceRaised)
    .padding(SpacingSize::extra("token_pad"))
    .radius(RadiusSize::extra("token_radius"))
    .typography(TypographyStyle::extra("token_font"));
  let token = InputTokens::new(&source.get(), vec![InputToken::new(4..15, decoration)]).unwrap();
  tree.set_root(
    TextInput::new(source.clone())
      .id("editor")
      .controller(controller.clone())
      .tokens(token)
      .width(250.0),
  );
  let mut app = App::new();
  app.theme().set_palette_color(PaletteColor::Accent, INK);
  app.theme().set_palette_color(PaletteColor::SurfaceRaised, FILL);
  app.theme().set_spacing_value(SpacingSize::extra("token_pad"), 3.0);
  app.theme().set_radius_value(RadiusSize::extra("token_radius"), 3.0);
  app.theme().set_typography_style(
    TypographyStyle::extra("token_font"),
    TextStyle {
      font_size: 13.0,
      ..TextStyle::default()
    },
  );
  let first = render_pass_with_app(&mut tree, &mut app);
  let width = first.rects.iter().find(|r| r.color == FILL).unwrap().width;
  app.theme().set_spacing_value(SpacingSize::extra("token_pad"), 6.0);
  let next = render_pass_with_app(&mut tree, &mut app);
  assert!((next.rects.iter().find(|r| r.color == FILL).unwrap().width - width - 6.0).abs() < 0.1);
  controller.select(6, 13).unwrap();
  assert!(controller.replace_selection("X").unwrap());
  assert_eq!(source.get(), "前 {{X}} 後");
}

#[test]
fn input_tokens_rtl_field_background_border_inset_clip_and_subtree_opacity_survive() {
  use lurq::node::box_shadow::BoxShadow;
  let source = Signal::new("שלום {{task.title}} עולם".to_owned());
  let mut tree = Tree::new();
  let mut app = App::new();
  let background = Color::new(30, 40, 50, 255);
  let border = Color::new(60, 70, 80, 255);
  tree.set_scale_factor(2.0);
  tree.set_root(
    TextInput::new(source.clone())
      .tokens(tokens(&source.get()))
      .width(150.0)
      .padding(6.0)
      .background(background)
      .border_inside(2.0, border)
      .rounded(8.0)
      .box_shadow(BoxShadow::new(1.0, 1.0, 2.0, Color::new(90, 90, 90, 255)).inset())
      .opacity(0.5),
  );
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let field = snapshot.rects.iter().find(|rect| rect.color == background).unwrap();
  let pill = snapshot.rects.iter().find(|rect| rect.color == FILL).unwrap();
  let inset = snapshot
    .rects
    .iter()
    .find(|rect| rect.shadow.is_some_and(|shadow| shadow.inset))
    .unwrap();
  assert!(field.order < inset.order && inset.order < pill.order);
  assert!(snapshot.rects.iter().any(|rect| rect.stroke_color == border));
  assert!(snapshot.layers.iter().any(|layer| layer.opacity == 0.5));
  let glyphs: Vec<_> = snapshot
    .glyphs
    .iter()
    .filter(|glyph| glyph.color[..3] == INK.to_linear_f32_array()[..3])
    .collect();
  assert!(!glyphs.is_empty());
  assert!(glyphs.iter().all(|glyph| glyph.clip.active
    && glyph.clip.width <= field.width
    && glyph.order > pill.order
    && glyph.color[3] == 1.0));
  assert_eq!(source.get(), "שלום {{task.title}} עולם");
}

#[test]
fn input_tokens_rtl_center_right_auto_size_uses_actual_field_width_for_clicks() {
  use lurq::layout::text_style::TextAlign;
  for align in [TextAlign::Center, TextAlign::Right] {
    let source = Signal::new("שלום {{task.title}} עולם".to_owned());
    let controller = TextInputController::new();
    let mut tree = Tree::new();
    let mut app = App::new();
    tree.set_root(
      TextInput::new(source.clone())
        .tokens(tokens(&source.get()))
        .controller(controller.clone())
        .id("editor")
        .text_align(align),
    );
    let snapshot = render_pass_with_app(&mut tree, &mut app);
    let bounds = tree.find_element(|_| true).unwrap().bounds();
    let pill = snapshot.rects.iter().find(|rect| rect.color == FILL).unwrap();
    assert!(pill.x >= bounds.x && pill.x + pill.width <= bounds.x + bounds.width + 0.02);
    pointer_click(&mut tree, pill.x + pill.width * 0.5, pill.y + 4.0, MouseButton::Left);
    let byte = controller.selection().unwrap().caret;
    assert!(byte > source.get().find(TOKEN).unwrap() && byte < source.get().find(TOKEN).unwrap() + TOKEN.len());
    controller.replace_selection("!").unwrap();
    let mut expected = "שלום {{task.title}} עולם".to_owned();
    expected.insert(byte, '!');
    assert_eq!(source.get(), expected);
  }
}
