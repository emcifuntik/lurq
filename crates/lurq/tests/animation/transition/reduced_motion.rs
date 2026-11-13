//! With motion reduced, a transition of where an element is or how large it
//! is ends at once; a fade still runs.

use std::time::{Duration, Instant};

use lurq::{
  animation::Transition,
  app::{App, Tree},
  components::{Column, Rect},
  layout::{Constraints, Size, quad::QuadContent},
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const FILL: Color = Color::new(240, 240, 240, 255);

struct Stage {
  app: App,
  tree: Tree,
  start: Instant,
}

impl Stage {
  fn new(reduced: Option<bool>, root: impl Into<Element>) -> Self {
    let mut stage = Self {
      app: App::new(),
      tree: Tree::new(),
      start: Instant::now(),
    };
    stage.app.theme().set_reduced_motion(reduced);
    stage.tree.set_root(root);
    stage
      .tree
      .set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
    stage.at(0);
    stage
  }

  fn at(&mut self, millis: u64) {
    self
      .tree
      .set_animation_clock(Some(self.start + Duration::from_millis(millis)));
    self.tree.request_redraw();
    self.tree.pass(&mut self.app, &TestSurface);
  }

  /// Width and opacity of the painted fill.
  fn fill(&self) -> (f32, f32) {
    self
      .tree
      .resolve_quads(self.tree.last_layout().unwrap())
      .iter()
      .find_map(|quad| match &quad.content {
        QuadContent::Rect { color, .. } if *color == FILL => Some((quad.width, quad.opacity)),
        _ => None,
      })
      .expect("the fill is painted")
  }
}

fn growing_and_fading() -> Column {
  Column::new().child(
    Rect::new(100.0, 40.0)
      .background(FILL)
      .transition(Transition::all().duration_ms(1000).linear())
      .hovered(|style| style.width(200.0).opacity(0.0)),
  )
}

#[test]
fn a_test_tree_does_not_reduce_motion_whatever_the_machine_says() {
  assert!(!App::new().theme().reduced_motion());
}

#[test]
fn a_size_transition_runs_while_motion_is_not_reduced() {
  let mut stage = Stage::new(None, growing_and_fading());

  stage.tree.mouse_move(50.0, 20.0);
  stage.at(0);
  stage.at(500);

  assert_eq!(stage.fill(), (150.0, 0.5));
}

#[test]
fn with_motion_reduced_the_size_is_there_at_once_and_the_fade_still_runs() {
  let mut stage = Stage::new(Some(true), growing_and_fading());

  stage.tree.mouse_move(50.0, 20.0);
  stage.at(0);
  assert_eq!(
    stage.fill(),
    (200.0, 1.0),
    "the size does not travel; the fade has not started"
  );

  stage.at(500);
  assert_eq!(stage.fill(), (200.0, 0.5), "the fade is on its way");
}

#[test]
fn reducing_motion_while_the_app_runs_takes_effect_on_the_next_change() {
  let mut stage = Stage::new(None, growing_and_fading());
  stage.app.theme().set_reduced_motion(Some(true));
  stage.at(0);

  stage.tree.mouse_move(50.0, 20.0);
  stage.at(0);

  assert_eq!(stage.fill().0, 200.0);
}
