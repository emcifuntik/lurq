use super::*;
use crate::{
  app::glyph_engine::input_tokens::{InputConfig, TokenStyle},
  layout::text_style::{TextShadow, TextStyle},
  node::transform::Transform2D,
};

fn fixture() -> (GlyphEngine, std::sync::Arc<DecoratedInputLayout>, Quad) {
  let mut engine = GlyphEngine::new();
  let base = TextStyle {
    font_size: 13.0,
    line_height: 1.4,
    ..TextStyle::default()
  };
  let token = TextStyle {
    color: Color::new(20, 80, 130, 255),
    shadow: Some(TextShadow::new(2.0, 3.0, 4.0, Color::new(2, 3, 4, 128))),
    ..base.clone()
  };
  let layout = engine.decorated_input(InputConfig {
    source: "a {{task.title}} b".into(),
    base,
    styles: vec![TokenStyle {
      range: 2..16,
      text: token,
      padding: 3.0,
      fill: None,
      lines: Default::default(),
    }],
    width: 150.0,
    min_width: 150.0,
    wrap: true,
  });
  let quad = Quad {
    x: 12.0,
    y: 18.0,
    width: 150.0,
    height: layout.size.height,
    opacity: 0.5,
    transform: Transform2D::IDENTITY,
    transform_origin: None,
    content: QuadContent::DecoratedInput { layout: layout.clone() },
    border_radius: None,
    border: None,
    clip: ClipRect::default(),
  };
  (engine, layout, quad)
}

#[test]
fn input_tokens_rtl_paint_exact_scale_clip_order_shadow_and_opacity_once() {
  let (mut engine, layout, quad) = fixture();
  let clip = ClipRect {
    x: 20.0,
    y: 30.0,
    width: 200.0,
    height: 40.0,
    active: true,
    ..ClipRect::default()
  };
  let mut one = Vec::new();
  let mut two = Vec::new();
  let before = layout.positions();
  resolve_input_quad(&mut engine, &layout, &quad, 1.0, 7, clip, &mut one);
  resolve_input_quad(&mut engine, &layout, &quad, 2.0, 7, clip, &mut two);
  assert!(!one.is_empty());
  assert_eq!(one.len(), two.len());
  assert_scale(&mut engine, &layout, &quad, clip, 1.0, &one);
  assert_scale(&mut engine, &layout, &quad, clip, 2.0, &two);
  assert_eq!(one.iter().filter(|glyph| glyph.shadow_sigma > 0.0).count(), 14);
  assert_eq!(layout.config.source.as_ref(), "a {{task.title}} b");
  assert_eq!(
    before
      .iter()
      .map(|caret| (caret.index, caret.x, caret.y))
      .collect::<Vec<_>>(),
    layout
      .positions()
      .iter()
      .map(|caret| (caret.index, caret.x, caret.y))
      .collect::<Vec<_>>()
  );
}

fn assert_scale(
  engine: &mut GlyphEngine,
  layout: &DecoratedInputLayout,
  quad: &Quad,
  clip: ClipRect,
  scale: f32,
  commands: &[GlyphCmd],
) {
  let mut index = 0;
  for glyph in &layout.glyphs {
    let Some(ink) =
      crate::app::glyph_engine::input_tokens::paint_oracle::snapshot(engine, glyph, (quad.x, quad.y), scale)
    else {
      continue;
    };
    let shadow = glyph.shadow.filter(TextShadow::is_visible);
    let main = &commands[index + usize::from(shadow.is_some())];
    assert_eq!(
      [main.x, main.y],
      ink.expected_corner,
      "scale {scale}, source {}..{}: {ink:?}",
      glyph.glyph.start,
      glyph.glyph.end
    );
    assert_eq!([main.width, main.height], ink.expected_size);
    assert_eq!(main.shadow_sigma, 0.0);
    let mut color = glyph.color.to_linear_f32_array();
    color[3] *= quad.opacity;
    assert_eq!(main.color, color);
    assert_command(main, clip);
    if let Some(shadow) = shadow {
      let command = &commands[index];
      assert_eq!(
        [command.x - main.x, command.y - main.y],
        [shadow.offset_x * scale, shadow.offset_y * scale]
      );
      assert_eq!([command.width, command.height], ink.expected_size);
      assert_eq!(command.shadow_sigma, shadow.blur_radius * scale * 0.5);
      let mut color = shadow.color.to_linear_f32_array();
      color[3] *= quad.opacity;
      assert_eq!(command.color, color);
      assert_eq!(
        command.transform_origin,
        [-shadow.offset_x * scale, -shadow.offset_y * scale]
      );
      assert_eq!(command.atlas_min, main.atlas_min);
      assert_eq!(command.atlas_max, main.atlas_max);
      assert_command(command, clip);
      index += 1;
    }
    index += 1;
  }
  assert_eq!(index, commands.len());
}

fn assert_command(command: &GlyphCmd, clip: ClipRect) {
  assert_eq!(command.order, 7);
  assert_eq!(
    [command.clip.x, command.clip.y, command.clip.width, command.clip.height],
    [clip.x, clip.y, clip.width, clip.height]
  );
  assert_eq!(command.clip.active, clip.active);
  assert_eq!(command.clip.border_radius, clip.border_radius);
  assert_eq!(command.transform, Transform2D::IDENTITY.matrix_2x2());
  assert!(!command.color_glyph);
}

#[test]
fn input_tokens_rtl_transformed_exact_glyphs_retain_logical_rows_and_clip() {
  let (mut engine, layout, mut quad) = fixture();
  quad.transform = Transform2D::scale(1.5, 1.2).then(&Transform2D::rotate_deg(12.0));
  quad.transform_origin = Some([5.0, 7.0]);
  let mut out = Vec::new();
  let before = layout.positions();
  let clip = ClipRect {
    x: 0.0,
    y: 0.0,
    width: 500.0,
    height: 300.0,
    active: true,
    ..ClipRect::default()
  };
  let range = resolve_input_quad(&mut engine, &layout, &quad, 2.0, 11, clip, &mut out);
  assert_eq!(range, 0..out.len());
  assert!(
    out
      .iter()
      .all(|glyph| glyph.transform == quad.transform.matrix_2x2() && glyph.order == 11 && glyph.clip.active)
  );
  assert!(
    out
      .iter()
      .all(|glyph| (glyph.x + glyph.transform_origin[0] - 34.0).abs() < 0.01
        && (glyph.y + glyph.transform_origin[1] - 50.0).abs() < 0.01)
  );
  let after = layout.positions();
  assert_eq!(
    before
      .iter()
      .map(|caret| (caret.index, caret.x, caret.y))
      .collect::<Vec<_>>(),
    after
      .iter()
      .map(|caret| (caret.index, caret.x, caret.y))
      .collect::<Vec<_>>()
  );
}

#[cfg(feature = "screenshot")]
#[test]
fn input_tokens_rtl_actual_glyph_redaction_range_includes_shadow_and_transform() {
  let (mut engine, layout, mut quad) = fixture();
  quad.transform = Transform2D::scale(1.3, 1.3);
  let mut out = Vec::new();
  let range = resolve_input_quad(&mut engine, &layout, &quad, 2.0, 3, ClipRect::default(), &mut out);
  let area = crate::app::capture_redaction::RedactedArea::of_glyph_run(&out[range]).unwrap();
  assert!(area.x0 < area.x1 && area.y0 < area.y1);
  let without_shadow: Vec<_> = out.iter().filter(|glyph| glyph.shadow_sigma == 0.0).cloned().collect();
  let ink = crate::app::capture_redaction::RedactedArea::of_glyph_run(&without_shadow).unwrap();
  assert!(area.x0 <= ink.x0 && area.y0 <= ink.y0 && area.x1 >= ink.x1 && area.y1 >= ink.y1);
}
