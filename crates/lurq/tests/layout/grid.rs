//! A grid lays its children out in the cells of columns and rows.

use lurq::{
  app::Tree,
  components::{Column, Grid, Popup, Rect, Row, Spacer, Text},
  core::{ElementRef, Signal},
  layout::{Alignment, Constraints, Size, grid::GridTrack, layout_result::LayoutResult},
};

use super::PassLayoutExt;

/// The box of child `index`: x, y, width, height.
fn cell(result: &LayoutResult, index: usize) -> (f32, f32, f32, f32) {
  let child = &result.children[index];
  (
    child.offset.x,
    child.offset.y,
    child.result.size.width,
    child.result.size.height,
  )
}

fn laid_out(root: impl Into<lurq::node::Element>, constraints: Constraints) -> LayoutResult {
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.pass_layout(constraints).unwrap()
}

/// An item with no size of its own, whose content is `width` by `height`: it takes the size of its cell
/// where it is stretched.
fn item(width: f32, height: f32) -> Column {
  Column::new().child(Rect::new(width, height))
}

fn loose(width: f32, height: f32) -> Constraints {
  Constraints::loose(Size::new(width, height))
}

#[test]
fn columns_of_pixels_shares_and_content_divide_the_width() {
  let grid = Grid::new()
    .columns([GridTrack::Px(40.0), GridTrack::Fr(1.0), GridTrack::Auto])
    .child(item(10.0, 20.0))
    .child(item(10.0, 20.0))
    .child(item(60.0, 20.0));
  let result = laid_out(grid, loose(300.0, 200.0));

  assert_eq!(result.size, Size::new(300.0, 20.0), "it fills the width it shares out");
  assert_eq!(cell(&result, 0), (0.0, 0.0, 40.0, 20.0), "stretched to its column");
  assert_eq!(cell(&result, 1), (40.0, 0.0, 200.0, 20.0));
  assert_eq!(cell(&result, 2), (240.0, 0.0, 60.0, 20.0));
}

#[test]
fn items_fill_rows_as_tall_as_their_tallest_item() {
  let grid = Grid::with_columns(2)
    .child(item(10.0, 20.0))
    .child(item(10.0, 50.0))
    .child(item(10.0, 30.0));
  let result = laid_out(grid, loose(200.0, 400.0));

  assert_eq!(result.size, Size::new(200.0, 80.0));
  assert_eq!(cell(&result, 0), (0.0, 0.0, 100.0, 50.0), "stretched to its row");
  assert_eq!(cell(&result, 1), (100.0, 0.0, 100.0, 50.0));
  assert_eq!(cell(&result, 2), (0.0, 50.0, 100.0, 30.0));
}

#[test]
fn gaps_are_between_tracks_and_not_around_them() {
  let grid = Grid::with_columns(2)
    .column_gap(10.0)
    .row_gap(6.0)
    .child(item(10.0, 20.0))
    .child(item(10.0, 20.0))
    .child(item(10.0, 20.0));
  let result = laid_out(grid, loose(210.0, 400.0));

  assert_eq!(result.size, Size::new(210.0, 46.0));
  assert_eq!(cell(&result, 1), (110.0, 0.0, 100.0, 20.0));
  assert_eq!(cell(&result, 2), (0.0, 26.0, 100.0, 20.0));
}

#[test]
fn an_item_spans_columns_and_rows_with_the_gaps_between_them() {
  let grid = Grid::with_columns(3)
    .gap(10.0)
    .child(item(10.0, 20.0).column_span(2))
    .child(item(10.0, 20.0).row_span(2))
    .child(item(10.0, 30.0))
    .child(item(10.0, 30.0));
  let result = laid_out(grid, loose(320.0, 400.0));

  assert_eq!(cell(&result, 0), (0.0, 0.0, 210.0, 20.0));
  assert_eq!(
    cell(&result, 1),
    (220.0, 0.0, 100.0, 60.0),
    "two rows and the gap between them"
  );
  assert_eq!(cell(&result, 2), (0.0, 30.0, 100.0, 30.0));
  assert_eq!(cell(&result, 3), (110.0, 30.0, 100.0, 30.0));
}

#[test]
fn an_item_can_name_its_cell() {
  let grid = Grid::with_columns(3)
    .child(item(10.0, 20.0).grid_column(2))
    .child(item(10.0, 20.0).grid_column(1).grid_row(2))
    .child(item(10.0, 20.0));
  let result = laid_out(grid, loose(300.0, 400.0));

  assert_eq!(cell(&result, 0), (200.0, 0.0, 100.0, 20.0));
  assert_eq!(cell(&result, 1).0, 100.0);
  assert_eq!(cell(&result, 1).1, 40.0, "the third row, under two rows of 20");
  assert_eq!(
    cell(&result, 2),
    (0.0, 20.0, 100.0, 20.0),
    "the flow goes on after the first"
  );
}

#[test]
fn items_that_are_not_stretched_keep_their_size_in_their_cell() {
  let grid = Grid::with_columns(2)
    .align_items(Alignment::Center)
    .justify_items(Alignment::End)
    .child(Rect::new(30.0, 20.0))
    .child(Rect::new(10.0, 60.0));
  let result = laid_out(grid, loose(200.0, 400.0));

  assert_eq!(cell(&result, 0), (70.0, 20.0, 30.0, 20.0));
  assert_eq!(cell(&result, 1), (190.0, 0.0, 10.0, 60.0));
}

#[test]
fn rows_of_a_grid_given_its_height_share_it() {
  let grid = Grid::new()
    .columns([GridTrack::Fr(1.0)])
    .rows([GridTrack::Px(30.0), GridTrack::Fr(1.0), GridTrack::Fr(3.0)])
    .height(230.0)
    .child(item(10.0, 5.0))
    .child(item(10.0, 5.0))
    .child(item(10.0, 5.0));
  let result = laid_out(grid, loose(100.0, 400.0));

  assert_eq!(result.size, Size::new(100.0, 230.0));
  assert_eq!(cell(&result, 0), (0.0, 0.0, 100.0, 30.0));
  assert_eq!(cell(&result, 1), (0.0, 30.0, 100.0, 50.0));
  assert_eq!(cell(&result, 2), (0.0, 80.0, 100.0, 150.0));
}

#[test]
fn rows_of_a_grid_as_tall_as_its_content_are_as_tall_as_theirs() {
  let grid = Grid::new()
    .columns([GridTrack::Fr(1.0)])
    .rows([GridTrack::Fr(1.0), GridTrack::Fr(1.0)])
    .child(item(10.0, 20.0))
    .child(item(10.0, 50.0));
  let result = laid_out(grid, loose(100.0, 400.0));

  // Equal shares, each as tall as the one that needs the most.
  assert_eq!(result.size, Size::new(100.0, 100.0));
  assert_eq!(cell(&result, 1), (0.0, 50.0, 100.0, 50.0));
}

#[test]
fn a_grid_in_unbounded_room_is_as_wide_as_its_columns() {
  // A row gives a child that does not flex all the width it asks for.
  let grid = Grid::new()
    .columns([GridTrack::Px(40.0), GridTrack::Auto, GridTrack::Fr(1.0)])
    .child(item(10.0, 20.0))
    .child(item(25.0, 20.0))
    .child(item(70.0, 20.0));
  let result = laid_out(Row::new().child(grid), loose(600.0, 200.0));

  let grid = &result.children[0].result;
  assert_eq!(grid.size, Size::new(135.0, 20.0));
  assert_eq!(cell(grid, 2), (65.0, 0.0, 70.0, 20.0));
}

#[test]
fn text_in_a_column_wraps_at_the_columns_width_and_sizes_its_row() {
  let words = "one two three four five six seven eight nine ten eleven twelve";
  let grid = Grid::new()
    .columns([GridTrack::Px(80.0), GridTrack::Fr(1.0)])
    .child(Text::new(words))
    .child(item(10.0, 10.0));
  let narrow = laid_out(grid, loose(400.0, 800.0));
  let single = laid_out(Text::new(words), loose(4000.0, 800.0));

  let text = cell(&narrow, 0);
  assert_eq!(text.2, 80.0);
  assert!(
    text.3 > single.size.height * 2.0,
    "wrapped over several lines: {text:?}"
  );
  assert_eq!(cell(&narrow, 1).3, text.3, "the row is as tall as the text");
}

#[test]
fn padding_is_around_the_tracks() {
  let grid = Grid::with_columns(2)
    .padding(10.0)
    .child(item(10.0, 20.0))
    .child(item(10.0, 20.0));
  let result = laid_out(grid, loose(220.0, 400.0));

  assert_eq!(result.size, Size::new(220.0, 40.0));
  assert_eq!(cell(&result, 0), (10.0, 10.0, 100.0, 20.0));
  assert_eq!(cell(&result, 1), (110.0, 10.0, 100.0, 20.0));
}

#[test]
fn an_overlay_declared_in_a_grid_takes_no_cell() {
  let anchor = ElementRef::new();
  let grid = Grid::with_columns(2)
    .child(item(10.0, 20.0).ref_element(anchor.clone()))
    .child(Popup::new(anchor, Rect::new(50.0, 50.0)).open_when(false))
    .child(item(10.0, 20.0));
  let result = laid_out(grid, loose(200.0, 400.0));

  assert_eq!(result.children.len(), 3);
  assert_eq!(
    cell(&result, 2),
    (100.0, 0.0, 100.0, 20.0),
    "the cell after the first item's"
  );
}

#[test]
fn an_absolutely_positioned_child_is_placed_over_the_grid() {
  let grid = Grid::with_columns(2)
    .child(item(10.0, 20.0))
    .child(Spacer::new().absolute(5.0, 7.0, 30.0, 12.0))
    .child(item(10.0, 20.0));
  let result = laid_out(grid, loose(200.0, 400.0));

  assert_eq!(cell(&result, 1), (5.0, 7.0, 30.0, 12.0));
  assert_eq!(cell(&result, 2), (100.0, 0.0, 100.0, 20.0));
  assert_eq!(result.size, Size::new(200.0, 20.0));
}

#[test]
fn an_item_that_changes_size_resizes_its_track() {
  let tall = Signal::new(false);
  struct Page;
  #[derive(Clone, lurq::DevtoolsInspectable)]
  struct Props {
    #[devtools_ignore]
    tall: Signal<bool>,
  }
  impl PartialEq for Props {
    fn eq(&self, _other: &Self) -> bool {
      true
    }
  }
  impl lurq::app::component::Component for Page {
    type Props = Props;

    fn create(_ctx: &mut lurq::app::ctx::Ctx) -> Self {
      Self
    }

    fn render(&self, ctx: &mut lurq::app::ctx::Ctx) -> impl Into<lurq::node::Element> {
      let tall = ctx.props::<Props>().tall.get();
      Column::new().child(
        Grid::with_columns(2)
          .child(Column::new().child(Rect::new(10.0, if tall { 70.0 } else { 20.0 })))
          .child(item(10.0, 30.0))
          .child(item(10.0, 10.0)),
      )
    }
  }
  let mut app = lurq::app::App::new();
  let mut tree = Tree::new();
  tree.mount_root::<Page>(&mut app, Props { tall: tall.clone() });
  let first = tree.pass_layout(loose(200.0, 400.0)).unwrap();
  let grid = &first.children[0].result;
  assert_eq!(cell(grid, 1).3, 30.0);
  assert_eq!(cell(grid, 2).1, 30.0);

  tall.set(true);
  let second = tree.pass_layout(loose(200.0, 400.0)).unwrap();
  let grid = &second.children[0].result;
  assert_eq!(cell(grid, 0).3, 70.0);
  assert_eq!(cell(grid, 1).3, 70.0, "its neighbour is stretched to the taller row");
  assert_eq!(cell(grid, 2).1, 70.0, "and the next row moves down");
}

#[test]
fn an_item_with_a_size_of_its_own_keeps_it_in_a_larger_cell() {
  let grid = Grid::with_columns(2)
    .child(Rect::new(30.0, 20.0))
    .child(item(10.0, 60.0));
  let result = laid_out(grid, loose(200.0, 400.0));

  assert_eq!(cell(&result, 0), (0.0, 0.0, 30.0, 20.0), "at the start of its cell");
  assert_eq!(cell(&result, 1), (100.0, 0.0, 100.0, 60.0));
}
