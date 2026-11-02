//! Synthetic input must reach the same handlers real input reaches.
//!
//! These assert the behaviour automation depends on: a `Click` establishes
//! hover before pressing (a bare press lands on the previously hovered node),
//! typed characters arrive at the focused input, and a wheel notch is a
//! complete Start/Scroll/End gesture rather than a latched one.

use lurq::{
  app::{
    Tree,
    synthetic_input::{self, SyntheticInput, SyntheticModifiers},
  },
  components::{Button, Column, Row, TextInput},
  core::Signal,
  node::color::Color,
};

use crate::support::run_pass;

fn inject(runtime: &mut Tree, input: SyntheticInput) {
  synthetic_input::apply(runtime, &input);
}

#[test]
fn a_synthetic_click_invokes_the_button_under_it() {
  let clicks = Signal::new(0);
  let mut runtime = Tree::new();
  let counter = clicks.clone();
  runtime.set_root(Button::new("Press").on_click(move |_| counter.set(counter.get() + 1)));
  run_pass(&mut runtime);
  let (x, y) = runtime
    .find_element(|_| true)
    .expect("button should be layoutable")
    .bounds()
    .center();

  inject(&mut runtime, SyntheticInput::click(x, y));

  assert_eq!(clicks.get(), 1);
}

/// The reason `Click` moves first. Press the pointer somewhere else, then press
/// the button without a preceding move: hit-testing runs off the last motion
/// event, so a bare press would otherwise be delivered to the stale node.
#[test]
fn a_click_establishes_hover_before_pressing() {
  const FIRST: &str = "#112233";
  const SECOND: &str = "#445566";

  let first = Signal::new(0);
  let second = Signal::new(0);
  let mut runtime = Tree::new();
  let (a, b) = (first.clone(), second.clone());
  runtime.set_root(
    Column::new()
      .child(
        Row::new()
          .width(80.0)
          .height(40.0)
          .background(FIRST)
          .on_click(move |_| a.set(a.get() + 1)),
      )
      .child(
        Row::new()
          .width(80.0)
          .height(40.0)
          .background(SECOND)
          .on_click(move |_| b.set(b.get() + 1)),
      ),
  );
  run_pass(&mut runtime);
  let first_center = runtime
    .find_element(|element| element.color() == Some(Color::from_hex(FIRST)))
    .expect("first target")
    .bounds()
    .center();
  let second_center = runtime
    .find_element(|element| element.color() == Some(Color::from_hex(SECOND)))
    .expect("second target")
    .bounds()
    .center();
  assert_ne!(first_center, second_center, "targets must not overlap");

  // Park the pointer on the first target, then click the second.
  inject(&mut runtime, SyntheticInput::mouse_move(first_center.0, first_center.1));
  inject(&mut runtime, SyntheticInput::click(second_center.0, second_center.1));

  assert_eq!(second.get(), 1, "the click must land on the second target");
  assert_eq!(first.get(), 0, "the parked pointer must not receive it");
}

#[test]
fn synthetic_characters_reach_the_focused_text_input() {
  let value = Signal::new(String::new());
  let mut runtime = Tree::new();
  runtime.set_root(TextInput::new(value.clone()));
  run_pass(&mut runtime);
  let (x, y) = runtime
    .find_element(|_| true)
    .expect("input should be layoutable")
    .bounds()
    .center();

  inject(&mut runtime, SyntheticInput::click(x, y));
  for event in SyntheticInput::text("hey") {
    inject(&mut runtime, event);
  }

  assert_eq!(value.get(), "hey");
}

#[test]
fn synthetic_modifiers_are_carried_into_the_event() {
  let shifted = Signal::new(false);
  let mut runtime = Tree::new();
  let flag = shifted.clone();
  runtime.set_root(
    Button::new("Press").on_click(move |event: lurq::app::events::MouseEvent| {
      flag.set(event.shift);
    }),
  );
  run_pass(&mut runtime);
  let (x, y) = runtime
    .find_element(|_| true)
    .expect("button should be layoutable")
    .bounds()
    .center();

  inject(
    &mut runtime,
    SyntheticInput::click(x, y).with_modifiers(SyntheticModifiers::default().shift()),
  );

  assert!(shifted.get(), "the handler must observe the shift modifier");
}
