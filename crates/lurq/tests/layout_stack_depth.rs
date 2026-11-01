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
