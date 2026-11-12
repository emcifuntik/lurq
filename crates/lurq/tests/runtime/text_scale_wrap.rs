use lurq::{
  app::Tree,
  components::{Column, Text},
  layout::{Alignment, text_style::TextStyle},
};

use crate::support::{RenderSnapshot, render_pass};

const SCALE: f32 = 1.5;

fn painted_rows(snapshot: &RenderSnapshot, line_height: f32) -> usize {
  let mut ys: Vec<f32> = snapshot.glyphs.iter().map(|glyph| glyph.y).collect();
  ys.sort_by(|a, b| a.partial_cmp(b).unwrap());

  let threshold = line_height * SCALE * 0.5;
  let mut rows = 0;
  let mut last = f32::NEG_INFINITY;
  for y in ys {
    if y - last > threshold {
      rows += 1;
    }
    last = y;
  }
  rows
}

fn studio_style() -> TextStyle {
  TextStyle {
    font_size: 11.9,
    line_height: 1.4,
    trim_line_box: true,
    ..TextStyle::default()
  }
}

fn measured_rows(height: f32, style: &TextStyle) -> usize {
  (((height - style.font_size) / (style.font_size * style.line_height)).round() as usize) + 1
}

#[test]
fn dpi_scaled_intrinsic_text_does_not_gain_a_paint_only_wrap() {
  let style = studio_style();
  let text = "Point PW Studio at your game element folder. We'll scan and validate the required files.";
  // Measure the system font first: this sentence is wider than 500px in
  // DejaVu Sans. The child must still paint at its own intrinsic width.
  let mut measure = Tree::new();
  measure.resize(4096, 300);
  measure.set_root(Column::new().width(4096.0).child(Text::styled(text, style.clone())));
  render_pass(&mut measure);
  let intrinsic_width = measure.last_layout().unwrap().children[0].result.size.width;
  let mut tree = Tree::new();
  tree.set_scale_factor(SCALE);
  tree.resize(((intrinsic_width + 100.0) * SCALE).ceil() as u32, 300);
  tree.set_root(
    Column::new()
      .align_items(Alignment::Center)
      .width(intrinsic_width + 1.0)
      .child(Text::styled(text, style.clone())),
  );

  let snapshot = render_pass(&mut tree);
  let text_layout = &tree.last_layout().expect("layout should be retained").children[0].result;
  let layout_rows = measured_rows(text_layout.size.height, &style);

  assert_eq!(
    layout_rows, 1,
    "logical layout should fit the modal subtitle on one line"
  );
  assert_eq!(
    painted_rows(&snapshot, style.font_size * style.line_height),
    layout_rows,
    "paint must preserve the line count chosen by logical layout"
  );
}

#[test]
fn dpi_scaled_wrapped_text_does_not_exceed_its_measured_rows() {
  let style = studio_style();
  let text = "Point PW Studio at your game element folder. We'll scan and validate the required files.";
  // How wide the sentence is in the system's font, which is not the same on every platform: the column is
  // narrower than that by enough to wrap it whatever the font.
  let mut measure = Tree::new();
  measure.resize(4096, 300);
  measure.set_root(Column::new().width(4096.0).child(Text::styled(text, style.clone())));
  render_pass(&mut measure);
  let intrinsic_width = measure.last_layout().unwrap().children[0].result.size.width;
  let mut tree = Tree::new();
  tree.set_scale_factor(SCALE);
  tree.resize(900, 300);
  tree.set_root(
    Column::new()
      .align_items(Alignment::Center)
      .width((intrinsic_width * 0.7).min(450.0))
      .child(Text::styled(text, style.clone())),
  );

  let snapshot = render_pass(&mut tree);
  let text_layout = &tree.last_layout().expect("layout should be retained").children[0].result;
  let layout_rows = measured_rows(text_layout.size.height, &style);

  assert!(layout_rows > 1, "narrow logical layout should genuinely wrap the text");
  assert_eq!(
    painted_rows(&snapshot, style.font_size * style.line_height),
    layout_rows,
    "physical paint must use the same line breaks whose height logical layout reserved"
  );
}
