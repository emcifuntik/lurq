//! Handlers can tell lurq's synthetic input from OS input through `input_source`.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    Tree,
    events::{
      InputSource, KeyboardEvent, MouseButton, MouseEvent, input_source, is_synthetic_input, with_synthetic_input,
    },
    synthetic_input::{SyntheticInput, apply},
  },
  components::Rect,
};

use crate::support::{pointer_click, run_pass};

type Seen = Arc<Mutex<Vec<InputSource>>>;

fn recording_tree(seen: &Seen) -> Tree {
  let clicks = seen.clone();
  let keys = seen.clone();
  let mut tree = Tree::new();
  tree.set_root(
    Rect::new(100.0, 100.0)
      .focusable(true)
      .on_click(move |_: MouseEvent| clicks.lock().unwrap().push(input_source()))
      .on_key_down(move |_: KeyboardEvent| keys.lock().unwrap().push(input_source())),
  );
  run_pass(&mut tree);
  tree
}

#[test]
fn os_input_reaches_handlers_as_os_input() {
  let seen = Seen::default();
  let mut tree = recording_tree(&seen);
  pointer_click(&mut tree, 10.0, 10.0, MouseButton::Left);
  tree.key_down("a".into(), "KeyA".into(), false, false, false);
  assert_eq!(*seen.lock().unwrap(), [InputSource::Os, InputSource::Os]);
}

#[test]
fn applied_synthetic_input_reaches_handlers_as_synthetic() {
  let seen = Seen::default();
  let mut tree = recording_tree(&seen);
  apply(&mut tree, &SyntheticInput::click(10.0, 10.0));
  apply(&mut tree, &SyntheticInput::key_down("a"));
  assert_eq!(*seen.lock().unwrap(), [InputSource::Synthetic, InputSource::Synthetic]);
  assert!(!is_synthetic_input(), "the mark ends with the delivery");

  pointer_click(&mut tree, 10.0, 10.0, MouseButton::Left);
  assert_eq!(seen.lock().unwrap().last(), Some(&InputSource::Os));
}

#[test]
fn with_synthetic_input_marks_direct_tree_input_and_restores_the_source() {
  let seen = Seen::default();
  let mut tree = recording_tree(&seen);
  with_synthetic_input(|| {
    with_synthetic_input(|| pointer_click(&mut tree, 10.0, 10.0, MouseButton::Left));
    assert!(is_synthetic_input(), "the inner call restores the outer mark");
  });
  assert_eq!(*seen.lock().unwrap(), [InputSource::Synthetic]);
  assert_eq!(input_source(), InputSource::Os);
}
