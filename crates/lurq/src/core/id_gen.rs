use std::sync::{
  Arc,
  atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u64);

impl NodeId {
  pub const UNASSIGNED: Self = Self(0);

  pub fn value(self) -> u64 {
    self.0
  }

  pub fn is_assigned(self) -> bool {
    self.0 != 0
  }
}

/// The next ID is a counter with no lock around it: a lock of the standard library's makes its first use an
/// allocation on the platforms whose lock is a `pthread` one, and a tree's first IDs are given in a pass that
/// allocates nothing.
#[derive(Clone)]
pub struct IdGenerator {
  next: Arc<AtomicU64>,
}

impl Default for IdGenerator {
  fn default() -> Self {
    Self::new()
  }
}

impl IdGenerator {
  pub fn new() -> Self {
    Self {
      next: Arc::new(AtomicU64::new(1)),
    }
  }

  pub fn next(&self) -> NodeId {
    let id = self.next.fetch_add(1, Ordering::Relaxed);
    assert!(id != u64::MAX, "node IDs exhausted");
    NodeId(id)
  }

  /// IDs are never reused: focus, input events and inspector refs may outlive
  /// the node they identify. Keeping retired IDs costs no additional storage.
  pub fn free(&self, _id: NodeId) {}
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn retired_ids_never_alias_new_nodes() {
    let ids = IdGenerator::new();
    let old = ids.next();
    ids.free(old);
    assert_ne!(old, ids.next());
  }
}
