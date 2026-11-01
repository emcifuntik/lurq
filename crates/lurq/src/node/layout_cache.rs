use std::{
  cell::{Cell, RefCell},
  sync::{Arc, Weak},
};

#[cfg(test)]
use crate::layout::layout_result::SubtreeHolds;
use crate::layout::{Constraints, layout_result::LayoutResult};
use crate::node::lazy_box::{LazyBox, empty_default};

/// Up to two layout results of one node, most recently used first.
///
/// The front entry is the layout the node was last laid out as or served
/// from, so its constraints are the ones the node's parent last gave it.
pub struct LayoutCache {
  // Keep the two result slots off Node's stack footprint as tracking grows.
  // An unlaid node needs neither the slots nor an allocation for them.
  inner: RefCell<LazyBox<CachedLayouts>>,
  // Inline: a rebuilt tree validates every node, and a box here was as many
  // allocations. Without the RefCell it costs Node no more than the box did.
  validated: Cell<Option<ValidatedLayout>>,
  pub(crate) watch: std::sync::Arc<crate::core::layout_dirty::LayoutWatch>,
  pub(crate) tracking_generation: Cell<u64>,
  pub(crate) inspected_generation: Cell<u64>,
  pub(crate) guards_generation: Cell<u64>,
  // A clear belongs to each node consuming the shared element handle.
  override_clear_generation: Cell<u64>,
  pub(crate) subtree: Cell<LayoutSubtree>,
  local_dirty: Cell<bool>,
  descendant_dirty: Cell<bool>,
  /// Whether every descendant cache's front entry belongs to this cache's
  /// front entry, i.e. no older entry was served since the last store.
  /// Serving an older entry skips the descendants, so their front entries
  /// still describe the layout they were last visited in (another size of
  /// this node), and repairing a dirty descendant under them would lay it
  /// out for that other size.
  descendants_match_front: Cell<bool>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct LayoutSubtree {
  pub(crate) text_input: bool,
  pub(crate) select: bool,
  pub(crate) timeline: bool,
  pub(crate) scroll: bool,
  pub(crate) canvas: bool,
  pub(crate) resource_images: bool,
  pub(crate) resource_svgs: bool,
  pub(crate) loading_images: bool,
}

struct ValidatedLayout {
  generation: u64,
  result: Weak<LayoutResult>,
}

struct CachedLayout {
  constraints: Constraints,
  result: Arc<LayoutResult>,
}

#[derive(Default)]
struct CachedLayouts {
  slots: [Option<CachedLayout>; 2],
  front: usize,
}

empty_default!(CachedLayouts);

impl CachedLayouts {
  fn front(&self) -> Option<&CachedLayout> {
    self.slots[self.front].as_ref()
  }

  fn find(&self, constraints: Constraints) -> Option<usize> {
    self.slots.iter().position(|slot| {
      slot.as_ref().is_some_and(|cached| cached.constraints == constraints)
    })
  }

  fn clear(&mut self) {
    self.slots = [None, None];
  }

  fn store(&mut self, constraints: Constraints, result: Arc<LayoutResult>) {
    let index = self.find(constraints).unwrap_or(1 - self.front);
    self.slots[index] = Some(CachedLayout { constraints, result });
    self.front = index;
  }
}

impl LayoutCache {
  pub fn new() -> Self {
    Self {
      inner: RefCell::new(LazyBox::new()),
      validated: Cell::new(None),
      watch: crate::core::layout_dirty::LayoutWatch::new(),
      tracking_generation: Cell::new(0),
      inspected_generation: Cell::new(0),
      guards_generation: Cell::new(0),
      override_clear_generation: Cell::new(0),
      subtree: Cell::new(LayoutSubtree::default()),
      local_dirty: Cell::new(false),
      descendant_dirty: Cell::new(false),
      descendants_match_front: Cell::new(true),
    }
  }

  /// Serves the entry laid out under `constraints` and makes it the front
  /// entry, so the constraints of the layout last served stay first.
  #[cfg(test)]
  pub fn get(&self, constraints: Constraints) -> Option<LayoutResult> {
    self.get_shared(constraints).map(|result| result.as_ref().clone())
  }

  pub(crate) fn get_shared(&self, constraints: Constraints) -> Option<Arc<LayoutResult>> {
    if self.is_dirty() {
      return None;
    }
    let mut entries = self.inner.borrow_mut();
    let index = entries.find(constraints)?;
    if index != entries.front {
      entries.front = index;
      self.descendants_match_front.set(false);
    }
    Some(entries.front()?.result.clone())
  }

  pub(crate) fn matches_validated(&self, result: &Arc<LayoutResult>) -> bool {
    let validated = self.validated.take();
    let matches = validated.as_ref().is_some_and(|validated| {
      validated.generation == self.watch.generation() && std::ptr::eq(validated.result.as_ptr(), Arc::as_ptr(result))
    });
    self.validated.set(validated);
    matches
  }

  pub(crate) fn remember_validation(&self, result: &Arc<LayoutResult>) {
    self.validated.set(Some(ValidatedLayout {
      generation: self.watch.generation(),
      result: Arc::downgrade(result),
    }));
  }

  pub(crate) fn record_validation(&self, constraints: Constraints, generation: u64) {
    let mut entries = self.inner.borrow_mut();
    if let Some(index) = entries.find(constraints) {
      let result = &mut entries.slots[index].as_mut().unwrap().result;
      if result.generation != generation {
        Arc::make_mut(result).generation = generation;
      }
    }
  }

  pub(crate) fn contains(&self, constraints: Constraints) -> bool {
    if self.is_dirty() {
      return false;
    }
    self.inner.borrow().find(constraints).is_some()
  }

  /// Records that the parent reused `result`, this node's layout under
  /// `constraints` from earlier in the parent's layout, instead of laying
  /// the node out again (a repaired child's override). Like a hit, it makes
  /// those constraints the front ones: the parent may have laid the node out
  /// under other constraints in between, and its next repair must not use
  /// them. The entry is put back if those layouts pushed it out.
  pub(crate) fn record_reuse(&self, constraints: Constraints, result: &Arc<LayoutResult>) {
    let mut entries = self.inner.borrow_mut();
    match entries.find(constraints) {
      Some(index) if index == entries.front => return,
      Some(index) => entries.front = index,
      None => entries.store(constraints, result.clone()),
    }
    self.descendants_match_front.set(false);
  }

  /// The cached result a dirty node may patch its dirty children into:
  /// the front entry, when it was laid out under `constraints` and its
  /// descendants were not laid out for another entry since. The patch lays
  /// each dirty child out again under that child's front constraints, which
  /// are only the ones this result gave it under those two conditions.
  pub(crate) fn get_repairable(&self, constraints: Constraints) -> Option<Arc<LayoutResult>> {
    if !self.descendants_match_front.get() {
      return None;
    }
    let entries = self.inner.borrow();
    let front = entries.front()?;
    (front.constraints == constraints).then(|| front.result.clone())
  }

  pub(crate) fn constraints(&self) -> Option<Constraints> {
    self.inner.borrow().front().map(|cached| cached.constraints)
  }

  pub(crate) fn has_cached_result(&self) -> bool {
    self.inner.borrow().front().is_some()
  }

  /// Constraints and size of the most recently stored result, if any.
  pub(crate) fn cached_entry(&self) -> Option<(Constraints, crate::layout::Size)> {
    self
      .inner
      .borrow()
      .front()
      .map(|cached| (cached.constraints, cached.result.size))
  }

  /// Takes over the entries of `old`, a cache of the tree being replaced:
  /// every caller drops that node right after reconciling (`Tree::set_root`,
  /// `Tree::rebuild`, the component slots, the live slot replacement, the
  /// overlays), and each old node is preserved from at most once, so moving
  /// the box saves cloning it for every node of a rebuilt tree. `old` serves
  /// nothing afterwards; only its flags stay as they were.
  pub(crate) fn preserve_from(&self, old: &Self) {
    self.watch.mark();
    self.override_clear_generation.set(old.override_clear_generation.get());
    *self.inner.borrow_mut() = old.inner.take();
    // The descendants take over the old descendants' caches the same way, so
    // whether they match the front entry carries over with the entries.
    self.descendants_match_front.set(old.descendants_match_front.get());
    // Carry the old cache's unresolved dirtiness instead of clearing it: the
    // old tree may hold marks no layout pass has consumed yet (re-render
    // chains between paints). Dropping them here laundered staleness — a
    // duplicate re-render inherited the stale results with clean flags and
    // the engine served them wholesale (stale spacer heights / stale text
    // measurements on screen).
    self.local_dirty.set(self.local_dirty.get() || old.local_dirty.get());
    self
      .descendant_dirty
      .set(self.descendant_dirty.get() || old.descendant_dirty.get());
  }

  #[cfg(test)]
  pub fn store(&self, constraints: Constraints, result: LayoutResult) {
    self.store_shared(constraints, Arc::new(result));
  }

  pub(crate) fn store_shared(&self, constraints: Constraints, result: Arc<LayoutResult>) {
    let mut borrow = self.inner.borrow_mut();
    if self.is_dirty() {
      // The dirty flags are cache-wide, but a store only replaces the entry
      // for the constraints just laid out. Any other cached entry predates
      // the invalidation and is equally stale — clearing the flags below
      // while keeping it would launder it into a servable result (observed
      // in production: a two-entry cache under oscillating constraints — a
      // scrollbar gutter toggling a column's width — served a pre-change
      // sibling layout with clean flags, freezing a text row at its old
      // child offsets).
      borrow.clear();
    }
    borrow.store(constraints, result);
    self.descendants_match_front.set(true);
    self.clear_dirty();
  }

  pub fn invalidate(&self) {
    self.watch.mark();
    self.validated.take();
    *self.inner.borrow_mut() = LazyBox::new();
    self.descendants_match_front.set(true);
    self.clear_dirty();
  }

  /// The node was placed by its parent's layout and holds no result of its
  /// own (an inline link, placed by its paragraph): the marks that asked for
  /// its layout are consumed by the parent's. A mark left behind would keep
  /// the layout dirty for every pass, since no store of its own clears it.
  #[cfg(feature = "markdown")]
  pub(crate) fn mark_placed_by_parent(&self) {
    self.clear_dirty();
  }

  pub(crate) fn mark_local_dirty(&self) {
    self.watch.mark();
    self.local_dirty.set(true);
  }

  pub(crate) fn mark_descendant_dirty(&self) {
    self.watch.mark_descendant();
    self.descendant_dirty.set(true);
  }

  pub(crate) fn needs_dirty_scan(&self) -> bool {
    self.inspected_generation.get() != self.watch.generation()
  }

  pub(crate) fn record_dirty_scan(&self, generation: u64, local: bool, descendant: bool) {
    self.local_dirty.set(self.local_dirty.get() || local);
    self.descendant_dirty.set(self.descendant_dirty.get() || descendant);
    // A shared handle can change while the walk consumes its marks. Only
    // acknowledge the generation the walk started with, so a later write
    // still asks for another inspection.
    self.inspected_generation.set(generation);
  }

  pub(crate) fn take_override_cleared(&self, generation: u64) -> bool {
    self.override_clear_generation.replace(generation) != generation
  }

  pub(crate) fn is_local_dirty(&self) -> bool {
    self.local_dirty.get()
  }

  pub(crate) fn is_descendant_dirty(&self) -> bool {
    self.descendant_dirty.get()
  }

  pub(crate) fn is_dirty(&self) -> bool {
    self.is_local_dirty() || self.is_descendant_dirty()
  }

  fn clear_dirty(&self) {
    self.local_dirty.set(false);
    self.descendant_dirty.set(false);
  }

  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    let borrow = self.inner.borrow();
    let cached_bytes = borrow
      .slots
      .iter()
      .flatten()
      .map(|cached| cached.result.estimated_memory_bytes() + std::mem::size_of::<[usize; 2]>())
      .sum::<usize>();
    let slots_bytes = if borrow.front().is_some() {
      std::mem::size_of::<CachedLayouts>()
    } else {
      0
    };
    std::mem::size_of::<Self>()
      + std::mem::size_of::<crate::core::layout_dirty::LayoutWatch>()
      + slots_bytes
      + cached_bytes
  }
}

impl Default for LayoutCache {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::layout::Size;

  #[test]
  fn dirty_scan_does_not_consume_a_later_notification() {
    let cache = LayoutCache::new();
    let generation = cache.watch.generation();
    cache.watch.mark();

    cache.record_dirty_scan(generation, false, false);

    assert!(cache.needs_dirty_scan());
  }

  fn constraints(max_width: f32) -> Constraints {
    Constraints {
      min_width: 0.0,
      min_height: 0.0,
      max_width,
      max_height: f32::MAX,
    }
  }

  fn result(width: f32) -> LayoutResult {
    LayoutResult {
      size: Size::new(width, 10.0),
      children: vec![].into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  #[test]
  fn a_validation_holds_for_its_result_until_the_generation_moves() {
    let cache = LayoutCache::new();
    let validated = Arc::new(result(40.0));
    let other = Arc::new(result(40.0));
    assert!(!cache.matches_validated(&validated), "nothing validated yet");

    cache.remember_validation(&validated);
    assert!(cache.matches_validated(&validated));
    assert!(cache.matches_validated(&validated), "asking keeps the record");
    assert!(!cache.matches_validated(&other), "another snapshot was not validated");

    cache.watch.mark();
    assert!(!cache.matches_validated(&validated), "a later change voids it");

    cache.remember_validation(&validated);
    assert!(cache.matches_validated(&validated));
    cache.remember_validation(&other);
    assert!(!cache.matches_validated(&validated), "the record holds one snapshot");
    assert!(cache.matches_validated(&other));

    cache.invalidate();
    assert!(!cache.matches_validated(&other));
  }

  /// A dirty cache must not keep sibling entries across a store: they were
  /// computed before the invalidation, and clearing the cache-wide dirty
  /// flags while keeping them would serve them as fresh once the caller's
  /// constraints oscillate back (the studio skills-viewer bug: a scrollbar
  /// gutter toggled a column's width per pass, and a text row whose content
  /// grew kept serving its pre-change layout from the second cache slot).
  #[test]
  fn store_on_a_dirty_cache_drops_stale_sibling_entries() {
    let cache = LayoutCache::new();
    cache.store(constraints(100.0), result(40.0));
    cache.store(constraints(90.0), result(38.0));
    assert!(cache.get(constraints(100.0)).is_some());
    assert!(cache.get(constraints(90.0)).is_some());

    // Content changed: both entries are stale. Repair relayouts under one
    // constraint set only.
    cache.mark_descendant_dirty();
    cache.store(constraints(90.0), result(45.0));

    assert_eq!(
      cache.get(constraints(90.0)).map(|cached| cached.size.width),
      Some(45.0),
      "the freshly stored entry is served"
    );
    assert!(
      cache.get(constraints(100.0)).is_none(),
      "the pre-invalidation sibling entry must not be served as fresh"
    );
  }

  /// A clean cache keeps memoizing both constraint sets (the two-slot memo
  /// exists for constraint oscillation with unchanged content).
  #[test]
  fn clean_stores_keep_both_entries() {
    let cache = LayoutCache::new();
    cache.store(constraints(100.0), result(40.0));
    cache.store(constraints(90.0), result(38.0));
    assert_eq!(
      cache.get(constraints(100.0)).map(|cached| cached.size.width),
      Some(40.0)
    );
    assert_eq!(cache.get(constraints(90.0)).map(|cached| cached.size.width), Some(38.0));
  }

  /// Serving the older entry makes it the front one: the parent's next
  /// repair asks for the constraints this node was last laid out under (the
  /// window resized back to an earlier size served the other slot, and a
  /// modal's content was then repaired at the size before).
  #[test]
  fn serving_an_entry_makes_its_constraints_the_front_ones() {
    let cache = LayoutCache::new();
    cache.store(constraints(1440.0), result(1440.0));
    cache.store(constraints(960.0), result(960.0));

    assert!(cache.get(constraints(1440.0)).is_some());

    assert_eq!(cache.constraints().map(|front| front.max_width), Some(1440.0));
    assert_eq!(cache.cached_entry().map(|(_, size)| size.width), Some(1440.0));
  }

  /// After the older entry was served, the descendants' front entries
  /// belong to the other layout, so the cache refuses a repair until the
  /// node is laid out (stored) again.
  #[test]
  fn a_repair_needs_the_front_entry_its_descendants_were_laid_out_for() {
    let cache = LayoutCache::new();
    cache.store(constraints(1440.0), result(1440.0));
    cache.store(constraints(960.0), result(960.0));
    assert!(cache.get_repairable(constraints(960.0)).is_some());
    assert!(
      cache.get_repairable(constraints(1440.0)).is_none(),
      "an entry behind the front one is not repairable"
    );

    assert!(cache.get(constraints(1440.0)).is_some());
    assert!(
      cache.get_repairable(constraints(1440.0)).is_none(),
      "a served older entry skipped its descendants"
    );
    assert!(
      cache.get(constraints(1440.0)).is_some(),
      "the served entry is still a clean hit"
    );

    cache.store(constraints(1440.0), result(1440.0));
    assert!(cache.get_repairable(constraints(1440.0)).is_some());
  }

  #[test]
  fn preserving_a_cache_keeps_whether_its_descendants_match_the_front_entry() {
    let old = LayoutCache::new();
    old.store(constraints(1440.0), result(1440.0));
    old.store(constraints(960.0), result(960.0));
    assert!(old.get(constraints(1440.0)).is_some());

    let new = LayoutCache::new();
    new.preserve_from(&old);

    assert!(new.get_repairable(constraints(1440.0)).is_none());
  }

  /// A rebuilt tree takes its layouts over from the tree it replaces: the
  /// entries move, in their order, and the old cache has none left to serve.
  #[test]
  fn preserving_a_cache_takes_the_entries_instead_of_cloning_them() {
    let old = LayoutCache::new();
    old.store(constraints(1440.0), result(1440.0));
    old.store(constraints(960.0), result(960.0));
    let older = old.get_shared(constraints(1440.0)).unwrap();
    let front = old.get_shared(constraints(960.0)).unwrap();

    let new = LayoutCache::new();
    new.preserve_from(&old);

    assert!(!old.has_cached_result(), "the old cache keeps no entry");
    assert!(old.get_shared(constraints(960.0)).is_none());
    assert_eq!(Arc::strong_count(&front), 2, "only the new cache holds the result besides this test");
    assert_eq!(new.constraints().map(|front| front.max_width), Some(960.0));
    assert!(Arc::ptr_eq(&new.get_shared(constraints(960.0)).unwrap(), &front));
    assert!(Arc::ptr_eq(&new.get_shared(constraints(1440.0)).unwrap(), &older));
  }

  /// Taking the entries must not launder what the old cache had pending.
  #[test]
  fn preserving_a_cache_carries_its_dirty_flags_with_the_entries() {
    let old = LayoutCache::new();
    old.store(constraints(960.0), result(960.0));
    old.mark_local_dirty();
    old.mark_descendant_dirty();

    let new = LayoutCache::new();
    new.preserve_from(&old);

    assert!(new.is_local_dirty());
    assert!(new.is_descendant_dirty());
    assert!(new.has_cached_result());
    assert!(new.get_shared(constraints(960.0)).is_none(), "a dirty cache serves nothing");
  }

  /// A reused override becomes the front entry, and comes back if the
  /// parent's layouts in between pushed it out of the cache.
  #[test]
  fn a_reused_result_becomes_the_front_entry() {
    let cache = LayoutCache::new();
    cache.store(constraints(292.0), result(292.0));
    cache.store(constraints(300.0), result(300.0));
    cache.record_reuse(constraints(292.0), &result(292.0).into());
    assert_eq!(cache.constraints().map(|front| front.max_width), Some(292.0));
    assert!(cache.get_repairable(constraints(292.0)).is_none());

    cache.store(constraints(300.0), result(300.0));
    cache.store(constraints(310.0), result(310.0));
    cache.record_reuse(constraints(292.0), &result(292.0).into());
    assert_eq!(
      cache.cached_entry().map(|(front, size)| (front.max_width, size.width)),
      Some((292.0, 292.0))
    );
    assert!(cache.get(constraints(310.0)).is_some(), "the newer entry stays");
  }
}
