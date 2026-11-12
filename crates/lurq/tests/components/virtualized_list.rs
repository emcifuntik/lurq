use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
  },
  components::{Rect, VirtualizedList},
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};
use lurq_macros::DevtoolsInspectable;

use crate::support::run_pass;

struct Shared<T>(Arc<T>);

impl<T> Clone for Shared<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

impl<T> PartialEq for Shared<T> {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

#[cfg(feature = "devtools")]
impl<T> lurq::app::component::DevtoolsInspectable for Shared<T> {}

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct RowData {
  id: usize,
  height: f32,
}

struct VirtualizedRoot;

impl Component for VirtualizedRoot {
  type Props = (Vec<RowData>, Shared<AtomicUsize>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, bottom_reached) = ctx.props::<Self::Props>().clone();
    let reached = bottom_reached.0.clone();
    VirtualizedList::new(ctx, items)
      .size(100.0, 100.0)
      .overscan_px(0.0)
      .on_bottom_reached(move || {
        reached.fetch_add(1, Ordering::SeqCst);
      })
      .mount_keyed::<MeasuredRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

struct MeasuredRow;

impl Component for MeasuredRow {
  type Props = RowData;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let row = ctx.props::<Self::Props>();
    Rect::new(100.0, row.height).background(row_color(row.id))
  }
}

struct VirtualizedTopRoot;

impl Component for VirtualizedTopRoot {
  type Props = (Vec<RowData>, Shared<AtomicUsize>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, top_reached) = ctx.props::<Self::Props>().clone();
    let reached = top_reached.0.clone();
    VirtualizedList::new(ctx, items)
      .size(100.0, 100.0)
      .overscan_px(0.0)
      .on_top_reached(move || {
        reached.fetch_add(1, Ordering::SeqCst);
      })
      .mount_keyed::<MeasuredRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

struct VirtualizedExternalScrollRoot;

impl Component for VirtualizedExternalScrollRoot {
  type Props = (Vec<RowData>, Shared<ScrollState>, Shared<AtomicUsize>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, scroll_state, scroll_events) = ctx.props::<Self::Props>().clone();
    let scroll_events = scroll_events.0.clone();
    VirtualizedList::new(ctx, items)
      .size(100.0, 100.0)
      .overscan_px(0.0)
      .with_scroll_state((*scroll_state.0).clone())
      .on_scroll(move |_| {
        scroll_events.fetch_add(1, Ordering::SeqCst);
      })
      .mount_keyed::<MeasuredRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

#[test]
fn virtualized_list_measures_all_rows_then_renders_visible_window() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(10), Shared(reached)));

  run_pass(&mut tree);
  let first = tree.last_layout().unwrap();
  assert_eq!(scroll_content_child_count(first), 10);

  run_pass(&mut tree);
  let second = tree.last_layout().unwrap();
  assert_eq!(scroll_content_child_count(second), 3);
  assert_eq!(scroll_content_height(second), 500.0);
}

#[test]
fn virtualized_list_updates_visible_window_after_scroll() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(10), Shared(reached)));

  run_pass(&mut tree);
  run_pass(&mut tree);

  tree.scroll(10.0, 10.0, 0.0, -150.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  let layout = tree.last_layout().unwrap();
  let content = scroll_content(layout);
  assert_eq!(scroll_offset(layout), 150.0);
  assert_eq!(content.children.len(), 4);
  assert_eq!(content.children[0].result.size.height, 150.0);
  assert_eq!(content.children[3].result.size.height, 250.0);
  assert_eq!(scroll_content_height(layout), 500.0);
}

#[test]
fn virtualized_list_accepts_external_scroll_state() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  let scroll_events = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedExternalScrollRoot>(
    &mut App::new(),
    (rows(10), Shared(scroll_state.clone()), Shared(scroll_events.clone())),
  );

  run_pass(&mut tree);
  run_pass(&mut tree);

  tree.scroll(10.0, 10.0, 0.0, -150.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  let layout = tree.last_layout().unwrap();
  let content = scroll_content(layout);
  assert_eq!(scroll_state.scroll_y(), 150.0);
  assert_eq!(scroll_offset(layout), 150.0);
  assert_eq!(content.children.len(), 4);
  assert_eq!(content.children[0].result.size.height, 150.0);
  assert_eq!(content.children[3].result.size.height, 250.0);
  assert_eq!(scroll_events.load(Ordering::SeqCst), 1);
}

#[test]
fn virtualized_list_forwards_bottom_reached_for_pagination() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(10), Shared(reached.clone())));

  run_pass(&mut tree);
  run_pass(&mut tree);

  tree.scroll(10.0, 10.0, 0.0, -1000.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  assert_eq!(reached.load(Ordering::SeqCst), 1);

  tree.scroll(10.0, 10.0, 0.0, -1000.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  assert_eq!(reached.load(Ordering::SeqCst), 1);
}

#[test]
fn virtualized_list_forwards_top_reached_for_pagination() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedTopRoot>(&mut App::new(), (rows(10), Shared(reached.clone())));

  run_pass(&mut tree);
  run_pass(&mut tree);

  tree.scroll(10.0, 10.0, 0.0, -150.0, ScrollPhase::Scroll);
  run_pass(&mut tree);
  assert_eq!(reached.load(Ordering::SeqCst), 0);

  tree.scroll(10.0, 10.0, 0.0, 200.0, ScrollPhase::Scroll);
  run_pass(&mut tree);
  assert_eq!(reached.load(Ordering::SeqCst), 1);

  tree.scroll(10.0, 10.0, 0.0, 200.0, ScrollPhase::Scroll);
  run_pass(&mut tree);
  assert_eq!(reached.load(Ordering::SeqCst), 1);
}

#[test]
fn virtualized_list_scrollbar_state_uses_full_measured_extent() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(10), Shared(reached)));

  run_pass(&mut tree);
  run_pass(&mut tree);

  tree.scroll(10.0, 10.0, 0.0, -1000.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  let layout = tree.last_layout().unwrap();
  let content = scroll_content(layout);
  assert_eq!(scroll_offset(layout), 400.0);
  assert_eq!(scroll_content_height(layout), 500.0);
  assert_eq!(content.children.len(), 3);
  assert_eq!(content.children[0].result.size.height, 400.0);
}

#[test]
fn virtualized_list_preserves_anchor_when_rows_are_prepended() {
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(10), Shared(reached.clone())));

  run_pass(&mut tree);
  run_pass(&mut tree);
  tree.scroll(10.0, 10.0, 0.0, -150.0, ScrollPhase::Scroll);
  run_pass(&mut tree);
  assert_eq!(scroll_offset(tree.last_layout().unwrap()), 150.0);

  let mut prepended = vec![RowData { id: 100, height: 50.0 }, RowData { id: 101, height: 50.0 }];
  prepended.extend(rows(10));
  tree.update_root_props::<VirtualizedRoot>((prepended, Shared(reached)));

  run_pass(&mut tree);
  assert!(scroll_content_child_count(tree.last_layout().unwrap()) < 12);
  run_pass(&mut tree);

  let layout = tree.last_layout().unwrap();
  assert_eq!(scroll_offset(layout), 250.0);
  assert_eq!(scroll_content_height(layout), 600.0);
}

fn rows(count: usize) -> Vec<RowData> {
  (0..count).map(|id| RowData { id, height: 50.0 }).collect()
}

fn row_color(index: usize) -> Color {
  Color::new(index as u8, 64, 128, 255)
}

fn scroll_content(layout: &lurq::layout::layout_result::LayoutResult) -> &lurq::layout::layout_result::LayoutResult {
  &layout.children[0].result
}

fn scroll_offset(layout: &lurq::layout::layout_result::LayoutResult) -> f32 {
  -layout.children[0].offset.y
}

fn scroll_content_child_count(layout: &lurq::layout::layout_result::LayoutResult) -> usize {
  scroll_content(layout).children.len()
}

fn scroll_content_height(layout: &lurq::layout::layout_result::LayoutResult) -> f32 {
  scroll_content(layout).size.height
}

// ── Viewport coverage under wheel scrolling ─────────────────────────────
// Mirrors the PW-studio text preview: uniform rows, overscan 600, ScrollBoth
// (horizontal panning). After every wheel event + redraw, every row that
// intersects the viewport must actually paint — a gap means the virtualized
// window went stale.

const COVERAGE_ROW_H: f32 = 22.0;
const COVERAGE_VIEW_W: f32 = 900.0;
const COVERAGE_VIEW_H: f32 = 700.0;
const COVERAGE_ROWS: usize = 5000;

fn coverage_color(id: usize) -> Color {
  Color::new((id & 0xff) as u8, ((id >> 8) & 0xff) as u8, 200, 255)
}

struct CoverageRow;

impl Component for CoverageRow {
  type Props = RowData;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let row = ctx.props::<Self::Props>();
    lurq::components::Row::new()
      .min_width(2400.0)
      .child(Rect::new(2400.0, row.height).background(coverage_color(row.id)))
  }
}

struct CoverageRoot;

impl Component for CoverageRoot {
  type Props = (Vec<RowData>, Shared<ScrollState>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, scroll_state) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(COVERAGE_VIEW_W, COVERAGE_VIEW_H)
      .overscan_px(600.0)
      .horizontal_scroll(true)
      .with_scroll_state((*scroll_state.0).clone())
      .mount_keyed::<CoverageRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

fn assert_viewport_covered(snapshot: &crate::support::RenderSnapshot, scroll_y: f32, label: &str) {
  let first = (scroll_y / COVERAGE_ROW_H).floor() as usize;
  let last = ((((scroll_y + COVERAGE_VIEW_H) / COVERAGE_ROW_H).ceil() as usize).min(COVERAGE_ROWS)).saturating_sub(1);
  for id in first..=last {
    let expected_y = id as f32 * COVERAGE_ROW_H - scroll_y;
    let found = snapshot
      .rects
      .iter()
      .any(|rect| rect.color == coverage_color(id) && (rect.y - expected_y).abs() < 0.5);
    assert!(
      found,
      "{label}: row {id} not painted at y={expected_y:.1} (scroll_y={scroll_y:.1})"
    );
  }
}

#[test]
fn virtualized_list_keeps_viewport_covered_while_wheel_scrolling() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<CoverageRoot>(
    &mut App::new(),
    (
      (0..COVERAGE_ROWS)
        .map(|id| RowData {
          id,
          height: COVERAGE_ROW_H,
        })
        .collect(),
      Shared(scroll_state.clone()),
    ),
  );

  // The default test window is 800x600 — smaller than the list, which would
  // cull bottom rows at the window edge and fake a coverage failure.
  tree.resize(1000, 800);

  // Bootstrap + settle (first pass measures the seed batch).
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);

  // Wheel patterns: slow ticks, medium steps, multi-event flicks (fast wheel
  // spins deliver several events per frame), then the same back up.
  let steps: &[&[f32]] = &[
    &[-120.0],
    &[-120.0],
    &[-40.0],
    &[-240.0, -240.0],
    &[-400.0],
    &[-120.0, -120.0, -120.0, -120.0, -120.0],
    &[-800.0],
    &[-2000.0, -2000.0],
    &[-120.0],
    &[120.0],
    &[400.0, 400.0],
    &[2500.0],
    &[-60.0],
    &[3000.0, 3000.0],
    &[-120.0],
  ];
  for (step, deltas) in steps.iter().enumerate() {
    for delta in deltas.iter() {
      tree.scroll(450.0, 350.0, 0.0, *delta, ScrollPhase::Scroll);
    }
    let snapshot = crate::support::render_pass(&mut tree);
    {
      let mut ids: Vec<usize> = snapshot
        .rects
        .iter()
        .filter(|r| r.color.a() == 255 && r.color.b() == 200)
        .map(|r| (r.color.r() as usize) | ((r.color.g() as usize) << 8))
        .collect();
      ids.sort_unstable();
      ids.dedup();
      eprintln!(
        "step {step} scroll_y={:.1} painted rows: {:?}..{:?} count={}",
        scroll_state.scroll_y(),
        ids.first(),
        ids.last(),
        ids.len()
      );
      for rect in snapshot.rects.iter() {
        if rect.color.a() == 255 && rect.color.b() == 200 {
          let id = (rect.color.r() as usize) | ((rect.color.g() as usize) << 8);
          if (30..=35).contains(&id) {
            eprintln!(
              "  row {id}: y={:.1} h={:.1} clip=({:.1},{:.1} {:.1}x{:.1} active={})",
              rect.y, rect.height, rect.clip.x, rect.clip.y, rect.clip.width, rect.clip.height, rect.clip.active
            );
          }
        }
      }
    }
    assert_viewport_covered(&snapshot, scroll_state.scroll_y(), &format!("step {step} after scroll"));
    // A settle pass (after_layout corrections) must not regress coverage.
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_covered(&snapshot, scroll_state.scroll_y(), &format!("step {step} settled"));
  }
}

#[test]
fn virtualized_list_keeps_viewport_covered_during_scrollbar_drag() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<CoverageRoot>(
    &mut App::new(),
    (
      (0..COVERAGE_ROWS)
        .map(|id| RowData {
          id,
          height: COVERAGE_ROW_H,
        })
        .collect(),
      Shared(scroll_state.clone()),
    ),
  );

  tree.resize(1000, 800);
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);

  // Grab the vertical thumb (right edge of the 900-wide list, top of track)
  // and drag in bursts — with 110k px of content each pixel of thumb travel
  // jumps the scroll by ~160px, so this exercises huge window jumps through
  // the scrollbar-drag dispatch path (delta=0, position pre-applied).
  tree.mouse_down(894.0, 6.0, MouseButton::Left);
  let mut y = 6.0;
  for burst in 0..12 {
    // Several move events per redraw, like a fast hand drag.
    for _ in 0..(1 + burst % 3) {
      y += 4.0;
      tree.mouse_move(894.0, y);
    }
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_covered(
      &snapshot,
      scroll_state.scroll_y(),
      &format!("drag burst {burst} (thumb y={y})"),
    );
  }
  // Drag back up.
  for burst in 0..6 {
    y -= 7.0;
    tree.mouse_move(894.0, y);
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_covered(
      &snapshot,
      scroll_state.scroll_y(),
      &format!("drag-up burst {burst} (thumb y={y})"),
    );
  }
  tree.mouse_up(894.0, y, MouseButton::Left);
  let snapshot = crate::support::render_pass(&mut tree);
  assert_viewport_covered(&snapshot, scroll_state.scroll_y(), "after release");
}

// ── In-place item swap while scrolled deep ───────────────────────────────
// Items are often rebuilt with identical keys but fresh allocations/props (a
// re-decoded document, a refreshed query). Evicting every measured height in
// that case used to collapse the prefix to the bootstrap window, clamping the
// scroll against the collapsed content (teleporting the viewport tens of
// thousands of pixels) and leaving the viewport blank.

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct SwapRowData {
  id: usize,
  generation: u32,
  height: f32,
}

struct SwapRow;

impl Component for SwapRow {
  type Props = SwapRowData;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let row = ctx.props::<Self::Props>();
    Rect::new(2400.0, row.height).background(coverage_color(row.id))
  }
}

struct SwapRoot;

impl Component for SwapRoot {
  type Props = (Vec<SwapRowData>, Shared<ScrollState>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, scroll_state) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(COVERAGE_VIEW_W, COVERAGE_VIEW_H)
      .overscan_px(600.0)
      .with_scroll_state((*scroll_state.0).clone())
      .mount_keyed::<SwapRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

fn swap_rows(generation: u32) -> Vec<SwapRowData> {
  (0..COVERAGE_ROWS)
    .map(|id| SwapRowData {
      id,
      generation,
      height: COVERAGE_ROW_H,
    })
    .collect()
}

/// The painted row rects must tile the viewport with no hole (heights vary,
/// so positions can't be predicted — coverage is asserted geometrically).
fn assert_swap_viewport_tiled(snapshot: &crate::support::RenderSnapshot, label: &str) {
  let mut spans: Vec<(f32, f32)> = snapshot
    .rects
    .iter()
    .filter(|rect| rect.color.a() == 255 && rect.color.b() == 200)
    .map(|rect| (rect.y, rect.y + rect.height))
    .collect();
  assert!(!spans.is_empty(), "{label}: no rows painted at all");
  spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
  let mut covered_to = spans[0].0;
  assert!(
    covered_to <= 1.0,
    "{label}: first painted row starts at y={covered_to:.1}"
  );
  for (top, bottom) in spans {
    assert!(
      top <= covered_to + 1.0,
      "{label}: hole in viewport between y={covered_to:.1} and y={top:.1}"
    );
    covered_to = covered_to.max(bottom);
    if covered_to >= COVERAGE_VIEW_H - 0.5 {
      return;
    }
  }
  panic!("{label}: painted rows end at y={covered_to:.1}, viewport is {COVERAGE_VIEW_H}");
}

/// Switching the same list (same keys/scroll state) to a document with
/// different row heights and a different row count — like previewing another
/// file in the same window. Old measurements are invalid, but the viewport
/// must stay covered and the scroll must stay within the new content.
#[test]
fn virtualized_list_survives_switch_to_different_document() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<SwapRoot>(&mut App::new(), (swap_rows(0), Shared(scroll_state.clone())));
  tree.resize(1000, 800);

  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);

  // Scroll deep into document A (5000 rows @ 22px).
  for _ in 0..40 {
    tree.scroll(450.0, 350.0, 0.0, -1000.0, ScrollPhase::Scroll);
  }
  let snapshot = crate::support::render_pass(&mut tree);
  assert_viewport_covered(&snapshot, scroll_state.scroll_y(), "document A");

  // Document B: 800 rows with varied heights (empty-line-ish rows mixed in).
  let doc_b: Vec<SwapRowData> = (0..800)
    .map(|id| SwapRowData {
      id,
      generation: 1,
      height: if id % 3 == 0 { 8.0 } else { 15.0 },
    })
    .collect();
  tree.update_root_props::<SwapRoot>((doc_b.clone(), Shared(scroll_state.clone())));
  let content_b: f32 = doc_b.iter().map(|row| row.height).sum();

  // Settle a couple frames (row heights genuinely changed — re-measure is
  // expected), then the viewport must be covered and stay covered while
  // wheel-scrolling through document B.
  crate::support::render_pass(&mut tree);
  crate::support::render_pass(&mut tree);
  let snapshot = crate::support::render_pass(&mut tree);
  assert!(
    scroll_state.scroll_y() <= content_b,
    "scroll {:.1} beyond document B content {content_b:.1}",
    scroll_state.scroll_y()
  );
  assert_swap_viewport_tiled(&snapshot, "after switching to document B");

  // Wheel up and down through the new document.
  for tick in 0..80usize {
    let delta = if tick % 5 == 4 { 400.0 } else { -160.0 };
    tree.scroll(450.0, 350.0, 0.0, delta, ScrollPhase::Scroll);
    let snapshot = crate::support::render_pass(&mut tree);
    assert_swap_viewport_tiled(&snapshot, &format!("document B tick {tick}"));
    let snapshot = crate::support::render_pass(&mut tree);
    assert_swap_viewport_tiled(&snapshot, &format!("document B tick {tick} settled"));
  }
}

#[test]
fn virtualized_list_survives_in_place_item_swap_at_depth() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<SwapRoot>(&mut App::new(), (swap_rows(0), Shared(scroll_state.clone())));
  tree.resize(1000, 800);

  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);

  // Scroll deep, then verify the viewport is covered.
  for _ in 0..40 {
    tree.scroll(450.0, 350.0, 0.0, -1000.0, ScrollPhase::Scroll);
  }
  let snapshot = crate::support::render_pass(&mut tree);
  let depth = scroll_state.scroll_y();
  assert!(depth > 30_000.0, "expected a deep scroll, got {depth}");
  assert_viewport_covered(&snapshot, depth, "before swap");

  // Same keys, new generation: every item compares unequal. The next frame
  // must keep the scroll anchored and the viewport covered.
  tree.update_root_props::<SwapRoot>((swap_rows(1), Shared(scroll_state.clone())));
  let snapshot = crate::support::render_pass(&mut tree);
  assert!(
    (scroll_state.scroll_y() - depth).abs() < COVERAGE_VIEW_H,
    "swap teleported the scroll: was {depth:.1}, now {:.1}",
    scroll_state.scroll_y()
  );
  assert_viewport_covered(&snapshot, scroll_state.scroll_y(), "first frame after swap");

  // A producer may swap every frame; the list must stay anchored anyway.
  for generation in 2..20u32 {
    tree.update_root_props::<SwapRoot>((swap_rows(generation), Shared(scroll_state.clone())));
    tree.scroll(450.0, 350.0, 0.0, -120.0, ScrollPhase::Scroll);
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_covered(
      &snapshot,
      scroll_state.scroll_y(),
      &format!("repeated swap generation {generation}"),
    );
  }
}

// ── Preview-shaped rows: text lines with uneven heights ─────────────────
// Mirrors the PW-studio text preview rows exactly: a background-tagged Row
// with line-number gutter + selectable nowrap text, extra padding on the
// first/last row, empty lines mixed in (their text node may collapse), 30k
// rows. Coverage is asserted geometrically: the painted row rects must tile
// the viewport without holes.

const PREVIEW_ROWS: usize = 30_000;

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct PreviewLine {
  id: usize,
  count: usize,
}

struct PreviewLikeRow;

impl Component for PreviewLikeRow {
  type Props = PreviewLine;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let line = ctx.props::<Self::Props>().clone();
    let text = if line.id % 7 == 3 {
      String::new()
    } else {
      format!("npc {id} limit info entry value {id}", id = line.id)
    };
    let pad_top = if line.id == 0 { 14.0 } else { 2.0 };
    let pad_bottom = if line.id + 1 == line.count { 14.0 } else { 2.0 };
    lurq::components::Row::new()
      .min_width(2400.0)
      .background(coverage_color(line.id))
      .padding(
        lurq::node::padding::Padding::new()
          .left(16.0)
          .right(24.0)
          .top(pad_top)
          .bottom(pad_bottom),
      )
      .child(
        lurq::components::Row::new()
          .width(64.0)
          .child(lurq::components::Text::new(&(line.id + 1).to_string())),
      )
      .child(lurq::components::Text::new(&text).nowrap().selectable(true))
  }
}

struct PreviewLikeRoot;

impl Component for PreviewLikeRoot {
  // The `u64` nonce forces parent re-renders without changing the items —
  // mirroring a window shell re-rendering on every input event.
  type Props = (Vec<PreviewLine>, Shared<ScrollState>, u64);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, scroll_state, _nonce) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(COVERAGE_VIEW_W, COVERAGE_VIEW_H)
      .overscan_px(600.0)
      .horizontal_scroll(true)
      .with_scroll_state((*scroll_state.0).clone())
      .mount_keyed::<PreviewLikeRow, _, _, _>(|line| line.id, |line| line.clone())
  }
}

/// The painted row rects must cover the viewport top-to-bottom with no hole
/// wider than a hairline (spacer gaps mean stale windowing).
fn assert_viewport_tiled(snapshot: &crate::support::RenderSnapshot, label: &str) {
  let mut spans: Vec<(f32, f32)> = snapshot
    .rects
    .iter()
    .filter(|rect| rect.color.a() == 255 && rect.color.b() == 200)
    .map(|rect| (rect.y, rect.y + rect.height))
    .collect();
  assert!(!spans.is_empty(), "{label}: no rows painted at all");
  spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
  let mut covered_to = spans[0].0;
  assert!(
    covered_to <= 1.0,
    "{label}: first painted row starts at y={covered_to:.1}"
  );
  for (top, bottom) in spans {
    assert!(
      top <= covered_to + 1.0,
      "{label}: hole in viewport between y={covered_to:.1} and y={top:.1}"
    );
    covered_to = covered_to.max(bottom);
    if covered_to >= COVERAGE_VIEW_H - 0.5 {
      return;
    }
  }
  panic!("{label}: painted rows end at y={covered_to:.1}, viewport is {COVERAGE_VIEW_H}");
}

#[test]
fn virtualized_list_preview_rows_stay_tiled_under_mixed_scrolling() {
  let mut tree = Tree::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<PreviewLikeRoot>(
    &mut App::new(),
    (
      (0..PREVIEW_ROWS)
        .map(|id| PreviewLine {
          id,
          count: PREVIEW_ROWS,
        })
        .collect(),
      Shared(scroll_state.clone()),
      0,
    ),
  );

  tree.resize(1000, 800);
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);
  crate::support::run_pass(&mut tree);

  // Wheel bursts down.
  for step in 0..10 {
    for _ in 0..(1 + step % 3) {
      tree.scroll(450.0, 350.0, 0.0, -120.0, ScrollPhase::Scroll);
    }
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_tiled(&snapshot, &format!("wheel step {step}"));
  }

  // Thumb drag: deep jumps through unmeasured territory and back.
  tree.mouse_down(894.0, 8.0, MouseButton::Left);
  let mut y = 8.0;
  for burst in 0..14 {
    for _ in 0..(1 + burst % 3) {
      y += 5.0;
      tree.mouse_move(894.0, y);
    }
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_tiled(&snapshot, &format!("drag burst {burst} (thumb y={y})"));
  }
  for burst in 0..8 {
    y -= 9.0;
    tree.mouse_move(894.0, y);
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_tiled(&snapshot, &format!("drag-up burst {burst} (thumb y={y})"));
  }
  tree.mouse_up(894.0, y, MouseButton::Left);

  // Wheel again where heights are now part-measured, part-estimated.
  for step in 0..6 {
    tree.scroll(
      450.0,
      350.0,
      0.0,
      if step % 2 == 0 { -400.0 } else { 250.0 },
      ScrollPhase::Scroll,
    );
    let snapshot = crate::support::render_pass(&mut tree);
    assert_viewport_tiled(&snapshot, &format!("post-drag wheel step {step}"));
  }
}

/// Real event loops coalesce many wheel ticks (or drag moves) into one paint:
/// several window rebuilds get diffed against retained trees that were never
/// laid out in between. The viewport must still be tiled at every paint.
#[test]
fn virtualized_list_preview_rows_stay_tiled_with_coalesced_event_bursts() {
  let items: Vec<PreviewLine> = (0..PREVIEW_ROWS)
    .map(|id| PreviewLine {
      id,
      count: PREVIEW_ROWS,
    })
    .collect();
  let mut tree = Tree::new();
  // Persistent App so incremental (non-force-dirtied) relayout actually runs.
  let mut app = App::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<PreviewLikeRoot>(&mut app, (items.clone(), Shared(scroll_state.clone()), 0));

  tree.resize(1000, 800);
  let paint = crate::support::render_pass_with_app;
  paint(&mut tree, &mut app);
  paint(&mut tree, &mut app);
  paint(&mut tree, &mut app);

  // Bursts of 10..=59 wheel ticks (400..=2360px) between paints — several
  // chained drift-rebuilds per frame, with a parent re-render interleaved
  // after every tick like the real window shell — down, then back up.
  let mut nonce = 0u64;
  for step in 0..80usize {
    let burst = 10 + (step * 7) % 50;
    for _ in 0..burst {
      tree.scroll(450.0, 350.0, 0.0, -40.0, ScrollPhase::Scroll);
      nonce += 1;
      tree.update_root_props::<PreviewLikeRoot>((items.clone(), Shared(scroll_state.clone()), nonce));
    }
    let snapshot = paint(&mut tree, &mut app);
    assert_viewport_tiled(
      &snapshot,
      &format!(
        "burst-down step {step} ({burst} ticks) scroll_y={:.1}",
        scroll_state.scroll_y()
      ),
    );
  }
  for step in 0..80usize {
    let burst = 10 + (step * 11) % 50;
    for _ in 0..burst {
      tree.scroll(450.0, 350.0, 0.0, 40.0, ScrollPhase::Scroll);
      nonce += 1;
      tree.update_root_props::<PreviewLikeRoot>((items.clone(), Shared(scroll_state.clone()), nonce));
    }
    let snapshot = paint(&mut tree, &mut app);
    assert_viewport_tiled(
      &snapshot,
      &format!(
        "burst-up step {step} ({burst} ticks) scroll_y={:.1}",
        scroll_state.scroll_y()
      ),
    );
  }
}

/// The real shell re-renders the window component on nearly every input
/// event, so the list rebuilds twice per wheel tick (drift bump + parent
/// re-render) — producing back-to-back duplicate window builds diffed against
/// retained trees that were never laid out. Combined with coalesced events
/// this is the exact production event pattern.
#[test]
fn virtualized_list_preview_rows_stay_tiled_with_parent_rerenders() {
  let items: Vec<PreviewLine> = (0..PREVIEW_ROWS)
    .map(|id| PreviewLine {
      id,
      count: PREVIEW_ROWS,
    })
    .collect();
  let mut tree = Tree::new();
  // One App for the whole test, like the real shell. `support::render_pass`
  // creates a fresh App per pass, which flips `theme_changed` and force-dirties
  // every layout — silently bypassing the incremental relayout path this test
  // exists to exercise.
  let mut app = App::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<PreviewLikeRoot>(&mut app, (items.clone(), Shared(scroll_state.clone()), 0));

  tree.resize(1000, 800);

  let paint = crate::support::render_pass_with_app;
  paint(&mut tree, &mut app);
  paint(&mut tree, &mut app);
  paint(&mut tree, &mut app);

  // Wheel down with a parent re-render after every tick, painting only every
  // few events (coalescing), then the same pattern back up.
  let mut nonce = 0u64;
  for step in 0..600usize {
    tree.scroll(450.0, 350.0, 0.0, -40.0, ScrollPhase::Scroll);
    nonce += 1;
    tree.update_root_props::<PreviewLikeRoot>((items.clone(), Shared(scroll_state.clone()), nonce));
    if step % 4 != 3 {
      continue;
    }
    let snapshot = paint(&mut tree, &mut app);
    assert_viewport_tiled(
      &snapshot,
      &format!("rerender-down step {step} scroll_y={:.1}", scroll_state.scroll_y()),
    );
  }
  for step in 0..300usize {
    tree.scroll(450.0, 350.0, 0.0, 40.0, ScrollPhase::Scroll);
    nonce += 1;
    tree.update_root_props::<PreviewLikeRoot>((items.clone(), Shared(scroll_state.clone()), nonce));
    if step % 4 != 3 {
      continue;
    }
    let snapshot = paint(&mut tree, &mut app);
    assert_viewport_tiled(
      &snapshot,
      &format!("rerender-up step {step} scroll_y={:.1}", scroll_state.scroll_y()),
    );
  }
}

// ── Randomized interleaving fuzz ─────────────────────────────────────────
// Ten targeted event-pattern tests failed to reproduce a production blank
// that provably exists (stale spacer geometry served from a clean cache), so
// this fuzzes the interleavings: wheel ticks, root re-renders (`rebuild()`
// preserves caches from the live laid tree), paints, and viewport resizes.
// On failure the seed + action log pinpoint the minimal sequence.

#[test]
fn virtualized_list_fuzz_scroll_rerender_paint() {
  for seed in 1..=6u64 {
    fuzz_one(seed);
  }
}

fn fuzz_one(seed: u64) {
  let items: Vec<PreviewLine> = (0..PREVIEW_ROWS)
    .map(|id| PreviewLine {
      id,
      count: PREVIEW_ROWS,
    })
    .collect();
  let mut tree = Tree::new();
  let mut app = App::new();
  let scroll_state = Arc::new(ScrollState::new());
  tree.mount_root::<PreviewLikeRoot>(&mut app, (items.clone(), Shared(scroll_state.clone()), 0));
  tree.resize(1000, 800);
  crate::support::render_pass_with_app(&mut tree, &mut app);
  crate::support::render_pass_with_app(&mut tree, &mut app);

  let mut rng = seed.wrapping_mul(0x9E3779B97F4A7C15).max(1);
  let mut next = |bound: u64| {
    rng ^= rng << 13;
    rng ^= rng >> 7;
    rng ^= rng << 17;
    rng % bound
  };

  let mut nonce = 0u64;
  let mut log: std::collections::VecDeque<String> = std::collections::VecDeque::new();
  let push_log = |log: &mut std::collections::VecDeque<String>, entry: String| {
    log.push_back(entry);
    if log.len() > 40 {
      log.pop_front();
    }
  };

  for step in 0..4000usize {
    match next(100) {
      // Wheel down (most common), wheel up.
      0..=44 => {
        let ticks = 1 + next(6);
        for _ in 0..ticks {
          tree.scroll(450.0, 350.0, 0.0, -40.0, ScrollPhase::Scroll);
        }
        push_log(&mut log, format!("wheel-down x{ticks}"));
      }
      45..=64 => {
        let ticks = 1 + next(6);
        for _ in 0..ticks {
          tree.scroll(450.0, 350.0, 0.0, 40.0, ScrollPhase::Scroll);
        }
        push_log(&mut log, format!("wheel-up x{ticks}"));
      }
      // Root re-render (window shell noise).
      65..=84 => {
        nonce += 1;
        tree.update_root_props::<PreviewLikeRoot>((items.clone(), Shared(scroll_state.clone()), nonce));
        push_log(&mut log, "rerender".to_owned());
      }
      // Paint.
      85..=97 => {
        let snapshot = crate::support::render_pass_with_app(&mut tree, &mut app);
        push_log(&mut log, format!("paint scroll={:.1}", scroll_state.scroll_y()));
        let label = format!(
          "fuzz seed={seed} step={step} scroll_y={:.1}\nlast actions: {:?}",
          scroll_state.scroll_y(),
          log
        );
        assert_viewport_tiled(&snapshot, &label);
      }
      // Rare viewport resize (also heals in production — keep rare).
      _ => {
        let height = 700 + (next(4) * 50) as u32;
        tree.resize(1000, height);
        push_log(&mut log, format!("resize h={height}"));
      }
    }
  }
}

// ── Timing harness (run explicitly) ─────────────────────────────────────
// cargo test -p lurq --features "winit wgpu image svg resources clipboard" \
//   virtualized_list_scroll_timing -- --ignored --nocapture

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct TimingLine {
  id: usize,
  text: std::sync::Arc<str>,
}

struct TimingRow;

impl Component for TimingRow {
  type Props = TimingLine;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let line = ctx.props::<Self::Props>().clone();
    lurq::components::Row::new()
      .min_width(2400.0)
      .child(
        lurq::components::Row::new()
          .width(48.0)
          .child(lurq::components::Text::new(&(line.id + 1).to_string())),
      )
      .child(lurq::components::Text::new(&line.text).nowrap().selectable(true))
  }
}

struct TimingRoot;

impl Component for TimingRoot {
  type Props = Shared<Vec<TimingLine>>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let items = ctx.props::<Self::Props>().0.as_ref().clone();
    VirtualizedList::new(ctx, items)
      .size(900.0, 700.0)
      .horizontal_scroll(true)
      .mount_keyed::<TimingRow, _, _, _>(|line| line.id, |line| line.clone())
  }
}

#[test]
#[ignore]
fn virtualized_list_scroll_timing() {
  use std::time::Instant;

  let lines: Vec<TimingLine> = (0..10_000)
    .map(|id| TimingLine {
      id,
      text: std::sync::Arc::from(format!(
        "<TerrainRoad id=\"{id}\" texture=\"surfaces/road/autobase_{id}.dds\" \
         u0=\"0.125\" v0=\"0.25\" u1=\"0.875\" v1=\"0.75\" blend=\"true\" layer=\"3\"/>"
      )),
    })
    .collect();

  let mut tree = Tree::new();
  let mut app = App::new();
  tree.mount_root::<TimingRoot>(&mut app, Shared(Arc::new(lines)));

  let mut timed_pass = |tree: &mut Tree, label: &str| {
    let started = Instant::now();
    tree.request_redraw();
    tree.pass(&mut app, &crate::support::TestSurface);
    eprintln!("{label}: {:.2}ms", started.elapsed().as_secs_f64() * 1000.0);
  };

  timed_pass(&mut tree, "bootstrap");
  timed_pass(&mut tree, "settle-1");
  timed_pass(&mut tree, "settle-2");

  for step in 0..12 {
    let started = Instant::now();
    tree.scroll(400.0, 300.0, 0.0, -400.0, ScrollPhase::Scroll);
    let scroll_ms = started.elapsed().as_secs_f64() * 1000.0;
    eprintln!("scroll-{step}: event {scroll_ms:.2}ms");
    timed_pass(&mut tree, &format!("  pass-{step}"));
    timed_pass(&mut tree, &format!("  settle-{step}"));
  }
}

struct PlainTextRow;

impl Component for PlainTextRow {
  type Props = TimingLine;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let line = ctx.props::<Self::Props>().clone();
    lurq::components::Row::new()
      .min_width(2400.0)
      .child(lurq::components::Text::new(&line.text).nowrap())
  }
}

struct RectRow;

impl Component for RectRow {
  type Props = TimingLine;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Rect::new(2400.0, 22.0).background(Color::from_hex("#334455"))
  }
}

struct PlainTimingRoot;

impl Component for PlainTimingRoot {
  type Props = Shared<Vec<TimingLine>>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let items = ctx.props::<Self::Props>().0.as_ref().clone();
    VirtualizedList::new(ctx, items)
      .size(900.0, 700.0)
      .mount_keyed::<PlainTextRow, _, _, _>(|line| line.id, |line| line.clone())
  }
}

struct RectTimingRoot;

impl Component for RectTimingRoot {
  type Props = Shared<Vec<TimingLine>>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let items = ctx.props::<Self::Props>().0.as_ref().clone();
    VirtualizedList::new(ctx, items)
      .size(900.0, 700.0)
      .mount_keyed::<RectRow, _, _, _>(|line| line.id, |line| line.clone())
  }
}

fn timing_lines() -> Vec<TimingLine> {
  (0..10_000)
    .map(|id| TimingLine {
      id,
      text: std::sync::Arc::from(format!(
        "<TerrainRoad id=\"{id}\" texture=\"surfaces/road/autobase_{id}.dds\" \
         u0=\"0.125\" v0=\"0.25\" u1=\"0.875\" v1=\"0.75\" blend=\"true\" layer=\"3\"/>"
      )),
    })
    .collect()
}

fn run_timing<R>(label: &str)
where
  R: Component<Props = Shared<Vec<TimingLine>>>,
{
  use std::time::Instant;
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.mount_root::<R>(&mut app, Shared(Arc::new(timing_lines())));

  let mut timed_pass = |tree: &mut Tree, label: String| {
    let started = Instant::now();
    tree.request_redraw();
    tree.pass(&mut app, &crate::support::TestSurface);
    eprintln!("{label}: {:.2}ms", started.elapsed().as_secs_f64() * 1000.0);
  };

  timed_pass(&mut tree, format!("[{label}] bootstrap"));
  timed_pass(&mut tree, format!("[{label}] settle"));
  for step in 0..3 {
    tree.scroll(400.0, 300.0, 0.0, -400.0, ScrollPhase::Scroll);
    timed_pass(&mut tree, format!("[{label}] jump-pass-{step}"));
    timed_pass(&mut tree, format!("[{label}] jump-settle-{step}"));
  }
  for step in 0..3 {
    tree.scroll(400.0, 300.0, 0.0, -160.0, ScrollPhase::Scroll);
    timed_pass(&mut tree, format!("[{label}] step-pass-{step}"));
  }
}

#[test]
#[ignore]
fn virtualized_list_scroll_timing_variants() {
  run_timing::<RectTimingRoot>("rect");
  run_timing::<PlainTimingRoot>("plain-text");
  run_timing::<TimingRoot>("selectable-text");
}

struct VirtualizedSharedRoot;

impl Component for VirtualizedSharedRoot {
  type Props = (Shared<Vec<RowData>>,);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items,) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new_shared(ctx, items.0)
      .size(100.0, 100.0)
      .overscan_px(0.0)
      .mount_keyed::<MeasuredRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

#[test]
fn virtualized_list_new_shared_windows_without_copying_the_items() {
  let mut tree = Tree::new();
  let items = Arc::new(rows(10));
  tree.mount_root::<VirtualizedSharedRoot>(&mut App::new(), (Shared(items.clone()),));

  run_pass(&mut tree);
  run_pass(&mut tree);
  let layout = tree.last_layout().unwrap();
  // Same windowing behaviour as `new`…
  assert_eq!(scroll_content_child_count(layout), 3);
  assert_eq!(scroll_content_height(layout), 500.0);
  // …and the list holds the caller's allocation instead of a copy.
  assert!(Arc::strong_count(&items) >= 2);
}

struct VirtualizedRevealRoot;

impl Component for VirtualizedRevealRoot {
  type Props = (Vec<RowData>, Option<String>);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, reveal) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(100.0, 100.0)
      .overscan_px(0.0)
      .reveal_key(reveal)
      .mount_keyed::<MeasuredRow, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

#[test]
fn deep_reveal_scrolls_from_estimates_without_measuring_every_row() {
  let mut tree = Tree::new();
  // A list far larger than the viewport, revealing a row near the end. The
  // reveal must come from the height estimates — mounting and measuring all
  // 1000 rows to place it caused seconds of first-render jank on big tables.
  tree.mount_root::<VirtualizedRevealRoot>(&mut App::new(), (rows(1000), Some("900".to_owned())));

  let mut max_children = 0;
  for _ in 0..6 {
    run_pass(&mut tree);
    max_children = max_children.max(scroll_content_child_count(tree.last_layout().unwrap()));
  }

  let layout = tree.last_layout().unwrap();
  // Uniform 50px rows: row 900's exact top. The estimate equals the real
  // height, so the reveal converges on the exact offset.
  assert_eq!(scroll_offset(layout), 45_000.0);
  assert_eq!(scroll_content_height(layout), 50_000.0);
  // Row wrappers plus spacers only — never the whole list.
  assert!(
    max_children < 80,
    "reveal mounted {max_children} children; it must stay windowed"
  );
}

#[test]
fn switching_to_an_all_new_dataset_never_marches_through_it() {
  // Same list key, entirely different row keys — switching record categories.
  // The stale anchor has no meaning in the new dataset; restoring it used to
  // measure-march every new row (first-render jank on multi-thousand-row
  // tables).
  let mut tree = Tree::new();
  let reached = Arc::new(AtomicUsize::new(0));
  tree.mount_root::<VirtualizedRoot>(&mut App::new(), (rows(1000), Shared(reached.clone())));
  run_pass(&mut tree);
  run_pass(&mut tree);
  tree.scroll(10.0, 10.0, 0.0, -2000.0, ScrollPhase::Scroll);
  run_pass(&mut tree);

  // New dataset: disjoint ids.
  let replacement: Vec<RowData> = (5000..6000).map(|id| RowData { id, height: 50.0 }).collect();
  tree.update_root_props::<VirtualizedRoot>((replacement, Shared(reached)));

  let mut max_children = 0;
  for _ in 0..6 {
    run_pass(&mut tree);
    max_children = max_children.max(scroll_content_child_count(tree.last_layout().unwrap()));
  }
  assert!(
    max_children < 80,
    "dataset switch mounted {max_children} children; it must stay windowed"
  );
  assert_eq!(scroll_content_height(tree.last_layout().unwrap()), 50_000.0);
}
