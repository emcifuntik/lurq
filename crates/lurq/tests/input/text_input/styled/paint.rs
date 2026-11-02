//! Marks are drawn from the theme's text-mark roles over the glyphs they
//! style: the code fill behind its text, the link in its color and
//! underlined, the strikethrough across its text.
use lurq::{
  app::theme::{TextMarkStyle, ThemeTextMarks},
  components::SpanStyle,
  layout::text_style::TextStyle,
  node::color::Color,
};

use super::{Editor, bold, span};
use crate::support::{GlyphSnapshot, RectSnapshot, RenderSnapshot};

const CODE_FILL: Color = Color::new(20, 200, 90, 255);
const LINK: Color = Color::new(200, 30, 160, 255);

fn marks() -> ThemeTextMarks {
  let mut marks = ThemeTextMarks::default();
  marks.code.background = Some(CODE_FILL.into());
  marks.link = TextMarkStyle {
    color: Some(LINK.into()),
    underline: true,
    ..TextMarkStyle::default()
  };
  marks
}

/// The left and right of `glyphs`.
fn extent<'a>(glyphs: impl Iterator<Item = &'a GlyphSnapshot>) -> (f32, f32) {
  glyphs.fold((f32::MAX, f32::MIN), |(left, right), glyph| {
    (left.min(glyph.x), right.max(glyph.x + glyph.width))
  })
}

fn thin_rects(snapshot: &RenderSnapshot, color: Color) -> Vec<&RectSnapshot> {
  snapshot
    .rects
    .iter()
    .filter(|rect| rect.color == color && rect.height <= 2.5 && rect.width > 1.0)
    .collect()
}

#[test]
fn styled_marks_paint_fills_colors_and_lines_from_the_theme() {
  let text = "bold code strike link";
  let spans = vec![
    span(0..4, bold()),
    span(5..9, SpanStyle::new().code()),
    span(10..16, SpanStyle::new().strikethrough()),
    span(17..21, SpanStyle::new().link("x")),
  ];
  let mut editor = Editor::new(text, spans);
  editor.app.theme().set_text_marks(marks());
  let snapshot = editor.render();

  let fill = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == CODE_FILL)
    .expect("the code fill");
  let link_glyphs: Vec<_> = snapshot
    .glyphs
    .iter()
    .filter(|glyph| glyph.color == LINK.to_linear_f32_array())
    .collect();
  assert_eq!(link_glyphs.len(), 4, "the link's four letters take the link color");
  let (link_left, link_right) = extent(link_glyphs.iter().copied());
  let underline = thin_rects(&snapshot, LINK);
  assert_eq!(underline.len(), 1, "one underline under the link");
  // Glyph boxes reach a little past the advances the underline spans.
  assert!(underline[0].x <= link_left + 1.0 && underline[0].x + underline[0].width >= link_right - 4.0);
  let glyph_bottom = link_glyphs
    .iter()
    .map(|glyph| glyph.y + glyph.height)
    .fold(f32::MIN, f32::max);
  assert!(
    underline[0].y > glyph_bottom - 6.0,
    "the underline sits at the baseline"
  );

  // The fill is behind the code text, left of the strikethrough and the link.
  assert!(fill.width > 10.0 && fill.x + fill.width <= link_left);
  let ink = TextStyle::default().color;
  let strike = thin_rects(&snapshot, ink);
  assert_eq!(strike.len(), 1, "one line through the struck word");
  assert!(strike[0].x > fill.x + fill.width - 1.0 && strike[0].x + strike[0].width < link_left);
  let strike_glyphs: Vec<_> = snapshot
    .glyphs
    .iter()
    .filter(|glyph| glyph.x >= strike[0].x - 1.0 && glyph.x + glyph.width <= strike[0].x + strike[0].width + 1.0)
    .collect();
  assert!(!strike_glyphs.is_empty());
  assert!(
    strike_glyphs
      .iter()
      .all(|glyph| glyph.y < strike[0].y && strike[0].y < glyph.y + glyph.height),
    "the line crosses the glyphs"
  );
}

#[test]
fn styled_spans_written_by_the_app_are_drawn_on_the_next_frame() {
  let mut editor = Editor::new("plain link", vec![]);
  editor.app.theme().set_text_marks(marks());
  let before = editor.render();
  assert!(
    before
      .glyphs
      .iter()
      .all(|glyph| glyph.color != LINK.to_linear_f32_array())
  );
  editor.spans.set(vec![span(6..10, SpanStyle::new().link("x"))]);
  let after = editor.render();
  let linked = after
    .glyphs
    .iter()
    .filter(|glyph| glyph.color == LINK.to_linear_f32_array())
    .count();
  assert_eq!(linked, 4);
}

#[test]
fn styled_a_composition_is_drawn_in_the_style_typing_would_give_it() {
  const BOLD: Color = Color::new(30, 90, 210, 255);
  let mut editor = Editor::new("ab cd", vec![span(0..2, bold())]);
  let mut marks = ThemeTextMarks::default();
  marks.bold.color = Some(BOLD.into());
  editor.app.theme().set_text_marks(marks);
  editor.select(2, 2);
  editor.tree.ime(lurq::app::events::ImeEvent::Preedit {
    text: "xy".into(),
    cursor: Some((2, 2)),
  });
  let snapshot = editor.render();
  let bold_glyphs = snapshot
    .glyphs
    .iter()
    .filter(|glyph| glyph.color == BOLD.to_linear_f32_array())
    .count();
  assert_eq!(bold_glyphs, 4, "\"ab\" and the composition \"xy\" are bold");
  assert_eq!(editor.value.get(), "ab cd");
}

#[test]
fn styled_an_input_that_gains_its_spans_on_a_later_render_draws_them() {
  let value = lurq::core::Signal::new("plain link".to_owned());
  let spans = lurq::core::Signal::new(vec![span(6..10, SpanStyle::new().link("x"))]);
  let mut app = lurq::app::App::new();
  let mut tree = lurq::app::Tree::new();
  tree.resize(800, 600);
  app.theme().set_text_marks(marks());
  tree.set_root(lurq::components::TextInput::new(value.clone()).width(300.0));
  crate::support::render_pass_with_app(&mut tree, &mut app);
  tree.set_root(lurq::components::TextInput::new(value).spans(spans).width(300.0));
  let snapshot = crate::support::render_pass_with_app(&mut tree, &mut app);
  let linked = snapshot
    .glyphs
    .iter()
    .filter(|glyph| glyph.color == LINK.to_linear_f32_array())
    .count();
  assert_eq!(linked, 4);
}

/// Paragraphs are laid out one at a time and kept: after an edit in one,
/// every paragraph sits exactly where a fresh layout of the same text puts it.
#[test]
fn styled_an_edit_in_one_paragraph_draws_like_a_fresh_input() {
  let paragraph = "A paragraph long enough to wrap onto a second row at this width, with bold words.";
  let text = [paragraph; 6].join("\n");
  let spans: Vec<_> = (0..6)
    .map(|index| {
      let start = index * (paragraph.len() + 1) + paragraph.len() - 11;
      span(start..start + 10, bold())
    })
    .collect();
  let mut edited = Editor::new(&text, spans);
  edited.app.theme().set_text_marks(marks());
  edited.render();
  let at = paragraph.len() + 1 + 2;
  edited.select(at, at);
  edited.type_text("xyz\nnew ");
  let after = edited.render();

  let mut fresh = Editor::new(&edited.value.get(), edited.spans());
  fresh.app.theme().set_text_marks(marks());
  let expected = fresh.render();
  let place = |snapshot: &RenderSnapshot| {
    snapshot
      .glyphs
      .iter()
      .map(|glyph| {
        (
          glyph.x.round() as i32,
          glyph.y.round() as i32,
          glyph.color.map(|c| (c * 255.0) as u8),
        )
      })
      .collect::<Vec<_>>()
  };
  assert_eq!(place(&after), place(&expected));
}
