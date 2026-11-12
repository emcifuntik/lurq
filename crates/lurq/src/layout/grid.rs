//! A grid: children placed in the cells of columns and rows, as CSS grid lays
//! out a subset of what it can.
//!
//! **Tracks.** A column or a row is a [`GridTrack`]: a length in pixels, a
//! share of the room left over (`Fr`), or as large as what is in it (`Auto`).
//! Tracks a template does not name are `Auto`: rows are added as the items
//! need them, and columns for an item placed past the last one.
//!
//! **Placement.** Items take the cells in their order, row after row, each
//! in the next cell that has room for its spans (`grid-auto-flow: row`). An
//! item can name its column, its row or both; one that names both is put
//! there whatever is there already.
//!
//! **Sizes.** Along an axis whose length is known, the `Fr` tracks share what
//! the others and the gaps leave, and `Auto` tracks that do not fit give way
//! in proportion. Along an axis whose length is the content's, an `Fr` track
//! is sized by content too, and all of them keep their proportions. An item
//! that spans several tracks widens the `Auto` ones among them when it does
//! not fit, unless an `Fr` track among them has a known share to take it.

use crate::{layout::Alignment, node::spacing_value::SpacingValue};

/// The size of a column or a row of a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrack {
  /// A length in logical pixels.
  Px(f32),
  /// A share of the room the other tracks and the gaps leave: two tracks of
  /// `Fr(1.0)` and `Fr(2.0)` split it one to two. Where the grid's length
  /// along the axis is its content's, as large as its content needs, in
  /// proportion with the other `Fr` tracks.
  Fr(f32),
  /// As large as the largest item in it.
  Auto,
}

impl From<f32> for GridTrack {
  fn from(pixels: f32) -> Self {
    Self::Px(pixels)
  }
}

/// What a grid lays its children out by.
#[derive(Clone, PartialEq)]
pub struct GridSpec {
  pub(crate) columns: Vec<GridTrack>,
  pub(crate) rows: Vec<GridTrack>,
  pub(crate) column_gap: SpacingValue,
  pub(crate) row_gap: SpacingValue,
  /// How an item sits in its cell from top to bottom.
  pub(crate) align: Alignment,
  /// And from side to side.
  pub(crate) justify: Alignment,
}

impl Default for GridSpec {
  fn default() -> Self {
    Self {
      columns: Vec::new(),
      rows: Vec::new(),
      column_gap: SpacingValue::from(0.0),
      row_gap: SpacingValue::from(0.0),
      align: Alignment::Stretch,
      justify: Alignment::Stretch,
    }
  }
}

/// Where a child of a grid asks to be: the column and the row it starts in,
/// each stored one past its index so that zero is "wherever there is room",
/// and how many of each it spans, zero and one both being one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GridCell {
  column: u16,
  row: u16,
  column_span: u16,
  row_span: u16,
}

impl GridCell {
  pub(crate) fn set_column(&mut self, index: usize) {
    self.column = one_past(index);
  }

  pub(crate) fn set_row(&mut self, index: usize) {
    self.row = one_past(index);
  }

  pub(crate) fn set_column_span(&mut self, span: usize) {
    self.column_span = span.min(usize::from(u16::MAX)) as u16;
  }

  pub(crate) fn set_row_span(&mut self, span: usize) {
    self.row_span = span.min(usize::from(u16::MAX)) as u16;
  }

  pub(crate) fn column(self) -> Option<usize> {
    self.column.checked_sub(1).map(usize::from)
  }

  pub(crate) fn row(self) -> Option<usize> {
    self.row.checked_sub(1).map(usize::from)
  }

  pub(crate) fn column_span(self) -> usize {
    usize::from(self.column_span.max(1))
  }

  pub(crate) fn row_span(self) -> usize {
    usize::from(self.row_span.max(1))
  }

  pub(crate) fn is_auto(self) -> bool {
    self == Self::default()
  }
}

fn one_past(index: usize) -> u16 {
  (index.min(usize::from(u16::MAX) - 1) + 1) as u16
}

/// The cells an item takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
  pub(crate) column: usize,
  pub(crate) row: usize,
  pub(crate) column_span: usize,
  pub(crate) row_span: usize,
}

/// Where each item of a grid is, and how many columns and rows that takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Placement {
  pub(crate) items: Vec<Placed>,
  pub(crate) columns: usize,
  pub(crate) rows: usize,
}

/// Which cells are taken, row by row; a row past the end is empty.
struct Taken {
  columns: usize,
  rows: Vec<Vec<bool>>,
}

impl Taken {
  fn is_free(&self, placed: Placed) -> bool {
    placed.column + placed.column_span <= self.columns
      && (placed.row..placed.row + placed.row_span).all(|row| {
        self.rows.get(row).is_none_or(|cells| {
          cells[placed.column..placed.column + placed.column_span]
            .iter()
            .all(|taken| !taken)
        })
      })
  }

  fn take(&mut self, placed: Placed) {
    let rows = placed.row + placed.row_span;
    if self.rows.len() < rows {
      self.rows.resize(rows, vec![false; self.columns]);
    }
    for row in placed.row..rows {
      for cell in &mut self.rows[row][placed.column..placed.column + placed.column_span] {
        *cell = true;
      }
    }
  }
}

/// Places `cells`, the items of a grid in their order, in a grid whose
/// template names `template_columns` columns and `template_rows` rows.
pub(crate) fn place_items(cells: &[GridCell], template_columns: usize, template_rows: usize) -> Placement {
  // An item placed past the template's last column adds the columns it needs.
  let columns = cells
    .iter()
    .filter_map(|cell| Some(cell.column()? + cell.column_span()))
    .fold(template_columns.max(1), usize::max);
  let mut taken = Taken {
    columns,
    rows: Vec::new(),
  };
  let mut cursor = (0, 0);
  let mut items = Vec::with_capacity(cells.len());
  for cell in cells {
    let column_span = cell.column_span().min(columns);
    let row_span = cell.row_span();
    let at = |row: usize, column: usize| Placed {
      column,
      row,
      column_span,
      row_span,
    };
    let placed = match (cell.column(), cell.row()) {
      (Some(column), Some(row)) => at(row, column.min(columns - column_span)),
      (Some(column), None) => {
        let column = column.min(columns - column_span);
        // The row the cursor is in, or the next one when the cursor is past this column.
        let mut row = if column < cursor.1 { cursor.0 + 1 } else { cursor.0 };
        while !taken.is_free(at(row, column)) {
          row += 1;
        }
        cursor = (row, column + column_span);
        at(row, column)
      }
      (None, Some(row)) => {
        let column = (0..=columns - column_span)
          .find(|column| taken.is_free(at(row, *column)))
          .unwrap_or(0);
        at(row, column)
      }
      (None, None) => {
        let (mut row, mut column) = cursor;
        loop {
          if column + column_span > columns {
            row += 1;
            column = 0;
          }
          if taken.is_free(at(row, column)) {
            break;
          }
          column += 1;
        }
        cursor = (row, column + column_span);
        at(row, column)
      }
    };
    taken.take(placed);
    items.push(placed);
  }
  Placement {
    items,
    columns,
    rows: taken.rows.len().max(template_rows),
  }
}

/// The sizes of the `count` tracks of one axis. `template` names the first
/// of them; the rest are `Auto`. `available` is the grid's length along the
/// axis where it is known. `contents` are the items: the track each starts
/// in, how many it spans, and how long it is along the axis.
pub(crate) fn size_tracks(
  template: &[GridTrack],
  count: usize,
  gap: f32,
  available: Option<f32>,
  contents: &[(usize, usize, f32)],
) -> Vec<f32> {
  let track = |index: usize| template.get(index).copied().unwrap_or(GridTrack::Auto);
  let share = |index: usize| match track(index) {
    GridTrack::Fr(share) if share.is_finite() && share > 0.0 => Some(share),
    _ => None,
  };
  // A track sized by what is in it: an `Auto` one, an `Fr` one where the length is the content's, and an
  // `Fr` one that has no share.
  let by_content = |index: usize| match track(index) {
    GridTrack::Px(_) => false,
    GridTrack::Auto => true,
    GridTrack::Fr(_) => available.is_none() || share(index).is_none(),
  };
  let mut sizes: Vec<f32> = (0..count)
    .map(|index| match track(index) {
      GridTrack::Px(pixels) if pixels.is_finite() => pixels.max(0.0),
      _ => 0.0,
    })
    .collect();

  for &(start, span, length) in contents {
    if span == 1 && start < count && by_content(start) {
      sizes[start] = sizes[start].max(length);
    }
  }
  // An item over several tracks widens the ones sized by content when it does not fit in them, the
  // narrower spans first. A track with a share of the room left takes the item in as it grows.
  let mut spanning: Vec<&(usize, usize, f32)> = contents.iter().filter(|(_, span, _)| *span > 1).collect();
  spanning.sort_by_key(|(_, span, _)| *span);
  for &&(start, span, length) in &spanning {
    let tracks = start..(start + span).min(count);
    if tracks.clone().any(|index| !by_content(index) && share(index).is_some()) {
      continue;
    }
    let growing = tracks.clone().filter(|index| by_content(*index)).count();
    let covered: f32 = tracks.clone().map(|index| sizes[index]).sum::<f32>() + gap * (tracks.len().max(1) - 1) as f32;
    if growing == 0 || length <= covered {
      continue;
    }
    let each = (length - covered) / growing as f32;
    for index in tracks.filter(|index| by_content(*index)) {
      sizes[index] += each;
    }
  }

  let gaps = gap * count.saturating_sub(1) as f32;
  match available {
    Some(available) => {
      let shares: f32 = (0..count).filter(|index| !by_content(*index)).filter_map(share).sum();
      let fixed: f32 = sizes.iter().sum::<f32>() + gaps;
      if shares > 0.0 {
        let free = (available - fixed).max(0.0);
        for index in (0..count).filter(|index| !by_content(*index)) {
          if let Some(share) = share(index) {
            sizes[index] = free * share / shares;
          }
        }
      }
      // What is sized by content gives way when the tracks do not fit.
      let over = fixed - available;
      let giving: f32 = (0..count)
        .filter(|index| by_content(*index))
        .map(|index| sizes[index])
        .sum();
      if over > 0.0 && giving > 0.0 {
        let scale = (giving - over).max(0.0) / giving;
        for index in (0..count).filter(|index| by_content(*index)) {
          sizes[index] *= scale;
        }
      }
    }
    None => {
      // The tracks with a share keep their proportions: each is as large as the one that needs the most
      // room per share.
      let unit = (0..count)
        .filter_map(|index| Some(sizes[index] / share(index)?))
        .fold(0.0, f32::max);
      for (index, size) in sizes.iter_mut().enumerate() {
        if let Some(share) = share(index) {
          *size = unit * share;
        }
      }
    }
  }
  sizes
}

#[cfg(test)]
mod tests {
  use super::*;

  fn cell(column: Option<usize>, row: Option<usize>, column_span: usize, row_span: usize) -> GridCell {
    let mut cell = GridCell::default();
    if let Some(column) = column {
      cell.set_column(column);
    }
    if let Some(row) = row {
      cell.set_row(row);
    }
    cell.set_column_span(column_span);
    cell.set_row_span(row_span);
    cell
  }

  fn auto() -> GridCell {
    GridCell::default()
  }

  fn places(placement: &Placement) -> Vec<(usize, usize)> {
    placement.items.iter().map(|item| (item.row, item.column)).collect()
  }

  #[test]
  fn items_take_the_cells_in_their_order_row_after_row() {
    let placement = place_items(&[auto(); 5], 3, 0);

    assert_eq!(places(&placement), [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]);
    assert_eq!((placement.columns, placement.rows), (3, 2));
  }

  #[test]
  fn an_item_that_spans_goes_to_the_next_row_when_its_row_has_no_room() {
    let placement = place_items(&[auto(), auto(), cell(None, None, 2, 1), auto()], 3, 0);

    assert_eq!(places(&placement), [(0, 0), (0, 1), (1, 0), (1, 2)]);
  }

  #[test]
  fn items_flow_around_one_that_spans_rows() {
    let placement = place_items(&[cell(None, None, 1, 2), auto(), auto(), auto()], 2, 0);

    assert_eq!(places(&placement), [(0, 0), (0, 1), (1, 1), (2, 0)]);
    assert_eq!(placement.rows, 3);
  }

  #[test]
  fn an_item_can_name_its_column_its_row_or_both() {
    let placement = place_items(
      &[
        auto(),
        cell(Some(2), None, 1, 1),
        cell(Some(0), None, 1, 1),
        cell(None, Some(3), 1, 1),
        cell(Some(1), Some(0), 1, 1),
      ],
      3,
      0,
    );

    assert_eq!(
      places(&placement),
      [
        (0, 0),
        (0, 2),
        (1, 0),
        (3, 0),
        // Named in full: it is put there, in the cell the flow left empty.
        (0, 1),
      ]
    );
    assert_eq!(placement.rows, 4);
  }

  #[test]
  fn a_column_past_the_template_adds_columns_and_a_span_wider_than_the_grid_is_cut() {
    let placement = place_items(&[cell(Some(3), None, 2, 1), cell(None, None, 9, 1)], 2, 0);

    assert_eq!(placement.columns, 5);
    assert_eq!(placement.items[0].column, 3);
    assert_eq!(placement.items[1].column_span, 5);
    assert_eq!(placement.items[1].row, 1);
  }

  #[test]
  fn a_template_of_rows_is_kept_when_the_items_need_fewer() {
    assert_eq!(place_items(&[auto()], 2, 3).rows, 3);
    assert_eq!(place_items(&[], 0, 0).columns, 1);
  }

  const PX_FR_AUTO: [GridTrack; 3] = [GridTrack::Px(40.0), GridTrack::Fr(1.0), GridTrack::Auto];

  #[test]
  fn shares_split_what_the_other_tracks_and_the_gaps_leave() {
    let sizes = size_tracks(&PX_FR_AUTO, 3, 10.0, Some(200.0), &[(2, 1, 30.0), (2, 1, 50.0)]);
    assert_eq!(sizes, [40.0, 90.0, 50.0]);

    let split = size_tracks(&[GridTrack::Fr(1.0), GridTrack::Fr(3.0)], 2, 0.0, Some(200.0), &[]);
    assert_eq!(split, [50.0, 150.0]);
  }

  #[test]
  fn tracks_past_the_template_are_as_large_as_their_content() {
    let sizes = size_tracks(
      &[GridTrack::Px(40.0)],
      3,
      0.0,
      Some(300.0),
      &[(1, 1, 25.0), (2, 1, 60.0)],
    );

    assert_eq!(sizes, [40.0, 25.0, 60.0]);
  }

  #[test]
  fn where_the_length_is_the_contents_shares_are_sized_by_content_in_proportion() {
    let tracks = [GridTrack::Fr(1.0), GridTrack::Fr(2.0)];
    let sizes = size_tracks(&tracks, 2, 0.0, None, &[(0, 1, 30.0), (1, 1, 40.0)]);

    // The first needs 30 per share, the second 20: both get 30 per share.
    assert_eq!(sizes, [30.0, 60.0]);
  }

  #[test]
  fn a_spanning_item_widens_the_tracks_sized_by_content() {
    let tracks = [GridTrack::Auto, GridTrack::Px(20.0), GridTrack::Auto];
    let sizes = size_tracks(&tracks, 3, 10.0, None, &[(0, 1, 30.0), (0, 3, 140.0)]);

    // 30 + 20 + 0 and two gaps cover 70 of 140: each of the two tracks takes half of the rest.
    assert_eq!(sizes, [65.0, 20.0, 35.0]);
  }

  #[test]
  fn a_share_among_the_tracks_an_item_spans_takes_the_item_in() {
    let tracks = [GridTrack::Auto, GridTrack::Fr(1.0)];
    let sizes = size_tracks(&tracks, 2, 0.0, Some(200.0), &[(0, 1, 30.0), (0, 2, 150.0)]);

    assert_eq!(sizes, [30.0, 170.0]);
  }

  #[test]
  fn content_gives_way_when_the_tracks_do_not_fit() {
    let tracks = [GridTrack::Px(60.0), GridTrack::Auto, GridTrack::Auto];
    let sizes = size_tracks(&tracks, 3, 0.0, Some(120.0), &[(1, 1, 30.0), (2, 1, 90.0)]);

    // 60 are left for 120 of content: each keeps half.
    assert_eq!(sizes, [60.0, 15.0, 45.0]);
  }

  #[test]
  fn a_share_of_nothing_is_sized_by_content() {
    let tracks = [GridTrack::Fr(0.0), GridTrack::Fr(1.0)];
    let sizes = size_tracks(&tracks, 2, 0.0, Some(100.0), &[(0, 1, 30.0)]);

    assert_eq!(sizes, [30.0, 70.0]);
  }
}
