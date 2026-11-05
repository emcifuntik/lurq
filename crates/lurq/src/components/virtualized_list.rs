use std::{
  collections::{HashMap, HashSet},
  fmt::Display,
  marker::PhantomData,
  sync::Arc,
};

use parking_lot::Mutex;

use super::{Column, ScrollBoth, ScrollVertical, Spacer};
use crate::{
  app::{component::Component, ctx::Ctx, events::ScrollEvent},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::{FrameConstraints, ScrollState},
    scrollbar::ScrollBarStyle,
  },
  node::{Element, EventHandler, IntoScrollEventHandler, dimension::Dimension},
};

const DEFAULT_OVERSCAN_PX: f32 = 256.0;
const HEIGHT_EPSILON: f32 = 0.01;
/// How far a revealed row's top may sit from the viewport's top and still
/// count as shown there.
const REVEAL_EPSILON: f32 = 0.5;

/// `LURQ_VLIST_DEBUG=1` paints diagnostics over every virtualized list: row
/// wrappers are tinted by the window-build generation (a stale reused subtree
/// keeps its old tint), and a stats panel shows the live windowing state.
fn debug_overlay_enabled() -> bool {
  static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
  *ENABLED.get_or_init(|| {
    std::env::var("LURQ_VLIST_DEBUG")
      .map(|value| value != "0" && !value.is_empty())
      .unwrap_or(false)
  })
}

/// Distinct translucent tints, rotated per window build.
fn debug_build_tint(build_seq: u64) -> crate::node::color::Color {
  const PALETTE: [(u8, u8, u8); 6] = [
    (46, 204, 113),
    (52, 152, 219),
    (155, 89, 182),
    (241, 196, 15),
    (230, 126, 34),
    (231, 76, 60),
  ];
  let (r, g, b) = PALETTE[(build_seq % PALETTE.len() as u64) as usize];
  crate::node::color::Color::new(r, g, b, 36)
}
/// Upper bound on not-yet-measured rows mounted per frame while a pending
/// scroll anchor needs exact heights.
const MEASURE_CHUNK: usize = 256;
/// Rows rendered on the very first pass, before any height is known, from
/// where the list opens; their measured heights seed the estimate that sizes
/// the rest of the list. See [`VirtualizedList::bootstrap_rows`].
const DEFAULT_BOOTSTRAP_ROWS: usize = 16;

/// The height an app gives a row before the list measured it; see
/// [`VirtualizedList::estimate_height`].
type RowEstimate<T> = Arc<dyn Fn(&T) -> f32 + Send + Sync>;

#[derive(Clone, Default)]
struct VirtualizedListOptions {
  frame: FrameConstraints,
  flex: Option<f32>,
  overscan_px: f32,
  scroll_state: Option<ScrollState>,
  scrollbar: Option<ScrollBarStyle>,
  scrollbar_hovered: Option<Arc<dyn Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync>>,
  scroll_handlers: Vec<EventHandler<ScrollEvent>>,
  top_handlers: Vec<EventHandler<ScrollEvent>>,
  bottom_handlers: Vec<EventHandler<ScrollEvent>>,
  list_key: Option<String>,
  /// Rows rendered before any height is known; see
  /// [`VirtualizedList::bootstrap_rows`].
  bootstrap_rows: usize,
  /// Scroll horizontally too (rows wider than the viewport pan; virtualization
  /// still works off the vertical axis only).
  horizontal: bool,
  /// A row key to scroll into view: revealed once when this value changes
  /// (and on the first render), and only when the row is not fully visible —
  /// an already-visible row does not move. Used to restore a selection after
  /// navigation without persisting scroll offsets.
  reveal_key: Option<String>,
  /// Middle-button autoscroll; see [`VirtualizedList::middle_button_autoscroll`].
  middle_button_autoscroll: bool,
}

/// A vertically scrolling list that mounts only the rows in and near its
/// viewport (`overscan_px` beyond each edge), so lists of any length cost
/// about as much as one screen of rows. Each row is a component mounted with
/// [`mount_keyed`](Self::mount_keyed): `key_fn` gives the row's stable key,
/// `props_fn` its props. Row heights are measured as rows are laid out;
/// rows never measured count as the average measured height, or as the
/// height [`estimate_height`](Self::estimate_height) gives them.
///
/// When measured heights change (a row grows, rows scrolled to measure other
/// than their estimate), the first row in view stays where it is on screen:
/// the offset moves with the content above it. A list at its end stays at
/// its end instead.
///
/// # Row state
///
/// A row that leaves the rendered window is unmounted, and created again
/// from its props when it comes back: whatever the row component holds
/// itself — fields set in `create`, signals from `ctx.signal`, element refs,
/// a text input's value signal created by the row, focus — starts over.
/// Keep state that must survive scrolling above the list, keyed by row key:
/// in the items themselves when it is data, or in a signal or store the
/// screen owns (for example `Signal<HashMap<Key, RowState>>`, or a
/// `HashMap<Key, Signal<String>>` of text-input values) and hands to rows
/// with `ctx.provide` / `ctx.use_context` or through their props. Remove
/// entries whose keys left the items when the items change.
///
/// ```ignore
/// // In the screen's `create`: one map for every row, keyed like the rows.
/// let expanded: Signal<HashMap<TaskId, bool>> = ctx.signal(HashMap::new());
/// ctx.provide(expanded);
///
/// // In the row: read its entry (subscribes) and write it on toggle.
/// let expanded = ctx.use_context::<Signal<HashMap<TaskId, bool>>>().expect("provided by the screen");
/// let open = expanded.with(|rows| rows.get(&task.id).copied().unwrap_or(false));
/// ```
pub struct VirtualizedList<'a, T> {
  ctx: &'a mut Ctx,
  items: Arc<Vec<T>>,
  options: VirtualizedListOptions,
  estimate: Option<RowEstimate<T>>,
}

impl<'a, T> VirtualizedList<'a, T> {
  pub fn new(ctx: &'a mut Ctx, items: impl IntoIterator<Item = T>) -> Self {
    Self::new_shared(ctx, Arc::new(items.into_iter().collect()))
  }

  /// Build the list from an already-shared item vector. Callers that memoize
  /// their (potentially huge) item list across re-renders pass the same `Arc`
  /// back in, so a re-render costs neither a rebuild nor a copy of the items.
  pub fn new_shared(ctx: &'a mut Ctx, items: Arc<Vec<T>>) -> Self {
    Self {
      ctx,
      items,
      options: VirtualizedListOptions {
        overscan_px: DEFAULT_OVERSCAN_PX,
        bootstrap_rows: DEFAULT_BOOTSTRAP_ROWS,
        ..VirtualizedListOptions::default()
      },
      estimate: None,
    }
  }

  pub fn list_key(mut self, key: impl Into<String>) -> Self {
    self.options.list_key = Some(key.into());
    self
  }

  pub fn overscan_px(mut self, overscan_px: f32) -> Self {
    self.options.overscan_px = overscan_px.max(0.0);
    self
  }

  /// How many rows the list renders before it has measured one (16 by
  /// default): the viewport is not laid out yet, so the first pass cannot
  /// size its window, and these rows' heights seed the estimate the next
  /// pass sizes it with. They are taken from where the list opens: its end
  /// when `scroll_to_bottom_pending` was queued on its scroll state, the row
  /// of its [`reveal_key`](Self::reveal_key), or else its top. A list whose
  /// rows are tall (a chat of long replies) passes fewer, one whose rows are
  /// short passes about a viewport's worth so its first frame is full.
  pub fn bootstrap_rows(mut self, rows: usize) -> Self {
    self.options.bootstrap_rows = rows.max(1);
    self
  }

  /// The height of a row the list has not measured yet, instead of the
  /// average of the rows it measured. A list whose rows differ much in height
  /// (a chat of short messages and long outputs) estimates them by kind, so
  /// one tall row measured does not move every row not measured with it, and
  /// the list is sized from the start without bootstrap rows. Taken as the
  /// same estimate on every render: an estimate that changes for the same
  /// items is not seen until the items change.
  pub fn estimate_height(mut self, estimate: impl Fn(&T) -> f32 + Send + Sync + 'static) -> Self {
    self.estimate = Some(Arc::new(move |item| estimate(item).max(0.0)));
    self
  }

  /// Also scroll horizontally: rows wider than the viewport pan sideways.
  /// Virtualization still windows on the vertical axis only.
  pub fn horizontal_scroll(mut self, enabled: bool) -> Self {
    self.options.horizontal = enabled;
    self
  }

  /// Scroll the row with this key into view: the reveal fires once each time
  /// the key changes (including the first render, so a list mounted with a
  /// key opens on that row), and only if the row is not fully visible — an
  /// already-visible row does not move; an off-screen row is scrolled to the
  /// top of the viewport. Pass the current selection's row key to restore it
  /// after navigation without persisting the scroll offset. A key that is not
  /// among the items is dropped, except while the list is still empty: then
  /// the reveal waits for the items to arrive.
  ///
  /// This is the way to reveal a row that is not mounted (outside the
  /// viewport and overscan); an element inside a mounted row can also be
  /// revealed with [`Ctx::scroll_into_view`].
  pub fn reveal_key(mut self, key: Option<String>) -> Self {
    self.options.reveal_key = key;
    self
  }

  pub fn width(mut self, width: impl Into<Dimension>) -> Self {
    self.options.frame.width = Some(width.into());
    self
  }

  pub fn height(mut self, height: impl Into<Dimension>) -> Self {
    self.options.frame.height = Some(height.into());
    self
  }

  pub fn size(mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    self.options.frame.width = Some(width.into());
    self.options.frame.height = Some(height.into());
    self
  }

  pub fn min_width(mut self, width: impl Into<Dimension>) -> Self {
    self.options.frame.min_width = Some(width.into());
    self
  }

  pub fn max_width(mut self, width: impl Into<Dimension>) -> Self {
    self.options.frame.max_width = Some(width.into());
    self
  }

  pub fn min_height(mut self, height: impl Into<Dimension>) -> Self {
    self.options.frame.min_height = Some(height.into());
    self
  }

  /// Cap the list height. Without a fixed `height` (or `flex`), the list
  /// sizes to its content up to this cap and scrolls beyond it.
  pub fn max_height(mut self, height: impl Into<Dimension>) -> Self {
    self.options.frame.max_height = Some(height.into());
    self
  }

  pub fn flex(mut self, flex: f32) -> Self {
    self.options.flex = Some(flex);
    self
  }

  pub fn scrollbar(mut self, style: ScrollBarStyle) -> Self {
    self.options.scrollbar = Some(style);
    self
  }

  pub fn scrollbar_hovered(mut self, f: impl Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync + 'static) -> Self {
    self.options.scrollbar_hovered = Some(Arc::new(f));
    self
  }

  pub fn with_scroll_state(mut self, scroll_state: ScrollState) -> Self {
    self.options.scroll_state = Some(scroll_state);
    self
  }

  /// Middle-button autoscroll, off by default: pressing the middle button
  /// over the list and moving the pointer away scrolls it, faster the farther
  /// the pointer is, until the release. See
  /// [`Node::middle_button_autoscroll`](crate::node::node::Node::middle_button_autoscroll).
  pub fn middle_button_autoscroll(mut self, enabled: bool) -> Self {
    self.options.middle_button_autoscroll = enabled;
    self
  }

  pub fn on_scroll(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.options.scroll_handlers.push(f.into_event_handler());
    self
  }

  pub fn on_top_reached(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
    self.options.top_handlers.push(EventHandler::new(move |_| f()));
    self
  }

  pub fn on_scroll_reach_top(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.options.top_handlers.push(f.into_event_handler());
    self
  }

  pub fn on_bottom_reached(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
    self.options.bottom_handlers.push(EventHandler::new(move |_| f()));
    self
  }

  pub fn on_scroll_reach_bottom(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.options.bottom_handlers.push(f.into_event_handler());
    self
  }

  pub fn mount_keyed<C, K, KF, PF>(self, key_fn: KF, props_fn: PF) -> Element
  where
    C: Component,
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Display + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    PF: Fn(&T) -> C::Props + Send + Sync + 'static,
  {
    let props = VirtualizedMountListProps::<T, KF, PF> {
      items: self.items,
      key_fn: Arc::new(key_fn),
      props_fn: Arc::new(props_fn),
      options: self.options.clone(),
      estimate: self.estimate,
    };

    match self.options.list_key.as_deref() {
      Some(key) => self
        .ctx
        .mount_keyed::<VirtualizedMountList<T, C, K, KF, PF>>(key, props),
      None => self.ctx.mount::<VirtualizedMountList<T, C, K, KF, PF>>(props),
    }
  }
}

struct VirtualizedMountListProps<T, KF, PF> {
  /// Shared, not cloned: steady-state scroll re-renders must not copy or
  /// deep-compare the item list.
  items: Arc<Vec<T>>,
  key_fn: Arc<KF>,
  props_fn: Arc<PF>,
  options: VirtualizedListOptions,
  estimate: Option<RowEstimate<T>>,
}

impl<T, KF, PF> PartialEq for VirtualizedMountListProps<T, KF, PF> {
  fn eq(&self, _other: &Self) -> bool {
    false
  }
}

#[cfg(feature = "devtools")]
impl<T, KF, PF> crate::app::component::DevtoolsInspectable for VirtualizedMountListProps<T, KF, PF> {
  fn inspect(&self, formatter: &mut crate::app::component::DevtoolsFormatter<'_>) {
    formatter.value("VirtualizedList", format!("len={}", self.items.len()));
  }
}

struct VirtualizedMountList<T, C, K, KF, PF>
where
  C: Component,
{
  scroll_state: ScrollState,
  active_scroll_state: Arc<Mutex<ScrollState>>,
  revision: Signal<u64>,
  runtime: Arc<Mutex<VirtualizedRuntime<T>>>,
  _marker: PhantomData<(C, K, KF, PF)>,
}

struct VirtualizedRuntime<T> {
  heights: HashMap<String, f32>,
  /// The average measured height, kept when the items are swapped for others
  /// (another conversation under the same list) so the new rows are windowed
  /// where the list opens instead of bootstrapped from its top again.
  estimate: Option<f32>,
  /// The app's estimate of a row not measured yet
  /// (`VirtualizedList::estimate_height`), used instead of `estimate`.
  row_estimate: Option<RowEstimate<T>>,
  /// The first row in view as the last render placed the window: its key and
  /// its top in the content. Measured heights that move that top move the
  /// offset with it (see `keep_shown_first`).
  shown_first: Option<ShownFirst>,
  /// Where the list opens, when the bootstrap rows could not be taken from
  /// there or could not hold it: the content of the first layout is those
  /// rows alone, so a queued scroll to the end lands on them, and an offset
  /// (a queued `set_scroll_pending`, the offset a shared scroll state keeps
  /// from before) is clamped to them. Once they are measured, the scroll is
  /// queued again, so the window built from the estimate lands there.
  bootstrap_restore: Option<BootstrapRestore>,
  /// The item list as of the last change, for cheap change detection — the
  /// steady-state scroll path must not rebuild keys or maps per frame.
  /// Compared by pointer first, contents only when the pointer changed.
  items_snapshot: Arc<Vec<T>>,
  order: Vec<String>,
  /// Cumulative row heights (`prefix[i]` = top of row `i`). Rows that were
  /// never measured count as the average measured height, so the extent and
  /// windowing work without ever mounting off-screen rows. Rebuilt only when
  /// items or measured heights change; the per-scroll visible-range query is
  /// a binary search.
  prefix: Vec<f32>,
  prefix_dirty: bool,
  rendered_refs: HashMap<String, ElementRef>,
  pending_anchor: Option<ScrollAnchor>,
  /// A row key requested via `reveal_key` that still needs to be scrolled into
  /// view (deferred until every row above it is measured for an exact offset).
  pending_reveal: Option<String>,
  /// The last `reveal_key` acted on, so a reveal fires only when it changes.
  last_reveal: Option<String>,
  /// Scroll position at which the row window was last built; re-renders only
  /// happen once the position drifts past `rebuild_threshold`.
  last_render_scroll_y: f32,
  /// Half the overscan margin (set from the options in `rows`).
  rebuild_threshold: f32,
  last_viewport_height: f32,
  /// Diagnostics for the viewport-coverage probe: the index window and its
  /// prefix top from the last `rows()` build.
  last_window: (usize, usize),
  last_window_prefix_start: f32,
  /// Ring of recent window builds: (scroll_y, start, end, prefix_start,
  /// items_changed) — lets the coverage probe identify which historical
  /// build the on-screen geometry actually corresponds to.
  recent_builds: std::collections::VecDeque<(f32, usize, usize, f32, bool)>,
  /// Monotonic window-build counter (drives the debug overlay row tint).
  build_seq: u64,
  /// Last coverage-probe failure, shown in the debug overlay panel.
  last_hole: Option<String>,
}

/// A scroll the bootstrap rows could not hold; see
/// `VirtualizedRuntime::bootstrap_restore`.
#[derive(Clone, Copy)]
enum BootstrapRestore {
  End,
  /// An offset, and the farthest the content let the list scroll when it was
  /// last queued (none before): the content is sized from the estimate,
  /// which the rows measured at that end refine, so the offset is queued
  /// again while the end moves out, until the list lands on it.
  Offset {
    offset: f32,
    reached: Option<f32>,
  },
}

/// See `VirtualizedRuntime::shown_first`.
struct ShownFirst {
  key: String,
  top: f32,
  /// `ScrollState::scroll_y_requests` at the render: a scroll request that
  /// took effect since puts the list where it asked, which stands.
  scroll_y_requests: u64,
}

struct ScrollAnchor {
  key: String,
  offset: f32,
  /// `ScrollState::scroll_y_requests` at capture. A scroll request that took
  /// effect since (an app's `set_scroll` or resolved `scroll_to_bottom_pending`,
  /// a reveal) moved the view on purpose; restoring the anchor would undo it.
  scroll_y_requests: u64,
}

impl<T> Default for VirtualizedRuntime<T> {
  fn default() -> Self {
    Self {
      heights: HashMap::new(),
      estimate: None,
      row_estimate: None,
      shown_first: None,
      bootstrap_restore: None,
      items_snapshot: Arc::new(Vec::new()),
      order: Vec::new(),
      prefix: vec![0.0],
      prefix_dirty: false,
      rendered_refs: HashMap::new(),
      pending_anchor: None,
      pending_reveal: None,
      last_reveal: None,
      last_render_scroll_y: 0.0,
      rebuild_threshold: DEFAULT_OVERSCAN_PX * 0.5,
      last_viewport_height: 0.0,
      last_window: (0, 0),
      last_window_prefix_start: 0.0,
      recent_builds: std::collections::VecDeque::new(),
      build_seq: 0,
      last_hole: None,
    }
  }
}

impl<T, C, K, KF, PF> Component for VirtualizedMountList<T, C, K, KF, PF>
where
  C: Component,
  T: Clone + PartialEq + Send + Sync + 'static,
  K: Display + Send + Sync + 'static,
  KF: Fn(&T) -> K + Send + Sync + 'static,
  PF: Fn(&T) -> C::Props + Send + Sync + 'static,
{
  type Props = VirtualizedMountListProps<T, KF, PF>;

  fn create(ctx: &mut Ctx) -> Self {
    let scroll_state = ScrollState::new();
    Self {
      scroll_state: scroll_state.clone(),
      active_scroll_state: Arc::new(Mutex::new(scroll_state)),
      revision: ctx.signal(0),
      runtime: Arc::new(Mutex::new(VirtualizedRuntime::default())),
      _marker: PhantomData,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, key_fn, props_fn, mut options, estimate) = {
      let props = ctx.props::<Self::Props>();
      (
        props.items.clone(),
        props.key_fn.clone(),
        props.props_fn.clone(),
        props.options.clone(),
        props.estimate.clone(),
      )
    };
    let _ = self.revision.get();
    // With the debug overlay the scroll gets wrapped in a Stack; the outer
    // frame/flex move onto the Stack while the scroll fills it.
    let outer_frame = options.frame;
    let outer_flex = options.flex;
    if debug_overlay_enabled() {
      options.frame = FrameConstraints {
        width: Some(Dimension::full()),
        height: Some(Dimension::full()),
        ..FrameConstraints::default()
      };
      options.flex = None;
    }
    let scroll_state = options
      .scroll_state
      .clone()
      .unwrap_or_else(|| self.scroll_state.clone());
    *self.active_scroll_state.lock() = scroll_state.clone();
    let rows = self.rows(ctx, &items, &*key_fn, &*props_fn, &options, estimate, &scroll_state);

    // Re-render only when the scroll position drifts past half the overscan
    // margin from where the window was last built — everything closer is
    // already materialized, so plain retained scrolling covers it. Without
    // this hysteresis every wheel tick rebuilds and re-lays the whole visible
    // window, which crawls on long lists.
    let rebuild_handler = {
      let revision = self.revision.clone();
      let runtime = self.runtime.clone();
      let handler_scroll_state = scroll_state.clone();
      let rebuild_threshold = (options.overscan_px * 0.5).max(1.0);
      move |event: ScrollEvent| {
        // Handlers run before the default scroll movement applies, so predict
        // the post-scroll position from the delta.
        let scroll_y = (handler_scroll_state.scroll_y() - event.delta_y).max(0.0);
        let drifted = (runtime.lock().last_render_scroll_y - scroll_y).abs() > rebuild_threshold;
        if drifted {
          revision.update(|value| *value = value.wrapping_add(1));
        }
      }
    };

    // The vertical and both-axes scroll containers are distinct types with
    // identical (macro-provided) builders, so the shared option chain is
    // applied through a macro.
    macro_rules! configure_scroll {
      ($scroll:expr) => {{
        let mut scroll = $scroll
          .with_scroll_state(scroll_state.clone())
          .middle_button_autoscroll(options.middle_button_autoscroll);
        if let Some(style) = options.scrollbar.clone() {
          scroll = scroll.scrollbar(style);
        }
        if let Some(hovered) = options.scrollbar_hovered.clone() {
          scroll = scroll.scrollbar_hovered(move |style| hovered(style));
        }
        for handler in options.top_handlers.iter().cloned() {
          scroll = scroll.on_scroll_reach_top(handler);
        }
        for handler in options.bottom_handlers.iter().cloned() {
          scroll = scroll.on_scroll_reach_bottom(handler);
        }
        for handler in options.scroll_handlers.iter().cloned() {
          scroll = scroll.on_scroll(handler);
        }
        scroll = scroll.on_scroll(rebuild_handler.clone());
        if let Some(width) = options.frame.width {
          scroll = scroll.width(width);
        }
        if let Some(height) = options.frame.height {
          scroll = scroll.height(height);
        }
        if let Some(min_width) = options.frame.min_width {
          scroll = scroll.min_width(min_width);
        }
        if let Some(max_width) = options.frame.max_width {
          scroll = scroll.max_width(max_width);
        }
        if let Some(min_height) = options.frame.min_height {
          scroll = scroll.min_height(min_height);
        }
        if let Some(max_height) = options.frame.max_height {
          scroll = scroll.max_height(max_height);
        }
        if let Some(flex) = options.flex {
          scroll = scroll.flex(flex);
        }
        scroll
      }};
    }

    let scroll: Element = if options.horizontal {
      configure_scroll!(ScrollBoth::new(rows)).into()
    } else {
      configure_scroll!(ScrollVertical::new(rows)).into()
    };
    if !debug_overlay_enabled() {
      return scroll;
    }
    self.wrap_with_debug_overlay(scroll, &scroll_state, &outer_frame, outer_flex)
  }

  fn after_layout(&self) {
    let mut changed = false;
    {
      let mut runtime = self.runtime.lock();
      let scroll_state = self.active_scroll_state.lock().clone();
      let scroll_y_requests = scroll_state.scroll_y_requests();
      let mut heights_changed = false;
      let rendered_refs: Vec<(String, ElementRef)> = runtime
        .rendered_refs
        .iter()
        .map(|(key, element_ref)| (key.clone(), element_ref.clone()))
        .collect();

      for (key, element_ref) in rendered_refs {
        if !element_ref.is_attached() {
          continue;
        }
        let height = element_ref.height().max(0.0);
        let previous = runtime.heights.insert(key, height);
        if previous.is_none_or(|old| (old - height).abs() > HEIGHT_EPSILON) {
          runtime.prefix_dirty = true;
          heights_changed = true;
          changed = true;
        }
      }

      let scroll_y = scroll_state.scroll_y();
      let viewport_height = scroll_state.viewport_height();

      // Paint-time invariant probe: the rows this list just laid out must
      // cover the scroll viewport (in absolute window coords). A hole here
      // is a windowing/layout failure; a hole the user sees WITHOUT this
      // warning is a renderer failure.
      if viewport_height > 0.0 && !runtime.rendered_refs.is_empty() {
        let viewport_top = scroll_state.layout_viewport_y();
        let viewport_bottom = viewport_top + viewport_height;
        let mut spans: Vec<(f32, f32)> = runtime
          .rendered_refs
          .values()
          .filter(|element_ref| element_ref.is_attached())
          .map(|element_ref| (element_ref.y(), element_ref.y() + element_ref.height()))
          .filter(|(top, bottom)| *bottom > viewport_top && *top < viewport_bottom)
          .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let content_bottom = viewport_top + (scroll_state.content_height() - scroll_y).max(0.0);
        let mut covered_to = if scroll_y <= 1.0 { viewport_top } else { f32::MIN };
        let mut hole = None;
        if let Some(first) = spans.first() {
          if covered_to == f32::MIN {
            covered_to = first.0;
          }
          if first.0 > viewport_top + 1.0 && scroll_y > 1.0 {
            hole = Some((viewport_top, first.0));
          }
          for (top, bottom) in &spans {
            if *top > covered_to + 1.0 {
              hole.get_or_insert((covered_to, *top));
            }
            covered_to = covered_to.max(*bottom);
          }
          if covered_to < viewport_bottom.min(content_bottom) - 1.0 {
            hole.get_or_insert((covered_to, viewport_bottom));
          }
        } else {
          hole = Some((viewport_top, viewport_bottom));
        }
        if let Some((from, to)) = hole {
          let all: Vec<(f32, f32)> = runtime
            .rendered_refs
            .values()
            .filter(|element_ref| element_ref.is_attached())
            .map(|element_ref| (element_ref.y(), element_ref.height()))
            .collect();
          let min_top = all.iter().map(|(y, _)| *y).fold(f32::MAX, f32::min);
          let max_bottom = all.iter().map(|(y, h)| y + h).fold(f32::MIN, f32::max);
          let zero_height = all.iter().filter(|(_, h)| *h < 0.5).count();
          let (first_key, first_top) = runtime
            .rendered_refs
            .iter()
            .filter(|(_, element_ref)| element_ref.is_attached())
            .min_by(|a, b| a.1.y().partial_cmp(&b.1.y()).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(key, element_ref)| (key.clone(), element_ref.y()))
            .unwrap_or_default();
          // Where the window's first row SHOULD be, per the prefix the window
          // was built from — the measured/expected delta separates a stale
          // scroll translation from a stale spacer or poisoned heights.
          let expected_first_top = viewport_top + runtime.last_window_prefix_start - scroll_y;
          let mut sorted_heights: Vec<f32> = runtime.heights.values().copied().collect();
          sorted_heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
          let (height_min, height_median, height_max) = if sorted_heights.is_empty() {
            (0.0, 0.0, 0.0)
          } else {
            (
              sorted_heights[0],
              sorted_heights[sorted_heights.len() / 2],
              sorted_heights[sorted_heights.len() - 1],
            )
          };
          let small_heights = sorted_heights.iter().filter(|height| **height < 10.0).count();
          tracing::warn!(
            target: "lurq::vlist",
            "laid-out rows leave viewport hole y=[{from:.1}..{to:.1}] (viewport=[{viewport_top:.1}..{viewport_bottom:.1}] scroll_y={scroll_y:.1} window_built_at={:.1} rows={} rows_span=[{min_top:.1}..{max_bottom:.1}] zero_h={zero_height} first_row=({first_key}@{first_top:.1}) expected_first_top={expected_first_top:.1} window={:?} prefix_start={:.1} content_h={:.1} prefix_len={} heights={} h_min={height_min:.1} h_med={height_median:.1} h_max={height_max:.1} h_small={small_heights})",
            runtime.last_render_scroll_y,
            runtime.rendered_refs.len(),
            runtime.last_window,
            runtime.last_window_prefix_start,
            scroll_state.content_height(),
            runtime.prefix.len(),
            runtime.heights.len(),
          );
          tracing::warn!(
            target: "lurq::vlist",
            "recent builds (scroll, start, end, prefix_start, items_changed): {:?}",
            runtime.recent_builds
          );
          runtime.last_hole = Some(format!(
            "y=[{from:.0}..{to:.0}] rows@[{min_top:.0}..{max_bottom:.0}] exp {expected_first_top:.0}"
          ));
        } else {
          runtime.last_hole = None;
        }
      }
      // Same hysteresis as the scroll handler: rebuild only once the position
      // leaves the already-materialized overscan margin (or the viewport
      // itself resized).
      let drifted = (runtime.last_render_scroll_y - scroll_y).abs() > runtime.rebuild_threshold;
      if drifted && (runtime.last_render_scroll_y - scroll_y).abs() > runtime.rebuild_threshold * 2.0 {
        // The scroll handler should have re-windowed well before this point —
        // reaching here means scroll moved without the handlers firing.
        tracing::warn!(
          target: "lurq::vlist",
          "virtualized window healed after missed scroll events: scroll_y={:.1} window_built_at={:.1}",
          scroll_y,
          runtime.last_render_scroll_y
        );
      }
      if drifted || (runtime.last_viewport_height - viewport_height).abs() > HEIGHT_EPSILON {
        runtime.last_viewport_height = viewport_height;
        changed = true;
      }

      // Restore the anchored row's viewport position once every height is
      // exact (the bounded measure march in `rows` fills the gaps). An
      // anchor whose row VANISHED with the new items — a different dataset
      // swapped in under the same list, e.g. switching record categories —
      // is dropped immediately instead: chasing it forced the march across
      // every row of the new list (seconds of first-render jank on
      // multi-thousand-row tables) for a position that no longer means
      // anything there. An anchor is dropped as well once a scroll request
      // took effect after it was captured (see `ScrollAnchor`), including
      // one that lands while the march is still running.
      if let Some(anchor) = runtime.pending_anchor.take()
        && anchor.scroll_y_requests == scroll_state.scroll_y_requests()
      {
        if runtime.heights.len() >= runtime.order.len() {
          if let Some(anchor_index) = runtime.order.iter().position(|key| key == &anchor.key) {
            let new_top = prefix_height(&runtime.order[..anchor_index], &runtime.heights);
            scroll_state.set_scroll(0.0, new_top + anchor.offset);
            changed = true;
          }
        } else {
          runtime.pending_anchor = Some(anchor);
        }
      }

      changed |= runtime.restore_bootstrap_scroll(&scroll_state);
      changed |= runtime.apply_pending_reveal(&scroll_state);
      // The row recorded at the render stands for every layout until the next
      // render, the scrolls between them included.
      if heights_changed && scroll_state.scroll_y_requests() == scroll_y_requests {
        let shown_first = runtime.shown_first.take();
        runtime.keep_shown_first(shown_first, &scroll_state);
      }
    }

    if changed {
      self.revision.update(|value| *value = value.wrapping_add(1));
    }
  }
}

impl<T, C, K, KF, PF> VirtualizedMountList<T, C, K, KF, PF>
where
  C: Component,
  T: Clone + PartialEq + Send + Sync + 'static,
  K: Display + Send + Sync + 'static,
  KF: Fn(&T) -> K + Send + Sync + 'static,
  PF: Fn(&T) -> C::Props + Send + Sync + 'static,
{
  /// Debug overlay (`LURQ_VLIST_DEBUG=1`): wraps the scroll in a Stack with a
  /// live stats panel pinned top-right. The outer frame/flex land on the
  /// Stack; the scroll fills it.
  fn wrap_with_debug_overlay(
    &self,
    scroll: Element,
    scroll_state: &ScrollState,
    outer_frame: &FrameConstraints,
    outer_flex: Option<f32>,
  ) -> Element {
    use super::{Stack, Text};
    use crate::{
      layout::text_style::TextStyle,
      node::{color::Color, padding::Padding},
    };

    let runtime = self.runtime.lock();
    let max_scroll = (scroll_state.content_height() - scroll_state.viewport_height()).max(0.0);
    let percent = if max_scroll > 0.0 {
      (scroll_state.scroll_y() / max_scroll * 100.0).clamp(0.0, 100.0)
    } else {
      0.0
    };
    let lines = vec![
      format!("build #{}", runtime.build_seq),
      format!(
        "scroll {:.0}/{max_scroll:.0} ({percent:.0}%)  row {}/{}",
        scroll_state.scroll_y(),
        runtime.last_window.0,
        runtime.order.len()
      ),
      format!(
        "scroll {:.1}  built@ {:.1}",
        scroll_state.scroll_y(),
        runtime.last_render_scroll_y
      ),
      format!(
        "window {}..{}  rows {}",
        runtime.last_window.0,
        runtime.last_window.1,
        runtime.rendered_refs.len()
      ),
      format!("prefix_start {:.1}", runtime.last_window_prefix_start),
      format!(
        "content {:.1}  viewport {:.1}",
        scroll_state.content_height(),
        scroll_state.viewport_height()
      ),
      format!("heights {}/{}", runtime.heights.len(), runtime.order.len()),
    ];
    let hole = runtime.last_hole.clone();
    drop(runtime);

    let line_style = |color: Color| TextStyle {
      font_size: 11.0,
      line_height: 1.25,
      color,
      ..TextStyle::default()
    };
    let mut panel = Column::new()
      .spacing(1.0)
      .padding(Padding::new().top(6.0).bottom(6.0).left(9.0).right(9.0))
      .background(Color::new(8, 10, 14, 216));
    for line in &lines {
      panel = panel.child(Text::styled(line, line_style(Color::new(150, 235, 170, 255))));
    }
    if let Some(hole) = hole {
      panel = panel.child(Text::styled(
        &format!("HOLE {hole}"),
        line_style(Color::new(255, 110, 110, 255)),
      ));
    }
    let overlay = Column::new()
      .width(Dimension::full())
      .align_items(Alignment::End)
      .padding(Padding::new().top(44.0).right(18.0))
      .child(panel);

    let mut stack = Stack::new().child(scroll).child(overlay);
    if let Some(width) = outer_frame.width {
      stack = stack.width(width);
    }
    if let Some(height) = outer_frame.height {
      stack = stack.height(height);
    }
    if let Some(min_width) = outer_frame.min_width {
      stack = stack.min_width(min_width);
    }
    if let Some(max_width) = outer_frame.max_width {
      stack = stack.max_width(max_width);
    }
    if let Some(min_height) = outer_frame.min_height {
      stack = stack.min_height(min_height);
    }
    if let Some(max_height) = outer_frame.max_height {
      stack = stack.max_height(max_height);
    }
    if let Some(flex) = outer_flex {
      stack = stack.flex(flex);
    }
    stack.into()
  }

  fn rows(
    &self,
    ctx: &mut Ctx,
    items: &Arc<Vec<T>>,
    key_fn: &KF,
    props_fn: &PF,
    options: &VirtualizedListOptions,
    estimate: Option<RowEstimate<T>>,
    scroll_state: &ScrollState,
  ) -> Column {
    let mut runtime = self.runtime.lock();
    if estimate.is_some() != runtime.row_estimate.is_some() {
      runtime.prefix_dirty = true;
    }
    runtime.row_estimate = estimate;

    // A scroll the app queued for this pass (`scroll_to_bottom_pending`,
    // `set_scroll_pending`, ...) wins over keeping the current rows in place:
    // no anchor is captured for the position being scrolled away from, and
    // one still marching is dropped instead of measuring rows for it.
    let scroll_requested = scroll_state.has_pending_scroll_y_request();
    if scroll_requested {
      runtime.pending_anchor = None;
    }

    // Steady-state scroll ticks must be free of O(n) work: the shared item
    // list is compared by pointer, and contents only when the pointer changed
    // (a parent re-render building an identical list).
    let items_changed =
      !Arc::ptr_eq(&runtime.items_snapshot, items) && runtime.items_snapshot.as_slice() != items.as_slice();
    if !items_changed && !Arc::ptr_eq(&runtime.items_snapshot, items) {
      // Same contents, new allocation — adopt it so the next compare is O(1).
      runtime.items_snapshot = items.clone();
    }
    if items_changed {
      let keys: Vec<String> = items.iter().map(|item| format!("{}", key_fn(item))).collect();
      tracing::warn!(
        target: "lurq::vlist",
        "items changed in place: old_len={} new_len={} keys_equal={} scroll_y={:.1}",
        runtime.order.len(),
        keys.len(),
        runtime.order == keys,
        scroll_state.scroll_y(),
      );
      let current_keys: HashSet<&str> = keys.iter().map(String::as_str).collect();
      let survivors = runtime
        .order
        .iter()
        .filter(|key| current_keys.contains(key.as_str()))
        .count();
      if !runtime.order.is_empty() && survivors == 0 {
        // No key survived: a DIFFERENT dataset swapped in under the same
        // list (e.g. switching record categories). There is no position to
        // restore and every retained height is meaningless — chasing an
        // anchor here forced a measure march across the whole new list
        // (seconds of first-render jank on multi-thousand-row tables).
        // The heights go; their average stays as the estimate, so the new
        // rows are windowed where the list opens (its top, or the end a
        // queued scroll asks for) instead of bootstrapped from the top.
        runtime.heights.clear();
        runtime.pending_anchor = None;
        // An offset the list itself queued again, to open the rows before
        // at it, is not a place in these rows: it goes with them. An offset
        // the app queued for them stays.
        if let Some(BootstrapRestore::Offset {
          offset,
          reached: Some(_),
        }) = runtime.bootstrap_restore
        {
          scroll_state.drop_pending_scroll_y_offset(offset);
        }
        runtime.bootstrap_restore = None;
        scroll_state.set_scroll(0.0, 0.0);
      } else {
        if runtime.order != keys && runtime.pending_anchor.is_none() && !scroll_requested && !runtime.order.is_empty() {
          // Anchor the first (partially) visible row using the pre-change
          // prefix, so the viewport is restored once the new heights settle.
          let scroll_y = scroll_state.scroll_y();
          let index = runtime.prefix[1..runtime.order.len() + 1]
            .partition_point(|&bottom| bottom <= scroll_y)
            .min(runtime.order.len() - 1);
          runtime.pending_anchor = Some(ScrollAnchor {
            key: runtime.order[index].clone(),
            offset: scroll_y - runtime.prefix[index],
            scroll_y_requests: scroll_state.scroll_y_requests(),
          });
        }

        // Evict heights only for keys that disappeared. Keys that survive an
        // item change keep their old measurement as a stale-but-usable
        // estimate: rendered rows re-measure on the very next layout, and
        // dropping every height at once would collapse the prefix to the
        // bootstrap window — teleporting the scroll (clamped against the
        // collapsed content) and blanking the viewport when the same-shaped
        // list is swapped in place (e.g. a preview re-decoded into a new
        // allocation while scrolled deep). A key whose item changed is another
        // row under the same key (another document's line 12): its height
        // goes too, so it counts as the estimate until it is measured, not
        // as the height of the row it replaced.
        runtime.heights.retain(|key, _| current_keys.contains(key.as_str()));
        let old_index: HashMap<&str, usize> = runtime
          .order
          .iter()
          .enumerate()
          .map(|(index, key)| (key.as_str(), index))
          .collect();
        let replaced: Vec<String> = keys
          .iter()
          .zip(items.iter())
          .filter(|(key, item)| {
            old_index
              .get(key.as_str())
              .is_some_and(|&old| runtime.items_snapshot.get(old) != Some(*item))
          })
          .map(|(key, _)| key.clone())
          .collect();
        drop(old_index);
        for key in replaced {
          runtime.heights.remove(&key);
        }
      }
      drop(current_keys);
      runtime.order = keys;
      runtime.items_snapshot = items.clone();
      runtime.prefix_dirty = true;
    }

    // A new `reveal_key` (e.g. a selection restored by navigation) queues a
    // scroll-into-view, applied once heights above the row are known. Fires
    // only on change, so it never fights the user's scrolling.
    if options.reveal_key != runtime.last_reveal {
      runtime.last_reveal = options.reveal_key.clone();
      runtime.pending_reveal = options.reveal_key.clone();
    }

    runtime.rebuild_prefix_if_dirty();

    let count = runtime.order.len();
    let has_estimate = runtime.has_estimate();
    // The layout that follows this render may move the offset without a
    // scroll event: a pending scroll resolves, and an offset at the bottom
    // follows the new end as estimates refine. The window is built where the
    // offset will land, so that layout draws rows there instead of a viewport
    // hole (healed only a frame later).
    let window_y = if has_estimate {
      scroll_state.next_layout_scroll_y(runtime.prefix[count])
    } else {
      scroll_state.scroll_y()
    };

    let visible_range = if has_estimate {
      visible_range_from_prefix(
        &runtime.prefix,
        window_y,
        scroll_state.viewport_height(),
        options.overscan_px,
      )
    } else {
      runtime.bootstrap_range(scroll_state, options.bootstrap_rows)
    };
    let mut rendered_indices: Vec<usize> = (visible_range.0..visible_range.1).collect();
    // A pending anchor (same rows re-shaped in place) restores an exact
    // position, so the not-yet-measured remainder is measured in bounded
    // chunks per frame. Reveals never march: they apply from the estimate
    // prefix in `after_layout` and converge as the revealed window measures.
    if runtime.pending_anchor.is_some() && runtime.heights.len() < count {
      rendered_indices.extend(
        runtime
          .order
          .iter()
          .enumerate()
          .filter_map(|(index, key)| (!runtime.heights.contains_key(key)).then_some(index))
          .take(MEASURE_CHUNK),
      );
      rendered_indices.sort_unstable();
      rendered_indices.dedup();
    }
    runtime.shown_first = (has_estimate && !scroll_requested && count > 0 && scroll_state.viewport_height() > 0.0)
      .then(|| {
        let index = runtime.prefix[1..]
          .partition_point(|&bottom| bottom <= window_y)
          .min(count - 1);
        ShownFirst {
          key: runtime.order[index].clone(),
          top: runtime.prefix[index],
          scroll_y_requests: scroll_state.scroll_y_requests(),
        }
      });
    let total_height = has_estimate.then(|| runtime.prefix[count]);
    // Only the handful of rendered rows need owned keys/offsets.
    let rendered: Vec<(usize, String, f32, f32)> = rendered_indices
      .into_iter()
      .map(|index| {
        (
          index,
          runtime.order[index].clone(),
          runtime.prefix[index],
          runtime.prefix[index + 1] - runtime.prefix[index],
        )
      })
      .collect();
    runtime.last_render_scroll_y = window_y;
    runtime.rebuild_threshold = (options.overscan_px * 0.5).max(1.0);
    runtime.last_window = visible_range;
    let window_prefix_start = runtime.prefix.get(visible_range.0).copied().unwrap_or(0.0);
    runtime.last_window_prefix_start = window_prefix_start;
    runtime.recent_builds.push_back((
      window_y,
      visible_range.0,
      visible_range.1,
      window_prefix_start,
      items_changed,
    ));
    if runtime.recent_builds.len() > 8 {
      runtime.recent_builds.pop_front();
    }
    runtime.build_seq = runtime.build_seq.wrapping_add(1);
    let build_seq = runtime.build_seq;
    runtime.rendered_refs.clear();
    drop(runtime);

    let debug_tint = debug_overlay_enabled().then(|| debug_build_tint(build_seq));
    let mut column = Column::new().spacing(0.0).align_items(Alignment::Start);
    let mut cursor_y = 0.0;

    for (index, key, row_y, row_height) in rendered {
      if row_y > cursor_y {
        column = column.child(Spacer::new().height(row_y - cursor_y));
      }
      let row_ref = ctx.element_ref();
      self.runtime.lock().rendered_refs.insert(key.clone(), row_ref.clone());
      let row = ctx.mount_keyed::<C>(&key, props_fn(&items[index]));
      // Key the wrapper too: it is the row's direct sibling in the column, so
      // keying it lets node-id reconciliation re-align wrappers by key when the
      // window shifts — keeping hover/focus glued to the row the pointer is
      // actually over instead of the position it used to occupy.
      let mut wrapper = Column::new()
        .spacing(0.0)
        .key(key.as_str())
        .ref_element(row_ref)
        .child(row);
      if let Some(tint) = debug_tint {
        wrapper = wrapper.background(tint);
      }
      column = column.child(wrapper);
      cursor_y = row_y + row_height;
    }

    if let Some(total_height) = total_height
      && total_height > cursor_y
    {
      column = column.child(Spacer::new().height(total_height - cursor_y));
    }

    column
  }
}

impl<T> VirtualizedRuntime<T> {
  /// Whether rows not measured have a height to be placed with: the average of
  /// measured rows, or the app's estimate.
  fn has_estimate(&self) -> bool {
    self.estimate.is_some() || self.row_estimate.is_some()
  }

  /// After measured heights changed, keeps the first row in view where it
  /// was on screen: the offset moves as far as that row's top moves in the
  /// content (rows above it measured, or placed from an estimate a grown row
  /// skewed), instead of everything in view moving. The row and its top come
  /// from the render that placed the window (`shown_first`), so a row that
  /// grew above it in this layout is made up for too. A scroll request that
  /// took effect since that render put the list where it asked, which
  /// stands: an offset is kept as asked, not moved to keep rows that an
  /// estimate placed there. Leaves a list at its end there too, and a scroll
  /// that something else is placing (an anchor across new items, a reveal,
  /// the bootstrap's restore) to it. Returns whether the offset moves.
  fn keep_shown_first(&mut self, shown_first: Option<ShownFirst>, scroll_state: &ScrollState) -> bool {
    if self.pending_anchor.is_some() || self.pending_reveal.is_some() || self.bootstrap_restore.is_some() {
      return false;
    }
    let Some(ShownFirst { key, top, .. }) =
      shown_first.filter(|shown| shown.scroll_y_requests == scroll_state.scroll_y_requests())
    else {
      return false;
    };
    let scroll_y = scroll_state.scroll_y();
    let max_scroll = (scroll_state.content_height() - scroll_state.viewport_height()).max(0.0);
    if max_scroll > 0.0 && max_scroll - scroll_y <= REVEAL_EPSILON {
      return false;
    }
    let Some(index) = self.order.iter().position(|k| *k == key) else {
      return false;
    };
    self.rebuild_prefix_if_dirty();
    let moved = self.prefix[index] - top;
    if moved.abs() <= HEIGHT_EPSILON {
      return false;
    }
    scroll_state.keep_scroll_y_pending((scroll_y + moved).max(0.0));
    true
  }

  /// Cumulative heights (`prefix[i]` = top of row `i`), rebuilt only when
  /// items or measured heights changed. Rows that were never measured count
  /// as the average measured height — off-screen rows are never mounted just
  /// to size them; the extent refines as real heights come in. Row offsets
  /// and the visible range then cost O(1)/O(log n) per scroll tick.
  fn rebuild_prefix_if_dirty(&mut self) {
    if !self.prefix_dirty {
      return;
    }
    let mut measured_sum = 0.0f64;
    let mut measured_count = 0usize;
    let row_heights: Vec<Option<f32>> = self
      .order
      .iter()
      .map(|key| {
        let height = self.heights.get(key).copied();
        if let Some(height) = height {
          measured_sum += f64::from(height);
          measured_count += 1;
        }
        height
      })
      .collect();
    if measured_count > 0 {
      // Rows measured at no height (collapsed, not yet laid out) give no
      // estimate: a window sized from a zero estimate holds no row and so
      // never measures one.
      let average = (measured_sum / measured_count as f64) as f32;
      self.estimate = (average > 0.0).then_some(average);
    }
    let estimate = self.estimate.unwrap_or(0.0);
    let mut prefix = Vec::with_capacity(row_heights.len() + 1);
    let mut sum = 0.0f32;
    prefix.push(0.0);
    for (index, height) in row_heights.into_iter().enumerate() {
      sum += height.unwrap_or_else(|| match (&self.row_estimate, self.items_snapshot.get(index)) {
        (Some(row_estimate), Some(item)) => row_estimate(item),
        _ => estimate,
      });
      prefix.push(sum);
    }
    self.prefix = prefix;
    self.prefix_dirty = false;
  }

  /// The rows rendered before any height is known, no more than `rows` of
  /// them, from where the list opens: a revealed row (`reveal_key`) comes
  /// first, then the end for a queued scroll to the bottom, else the top. A
  /// chat opened at its end builds and lays out its last rows only, however
  /// long it is; its first rows wait until they are scrolled to. An offset
  /// to open at (queued, or kept by a shared scroll state) names no row
  /// while no height is known: the rows come from the top and the offset is
  /// restored once they are measured.
  fn bootstrap_range(&mut self, scroll_state: &ScrollState, rows: usize) -> (usize, usize) {
    let count = self.order.len();
    let rows = rows.max(1).min(count);
    let revealed = self
      .pending_reveal
      .as_ref()
      .and_then(|key| self.order.iter().position(|k| k == key));
    let opens_at_end = revealed.is_none() && scroll_state.opens_at_end();
    let offset = scroll_state
      .pending_scroll_y_offset()
      .unwrap_or_else(|| scroll_state.scroll_y());
    self.bootstrap_restore = if revealed.is_some() {
      None
    } else if opens_at_end {
      Some(BootstrapRestore::End)
    } else if offset > 0.0 {
      Some(BootstrapRestore::Offset { offset, reached: None })
    } else {
      None
    };
    let start = match revealed {
      Some(index) => index.min(count - rows),
      None if opens_at_end => count - rows,
      None => 0,
    };
    (start, start + rows)
  }

  /// Queues again the scroll the bootstrap rows could not hold, once they
  /// are measured, unless the list was scrolled elsewhere since: the offset
  /// the layout settled on is the one that scroll gave within the content it
  /// had, so another offset is someone's scroll, which stands. An offset the
  /// content, sized from the estimate, is still too short for is queued
  /// again while the content grows with the rows measured at its end; one
  /// the content stops short of is given up. Returns whether a scroll was
  /// queued.
  fn restore_bootstrap_scroll(&mut self, scroll_state: &ScrollState) -> bool {
    let Some(restore) = self.bootstrap_restore else {
      return false;
    };
    if self.heights.is_empty() {
      return false;
    }
    let max_scroll = (scroll_state.content_height() - scroll_state.viewport_height()).max(0.0);
    let (target, reached) = match restore {
      BootstrapRestore::End => (max_scroll, None),
      BootstrapRestore::Offset { offset, reached } => (offset, reached),
    };
    let expected = target.min(max_scroll);
    let scrolled_elsewhere = (scroll_state.scroll_y() - expected).abs() > REVEAL_EPSILON;
    let landed = matches!(restore, BootstrapRestore::Offset { .. }) && expected >= target - REVEAL_EPSILON;
    let stuck = reached.is_some_and(|reached| max_scroll <= reached + REVEAL_EPSILON);
    if scrolled_elsewhere || landed || stuck {
      self.bootstrap_restore = None;
      return false;
    }
    match restore {
      BootstrapRestore::End => {
        self.bootstrap_restore = None;
        scroll_state.scroll_to_bottom_pending();
      }
      BootstrapRestore::Offset { offset, .. } => {
        self.bootstrap_restore = Some(BootstrapRestore::Offset {
          offset,
          reached: Some(max_scroll),
        });
        scroll_state.set_scroll_pending(scroll_state.scroll_x(), offset);
      }
    }
    true
  }

  /// Reveal the requested row: when it is not already fully visible, scroll
  /// it to the top of the viewport (`scrollIntoView`, `block: start`); an
  /// already-visible row does not move, so clicking a visible row never
  /// jumps. A row taller than the viewport is never fully visible: it is
  /// revealed once its top is at the viewport's top, or the reveal stayed
  /// pending and scrolled it back there after every layout, whatever the user
  /// scrolled since. Visibility and the target offset come from the estimate prefix,
  /// rebuilt first from the heights this layout just measured: the prefix of
  /// the previous render is all zeros before the first measurement, which
  /// made a row below the viewport look visible and dropped the reveal of a
  /// list mounted with its key already set. The reveal stays pending until
  /// the row itself has a real measurement, so the position converges as the
  /// revealed window's heights land, and while the list has no items yet
  /// (items that load after the list mounted). Returns whether it scrolled.
  fn apply_pending_reveal(&mut self, scroll_state: &ScrollState) -> bool {
    let Some(key) = self.pending_reveal.clone() else {
      return false;
    };
    let Some(index) = self.order.iter().position(|k| k == &key) else {
      if !self.order.is_empty() {
        self.pending_reveal = None;
      }
      return false;
    };
    self.rebuild_prefix_if_dirty();
    let row_top = self.prefix.get(index).copied().unwrap_or(0.0);
    let row_bottom = self.prefix.get(index + 1).copied().unwrap_or(row_top);
    let view_top = scroll_state.scroll_y();
    let view_bottom = view_top + scroll_state.viewport_height();
    let fully_visible = row_top >= view_top && row_bottom <= view_bottom;
    let top_shown = row_bottom - row_top > view_bottom - view_top && (row_top - view_top).abs() <= REVEAL_EPSILON;
    if fully_visible || top_shown {
      if self.heights.contains_key(&key) {
        self.pending_reveal = None;
      }
      return false;
    }
    scroll_state.set_scroll(0.0, row_top.max(0.0));
    true
  }
}

fn prefix_height(keys: &[String], heights: &HashMap<String, f32>) -> f32 {
  keys.iter().map(|key| heights.get(key).copied().unwrap_or(0.0)).sum()
}

/// Visible index window from the cumulative-height prefix (see
/// `VirtualizedRuntime::prefix`): two binary searches instead of an O(n) walk.
fn visible_range_from_prefix(prefix: &[f32], scroll_y: f32, viewport_height: f32, overscan_px: f32) -> (usize, usize) {
  let count = prefix.len().saturating_sub(1);
  if count == 0 {
    return (0, 0);
  }
  let start_y = (scroll_y - overscan_px).max(0.0);
  let end_y = scroll_y + viewport_height + overscan_px;

  // First row whose bottom edge is past `start_y`.
  let start = prefix[1..].partition_point(|&bottom| bottom <= start_y).min(count);
  // One past the last row whose top edge is before `end_y`.
  let end = prefix[..count].partition_point(|&top| top < end_y).max(start);
  (start, end)
}
