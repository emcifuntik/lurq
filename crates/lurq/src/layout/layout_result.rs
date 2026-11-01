use std::sync::Arc;

use crate::layout::{Offset, Size, layout_engine::TextLayoutOutput};

#[derive(Clone)]
pub struct LayoutResult {
  pub size: Size,
  pub children: Vec<ChildLayout>,
  /// What laying out a text leaf wrote into its node's text state, written
  /// back when the result is served from a layout cache.
  pub(crate) text_layout: Option<Arc<TextLayoutOutput>>,
  /// Dropped from its single-line `Row`/`Column` by
  /// [`ShrinkLimit::Drop`](crate::layout::layout_kind::ShrinkLimit::Drop):
  /// laid out at zero size, neither drawn nor hit.
  pub(crate) dropped: bool,
  /// What in this subtree a result served from a layout cache must still
  /// write back into its nodes. Set when the layout engine prepares the
  /// result; a result built anywhere else may hold anything.
  pub(crate) holds: SubtreeHolds,
}

/// What a laid-out subtree holds that serving it from a cache writes back:
/// a scroll container syncs its scroll state and its content's offset, a
/// text leaf restores its text layout output. A subtree holding neither is
/// served as it is: the layout engine neither walks nor copies it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SubtreeHolds {
  pub(crate) scroll: bool,
  pub(crate) text: bool,
}

impl SubtreeHolds {
  /// For a result nobody has looked into yet: it may hold either.
  pub(crate) const UNKNOWN: Self = Self {
    scroll: true,
    text: true,
  };
}

#[derive(Clone)]
pub struct ChildLayout {
  pub offset: Offset,
  pub result: Arc<LayoutResult>,
}

impl LayoutResult {
  /// Whether its single-line `Row`/`Column` dropped this child to make room
  /// ([`ShrinkLimit::Drop`](crate::layout::layout_kind::ShrinkLimit::Drop)).
  /// A dropped child has zero size and is neither drawn nor hit.
  pub fn is_dropped(&self) -> bool {
    self.dropped
  }

  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    std::mem::size_of::<Self>()
      + self
        .text_layout
        .as_ref()
        .map_or(0, |output| output.estimated_memory_bytes())
      + self.children.capacity() * std::mem::size_of::<ChildLayout>()
      + self
        .children
        .iter()
        .map(|child| child.result.estimated_memory_bytes())
        .sum::<usize>()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cloning_layout_result_shares_nested_child_results() {
    let result = LayoutResult {
      size: Size::new(10.0, 10.0),
      children: vec![ChildLayout {
        offset: Offset::default(),
        result: LayoutResult {
          size: Size::new(5.0, 5.0),
          children: Vec::new(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
        }
        .into(),
      }],
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
    };

    let cloned = result.clone();

    assert!(Arc::ptr_eq(&result.children[0].result, &cloned.children[0].result));
  }
}
