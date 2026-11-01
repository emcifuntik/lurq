use std::sync::Arc;

use crate::layout::{Offset, Size, layout_engine::TextLayoutOutput};

#[derive(Clone)]
pub struct LayoutResult {
  pub size: Size,
  pub children: ChildLayouts,
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
  /// The subtree generation this geometry was prepared for. Carried caches
  /// retain their old generation and must be checked against the new nodes.
  pub(crate) generation: u64,
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

/// Child placements shared by cached results, copied only when a layout
/// changes them. Reading and iterating the children works like a slice.
#[derive(Clone, Default)]
pub struct ChildLayouts(Option<Arc<[ChildLayout]>>);

impl ChildLayouts {
  pub(crate) fn same_storage(&self, other: &Self) -> bool {
    match (&self.0, &other.0) {
      (None, None) => true,
      (Some(left), Some(right)) => Arc::ptr_eq(left, right),
      _ => false,
    }
  }
}

impl std::ops::Deref for ChildLayouts {
  type Target = [ChildLayout];

  fn deref(&self) -> &Self::Target {
    self.0.as_deref().unwrap_or(&[])
  }
}

impl std::ops::DerefMut for ChildLayouts {
  fn deref_mut(&mut self) -> &mut Self::Target {
    match &mut self.0 {
      Some(children) => Arc::make_mut(children),
      None => &mut [],
    }
  }
}

impl From<Vec<ChildLayout>> for ChildLayouts {
  fn from(children: Vec<ChildLayout>) -> Self {
    Self((!children.is_empty()).then(|| children.into()))
  }
}

impl FromIterator<ChildLayout> for ChildLayouts {
  fn from_iter<T: IntoIterator<Item = ChildLayout>>(iter: T) -> Self {
    Vec::from_iter(iter).into()
  }
}

impl<'a> IntoIterator for &'a ChildLayouts {
  type Item = &'a ChildLayout;
  type IntoIter = std::slice::Iter<'a, ChildLayout>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter()
  }
}

impl<'a> IntoIterator for &'a mut ChildLayouts {
  type Item = &'a mut ChildLayout;
  type IntoIter = std::slice::IterMut<'a, ChildLayout>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter_mut()
  }
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
      + self.children.len() * std::mem::size_of::<ChildLayout>()
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
          children: Vec::new().into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        }
        .into(),
      }].into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    };

    let mut cloned = result.clone();

    assert_eq!(result.children.as_ptr(), cloned.children.as_ptr());
    assert!(Arc::ptr_eq(&result.children[0].result, &cloned.children[0].result));
    cloned.children[0].offset.x = 3.0;
    assert_eq!(result.children[0].offset.x, 0.0);
    assert_eq!(cloned.children[0].offset.x, 3.0);
    assert!(Arc::ptr_eq(&result.children[0].result, &cloned.children[0].result));
  }
}
