use lurq::{
  app::{App, Tree},
  components::{Column, Rect, Row},
  layout::{Alignment, Size},
  node::Element,
};

// The debug-build component/rebuild guard lives in runtime/stack_depth.rs.
// This guard measures optimized layout frames: run with --release.
#[test]
#[cfg_attr(
  debug_assertions,
  ignore = "measures release frames; run cargo test -p lurq --release --test layout_stack_depth"
)]
fn deep_chain_fits_a_one_mebibyte_stack() {
  std::thread::Builder::new()
    .name("layout-stack-depth".into())
    .stack_size(1024 * 1024)
    .spawn(|| {
      let mut element: Element = Rect::new(10.0, 10.0).into();
      for i in 0..250 {
        element = if i % 2 == 0 {
          Row::with(0.0, Alignment::Start, [element]).padding(1.0).into()
        } else {
          Column::with(0.0, Alignment::Start, [element]).padding(1.0).into()
        };
      }
      let mut app = App::new();
      let mut tree = Tree::new();
      tree.resize(1200, 800);
      tree.set_root(element);
      let result = tree.bench_layout_only(&mut app, true).expect("root");
      let mut leaf = &result;
      for _ in 0..250 {
        assert_eq!(leaf.children.len(), 1);
        let child = &leaf.children[0];
        assert_eq!((child.offset.x, child.offset.y), (1.0, 1.0));
        leaf = &child.result;
      }
      assert_eq!(leaf.size, Size::new(10.0, 10.0));
      assert!(leaf.children.is_empty());
    })
    .expect("spawn the layout stack-depth thread")
    .join()
    .expect("the layout stack-depth thread finished");
}

/// The chain built the way a render function builds a tree, recursively,
/// each level's children mapped from the level below (vs_taffy's
/// `lurq_element`), and only built: the layout has the guard above. The
/// builders inlined into `build` (`Row::with`, `Column::with`, `Rect::new`)
/// held whole nodes (over 1 KiB each) in its frames, once per level of the
/// build: 2.4 to 3.5 KiB a level, about 300 to 420 levels (LUR-13). The
/// recursion holds no node now, a few hundred bytes a level.
#[test]
#[cfg_attr(
  debug_assertions,
  ignore = "measures release frames; run cargo test -p lurq --release --test layout_stack_depth"
)]
fn deep_chain_built_recursively_fits_a_one_mebibyte_stack() {
  enum Spec {
    Leaf,
    Box { row: bool, children: Vec<Spec> },
  }
  fn build(spec: &Spec) -> Element {
    match spec {
      Spec::Leaf => Rect::new(10.0, 10.0).into(),
      Spec::Box { row, children } => {
        let children = children.iter().map(build);
        if *row {
          Row::with(0.0, Alignment::Start, children).padding(1.0).into()
        } else {
          Column::with(0.0, Alignment::Start, children).padding(1.0).into()
        }
      }
    }
  }
  const DEPTH: usize = 1000;
  std::thread::Builder::new()
    .name("build-stack-depth".into())
    .stack_size(1024 * 1024)
    .spawn(|| {
      let mut spec = Spec::Leaf;
      for i in 0..DEPTH {
        spec = Spec::Box {
          row: i % 2 == 0,
          children: vec![spec],
        };
      }
      let mut tree = Tree::new();
      tree.resize(1200, 800);
      tree.set_root(build(&spec));
      // Dropping a deep tree or spec recurses as well; this guard is for the build.
      std::mem::forget((tree, spec));
    })
    .expect("spawn the build stack-depth thread")
    .join()
    .expect("the build stack-depth thread finished");
}
