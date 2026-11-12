//! `.shrink_drop_below(size)`: a child shrinks to `size`, then drops when its
//! order still cannot absorb the overflow, before the next order shrinks.

use lurq::{
  app::{App, Tree},
  components::{Rect, Row},
  layout::layout_kind::ShrinkLimit,
};

use super::{
  COUNT, DETAIL, LINE_HEIGHT, SPACING, WIDE, assert_close, child_widths, drawn_texts, label, layout_at, layout_once,
  tight,
};

const FLOOR: f32 = 100.0;
const STOP: f32 = 30.0;

/// The approval request trims to 100 px, then drops; the count gives way
/// after it.
fn chip() -> Row {
  Row::new()
    .spacing(SPACING)
    .child(label(DETAIL).flex_shrink(1.0).shrink_order(1).shrink_drop_below(FLOOR))
    .child(label(COUNT).flex_shrink(1.0).shrink_order(2))
    .child(Rect::new(STOP, 10.0))
}

/// Natural widths of the request, the count and the stop block.
fn natural() -> Vec<f32> {
  let (_, layout) = layout_once(Row::new().child(chip()), WIDE, LINE_HEIGHT);
  child_widths(&layout.children[0].result)
}

fn line(widths: &[f32]) -> f32 {
  widths.iter().sum::<f32>() + SPACING * 2.0
}

#[test]
fn give_way_drop_below_trims_first() {
  let natural = natural();
  let (tree, layout) = layout_once(chip(), line(&natural) - 20.0, LINE_HEIGHT);
  let widths = child_widths(&layout);
  assert_close(widths[0], natural[0] - 20.0, "the request trims");
  assert_eq!(widths[1], natural[1], "the count keeps its width");
  let drawn = drawn_texts(&tree, &layout);
  assert!(drawn[0].ends_with('…'), "the request is ellipsized: {drawn:?}");
  assert_eq!(drawn[1], COUNT);
}

#[test]
fn give_way_drop_below_stops_at_its_size() {
  let natural = natural();
  let (_, layout) = layout_once(chip(), line(&natural) - (natural[0] - FLOOR), LINE_HEIGHT);
  assert_close(child_widths(&layout)[0], FLOOR, "the request trims to its size");
  assert!(!layout.children[0].result.is_dropped());
}

#[test]
fn give_way_drop_below_drops_once_its_order_has_no_room() {
  let natural = natural();
  let (tree, layout) = layout_once(chip(), line(&natural) - (natural[0] - FLOOR) - 1.0, LINE_HEIGHT);
  assert!(layout.children[0].result.is_dropped(), "the request drops");
  assert_eq!(child_widths(&layout)[1], natural[1], "the count keeps its width");
  assert_close(layout.children[1].offset.x, 0.0, "the count starts the line");
  assert_eq!(drawn_texts(&tree, &layout), [COUNT], "the request is not drawn");
}

#[test]
fn give_way_drop_below_then_the_next_order_shrinks() {
  let natural = natural();
  let without_request = natural[1] + SPACING + STOP;
  let (_, layout) = layout_once(chip(), without_request - 10.0, LINE_HEIGHT);
  assert!(layout.children[0].result.is_dropped());
  assert_close(child_widths(&layout)[1], natural[1] - 10.0, "the count gives way next");
}

#[test]
fn give_way_drop_below_drops_a_child_already_narrower_than_its_size() {
  let row = Row::new()
    .spacing(0.0)
    .child(Rect::new(80.0, 10.0).flex_shrink(1.0).shrink_drop_below(FLOOR))
    .child(Rect::new(100.0, 10.0));
  let (_, layout) = layout_once(row, 175.0, 10.0);
  assert!(layout.children[0].result.is_dropped(), "it has nothing to trim");
  assert_eq!(child_widths(&layout), [0.0, 100.0]);
}

#[test]
fn give_way_drop_below_shares_its_order_before_dropping() {
  let row = || {
    Row::new()
      .spacing(0.0)
      .child(Rect::new(200.0, 10.0).flex_shrink(1.0).shrink_drop_below(FLOOR))
      .child(Rect::new(100.0, 10.0).flex_shrink(1.0).min_width(60.0))
      .child(Rect::new(100.0, 10.0).flex_shrink(1.0).shrink_order(1))
  };
  // Order 0 can absorb 100 + 40: an overflow of 120 is shared (the second
  // stops at 60), 141 drops the first rect and frees the second again.
  let (_, shared) = layout_once(row(), 280.0, 10.0);
  assert_eq!(child_widths(&shared), [120.0, 60.0, 100.0]);
  let (_, dropped) = layout_once(row(), 259.0, 10.0);
  assert_eq!(child_widths(&dropped), [0.0, 100.0, 100.0]);
}

#[test]
fn give_way_drop_below_counts_as_gone_in_a_content_limit() {
  let item = Row::new()
    .spacing(SPACING)
    .child(Rect::new(14.0, 14.0))
    .child(label(DETAIL).flex_shrink(1.0).shrink_drop_below(FLOOR))
    .flex_shrink(1.0)
    .shrink_limit(ShrinkLimit::Content);
  let (_, layout) = layout_once(Row::new().child(item), 5.0, LINE_HEIGHT);
  let item = &layout.children[0].result;
  assert_eq!(item.size.width, 14.0);
  assert!(item.children[1].result.is_dropped());
}

#[test]
fn give_way_drop_below_restores_on_widen() {
  let natural = natural();
  let narrow = natural[1] + SPACING + STOP;
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(chip());
  let passes: Vec<(Vec<f32>, Vec<String>)> = [line(&natural), narrow, line(&natural), narrow]
    .into_iter()
    .map(|width| {
      let layout = layout_at(&mut tree, &mut app, tight(width, LINE_HEIGHT));
      (child_widths(&layout), drawn_texts(&tree, &layout))
    })
    .collect();
  assert_eq!(passes[0].1, [DETAIL, COUNT], "a wide chip draws both texts");
  assert_eq!(passes[1].0[0], 0.0, "the narrow chip drops the request");
  assert_eq!(passes[2], passes[0], "the wide chip is restored");
  assert_eq!(passes[3], passes[1], "the narrow chip drops the request again");
}

#[test]
fn give_way_drop_below_follows_a_resize_continuously_until_it_drops() {
  let natural = natural();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(chip());
  let start = line(&natural);
  let mut previous: Option<(bool, Vec<f32>)> = None;
  let mut drops = 0;
  for step in 0..=((start - 60.0) * 10.0) as usize {
    let width = start - step as f32 * 0.1;
    let layout = layout_at(&mut tree, &mut app, tight(width, LINE_HEIGHT));
    let dropped = layout.children[0].result.is_dropped();
    let widths = child_widths(&layout);
    if let Some((was_dropped, before)) = &previous {
      assert!(
        !was_dropped || dropped,
        "a dropped request stays dropped as the line narrows at {width}"
      );
      if *was_dropped == dropped {
        for (index, (now, then)) in widths.iter().zip(before).enumerate() {
          assert!(
            (now - then).abs() <= 1.0 + 0.01,
            "child {index} jumps from {then} to {now} at {width}"
          );
        }
      } else {
        drops += 1;
        // The line narrows a tenth of a pixel at a time: the step before the drop is within one of the size,
        // wherever the font's widths put the first step.
        assert!(
          before[0] <= FLOOR + 0.1 + 0.01,
          "the request reached its size before dropping: {before:?}"
        );
      }
    }
    previous = Some((dropped, widths));
  }
  assert_eq!(drops, 1, "the request drops exactly once");
}

#[test]
fn give_way_drop_below_a_negative_or_nan_size_counts_as_zero() {
  for size in [f32::NAN, -5.0, 0.0] {
    let row = Row::new()
      .spacing(0.0)
      .child(Rect::new(100.0, 10.0).flex_shrink(1.0).shrink_drop_below(size))
      .child(Rect::new(50.0, 10.0));
    let (_, layout) = layout_once(row, 60.0, 10.0);
    assert_eq!(child_widths(&layout), [10.0, 50.0], "size {size}");
  }
}
