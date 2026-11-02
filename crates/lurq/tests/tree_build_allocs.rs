use std::{
  alloc::{GlobalAlloc, Layout, System},
  cell::Cell,
  sync::Arc,
};

use lurq::{
  app::{App, Tree},
  components::{Column, Rect, Row},
  core::NodeId,
  layout::Alignment,
  node::{Element, ElementRef},
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
/// only the root's temporary `Vec`s come on top of one result per node (2.0
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
