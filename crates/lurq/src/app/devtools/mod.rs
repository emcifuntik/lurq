mod inspector;
#[cfg(feature = "persistent_storage")]
mod persistent_storage_panel;
mod profiler;
mod profiler_analysis;
mod profiler_capture;
mod signals;
mod snapshot;
mod style;
mod top_bar;
mod tree_panel;

use std::sync::{
  Arc,
  atomic::{AtomicU64, Ordering},
};

pub use snapshot::{DevToolsNode, DevToolsSnapshot, FrameProfileSnapshot};

pub use self::snapshot::DevToolsSnapshot as Snapshot;
use crate::{
  app::{
    component::Component,
    ctx::{Ctx, Interval},
    profiler::ProfilingHandle,
  },
  components::{Column, Row},
  core::{NodeId, Signal, Store},
  layout::layout_kind::ScrollState,
  node::Element,
};

static LUCIDE_TTF: &[u8] = include_bytes!("../../../assets/lucide.ttf");

pub type DevToolsDebugOverlayCallback = Arc<dyn Fn(Option<Vec<usize>>) + Send + Sync>;
pub type DevToolsBoolCallback = Arc<dyn Fn(bool) + Send + Sync>;
pub type DevToolsPathCallback = Arc<dyn Fn(Vec<usize>) + Send + Sync>;

pub fn load_fonts(app: &mut crate::app::App) {
  app.load_font(LUCIDE_TTF.to_vec());
  app.register_font("lucide", "lucide");
}

#[derive(Clone, crate::DevtoolsInspectable)]
pub struct DevToolsProps {
  #[devtools_ignore]
  pub snapshot: DevToolsSnapshot,
  #[devtools_ignore]
  pub snapshot_revision: u64,
  #[cfg(feature = "persistent_storage")]
  #[devtools_ignore]
  pub persistent_storage_revision: u64,
  pub picked_path: Option<Vec<usize>>,
  pub picked_revision: u64,
  #[devtools_ignore]
  pub on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  #[devtools_ignore]
  pub on_overlay_enabled: Option<DevToolsBoolCallback>,
  #[devtools_ignore]
  pub on_pick_inspected: Option<DevToolsBoolCallback>,
  #[devtools_ignore]
  pub on_selected_path: Option<DevToolsPathCallback>,
  #[devtools_ignore]
  pub on_save_node_screenshot: Option<DevToolsPathCallback>,
  /// The inspected tree's profiling handle, which the Profiler tab records on.
  #[devtools_ignore]
  pub profiling: Option<ProfilingHandle>,
}

pub struct DevTools {
  selected_path: Signal<Vec<usize>>,
  collapsed_nodes: Signal<Vec<NodeId>>,
  collapsed_sections: Signal<Vec<String>>,
  component_search: Signal<String>,
  show_inspected_overlay: Signal<bool>,
  pick_inspected: Signal<bool>,
  active_tab: Signal<DevToolsTab>,
  profiler: Signal<profiler::ProfilerState>,
  /// Rereads a running recording's status; runs only while one does.
  profiler_status_ticker: Interval,
  signals_recording: Store<signals::SignalsRecordingState>,
  selected_signal: Signal<Option<String>>,
  #[cfg(feature = "persistent_storage")]
  persistent_storage_type_tooltip: Signal<Option<String>>,
  tree: DevToolsTree,
  last_picked_revision: AtomicU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, crate::DevtoolsInspectable)]
pub(crate) enum DevToolsTab {
  Components,
  Profiler,
  Signals,
  #[cfg(feature = "persistent_storage")]
  PersistentStorage,
}

struct DevToolsTree {
  scroll: ScrollState,
}

impl DevToolsTree {
  fn new() -> Self {
    Self {
      scroll: ScrollState::new(),
    }
  }

  fn scroll_state(&self) -> ScrollState {
    self.scroll.clone()
  }

  fn expand_ancestors(&self, snapshot: &DevToolsSnapshot, path: &[usize], collapsed_nodes: &mut Vec<NodeId>) {
    let Some(mut node) = snapshot.root.as_ref() else {
      return;
    };

    for index in path {
      collapsed_nodes.retain(|node_id| *node_id != node.id);
      let Some(child) = node.children.get(*index) else {
        return;
      };
      node = child;
    }
  }

  fn scroll_into(&self, snapshot: &DevToolsSnapshot, path: &[usize], collapsed_nodes: &[NodeId]) {
    let Some(index) = self.visible_row_index(snapshot, path, collapsed_nodes) else {
      return;
    };

    let row_top = index as f32 * tree_panel::TREE_ROW_HEIGHT;
    let row_bottom = row_top + tree_panel::TREE_ROW_HEIGHT;
    let viewport_height = self.scroll.viewport_height();
    let current_y = self.scroll.scroll_y();
    let margin = tree_panel::TREE_ROW_HEIGHT * 2.0;

    let next_y = if viewport_height <= 0.0 {
      row_top
    } else if row_top < current_y + margin {
      row_top - margin
    } else if row_bottom > current_y + viewport_height - margin {
      row_bottom + margin - viewport_height
    } else {
      current_y
    };

    let target_x = path.len() as f32 * 16.0;
    let viewport_width = self.scroll.viewport_width();
    let current_x = self.scroll.scroll_x();
    let next_x = if viewport_width <= 0.0 {
      target_x
    } else if target_x < current_x + 24.0 {
      target_x - 24.0
    } else if target_x + 220.0 > current_x + viewport_width {
      target_x + 220.0 - viewport_width
    } else {
      current_x
    };

    self.scroll.set_scroll_pending(next_x, next_y);
  }

  fn visible_row_index(
    &self,
    snapshot: &DevToolsSnapshot,
    target_path: &[usize],
    collapsed_nodes: &[NodeId],
  ) -> Option<usize> {
    let root = snapshot.root.as_ref()?;
    let mut row = 0;
    Self::visible_row_index_in(root, target_path, &mut Vec::new(), collapsed_nodes, &mut row)
  }

  fn visible_row_index_in(
    node: &DevToolsNode,
    target_path: &[usize],
    path: &mut Vec<usize>,
    collapsed_nodes: &[NodeId],
    row: &mut usize,
  ) -> Option<usize> {
    if path.as_slice() == target_path {
      return Some(*row);
    }

    *row += 1;
    if collapsed_nodes.contains(&node.id) {
      return None;
    }

    for (index, child) in node.children.iter().enumerate() {
      path.push(index);
      let found = Self::visible_row_index_in(child, target_path, path, collapsed_nodes, row);
      path.pop();
      if found.is_some() {
        return found;
      }
    }

    None
  }
}

impl PartialEq for DevToolsProps {
  fn eq(&self, other: &Self) -> bool {
    self.snapshot_revision == other.snapshot_revision && self.picked_revision == other.picked_revision && {
      #[cfg(feature = "persistent_storage")]
      {
        self.persistent_storage_revision == other.persistent_storage_revision
      }
      #[cfg(not(feature = "persistent_storage"))]
      {
        true
      }
    }
  }
}

impl Component for DevTools {
  type Props = DevToolsProps;

  fn create(ctx: &mut Ctx) -> Self {
    let profiler = ctx.signal(profiler::ProfilerState::default());
    let profiler_status_ticker = ctx.create_interval(profiler::STATUS_INTERVAL, {
      let profiler = profiler.clone();
      move || profiler.update(profiler::ProfilerState::refresh_status)
    });
    Self {
      selected_path: ctx.signal(Vec::new()),
      collapsed_nodes: ctx.signal(Vec::new()),
      collapsed_sections: ctx.signal(Vec::new()),
      component_search: ctx.signal(String::new()),
      show_inspected_overlay: ctx.signal(true),
      pick_inspected: ctx.signal(false),
      active_tab: ctx.signal(DevToolsTab::Components),
      profiler,
      profiler_status_ticker,
      signals_recording: ctx.store(signals::SignalsRecordingState::default()),
      selected_signal: ctx.signal(None),
      #[cfg(feature = "persistent_storage")]
      persistent_storage_type_tooltip: ctx.signal(None),
      tree: DevToolsTree::new(),
      last_picked_revision: AtomicU64::new(0),
    }
  }

  fn on_unmounted(&self) {
    // A recording left running would keep counting against the session limit.
    self.profiler_status_ticker.stop();
    self.profiler.update(profiler::ProfilerState::clear);
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<DevToolsProps>().clone();
    let snapshot = props.snapshot;
    #[cfg(feature = "persistent_storage")]
    let persistent_storage_snapshot = ctx.app_ref().persistent_storage().snapshot();
    let mut selected_path = self.selected_path.get();
    let mut collapsed_nodes = self.collapsed_nodes.get();
    let show_inspected_overlay = self.show_inspected_overlay.get();
    let mut pick_inspected = self.pick_inspected.get();
    let active_tab = self.active_tab.get();
    let component_search = self.component_search.get();
    let profiler_state = self.profiler.get();

    let mut signals_recording = self.signals_recording.get();
    if signals_recording.recording() {
      signals::record_signal_changes(&snapshot, &self.signals_recording);
      signals_recording = self.signals_recording.get();
    }
    let selected_signal = self.selected_signal.get();

    if props.picked_revision != self.last_picked_revision.load(Ordering::Relaxed) {
      self
        .last_picked_revision
        .store(props.picked_revision, Ordering::Relaxed);
      self.pick_inspected.set(false);
      pick_inspected = false;
      if let Some(path) = props.picked_path.clone() {
        let tree = &self.tree;
        tree.expand_ancestors(&snapshot, &path, &mut collapsed_nodes);
        self.collapsed_nodes.set(collapsed_nodes.clone());
        self.selected_path.set(path.clone());
        if let Some(on_selected_path) = &props.on_selected_path {
          on_selected_path(path.clone());
        }
        selected_path = path;
        tree.scroll_into(&snapshot, &selected_path, &collapsed_nodes);
      }
    }

    let selected = snapshot.selected_node(&selected_path);

    Column::new()
      .child(top_bar::top_bar(
        0,
        show_inspected_overlay,
        self.show_inspected_overlay.clone(),
        pick_inspected,
        self.pick_inspected.clone(),
        selected_path.clone(),
        selected.is_some(),
        self.component_search.clone(),
        props.on_debug_overlay_path.clone(),
        props.on_overlay_enabled.clone(),
        props.on_pick_inspected.clone(),
        props.on_save_node_screenshot.clone(),
        active_tab,
        self.active_tab.clone(),
        signals_recording.recording(),
        self.signals_recording.clone(),
      ))
      .child(match active_tab {
        DevToolsTab::Components => components_view(
          &snapshot,
          selected,
          selected_path,
          self.selected_path.clone(),
          collapsed_nodes,
          self.collapsed_nodes.clone(),
          self.collapsed_sections.get(),
          self.collapsed_sections.clone(),
          component_search,
          self.tree.scroll_state(),
          show_inspected_overlay,
          props.on_debug_overlay_path.clone(),
          props.on_selected_path.clone(),
        ),
        DevToolsTab::Profiler => {
          let analysis = profiler::analysis_props(&profiler_state, self.profiler.clone())
            .map(|props| ctx.mount::<profiler_analysis::ProfilerAnalysis>(props))
            .unwrap_or_default();
          // The tab's own toolbar: the top bar has no room for its buttons
          // next to the tabs in a window of the default width.
          Column::new()
            .child(profiler::profiler_actions(
              &profiler_state,
              self.profiler.clone(),
              props.profiling.clone(),
              self.profiler_status_ticker.clone(),
            ))
            .child(profiler::profiler_view(
              &profiler_state,
              self.profiler.clone(),
              analysis,
            ))
            .height(style::FILL)
            .width(style::FILL)
            .flex(1.0)
            .into()
        }
        DevToolsTab::Signals => signals::signals_view(
          &snapshot,
          selected_signal,
          self.selected_signal.clone(),
          &signals_recording,
        ),
        #[cfg(feature = "persistent_storage")]
        DevToolsTab::PersistentStorage => match persistent_storage_snapshot {
          Ok(entries) => persistent_storage_panel::persistent_storage_view(
            ctx,
            &entries,
            self.persistent_storage_type_tooltip.clone(),
          ),
          Err(error) => persistent_storage_panel::persistent_storage_error_view(&error.to_string()),
        },
      })
      .width(style::FILL)
      .height(style::FILL)
      .background(style::BG)
  }
}

fn components_view(
  snapshot: &DevToolsSnapshot,
  selected: Option<&DevToolsNode>,
  selected_path: Vec<usize>,
  selected_path_signal: Signal<Vec<usize>>,
  collapsed_nodes: Vec<NodeId>,
  collapsed_nodes_signal: Signal<Vec<NodeId>>,
  collapsed_sections: Vec<String>,
  collapsed_sections_signal: Signal<Vec<String>>,
  component_search: String,
  tree_scroll: ScrollState,
  show_inspected_overlay: bool,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_selected_path: Option<DevToolsPathCallback>,
) -> Element {
  Row::new()
    .child(tree_panel::tree_panel(
      snapshot,
      selected_path,
      selected_path_signal,
      collapsed_nodes,
      collapsed_nodes_signal,
      component_search,
      tree_scroll,
      show_inspected_overlay,
      on_debug_overlay_path,
      on_selected_path,
    ))
    .child(inspector::inspector_panel(
      selected,
      snapshot.frame,
      collapsed_sections,
      collapsed_sections_signal,
    ))
    .width(style::FILL)
    .height(style::FILL)
    .flex(1.0)
    .into()
}

pub(crate) fn debug_overlay_path_for_selection(
  overlay_enabled: bool,
  selected_path: Vec<usize>,
  has_selection: bool,
) -> Option<Vec<usize>> {
  if overlay_enabled && has_selection {
    Some(selected_path)
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    app::{App, devtools::snapshot::DevToolsNodeKind},
    components::Column,
    layout::text_style::FontWeight,
  };

  #[test]
  fn snapshot_collects_tree_nodes() {
    let mut app = App::new();
    let mut tree = crate::app::Tree::new();
    tree.mount_root::<SnapshotTestApp>(&mut app, ());
    assert!(tree.draw_debug_overlay_over_node(vec![0]));
    assert!(!tree.draw_debug_overlay_over_node(vec![0]));
    assert!(tree.clear_debug_overlay());
    tree.draw_perf_overlay();
    assert!(tree.perf_overlay_enabled());
    tree.tick_perf_overlay();

    let snapshot = DevToolsSnapshot::from_tree(&tree);

    assert!(snapshot.node_count() >= 2);
    assert_eq!(
      snapshot.root.as_ref().map(|node| style::short_tag(&node.tag)),
      Some("SnapshotTestApp")
    );
    assert_eq!(
      snapshot.root.as_ref().map(|node| node.kind),
      Some(DevToolsNodeKind::Component)
    );
    assert_eq!(
      snapshot
        .root
        .as_ref()
        .and_then(|node| node.children.first())
        .map(|node| node.kind),
      Some(DevToolsNodeKind::Element)
    );
    let child_shape = snapshot
      .root
      .as_ref()
      .and_then(|node| node.children.first())
      .map(|node| node.shape.as_slice())
      .unwrap_or(&[]);
    assert_eq!(
      child_shape
        .iter()
        .find(|row| row.label == "layout")
        .and_then(|row| row.value.as_deref()),
      Some("Leaf")
    );
    let padding = child_shape.iter().find(|row| row.label == "padding").unwrap();
    assert_eq!(padding.value, None);
    assert_eq!(
      padding
        .children
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_deref()))
        .collect::<Vec<_>>(),
      vec![
        ("left", Some("8px")),
        ("right", Some("8px")),
        ("top", Some("8px")),
        ("bottom", Some("8px")),
      ]
    );
    let hover_style = child_shape.iter().find(|row| row.label == "hover style").unwrap();
    assert_eq!(
      hover_style
        .children
        .iter()
        .find(|row| row.label == "fill")
        .and_then(|row| row.value.as_deref()),
      Some("#ff0000")
    );
    let hover_padding = hover_style.children.iter().find(|row| row.label == "padding").unwrap();
    assert_eq!(
      hover_padding
        .children
        .iter()
        .find(|row| row.label == "left")
        .and_then(|row| row.value.as_deref()),
      Some("4px")
    );
    let active_style = child_shape.iter().find(|row| row.label == "active style").unwrap();
    let active_frame = active_style.children.iter().find(|row| row.label == "frame").unwrap();
    assert_eq!(
      active_frame
        .children
        .iter()
        .find(|row| row.label == "width")
        .and_then(|row| row.value.as_deref()),
      Some("32px")
    );
    let root_effects = snapshot
      .root
      .as_ref()
      .map(|node| node.effects.as_slice())
      .unwrap_or(&[]);
    assert_eq!(root_effects.len(), 1);
    assert!(root_effects[0].id > 0);
    let keyed_child = snapshot
      .root
      .as_ref()
      .and_then(|node| {
        node
          .children
          .iter()
          .find(|child| style::short_tag(&child.tag) == "KeyedChild")
      })
      .expect("keyed child should be captured");
    assert_eq!(keyed_child.key.as_deref(), Some("child-key"));
    let keyed_child_props = keyed_child
      .props
      .as_ref()
      .expect("keyed child props should be captured");
    assert_eq!(style::short_tag(&keyed_child_props.type_name), "KeyedChildProps");
    assert_eq!(keyed_child_props.fields[0].name(), "title");
    assert_eq!(keyed_child_props.fields[0].type_name(), "&str");
    assert_eq!(keyed_child_props.fields[0].formatted_value(), Some("\"keyed child\""));
    assert_eq!(keyed_child_props.fields[1].name(), "count");
    assert_eq!(keyed_child_props.fields[1].type_name(), "u32");
    assert_eq!(keyed_child_props.fields[1].formatted_value(), Some("7"));
    assert_eq!(keyed_child_props.fields[2].name(), "details");
    assert_eq!(
      style::short_tag(keyed_child_props.fields[2].type_name()),
      "KeyedChildDetails"
    );
    assert_eq!(keyed_child_props.fields[2].children()[0].name(), "enabled");
    assert_eq!(keyed_child_props.fields[2].children()[0].type_name(), "bool");
    assert_eq!(
      keyed_child_props.fields[2].children()[0].formatted_value(),
      Some("true")
    );
    assert_eq!(
      snapshot
        .root
        .as_ref()
        .and_then(|node| node.props.as_ref())
        .map(|props| props.fields.len()),
      Some(0)
    );
    assert_eq!(
      snapshot
        .root
        .as_ref()
        .and_then(|node| node.signals.first())
        .map(|signal| signal.type_name.as_ref()),
      Some("i32")
    );
    assert_eq!(
      snapshot
        .root
        .as_ref()
        .and_then(|node| node.signals.first())
        .and_then(|signal| signal.formatted_value()),
      Some("0".to_owned())
    );
    assert_eq!(snapshot.root.as_ref().map(|node| node.contexts.len()), Some(2));
  }

  #[test]
  fn selected_snapshot_only_collects_inspector_details_for_selected_path() {
    let mut app = App::new();
    let mut tree = crate::app::Tree::new();
    tree.mount_root::<SnapshotTestApp>(&mut app, ());

    let snapshot = DevToolsSnapshot::from_tree_for_selection(&tree, &[0]);
    let root = snapshot.root.as_ref().expect("root should be captured");
    let selected = root.children.first().expect("selected child should be captured");

    assert!(root.shape.is_empty());
    assert!(root.props.is_none());
    assert!(!root.signals.is_empty());
    assert!(!selected.shape.is_empty());
    assert!(selected.layout_box.is_none());
  }

  #[test]
  fn debug_overlay_path_for_selection_respects_toggle_and_selection() {
    assert_eq!(
      debug_overlay_path_for_selection(true, vec![1, 2], true),
      Some(vec![1, 2])
    );
    assert_eq!(debug_overlay_path_for_selection(false, vec![1, 2], true), None);
    assert_eq!(debug_overlay_path_for_selection(true, vec![1, 2], false), None);
  }

  #[test]
  fn picked_tree_path_expands_ancestors_and_resolves_visible_row() {
    let ids = crate::core::IdGenerator::new();
    let root_id = ids.next();
    let child_id = ids.next();
    let snapshot = DevToolsSnapshot {
      root: Some(test_node(
        root_id,
        "Root",
        vec![test_node(
          child_id,
          "Child",
          vec![test_node(ids.next(), "Grandchild", Vec::new())],
        )],
      )),
      frame: Default::default(),
    };
    let mut collapsed = vec![root_id, child_id];
    let tree = DevToolsTree::new();

    tree.expand_ancestors(&snapshot, &[0, 0], &mut collapsed);

    assert!(collapsed.is_empty());
    assert_eq!(tree.visible_row_index(&snapshot, &[0, 0], &collapsed), Some(2));
  }

  #[test]
  fn visible_tree_row_index_respects_collapsed_nodes() {
    let ids = crate::core::IdGenerator::new();
    let root_id = ids.next();
    let snapshot = DevToolsSnapshot {
      root: Some(test_node(
        root_id,
        "Root",
        vec![test_node(ids.next(), "Child", Vec::new())],
      )),
      frame: Default::default(),
    };
    let tree = DevToolsTree::new();

    assert_eq!(tree.visible_row_index(&snapshot, &[0], &[root_id]), None);
  }

  fn test_node(id: NodeId, tag: &str, children: Vec<DevToolsNode>) -> DevToolsNode {
    DevToolsNode {
      id,
      tag: tag.to_owned(),
      kind: DevToolsNodeKind::Element,
      key: None,
      attrs: Vec::new(),
      text: None,
      color: None,
      props: None,
      signals: Vec::new(),
      memos: Vec::new(),
      contexts: Vec::new(),
      shape: Vec::new(),
      effects: Vec::new(),
      layout_box: None,
      hovered: false,
      active: false,
      focused: false,
      children,
    }
  }

  struct SnapshotTestApp {
    count: crate::core::Signal<i32>,
  }

  impl Component for SnapshotTestApp {
    type Props = ();

    fn create(ctx: &mut Ctx) -> Self {
      let count = ctx.signal(0);
      let effect_count = count.clone();
      ctx.on_effect(move || {
        let _ = effect_count.get();
      });
      Self { count }
    }

    fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
      ctx.provide(42_i32);
      let _ = ctx.use_context::<i32>();
      Column::new()
        .child(
          style::text(
            &format!("child {}", self.count.get()),
            12.0,
            FontWeight::Normal,
            style::TEXT,
          )
          .padding(8.0)
          .hovered(|style| style.background("#ff0000").padding_left(4.0))
          .active(|style| style.width(32.0)),
        )
        .child(ctx.mount_keyed::<KeyedChild>(
          "child-key",
          KeyedChildProps {
            title: "keyed child",
            count: 7,
            details: KeyedChildDetails { enabled: true },
          },
        ))
    }
  }

  struct KeyedChild;

  #[derive(Clone, PartialEq, crate::DevtoolsInspectable)]
  struct KeyedChildProps {
    title: &'static str,
    count: u32,
    details: KeyedChildDetails,
  }

  #[derive(Clone, PartialEq, crate::DevtoolsInspectable)]
  struct KeyedChildDetails {
    enabled: bool,
  }

  impl Component for KeyedChild {
    type Props = KeyedChildProps;

    fn create(_ctx: &mut Ctx) -> Self {
      Self
    }

    fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
      style::text(ctx.props::<Self::Props>().title, 12.0, FontWeight::Normal, style::TEXT)
    }
  }
}
