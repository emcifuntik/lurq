use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
pub struct IdGenerator {
  pool: Arc<Mutex<IdPool>>,
}

struct IdPool {
  current: u64,
}

impl Default for IdGenerator {
  fn default() -> Self {
    Self::new()
  }
}

impl IdGenerator {
  pub fn new() -> Self {
    Self {
      pool: Arc::new(Mutex::new(IdPool { current: 1 })),
    }
  }

  pub fn next(&self) -> NodeId {
    let mut pool = self.pool.lock().unwrap();
    let id = pool.current;
    pool.current = pool.current.checked_add(1).expect("node IDs exhausted");
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
