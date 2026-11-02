use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{Tree, events::MouseEvent},
  components::{Button, Column, Rect, Stack},
};

use crate::support::run_pass;

#[test]
fn click_fires_on_click_at_bounds_center_without_hit_testing() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let observed = clicks.clone();
  let event_position = Arc::new(Mutex::new(None));
  let observed_position = event_position.clone();
  let mut tree = Tree::new();
  // The target is fully occluded by a sibling stacked on top — a real
  // pointer click would hit the cover, but DOM-style `click()` does not
  // hit-test.
  tree.set_root(
    Stack::new()
      .child(
        Rect::new(40.0, 20.0)
          .id("target")
          .background("#22c55e")
          .on_click(move |event: MouseEvent| {
            observed.fetch_add(1, Ordering::Relaxed);
            *observed_position.lock().unwrap() = Some((event.x, event.y));
          }),
      )
      .child(Rect::new(200.0, 200.0).background("#111111")),
  );
  run_pass(&mut tree);

  tree.get_element_by_id_mut("target").unwrap().click();

  assert_eq!(clicks.load(Ordering::Relaxed), 1);
  assert_eq!(*event_position.lock().unwrap(), Some((20.0, 10.0)));
}

#[test]
fn click_on_button_focuses_it_and_fires_focus_handlers() {
  let focused = Arc::new(AtomicUsize::new(0));
  let observed = focused.clone();
  let mut tree = Tree::new();
  tree.set_root(
    Column::new().child(
      Button::new("Save")
        .id("save")
        .on_focus(move || {
          observed.fetch_add(1, Ordering::Relaxed);
        })
        .on_click(|_event: MouseEvent| {}),
    ),
  );
  run_pass(&mut tree);

  tree.get_element_by_id_mut("save").unwrap().click();

  assert_eq!(focused.load(Ordering::Relaxed), 1);
}

#[test]
fn click_works_before_layout_with_zeroed_coordinates() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let observed = clicks.clone();
  let mut tree = Tree::new();
  tree.set_root(
    Column::new().child(Rect::new(10.0, 10.0).id("target").on_click(move |_event: MouseEvent| {
      observed.fetch_add(1, Ordering::Relaxed);
    })),
  );

  tree.get_element_by_id_mut("target").unwrap().click();
  assert_eq!(clicks.load(Ordering::Relaxed), 1);
}
