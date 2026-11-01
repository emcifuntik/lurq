use lurq::{
  app::{App, Tree},
  components::Rect,
  layout::layout_result::ChildLayout,
};

#[test]
fn children_keep_the_vec_api() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(100, 100);
  tree.set_root(Rect::new(10.0, 10.0));
  tree.pass_headless(&mut app);
  let mut result = tree.last_layout().unwrap().clone();

  result.children = Vec::<ChildLayout>::with_capacity(2);
  let mut children = vec![ChildLayout {
    offset: Default::default(),
    result: result.clone().into(),
  }];
  result.children.append(&mut children);
  result.children.reserve(4);
  result.children.retain(|child| child.result.size.width > 0.0);
  assert_eq!(result.children.drain(..).count(), 1);
  assert!(result.children.is_empty());
}
