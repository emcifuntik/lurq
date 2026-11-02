use lurq::{
  canvas::{CanvasItem, CanvasItemShape},
  components::{Canvas, Column, ScrollVertical},
  layout::layout_kind::ScrollState,
  node::transform::Transform2D,
};

use super::{setup, support};

fn assert_bounds(actual: Option<(f32, f32, f32, f32)>, expected: (f32, f32, f32, f32)) {
  let actual = actual.expect("item has window bounds");
  let close = |a: f32, b: f32| (a - b).abs() < 0.01;
  assert!(
    close(actual.0, expected.0)
      && close(actual.1, expected.1)
      && close(actual.2, expected.2)
      && close(actual.3, expected.3),
    "{actual:?} != {expected:?}"
  );
}

#[test]
fn item_bounds_follow_padding_ancestor_transforms_and_scrolling() {
  let (mut app, mut tree, r) = setup(100.0, 80.0);
  let scroll = ScrollState::new();
  tree.set_root(
    ScrollVertical::new(
      Column::new()
        .padding(10.0)
        .transform(Transform2D::translate(20.0, 30.0))
        .child(
          Canvas::new()
            .software()
            .ref_element(r.clone())
            .width(100.0)
            .height(800.0)
            .padding(5.0),
        ),
    )
    .with_scroll_state(scroll.clone())
    .height(200.0),
  );
  tree.pass(&mut app, &support::TestSurface);
  let canvas = r.as_canvas().unwrap();
  canvas.set_items([
    CanvasItem::rect("mon", "bar", 0.0, 0.0, 10.0, 20.0),
    CanvasItem::point("peak", "point", 40.0, 300.0, 4.0),
  ]);
  assert_bounds(canvas.item_window_bounds("mon"), (35.0, 45.0, 10.0, 20.0));
  assert_bounds(canvas.item_window_bounds("peak"), (71.0, 341.0, 8.0, 8.0));
  assert!(canvas.item_window_bounds("missing").is_none());

  scroll.set_scroll(0.0, 250.0);
  tree.request_redraw();
  tree.pass(&mut app, &support::TestSurface);
  assert_bounds(canvas.item_window_bounds("peak"), (71.0, 91.0, 8.0, 8.0));
  // The drawing transform is not part of item coordinates.
  canvas.context_2d().scale(3.0, 3.0);
  assert_bounds(canvas.item_window_bounds("peak"), (71.0, 91.0, 8.0, 8.0));
}

#[test]
fn items_are_replaced_as_a_set_and_dropped_with_the_pixels_on_resize() {
  let (mut app, mut tree, r) = setup(64.0, 64.0);
  let canvas = r.as_canvas().unwrap();
  canvas.set_items([CanvasItem::rect("a", "bar", 0.0, 0.0, 4.0, 4.0).label("A").value("1")]);
  canvas.set_items([
    CanvasItem::rect("b", "bar", 0.0, 0.0, 4.0, 4.0),
    CanvasItem::rect("c", "bar", 8.0, 0.0, 4.0, 4.0),
  ]);
  let ids: Vec<_> = canvas.items().iter().map(|item| item.id.clone()).collect();
  assert_eq!(ids, ["b", "c"]);

  // A display-scale change keeps the pixels, so it keeps the items.
  tree.set_scale_factor(2.0);
  tree.pass(&mut app, &support::TestSurface);
  assert_eq!(canvas.items().len(), 2);

  tree.set_root(Canvas::new().software().ref_element(r.clone()).width(96.0).height(64.0));
  tree.pass(&mut app, &support::TestSurface);
  assert_eq!(r.as_canvas().unwrap().surface_id(), canvas.surface_id());
  assert!(canvas.items().is_empty());
}

#[test]
fn item_at_prefers_the_last_registered_item_and_honours_shapes() {
  let (_app, _tree, r) = setup(64.0, 64.0);
  let canvas = r.as_canvas().unwrap();
  canvas.set_items([
    CanvasItem::rect("under", "bar", 0.0, 0.0, 40.0, 40.0),
    CanvasItem::rect("over", "bar", 30.0, 30.0, -10.0, -10.0),
    CanvasItem::point("dot", "point", 50.0, 50.0, 3.0),
    CanvasItem::rect("broken", "bar", f32::NAN, 0.0, 10.0, 10.0),
  ]);
  let hit = |x, y| canvas.item_at(x, y).map(|item| item.id);
  assert_eq!(hit(5.0, 5.0).as_deref(), Some("under"));
  assert_eq!(hit(25.0, 25.0).as_deref(), Some("over"));
  assert_eq!(hit(52.0, 52.0).as_deref(), Some("dot"));
  assert_eq!(hit(54.0, 54.0), None);
  assert_eq!(
    canvas.items()[2].shape,
    CanvasItemShape::Point {
      x: 50.0,
      y: 50.0,
      radius: 3.0
    }
  );
}

#[test]
fn items_sharing_an_id_keep_the_last_one_everywhere() {
  let (_app, _tree, r) = setup(64.0, 64.0);
  let canvas = r.as_canvas().unwrap();
  canvas.set_items([
    CanvasItem::rect("a", "bar", 0.0, 0.0, 10.0, 10.0).label("first"),
    CanvasItem::rect("b", "bar", 20.0, 0.0, 10.0, 10.0),
    CanvasItem::rect("a", "bar", 40.0, 0.0, 10.0, 10.0).label("second"),
  ]);
  let items = canvas.items();
  let kept: Vec<_> = items
    .iter()
    .map(|item| (item.id.as_str(), item.label.as_deref()))
    .collect();
  assert_eq!(kept, [("b", None), ("a", Some("second"))]);
  assert_eq!(canvas.item_at(5.0, 5.0), None);
  assert_eq!(canvas.item_at(45.0, 5.0).unwrap().label.as_deref(), Some("second"));
  assert_bounds(canvas.item_window_bounds("a"), (40.0, 0.0, 10.0, 10.0));
}

/// Counts warnings, to check how often duplicates are reported.
#[derive(Default)]
struct WarningCounter(std::sync::atomic::AtomicUsize);

impl tracing::Subscriber for WarningCounter {
  fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
    *metadata.level() == tracing::Level::WARN
  }
  fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
    tracing::span::Id::from_u64(1)
  }
  fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
  fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
  fn event(&self, event: &tracing::Event<'_>) {
    if event.metadata().target().starts_with("lurq::canvas") {
      self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
  }
  fn enter(&self, _: &tracing::span::Id) {}
  fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn duplicate_ids_are_reported_once_per_set_not_every_redraw() {
  let (_app, _tree, r) = setup(64.0, 64.0);
  let canvas = r.as_canvas().unwrap();
  let items = |second: &str| {
    [
      CanvasItem::rect("a", "bar", 0.0, 0.0, 4.0, 4.0),
      CanvasItem::rect(second, "bar", 8.0, 0.0, 4.0, 4.0),
      CanvasItem::rect("a", "bar", 16.0, 0.0, 4.0, 4.0),
    ]
  };
  let counter = std::sync::Arc::new(WarningCounter::default());
  let count = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
  // Another test may first hit the warning's callsite on its thread, without
  // this subscriber, and cache that nothing listens to it after this one is
  // set: rebuild the cache before each call whose warnings are counted.
  let set_items = |items: Vec<CanvasItem>| {
    tracing::callsite::rebuild_interest_cache();
    canvas.set_items(items);
  };
  tracing::subscriber::with_default(counter.clone(), || {
    for _ in 0..5 {
      set_items(items("b").into());
    }
    assert_eq!(count(), 1);
    set_items(items("a").into());
    assert_eq!(count(), 1, "the same duplicated ids in another layout");
    set_items(vec![
      CanvasItem::rect("b", "bar", 0.0, 0.0, 4.0, 4.0),
      CanvasItem::rect("b", "bar", 8.0, 0.0, 4.0, 4.0),
    ]);
    assert_eq!(count(), 2, "a different duplicate set is reported");
    set_items(vec![CanvasItem::rect("a", "bar", 0.0, 0.0, 4.0, 4.0)]);
    set_items(items("b").into());
    assert_eq!(count(), 3, "duplicates that come back are reported again");
  });
}
