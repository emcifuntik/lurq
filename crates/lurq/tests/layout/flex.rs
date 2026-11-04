use lurq::{
  app::Tree,
  components::{Row, Text, TextOverflow},
  layout::{Alignment, Constraints, Size, layout_kind::FrameConstraints, quad::QuadContent},
  node::dimension::Dimension,
};

use super::PassLayoutExt;

fn rt() -> Tree {
  Tree::new()
}

#[test]
fn row_flex_equal_split() {
  let mut rt = rt();
  let node = lurq::components::Row::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(200.0, 100.0))).unwrap();
  assert_eq!(result.children[0].result.size.width, 100.0);
  assert_eq!(result.children[1].result.size.width, 100.0);
  assert_eq!(result.children[0].offset.x, 0.0);
  assert_eq!(result.children[1].offset.x, 100.0);
}

/// Children sized on their own are laid out first and growing ones after them,
/// but each keeps its place in the line.
#[test]
fn row_mixes_sized_and_growing_children_in_order() {
  let mut rt = rt();
  let sized = |width: f32| lurq::components::Spacer::new().width(width).height(50.0);
  let growing = |grow: f32| lurq::components::Spacer::new().height(50.0).flex(grow);
  let node = lurq::components::Row::with(
    10.0,
    Alignment::Start,
    vec![
      lurq::node::Element::from(sized(50.0)),
      growing(1.0).into(),
      sized(30.0).into(),
      growing(3.0).into(),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(400.0, 100.0))).unwrap();
  let placed: Vec<(f32, f32)> = result
    .children
    .iter()
    .map(|child| (child.offset.x, child.result.size.width))
    .collect();
  assert_eq!(placed, [(0.0, 50.0), (60.0, 72.5), (142.5, 30.0), (182.5, 217.5)]);
}

#[test]
fn row_flex_weighted_split() {
  let mut rt = rt();
  let node = lurq::components::Row::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(3.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(400.0, 100.0))).unwrap();
  assert_eq!(result.children[0].result.size.width, 100.0);
  assert_eq!(result.children[1].result.size.width, 300.0);
}

#[test]
fn row_flex_with_fixed_sibling() {
  let mut rt = rt();
  let node = lurq::components::Row::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new().frame(FrameConstraints {
        width: Some(lurq::node::dimension::Dimension::Px(100.0)),
        height: Some(lurq::node::dimension::Dimension::Px(50.0)),
        ..Default::default()
      }),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(300.0, 100.0))).unwrap();
  assert_eq!(result.children[0].result.size.width, 100.0);
  assert_eq!(result.children[1].result.size.width, 200.0);
  assert_eq!(result.children[1].offset.x, 100.0);
}

#[test]
fn row_flex_with_spacing() {
  let mut rt = rt();
  let node = lurq::components::Row::with(
    20.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          height: Some(lurq::node::dimension::Dimension::Px(50.0)),
          ..Default::default()
        })
        .flex(1.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(220.0, 100.0))).unwrap();
  assert_eq!(result.children[0].result.size.width, 100.0);
  assert_eq!(result.children[1].result.size.width, 100.0);
  assert_eq!(result.children[1].offset.x, 120.0); // 100 + 20 spacing
}

#[test]
fn column_flex_equal_split() {
  let mut rt = rt();
  let node = lurq::components::Column::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          width: Some(lurq::node::dimension::Dimension::Px(100.0)),
          ..Default::default()
        })
        .flex(1.0),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          width: Some(lurq::node::dimension::Dimension::Px(100.0)),
          ..Default::default()
        })
        .flex(1.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(200.0, 300.0))).unwrap();
  assert_eq!(result.children[0].result.size.height, 150.0);
  assert_eq!(result.children[1].result.size.height, 150.0);
  assert_eq!(result.children[1].offset.y, 150.0);
}

#[test]
fn column_flex_with_fixed_sibling() {
  let mut rt = rt();
  let node = lurq::components::Column::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new().frame(FrameConstraints {
        width: Some(lurq::node::dimension::Dimension::Px(100.0)),
        height: Some(lurq::node::dimension::Dimension::Px(60.0)),
        ..Default::default()
      }),
      lurq::components::Spacer::new()
        .frame(FrameConstraints {
          width: Some(lurq::node::dimension::Dimension::Px(100.0)),
          ..Default::default()
        })
        .flex(1.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(200.0, 300.0))).unwrap();
  assert_eq!(result.children[0].result.size.height, 60.0);
  assert_eq!(result.children[1].result.size.height, 240.0);
}

#[test]
fn three_way_flex_split() {
  let mut rt = rt();
  let node = lurq::components::Row::with(
    0.0,
    Alignment::Start,
    vec![
      lurq::components::Spacer::new().flex(1.0),
      lurq::components::Spacer::new().flex(2.0),
      lurq::components::Spacer::new().flex(3.0),
    ],
  );
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::tight(Size::new(600.0, 100.0))).unwrap();
  assert_eq!(result.children[0].result.size.width, 100.0);
  assert_eq!(result.children[1].result.size.width, 200.0);
  assert_eq!(result.children[2].result.size.width, 300.0);
}

#[test]
fn flex_text_overflow_ellipsizes_before_a_fixed_sibling() {
  let mut rt = rt();
  rt.set_root(
    Row::new()
      .width(180.0)
      .child(
        Row::new().flex(1.0).clip().child(
          Text::new("connect-to-production-cluster")
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        ),
      )
      .child(Row::new().width(Dimension::Px(60.0))),
  );

  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 100.0))).unwrap();
  let quads = rt.resolve_quads(&result);
  let text = quads
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Text { text, .. } => Some(text.as_str()),
      _ => None,
    })
    .expect("text quad");

  assert!(text.ends_with('…'), "expected ellipsis text, got {text:?}");
}
