//! The scenarios and the two engines' builders, shared by `vs_taffy` and
//! `vs_taffy_allocs`.
#![allow(dead_code)]

use lurq::{
  app::{App, Tree},
  components::{Column, Rect, Row},
  layout::{Alignment, layout_result::LayoutResult},
  node::{Element, dimension::Dimension as LDim},
};
use taffy::prelude::*;

pub const VIEW_W: f32 = 1200.0;
pub const VIEW_H: f32 = 800.0;
pub const WIDTHS: [f32; 3] = [1200.0, 1100.0, 1000.0];

#[derive(Clone)]
pub enum Spec {
  Leaf {
    w: Option<f32>,
    h: f32,
    grow: f32,
  },
  Box {
    row: bool,
    gap: f32,
    pad: f32,
    grow: f32,
    wrap: bool,
    width: Option<f32>,
    children: Vec<Spec>,
  },
}

pub fn leaf(w: f32, h: f32) -> Spec {
  Spec::Leaf {
    w: Some(w),
    h,
    grow: 0.0,
  }
}

pub fn container(row: bool, gap: f32, pad: f32, children: Vec<Spec>) -> Spec {
  Spec::Box {
    row,
    gap,
    pad,
    grow: 0.0,
    wrap: false,
    width: None,
    children,
  }
}

pub fn count(spec: &Spec) -> usize {
  match spec {
    Spec::Leaf { .. } => 1,
    Spec::Box { children, .. } => 1 + children.iter().map(count).sum::<usize>(),
  }
}

// ---- scenarios ----

pub fn flat_column(n: usize) -> Spec {
  container(false, 0.0, 0.0, (0..n).map(|_| leaf(100.0, 20.0)).collect())
}

pub fn nested(depth: usize, items: usize) -> Spec {
  fn go(depth: usize, items: usize, row: bool) -> Spec {
    if depth == 0 {
      return leaf(40.0, 20.0);
    }
    container(row, 4.0, 2.0, (0..items).map(|_| go(depth - 1, items, !row)).collect())
  }
  go(depth, items, true)
}

pub fn grow_rows(rows: usize) -> Spec {
  let row = Spec::Box {
    row: true,
    gap: 4.0,
    pad: 0.0,
    grow: 0.0,
    wrap: false,
    width: Some(1000.0),
    children: (0..10)
      .map(|_| Spec::Leaf {
        w: None,
        h: 20.0,
        grow: 1.0,
      })
      .collect(),
  };
  container(false, 8.0, 8.0, vec![row; rows])
}

pub fn wrap_row(n: usize) -> Spec {
  Spec::Box {
    row: true,
    gap: 4.0,
    pad: 0.0,
    grow: 0.0,
    wrap: true,
    width: None,
    children: (0..n).map(|_| leaf(50.0, 30.0)).collect(),
  }
}

pub fn deep_chain(depth: usize) -> Spec {
  let mut spec = leaf(10.0, 10.0);
  for i in 0..depth {
    spec = container(i % 2 == 0, 0.0, 1.0, vec![spec]);
  }
  spec
}

pub fn scenarios() -> Vec<(String, Spec)> {
  let mut out = Vec::new();
  for n in [100, 1_000, 10_000] {
    out.push((format!("flat_column/{n}"), flat_column(n)));
  }
  for (d, i) in [(4, 6), (6, 4)] {
    out.push((format!("nested/d{d}_i{i}"), nested(d, i)));
  }
  for r in [10, 100, 1_000] {
    out.push((format!("grow_rows/{r}"), grow_rows(r)));
  }
  for n in [100, 1_000] {
    out.push((format!("wrap_row/{n}"), wrap_row(n)));
  }
  for d in [16, 64] {
    out.push((format!("deep_chain/{d}"), deep_chain(d)));
  }
  out
}

// ---- lurq ----

pub fn lurq_element(spec: &Spec) -> Element {
  match spec {
    Spec::Leaf { w, h, grow } => {
      let width = w.map_or(LDim::Auto, LDim::Px);
      let rect = Rect::new(width, *h);
      if *grow > 0.0 {
        rect.flex(*grow).into()
      } else {
        rect.into()
      }
    }
    Spec::Box {
      row,
      gap,
      pad,
      grow,
      wrap,
      width,
      children,
    } => {
      let kids = children.iter().map(lurq_element);
      if *row {
        let mut r = Row::with(*gap, Alignment::Start, kids).padding(*pad);
        if *wrap {
          r = r.wrap();
        }
        if let Some(w) = width {
          r = r.width(*w);
        }
        if *grow > 0.0 { r.flex(*grow).into() } else { r.into() }
      } else {
        let mut c = Column::with(*gap, Alignment::Start, kids).padding(*pad);
        if *wrap {
          c = c.wrap();
        }
        if let Some(w) = width {
          c = c.width(*w);
        }
        if *grow > 0.0 { c.flex(*grow).into() } else { c.into() }
      }
    }
  }
}

pub fn lurq_tree(spec: &Spec, app: &mut App) -> Tree {
  let mut tree = Tree::new();
  tree.resize(VIEW_W as u32, VIEW_H as u32);
  tree.set_root(lurq_element(spec));
  tree.bench_layout_only(app, true);
  tree
}

pub fn lurq_leaf_positions(result: &LayoutResult, x: f32, y: f32, out: &mut Vec<(f32, f32, f32, f32)>) {
  if result.children.is_empty() {
    out.push((x, y, result.size.width, result.size.height));
  }
  for child in &result.children {
    lurq_leaf_positions(&child.result, x + child.offset.x, y + child.offset.y, out);
  }
}

// ---- taffy ----

pub fn taffy_style(spec: &Spec) -> Style {
  let no_shrink = Style {
    flex_shrink: 0.0,
    ..Style::default()
  };
  match spec {
    Spec::Leaf { w, h, grow } => Style {
      size: Size {
        width: w.map_or(Dimension::auto(), length),
        height: length(*h),
      },
      flex_grow: *grow,
      ..no_shrink
    },
    Spec::Box {
      row,
      gap,
      pad,
      grow,
      wrap,
      width,
      ..
    } => Style {
      size: Size {
        width: width.map_or(Dimension::auto(), length),
        height: Dimension::auto(),
      },
      display: Display::Flex,
      flex_direction: if *row {
        FlexDirection::Row
      } else {
        FlexDirection::Column
      },
      flex_wrap: if *wrap { FlexWrap::Wrap } else { FlexWrap::NoWrap },
      align_items: AlignItems::START,
      align_content: AlignContent::START,
      gap: Size {
        width: length(*gap),
        height: length(*gap),
      },
      padding: taffy::Rect {
        left: length(*pad),
        right: length(*pad),
        top: length(*pad),
        bottom: length(*pad),
      },
      flex_grow: *grow,
      ..no_shrink
    },
  }
}

pub struct TaffyBuilt {
  pub tree: TaffyTree<()>,
  pub root: NodeId,
  pub leaves: Vec<NodeId>,
}

pub fn taffy_build(spec: &Spec) -> TaffyBuilt {
  fn go(tree: &mut TaffyTree<()>, spec: &Spec, leaves: &mut Vec<NodeId>) -> NodeId {
    match spec {
      Spec::Leaf { .. } => {
        let id = tree.new_leaf(taffy_style(spec)).unwrap();
        leaves.push(id);
        id
      }
      Spec::Box { children, .. } => {
        let kids: Vec<NodeId> = children.iter().map(|c| go(tree, c, leaves)).collect();
        tree.new_with_children(taffy_style(spec), &kids).unwrap()
      }
    }
  }
  let mut tree = TaffyTree::with_capacity(count(spec));
  // lurq lays out in unrounded logical pixels.
  tree.disable_rounding();
  let mut leaves = Vec::new();
  let inner = go(&mut tree, spec, &mut leaves);
  // lurq's root takes the viewport's tight constraints.
  let mut root_style = taffy_style(spec);
  root_style.size = Size {
    width: length(VIEW_W),
    height: length(VIEW_H),
  };
  tree.set_style(inner, root_style).unwrap();
  TaffyBuilt {
    tree,
    root: inner,
    leaves,
  }
}

pub fn taffy_compute(built: &mut TaffyBuilt) {
  built.tree.compute_layout(built.root, Size::MAX_CONTENT).unwrap();
}

pub fn taffy_set_root_width(built: &mut TaffyBuilt, width: f32) {
  let mut style = built.tree.style(built.root).unwrap().clone();
  style.size.width = length(width);
  built.tree.set_style(built.root, style).unwrap();
}

pub fn taffy_leaf_positions(tree: &TaffyTree<()>, node: NodeId, x: f32, y: f32, out: &mut Vec<(f32, f32, f32, f32)>) {
  let layout = tree.layout(node).unwrap();
  let (x, y) = (x + layout.location.x, y + layout.location.y);
  let children = tree.children(node).unwrap();
  if children.is_empty() {
    out.push((x, y, layout.size.width, layout.size.height));
  }
  for child in children {
    taffy_leaf_positions(tree, child, x, y, out);
  }
}
