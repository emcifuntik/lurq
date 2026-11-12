use crate::{
  impl_into_node,
  layout::{
    Alignment,
    grid::{GridSpec, GridTrack},
  },
  node::{Element, SpacingValue},
};

impl_into_node!(Grid);

/// A container that lays its children out in the cells of columns and rows,
/// as CSS grid does. See [`crate::layout::grid`] for how items are placed and
/// tracks are sized.
///
/// ```ignore
/// Grid::new()
///   .columns([GridTrack::Px(120.0), GridTrack::Fr(1.0)])
///   .gap(8.0)
///   .child(Text::new("Name"))
///   .child(TextInput::new(name))
///   .child(Text::new("Notes").column_span(2))
/// ```
impl Grid {
  pub fn new() -> Self {
    Self::from_node(crate::node::Node::grid(GridSpec::default(), vec![]))
  }

  /// A grid of `count` columns that share its width equally.
  pub fn with_columns(count: usize) -> Self {
    Self::new().columns(vec![GridTrack::Fr(1.0); count])
  }

  pub fn child(mut self, child: impl Into<Element>) -> Self {
    self.update_node(|node| node.push_child_element(child.into()));
    self
  }

  pub fn with_children(mut self, children: impl IntoIterator<Item = impl Into<Element>>) -> Self {
    self.update_node(|node| node.extend_child_elements(children.into_iter().map(Into::into)));
    self
  }

  /// The columns, from the left. An item placed past the last of them adds columns as wide as their content.
  pub fn columns(mut self, columns: impl IntoIterator<Item = impl Into<GridTrack>>) -> Self {
    let columns = columns.into_iter().map(Into::into).collect();
    self.update_node(|node| node.update_grid(|spec| spec.columns = columns));
    self
  }

  /// The rows, from the top. The rows the items need past the last of them are as tall as their content.
  pub fn rows(mut self, rows: impl IntoIterator<Item = impl Into<GridTrack>>) -> Self {
    let rows = rows.into_iter().map(Into::into).collect();
    self.update_node(|node| node.update_grid(|spec| spec.rows = rows));
    self
  }

  /// The gap between columns and between rows: logical pixels or a `SpacingSize` role.
  pub fn gap(mut self, gap: impl Into<SpacingValue>) -> Self {
    let gap = gap.into();
    self.update_node(|node| {
      node.update_grid(|spec| {
        spec.column_gap = gap;
        spec.row_gap = gap;
      })
    });
    self
  }

  pub fn column_gap(mut self, gap: impl Into<SpacingValue>) -> Self {
    let gap = gap.into();
    self.update_node(|node| node.update_grid(|spec| spec.column_gap = gap));
    self
  }

  pub fn row_gap(mut self, gap: impl Into<SpacingValue>) -> Self {
    let gap = gap.into();
    self.update_node(|node| node.update_grid(|spec| spec.row_gap = gap));
    self
  }

  /// How an item sits in its cell from top to bottom. `Alignment::Stretch`, the default, makes it as tall
  /// as the cell.
  pub fn align_items(mut self, align: Alignment) -> Self {
    self.update_node(|node| node.update_grid(|spec| spec.align = align));
    self
  }

  /// How an item sits in its cell from side to side. `Alignment::Stretch`, the default, makes it as wide as
  /// the cell.
  pub fn justify_items(mut self, justify: Alignment) -> Self {
    self.update_node(|node| node.update_grid(|spec| spec.justify = justify));
    self
  }
}

impl Default for Grid {
  fn default() -> Self {
    Self::new()
  }
}
