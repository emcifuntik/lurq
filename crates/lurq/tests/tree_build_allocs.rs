use std::{
  alloc::{GlobalAlloc, Layout, System},
  cell::Cell,
  sync::Arc,
};

use lurq::{
  app::{App, Tree},
  components::{Column, Rect, Row, Stack},
  core::NodeId,
  layout::{Alignment, StackAlignment},
  node::{Element, ElementRef, dimension::Dimension},
};

struct Counting;

thread_local! {
  static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation() {
  let _ = ALLOCS.try_with(|count| {
    if let Some(value) = count.get() {
      count.set(Some(value + 1));
    }
  });
}

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    record_allocation();
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    record_allocation();
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn chain(names: &[Arc<str>], named: bool) -> Element {
  let mut element: Element = Rect::new(10.0, 10.0).into();
  for (index, name) in names.iter().enumerate() {
    element = if named { element.id(name.clone()) } else { element.key(name.clone()) };
    element = if index % 2 == 0 {
      Row::with(0.0, Alignment::Start, [element]).into()
    } else {
      Column::with(0.0, Alignment::Start, [element]).into()
    };
  }
  element
}

fn collect_ids(node: ElementRef<'_>, ids: &mut Vec<NodeId>) {
  ids.push(node.node_id());
  for child in node.children() {
    collect_ids(child, ids);
  }
}

#[test]
fn deep_set_root_preserves_identified_nodes_without_matching_allocations() {
  for depth in [16, 64] {
    let names: Vec<Arc<str>> = (0..depth).map(|index| Arc::from(format!("node-{index}"))).collect();
    for named in [false, true] {
      let mut tree = Tree::new();
      tree.set_root(chain(&names, named));
      let mut before = Vec::new();
      collect_ids(tree.root().unwrap(), &mut before);
      let replacement = chain(&names, named);

      ALLOCS.set(Some(0));
      tree.set_root(replacement);
      let allocations = ALLOCS.replace(None).unwrap();

      let mut after = Vec::new();
      collect_ids(tree.root().unwrap(), &mut after);
      assert_eq!(after, before);
      assert_eq!(allocations, 0, "depth={depth}, named={named}");
    }
  }
}

/// A first layout stores one cached result per node, inline. On a flat column
/// only the root's children `Vec` comes on top of one result per node (2.0
/// allocations per node when a box held each node's cached results).
#[test]
fn first_layout_allocates_no_cache_box_per_node() {
  let mut app = App::new();
  let nodes = 1_001;
  for _ in 0..2 {
    let mut tree = Tree::new();
    tree.resize(1200, 800);
    tree.set_root(Column::with(0.0, Alignment::Start, (1..nodes).map(|_| Element::from(Rect::new(100.0, 20.0)))));

    ALLOCS.set(Some(0));
    tree.bench_layout_only_shared(&mut app, false);
    let allocations = ALLOCS.replace(None).unwrap();

    assert!(allocations <= nodes + 16, "{allocations} allocations over {nodes} nodes");
  }
}

fn rects(count: usize) -> Vec<Element> {
  (0..count).map(|_| Rect::new(40.0, 20.0).into()).collect()
}

fn nested(depth: usize, items: usize, row: bool) -> Element {
  if depth == 0 {
    return Rect::new(40.0, 20.0).into();
  }
  let children = (0..items).map(|_| nested(depth - 1, items, !row));
  if row {
    Row::with(4.0, Alignment::Start, children).into()
  } else {
    Column::with(4.0, Alignment::Start, children).into()
  }
}

/// Every container shape, with its node and container counts.
fn container_trees() -> Vec<(&'static str, usize, usize, Element)> {
  let names: Vec<Arc<str>> = (0..64).map(|index| Arc::from(format!("node-{index}"))).collect();
  vec![
    ("nested", 341, 85, nested(4, 4, true)),
    ("chain", 65, 64, chain(&names, false)),
    (
      "growing",
      121,
      21,
      Column::with(
        0.0,
        Alignment::Start,
        (0..20).map(|_| {
          let growing = (0..5).map(|_| Element::from(Rect::new(Dimension::Auto, 20.0).flex(1.0)));
          Element::from(Row::with(4.0, Alignment::Start, growing))
        }),
      )
      .into(),
    ),
    (
      "wrapping",
      101,
      1,
      Row::with(4.0, Alignment::Start, rects(100)).wrap().into(),
    ),
    (
      "stacked",
      81,
      21,
      Column::with(
        0.0,
        Alignment::Start,
        (0..20).map(|_| Element::from(Stack::with(StackAlignment::Center, rects(3)))),
      )
      .into(),
    ),
  ]
}

/// A container allocates one `Vec`, its result's children, and one result:
/// the per-child data it works with while laying out lives in that `Vec`.
/// A `Vec` per kind of scratch data came to 6 per container, and a wrapping
/// line built one throwaway result per child on top.
#[test]
fn layout_allocates_one_vec_per_container() {
  let mut app = App::new();
  for (name, nodes, containers, root) in container_trees() {
    let mut tree = Tree::new();
    tree.resize(1200, 800);
    tree.set_root(root);
    let limit = nodes + containers + 16;

    ALLOCS.set(Some(0));
    tree.bench_layout_only_shared(&mut app, false);
    let first = ALLOCS.replace(None).unwrap();
    assert!(first <= limit, "{name}: first layout: {first} allocations, limit {limit}");

    ALLOCS.set(Some(0));
    tree.bench_layout_only_shared(&mut app, true);
    let forced = ALLOCS.replace(None).unwrap();
    assert!(forced <= limit, "{name}: forced layout: {forced} allocations, limit {limit}");
  }
}
