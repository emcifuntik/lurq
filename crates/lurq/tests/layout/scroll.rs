use lurq::{
  app::{App, Tree, events::ScrollEvent},
  components::{Column, Row, ScrollVertical, Spacer},
  layout::{
    Constraints, Size,
    layout_kind::{FrameConstraints, ScrollState},
    quad::QuadContent,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle, ScrollBarVisibility},
  },
  node::{Element, color::Color, dimension::Dimension},
};

use super::PassLayoutExt;
use crate::support::{TestSurface, run_pass};

fn rt() -> Tree {
  Tree::new()
}

#[test]
fn scroll_vertical_child_grows_unbounded() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(lurq::components::Column::new().spacing(0.0).with_children(
    (0..10).map(|_| {
      lurq::components::Spacer::new().frame(FrameConstraints {
        width: Some(lurq::node::dimension::Dimension::Px(100.0)),
        height: Some(lurq::node::dimension::Dimension::Px(50.0)),
        ..Default::default()
      })
    }),
  ))
  .size(100.0, 200.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  // Scroll container itself is 200 tall
  assert_eq!(result.size.height, 200.0);
  assert_eq!(result.size.width, 100.0);
  // The child (column) inside should be 500 tall (10 * 50)
  let column_child = &result.children[0].result;
  assert_eq!(column_child.size.height, 500.0);
}

#[test]
fn scroll_vertical_offset_applied() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().frame(FrameConstraints {
    width: Some(lurq::node::dimension::Dimension::Px(100.0)),
    height: Some(lurq::node::dimension::Dimension::Px(500.0)),
    ..Default::default()
  }))
  .size(100.0, 200.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  // Default scroll offset is 0
  assert_eq!(result.children[0].offset.y, 0.0);
}

#[test]
fn scroll_prevent_default_blocks_auto_scroll() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(500.0))
    .size(100.0, 100.0)
    .on_scroll(|event: ScrollEvent| event.prevent_default());

  rt.set_root(node);
  run_pass(&mut rt);
  rt.scroll(50.0, 50.0, 0.0, 60.0, lurq::app::events::ScrollPhase::Scroll);

  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  assert_eq!(result.children[0].offset.y, 0.0);
}

#[test]
fn scroll_horizontal_child_grows_unbounded() {
  let mut rt = rt();
  let node = lurq::components::ScrollHorizontal::new(lurq::components::Row::new().spacing(0.0).with_children(
    (0..10).map(|_| {
      lurq::components::Spacer::new().frame(FrameConstraints {
        width: Some(lurq::node::dimension::Dimension::Px(100.0)),
        height: Some(lurq::node::dimension::Dimension::Px(50.0)),
        ..Default::default()
      })
    }),
  ))
  .size(200.0, 50.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  assert_eq!(result.size.width, 200.0);
  assert_eq!(result.size.height, 50.0);
  let row_child = &result.children[0].result;
  assert_eq!(row_child.size.width, 1000.0);
}

#[test]
fn scroll_both_unbounded() {
  let mut rt = rt();
  let node = lurq::components::ScrollBoth::new(lurq::components::Spacer::new().frame(FrameConstraints {
    width: Some(lurq::node::dimension::Dimension::Px(800.0)),
    height: Some(lurq::node::dimension::Dimension::Px(600.0)),
    ..Default::default()
  }))
  .size(200.0, 150.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  assert_eq!(result.size.width, 200.0);
  assert_eq!(result.size.height, 150.0);
  let scroll_child = &result.children[0].result;
  assert_eq!(scroll_child.size.width, 800.0);
  assert_eq!(scroll_child.size.height, 600.0);
}

#[test]
fn scroll_container_without_frame_uses_parent_constraints() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(lurq::components::Column::new().spacing(0.0).with_children(
    (0..5).map(|_| {
      lurq::components::Spacer::new().frame(FrameConstraints {
        width: Some(lurq::node::dimension::Dimension::Px(100.0)),
        height: Some(lurq::node::dimension::Dimension::Px(40.0)),
        ..Default::default()
      })
    }),
  ));

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(300.0, 100.0))).unwrap();
  // Container takes parent's tight constraint
  assert_eq!(result.size.width, 300.0);
  assert_eq!(result.size.height, 100.0);
  // Child column is taller
  assert_eq!(result.children[0].result.size.height, 200.0);
}

#[test]
fn flexed_scroll_with_zero_height_frame_uses_parent_flex_size() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let scroll = ScrollVertical::new(Spacer::new().height(400.0))
    .height(0.0)
    .flex(1.0)
    .with_scroll_state(scroll_state.clone())
    .scrollbar(ScrollBarStyle {
      placement: ScrollBarPlacement::Reserved,
      ..Default::default()
    });
  let node = Column::new()
    .width(100.0)
    .height(200.0)
    .child(scroll)
    .child(Spacer::new().height(64.0));

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();

  assert_eq!(result.children[0].result.size.height, 136.0);
  assert_eq!(scroll_state.viewport_height(), 136.0);
  assert_eq!(scroll_state.content_height(), 400.0);
  assert!(scroll_state.content_height() > scroll_state.viewport_height());
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn flexed_scroll_with_zero_height_frame_consumes_wheel_scroll() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let scroll = ScrollVertical::new(Spacer::new().height(400.0))
    .height(0.0)
    .flex(1.0)
    .with_scroll_state(scroll_state.clone())
    .scrollbar(ScrollBarStyle {
      placement: ScrollBarPlacement::Reserved,
      ..Default::default()
    });
  let node = Column::new()
    .width(100.0)
    .height(200.0)
    .child(scroll)
    .child(Spacer::new().height(64.0));

  rt.set_root(node);
  run_pass(&mut rt);
  rt.scroll(10.0, 10.0, 0.0, -80.0, lurq::app::events::ScrollPhase::Scroll);

  assert!(scroll_state.scroll_y() > 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn flexed_scroll_with_zero_height_frame_keeps_scroll_metrics_after_cached_pass() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let scroll = ScrollVertical::new(Spacer::new().height(400.0))
    .height(0.0)
    .flex(1.0)
    .with_scroll_state(scroll_state.clone())
    .scrollbar(ScrollBarStyle {
      placement: ScrollBarPlacement::Reserved,
      ..Default::default()
    });
  let node = Column::new()
    .width(100.0)
    .height(200.0)
    .child(scroll)
    .child(Spacer::new().height(64.0));

  rt.set_root(node);
  run_pass(&mut rt);
  run_pass(&mut rt);

  assert_eq!(scroll_state.viewport_height(), 136.0);
  assert_eq!(scroll_state.content_height(), 400.0);
  rt.scroll(10.0, 10.0, 0.0, -80.0, lurq::app::events::ScrollPhase::Scroll);
  assert!(scroll_state.scroll_y() > 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn flex_shrunk_scroll_state_uses_final_layout_size() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let scroll = ScrollVertical::new(Spacer::new().width(689.0).height(1200.0))
    .width(Dimension::Pct(100.0))
    .height(464.0)
    .flex_full(0.0, 1.0, Some(701.0))
    .with_scroll_state(scroll_state.clone())
    .scrollbar(ScrollBarStyle {
      placement: ScrollBarPlacement::Reserved,
      ..Default::default()
    });
  let node = Row::new()
    .width(733.0)
    .height(464.0)
    .child(Spacer::new().width(701.0).height(1.0))
    .child(scroll);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(800.0, 600.0))).unwrap();

  assert_eq!(result.children[1].result.size.width, 32.0);
  assert_eq!(scroll_state.viewport_width(), 20.0);
  assert_eq!(scroll_state.viewport_height(), 464.0);
  assert!(scroll_state.content_height() > scroll_state.viewport_height());
}

#[test]
fn scroll_stays_at_bottom_when_viewport_shrinks() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let node = |height| {
    ScrollVertical::new(Spacer::new().height(1000.0))
      .width(100.0)
      .height(height)
      .with_scroll_state(scroll_state.clone())
  };

  rt.set_root(node(200.0));
  run_pass(&mut rt);
  scroll_state.scroll_to_bottom_pending();
  run_pass(&mut rt);
  assert_eq!(scroll_state.scroll_y(), 800.0);

  rt.set_root(node(100.0));
  run_pass(&mut rt);

  assert_eq!(scroll_state.scroll_y(), 900.0);
}

#[test]
fn scroll_resize_preserves_detached_offset() {
  let mut rt = rt();
  let scroll_state = ScrollState::new();
  let node = |height| {
    ScrollVertical::new(Spacer::new().height(1000.0))
      .width(100.0)
      .height(height)
      .with_scroll_state(scroll_state.clone())
  };

  rt.set_root(node(200.0));
  run_pass(&mut rt);
  scroll_state.set_scroll(0.0, 500.0);

  rt.set_root(node(100.0));
  run_pass(&mut rt);

  assert_eq!(scroll_state.scroll_y(), 500.0);
}

#[test]
fn scroll_empty_child() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(Element::new()).size(100.0, 100.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  assert_eq!(result.size.width, 100.0);
  assert_eq!(result.size.height, 100.0);
}

#[test]
fn scrollbar_uses_theme_style_by_default() {
  let mut app = App::new();
  let thumb_color = Color::from_hex("#8b5cf6");
  app.theme().set_scrollbar(ScrollBarStyle {
    visible: ScrollBarVisibility::Always,
    width: 6.0,
    thumb_color,
    ..Default::default()
  });

  let mut rt = rt();
  rt.set_root(lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(300.0)).size(100.0, 100.0));
  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  rt.pass(&mut app, &TestSurface);

  let quads = rt.resolve_quads(rt.last_layout().unwrap());
  assert!(quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == thumb_color && quad.width == 6.0,
    _ => false,
  }));
}

#[test]
fn scrollbar_node_style_overrides_theme_style() {
  let mut app = App::new();
  let theme_thumb_color = Color::from_hex("#8b5cf6");
  let node_thumb_color = Color::from_hex("#14b8a6");
  app.theme().set_scrollbar(ScrollBarStyle {
    visible: ScrollBarVisibility::Always,
    width: 6.0,
    thumb_color: theme_thumb_color,
    ..Default::default()
  });

  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(300.0))
    .scrollbar(ScrollBarStyle {
      visible: ScrollBarVisibility::Always,
      width: 9.0,
      thumb_color: node_thumb_color,
      ..Default::default()
    })
    .size(100.0, 100.0);
  rt.set_root(node);
  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  rt.pass(&mut app, &TestSurface);

  let quads = rt.resolve_quads(rt.last_layout().unwrap());
  assert!(quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == node_thumb_color && quad.width == 9.0,
    _ => false,
  }));
  assert!(!quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == theme_thumb_color && quad.width == 6.0,
    _ => false,
  }));
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn scrollbar_hovered_overrides_scrollbar_style() {
  let mut rt = rt();
  let thumb_color = Color::from_hex("#ef4444");

  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(300.0))
    .scrollbar(ScrollBarStyle {
      visible: ScrollBarVisibility::Always,
      ..Default::default()
    })
    .scrollbar_hovered(move |style| style.with_thumb_color(thumb_color))
    .size(100.0, 100.0);

  rt.set_root(node);
  run_pass(&mut rt);
  rt.mouse_move(100.0 - 4.0, 10.0);

  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert!(quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == thumb_color,
    _ => false,
  }));
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn horizontal_scrollbar_hovered_overrides_scrollbar_style() {
  let mut rt = rt();
  let thumb_color = Color::from_hex("#ef4444");

  let node = lurq::components::ScrollHorizontal::new(lurq::components::Spacer::new().width(300.0))
    .scrollbar(ScrollBarStyle {
      visible: ScrollBarVisibility::Always,
      ..Default::default()
    })
    .scrollbar_hovered(move |style| style.with_thumb_color(thumb_color))
    .size(100.0, 100.0);

  rt.set_root(node);
  run_pass(&mut rt);
  rt.mouse_move(10.0, 100.0 - 4.0);

  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert!(quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == thumb_color,
    _ => false,
  }));
}

#[test]
fn scrollbar_auto_does_not_render_when_content_fits() {
  let mut rt = rt();
  let thumb_color = Color::from_hex("#3b82f6");

  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(80.0))
    .scrollbar(ScrollBarStyle {
      visible: ScrollBarVisibility::Auto,
      thumb_color,
      ..Default::default()
    })
    .size(100.0, 100.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert!(!quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == thumb_color,
    _ => false,
  }));
}

#[test]
fn scrollbar_renders_when_styled_before_width_and_fill() {
  let mut rt = rt();
  let thumb_color = Color::from_hex("#3b82f6");

  let node = lurq::components::ScrollVertical::new(lurq::components::Spacer::new().height(300.0).background("#162032"))
    .scrollbar(ScrollBarStyle {
      visible: ScrollBarVisibility::Always,
      width: 6.0,
      thumb_color,
      thumb_radius: 4.0,
      ..ScrollBarStyle::default()
    })
    .scrollbar_hovered(|style| style.with_thumb_color(Color::from_hex("#06b6d4")))
    .width(200.0)
    .background("#162032");

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(200.0, 100.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert!(quads.iter().any(|quad| match &quad.content {
    QuadContent::Rect { color, .. } => *color == thumb_color && quad.width == 6.0,
    _ => false,
  }));
}

#[test]
fn scroll_culling_culls_offscreen_child_quads_and_preserves_content_extent() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(colored_scroll_rows())
    .scrollbar(ScrollBarStyle::hidden())
    .size(100.0, 100.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert_eq!(result.children[0].result.size.height, 500.0);
  assert_eq!(
    scroll_row_colors(&quads),
    vec![scroll_row_color(0), scroll_row_color(1)]
  );
}

#[test]
fn culling_false_keeps_offscreen_child_quads() {
  let mut rt = rt();
  let node = lurq::components::ScrollVertical::new(colored_scroll_rows())
    .culling(false)
    .scrollbar(ScrollBarStyle::hidden())
    .size(100.0, 100.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert_eq!(result.children[0].result.size.height, 500.0);
  assert_eq!(scroll_row_colors(&quads).len(), 10);
}

#[test]
fn scroll_culling_respects_scroll_offset() {
  let mut rt = rt();
  let state = lurq::layout::layout_kind::ScrollState::new();
  state.set_scroll_pending(0.0, 150.0);
  let node = lurq::components::ScrollVertical::new(colored_scroll_rows())
    .with_scroll_state(state.clone())
    .culling(true)
    .scrollbar(ScrollBarStyle::hidden())
    .size(100.0, 100.0);

  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 400.0))).unwrap();
  let quads = rt.resolve_quads(&result);

  assert_eq!(state.scroll_y(), 150.0);
  assert_eq!(result.children[0].result.size.height, 500.0);
  assert_eq!(
    scroll_row_colors(&quads),
    vec![scroll_row_color(3), scroll_row_color(4)]
  );
}

fn colored_scroll_rows() -> lurq::components::Column {
  lurq::components::Column::new()
    .spacing(0.0)
    .with_children((0..10).map(|index| lurq::components::Rect::new(100.0, 50.0).background(scroll_row_color(index))))
}

fn scroll_row_color(index: u8) -> Color {
  Color::new(index, 32, 64, 255)
}

fn scroll_row_colors(quads: &[lurq::layout::quad::Quad]) -> Vec<Color> {
  quads
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Rect { color, .. } if color.g() == 32 && color.b() == 64 => Some(*color),
      _ => None,
    })
    .collect()
}
