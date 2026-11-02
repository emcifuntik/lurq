use super::*;

const KIB: usize = 1024;

/// A frame drawing assets `ids` of 1 KiB each in one encode, uploading those it
/// lacks. Returns the ids it uploaded and the textures the frame retired.
fn frame(cache: &mut AssetCache<u64>, ids: impl IntoIterator<Item = u64>) -> (Vec<u64>, Vec<u64>, AssetCacheStats) {
  encode(cache, ids, true)
}

/// One encode drawing assets `ids`; `frame_ends` as the renderer decided.
fn encode(
  cache: &mut AssetCache<u64>,
  ids: impl IntoIterator<Item = u64>,
  frame_ends: bool,
) -> (Vec<u64>, Vec<u64>, AssetCacheStats) {
  let (mut uploaded, mut retired) = (Vec::new(), Vec::new());
  for id in ids {
    if cache.get(id).is_none() {
      uploaded.push(id);
      cache.insert(id, id, KIB, |texture| retired.push(texture));
    }
    assert_eq!(cache.peek(id), Some(&id));
  }
  let stats = cache.finish_encode(frame_ends, |texture| retired.push(texture));
  retired.sort_unstable();
  (uploaded, retired, stats)
}

#[test]
fn the_budget_is_clamped_and_the_ceiling_is_twice_it() {
  assert_eq!(CanvasAssetBudget::default().bytes(), 64 * KIB * KIB);
  assert_eq!(CanvasAssetBudget::new(usize::MAX).bytes(), CanvasAssetBudget::MAX_BYTES);
  assert_eq!(CanvasAssetBudget::new(usize::MAX).ceiling_bytes(), 2 * 1024 * KIB * KIB);
  assert_eq!(CanvasAssetBudget::new(10).ceiling_bytes(), 20);
}

#[test]
fn small_textures_are_charged_64_kib() {
  let mut cache = AssetCache::new(CanvasAssetBudget::default());
  cache.insert(1, 1, 10, drop);
  cache.insert(2, 2, 100 * KIB, drop);
  assert_eq!(cache.bytes(), 164 * KIB);
}

#[test]
fn a_frame_over_the_ceiling_uploads_its_overflow_once_and_keeps_the_rest() {
  // Four textures of budget, eight of ceiling.
  let mut cache = AssetCache::new(CanvasAssetBudget::new(4 * 64 * KIB));
  let (uploaded, retired, stats) = frame(&mut cache, (0..10).chain(8..10));
  assert_eq!(
    uploaded,
    (0..10).collect::<Vec<_>>(),
    "a repeat within the frame is not uploaded again"
  );
  assert_eq!(retired, [8, 9], "what was not kept lives until the frame ends");
  assert_eq!(
    stats,
    AssetCacheStats {
      evictions: 0,
      uncached: 2,
      uncached_bytes: 2 * 64 * KIB,
      stretch_bytes: 4 * 64 * KIB,
    }
  );
  for _ in 0..3 {
    let (uploaded, retired, stats) = frame(&mut cache, 0..10);
    assert_eq!(uploaded, [8, 9]);
    assert_eq!(retired, [8, 9]);
    assert_eq!((stats.evictions, stats.uncached), (0, 2));
  }
  assert_eq!((cache.len(), cache.bytes()), (8, 8 * 64 * KIB));
}

#[test]
fn a_zero_budget_keeps_nothing_between_frames() {
  let mut cache = AssetCache::new(CanvasAssetBudget::new(0));
  for _ in 0..2 {
    let (uploaded, retired, stats) = frame(&mut cache, [1, 2, 1]);
    assert_eq!((uploaded, retired), (vec![1, 2], vec![1, 2]));
    assert_eq!(stats.uncached, 2);
  }
  assert_eq!(cache.len(), 0);
}

#[test]
fn textures_two_frames_old_make_room_and_are_retired() {
  let mut cache = AssetCache::new(CanvasAssetBudget::new(4 * 64 * KIB));
  frame(&mut cache, 0..4);
  frame(&mut cache, 4..8);
  assert_eq!(cache.bytes(), 8 * 64 * KIB, "the previous frame's textures stay");
  let (uploaded, retired, stats) = frame(&mut cache, 8..10);
  assert_eq!(uploaded, [8, 9]);
  assert_eq!(
    retired,
    [0, 1, 2, 3],
    "only textures neither of the last two frames drew"
  );
  assert_eq!(stats.evictions, 4);
  let (uploaded, retired, _) = frame(&mut cache, 8..10);
  assert!(uploaded.is_empty());
  assert_eq!(retired, [4, 5], "oldest first, back to the budget");
  assert_eq!(cache.bytes(), cache.budget());
}

#[test]
fn a_frame_drawn_over_several_encodes_stays_whole_and_retires_overflow_per_encode() {
  // Four textures of budget, eight of ceiling.
  let mut cache = AssetCache::new(CanvasAssetBudget::new(4 * 64 * KIB));
  // A replacement drawn in three batches: twelve textures, four past the ceiling.
  let batches = [(0..4, false), (4..8, false), (8..12, true)];
  for (ids, ends) in batches.clone() {
    let (uploaded, retired, stats) = encode(&mut cache, ids.clone(), ends);
    assert_eq!(uploaded, ids.clone().collect::<Vec<_>>());
    let overflow: Vec<_> = ids.filter(|id| *id >= 8).collect();
    assert_eq!(retired, overflow, "nothing of the frame is evicted while it is drawn");
    assert_eq!(stats.evictions, 0);
  }
  // The next replacement draws the same page: only what was never kept is
  // uploaded again. Ending a frame at every encode would evict the first
  // batches while the last one is drawn, and upload them again here.
  for _ in 0..3 {
    for (ids, ends) in batches.clone() {
      let (uploaded, _, stats) = encode(&mut cache, ids.clone(), ends);
      assert_eq!(uploaded, ids.filter(|id| *id >= 8).collect::<Vec<_>>());
      assert_eq!(stats.evictions, 0);
    }
  }
  // A texture not kept lives only for its encode, even within one frame.
  let (uploaded, retired, _) = encode(&mut cache, [8], false);
  assert_eq!((uploaded, retired), (vec![8], vec![8]));
  let (uploaded, ..) = encode(&mut cache, [8], true);
  assert_eq!(uploaded, [8]);
  assert_eq!((cache.len(), cache.bytes()), (8, 8 * 64 * KIB));
}

#[test]
fn ending_every_encode_splits_a_page_drawn_in_batches_and_thrashes() {
  // The same page as above with a frame ended at every encode, which is what
  // the renderer must not do for a replacement drawn in batches.
  let mut cache = AssetCache::new(CanvasAssetBudget::new(4 * 64 * KIB));
  for ids in [0..4, 4..8, 8..12] {
    encode(&mut cache, ids, true);
  }
  let (uploaded, _, stats) = encode(&mut cache, 0..4, true);
  assert_eq!(uploaded, [0, 1, 2, 3], "the first batch was evicted by the last");
  assert!(stats.evictions > 0);
}
