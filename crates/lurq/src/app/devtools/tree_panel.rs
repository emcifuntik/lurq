use super::{
  DevToolsDebugOverlayCallback, DevToolsPathCallback, debug_overlay_path_for_selection,
  snapshot::{DevToolsNode, DevToolsSnapshot},
  style::{
    BLUE, BORDER, FILL, GREEN, MUTED, ORANGE, PRIMARY, SELECTED, SURFACE, SURFACE_2, badge, empty_state, icon,
    section_header, short_tag, text,
  },
};
use crate::{
  components::{Column, Row, ScrollBoth, Spacer},
  core::{NodeId, Signal},
  layout::{
    Alignment,
    layout_kind::{FrameConstraints, ScrollState},
    text_style::FontWeight,
  },
  node::{CursorIcon, Element, border::Border, color::Color, dimension::Dimension},
};

const TREE_PANEL_WIDTH: f32 = 380.0;
const TREE_CONTENT_MIN_WIDTH: f32 = 640.0;
pub(crate) const TREE_ROW_HEIGHT: f32 = 26.0;
const TREE_OVERSCAN_ROWS: usize = 16;
const TREE_INITIAL_ROWS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TreeRowWindow {
  start: usize,
  end: usize,
}

pub(crate) fn tree_panel(
  snapshot: &DevToolsSnapshot,
  selected_path: Vec<usize>,
  selected: Signal<Vec<usize>>,
  collapsed_nodes: Vec<NodeId>,
  collapsed: Signal<Vec<NodeId>>,
  search_query: String,
  scroll_state: ScrollState,
  overlay_enabled: bool,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_selected_path: Option<DevToolsPathCallback>,
) -> Element {
  let mut rows = Vec::new();
  let mut row_count = 0;
  let search_query = parse_search_query(&search_query);
  if let Some(root) = &snapshot.root {
    let row_window = tree_row_window(&scroll_state);
    collect_tree_rows(
      root,
      &mut Vec::new(),
      0,
      row_window,
      &mut row_count,
      &selected_path,
      selected,
      &collapsed_nodes,
      collapsed,
      search_query.as_ref(),
      overlay_enabled,
      on_debug_overlay_path,
      on_selected_path,
      &mut rows,
    );

    let top_spacer = row_window.start.min(row_count) as f32 * TREE_ROW_HEIGHT;
    let rendered_rows = rows.len();
    let bottom_rows = row_count.saturating_sub(row_window.start.min(row_count) + rendered_rows);
    let bottom_spacer = bottom_rows as f32 * TREE_ROW_HEIGHT;
    if top_spacer > 0.0 {
      rows.insert(0, Spacer::new().height(top_spacer).into());
    }
    if bottom_spacer > 0.0 {
      rows.push(Spacer::new().height(bottom_spacer).into());
    }
  } else {
    rows.push(empty_state("No mounted root"));
  }

  if snapshot.root.is_some() && row_count == 0 {
    rows.push(empty_state("No components match search"));
  }

  let count_label = if search_query.is_some() {
    format!("{row_count} matches")
  } else {
    format!("{row_count} components")
  };

  Column::new()
    .child(section_header("COMPONENT TREE", &count_label))
    .child(
      ScrollBoth::new(Column::new().with_children(rows))
        .with_scroll_state(scroll_state)
        .height(FILL)
        .width(FILL)
        .flex(1.0),
    )
    .width(TREE_PANEL_WIDTH)
    .height(FILL)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .into()
}

fn tree_row_window(scroll_state: &ScrollState) -> TreeRowWindow {
  let scroll_y = scroll_state.scroll_y().max(0.0);
  let viewport_height = scroll_state.viewport_height();
  let first_visible = (scroll_y / TREE_ROW_HEIGHT).floor().max(0.0) as usize;
  let start = first_visible.saturating_sub(TREE_OVERSCAN_ROWS);
  let visible_rows = if viewport_height > 0.0 {
    (viewport_height / TREE_ROW_HEIGHT).ceil().max(1.0) as usize
  } else {
    TREE_INITIAL_ROWS
  };
  let end = first_visible
    .saturating_add(visible_rows)
    .saturating_add(TREE_OVERSCAN_ROWS);

  TreeRowWindow { start, end }
}

// Each argument is a separate piece of panel state; bundling them is a refactor of its own.
#[allow(clippy::too_many_arguments)]
fn collect_tree_rows(
  node: &DevToolsNode,
  path: &mut Vec<usize>,
  depth: usize,
  row_window: TreeRowWindow,
  row_count: &mut usize,
  selected_path: &[usize],
  selected: Signal<Vec<usize>>,
  collapsed_nodes: &[NodeId],
  collapsed: Signal<Vec<NodeId>>,
  search_query: Option<&TreeSearchQuery>,
  overlay_enabled: bool,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_selected_path: Option<DevToolsPathCallback>,
  rows: &mut Vec<Element>,
) {
  if let Some(query) = search_query
    && !node_subtree_matches_search(node, query)
  {
    return;
  }

  let is_collapsed = search_query.is_none() && !node.children.is_empty() && collapsed_nodes.contains(&node.id);
  let row_index = *row_count;
  *row_count += 1;
  if row_index >= row_window.start && row_index < row_window.end {
    rows.push(tree_row(
      node,
      path.clone(),
      depth,
      path.as_slice() == selected_path,
      is_collapsed,
      selected.clone(),
      collapsed.clone(),
      search_query.is_some(),
      overlay_enabled,
      on_debug_overlay_path.clone(),
      on_selected_path.clone(),
    ));
  }
  if is_collapsed {
    return;
  }

  for (index, child) in node.children.iter().enumerate() {
    path.push(index);
    collect_tree_rows(
      child,
      path,
      depth + 1,
      row_window,
      row_count,
      selected_path,
      selected.clone(),
      collapsed_nodes,
      collapsed.clone(),
      search_query,
      overlay_enabled,
      on_debug_overlay_path.clone(),
      on_selected_path.clone(),
      rows,
    );
    path.pop();
  }
}

// Each argument is a separate piece of panel state; bundling them is a refactor of its own.
#[allow(clippy::too_many_arguments)]
fn tree_row(
  node: &DevToolsNode,
  path: Vec<usize>,
  depth: usize,
  selected: bool,
  collapsed: bool,
  selected_path: Signal<Vec<usize>>,
  collapsed_nodes: Signal<Vec<NodeId>>,
  search_active: bool,
  overlay_enabled: bool,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_selected_path: Option<DevToolsPathCallback>,
) -> Element {
  let indent = 8.0 + depth as f32 * 16.0;
  let child_count = node.children.len();
  let click_path = path;
  let collapse_id = node.id;
  let tag = short_tag(&node.tag);
  let key_preview = node.key.as_deref().map(format_attr_value);
  let text_preview = node.text.as_deref().filter(|_| tag == "Text").map(format_attr_value);
  let tag_color = if node.tag.starts_with("lurq::") || node.tag.contains("Demo") {
    BLUE
  } else {
    MUTED
  };
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .spacing(6.0)
    .child(Spacer::new().width(indent))
    .child(if child_count > 0 {
      let icon_name = if collapsed { "chevron-right" } else { "chevron-down" };
      let mut collapse_icon = icon(icon_name, 12.0, MUTED)
        .width(12.0)
        .height(18.0)
        .cursor(CursorIcon::Pointer);
      if !search_active {
        collapse_icon = collapse_icon.on_click(move |_| toggle_collapsed(&collapsed_nodes, collapse_id));
      }
      collapse_icon
    } else {
      text("", 12.0, FontWeight::Normal, MUTED).width(12.0)
    })
    .child(text("<", 12.0, FontWeight::Normal, MUTED))
    .child(text(tag, 12.0, FontWeight::Bold, tag_color).nowrap());

  if let Some(preview) = key_preview {
    row = row
      .child(text(" key=", 12.0, FontWeight::Normal, MUTED))
      .child(text(&preview, 12.0, FontWeight::Normal, ORANGE).nowrap());
  }

  for (name, value) in &node.attrs {
    let preview = format_attr_value(value);
    row = row
      .child(text(&format!(" {name}="), 12.0, FontWeight::Normal, MUTED))
      .child(text(&preview, 12.0, FontWeight::Normal, ORANGE).nowrap());
  }

  if let Some(preview) = text_preview {
    row = row
      .child(text(" text=", 12.0, FontWeight::Normal, MUTED))
      .child(text(&preview, 12.0, FontWeight::Normal, ORANGE).nowrap());
  }

  row = row.child(text("/>", 12.0, FontWeight::Normal, MUTED));
  if child_count > 0 {
    row = row.child(badge(&child_count.to_string(), GREEN, SURFACE_2));
  }

  row = row
    .height(TREE_ROW_HEIGHT)
    .background(if selected { SELECTED } else { "#00000000" })
    .frame(FrameConstraints {
      min_width: Some(Dimension::Px(TREE_CONTENT_MIN_WIDTH)),
      ..Default::default()
    })
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| {
      selected_path.set(click_path.clone());
      if let Some(on_selected_path) = &on_selected_path {
        on_selected_path(click_path.clone());
      }
      if let Some(on_debug_overlay_path) = &on_debug_overlay_path {
        on_debug_overlay_path(debug_overlay_path_for_selection(
          overlay_enabled,
          click_path.clone(),
          true,
        ));
      }
    });
  if selected {
    row = row.border_left(Border::inside(2.0, Color::from_hex(PRIMARY)));
  }

  row.into()
}

fn format_attr_value(content: &str) -> String {
  const MAX_CHARS: usize = 48;

  let mut escaped = content
    .replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
    .replace('\t', "\\t");

  if escaped.chars().count() > MAX_CHARS {
    escaped = escaped.chars().take(MAX_CHARS).collect::<String>();
    escaped.push_str("...");
  }

  format!("\"{escaped}\"")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeSearchQuery {
  terms: Vec<TreeSearchTerm>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeSearchTerm {
  field: TreeSearchField,
  value: String,
  negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeSearchField {
  Any,
  Tag,
  Key,
  Text,
  Kind,
  State,
}

fn parse_search_query(query: &str) -> Option<TreeSearchQuery> {
  let terms = tokenize_search_query(query)
    .into_iter()
    .filter_map(parse_search_term)
    .collect::<Vec<_>>();
  (!terms.is_empty()).then_some(TreeSearchQuery { terms })
}

fn tokenize_search_query(query: &str) -> Vec<String> {
  let mut tokens = Vec::new();
  let mut current = String::new();
  let mut chars = query.chars();
  let mut in_quote = false;

  while let Some(ch) = chars.next() {
    match ch {
      '"' => in_quote = !in_quote,
      '\\' if in_quote => {
        if let Some(next) = chars.next() {
          current.push(next);
        }
      }
      ch if ch.is_whitespace() && !in_quote => {
        if !current.is_empty() {
          tokens.push(std::mem::take(&mut current));
        }
      }
      _ => current.push(ch),
    }
  }

  if !current.is_empty() {
    tokens.push(current);
  }

  tokens
}

fn parse_search_term(token: String) -> Option<TreeSearchTerm> {
  let token = token.trim();
  if token.is_empty() {
    return None;
  }

  let (negated, token) = token.strip_prefix('-').map_or((false, token), |token| (true, token));
  let (field, value) = parse_search_field(token).unwrap_or((TreeSearchField::Any, token));
  let value = value.trim();
  if value.is_empty() {
    return None;
  }

  Some(TreeSearchTerm {
    field,
    value: value.to_ascii_lowercase(),
    negated,
  })
}

fn parse_search_field(token: &str) -> Option<(TreeSearchField, &str)> {
  let (field, value) = token.split_once(':')?;
  let field = match field.to_ascii_lowercase().as_str() {
    "tag" | "name" | "component" => TreeSearchField::Tag,
    "key" => TreeSearchField::Key,
    "text" | "value" => TreeSearchField::Text,
    "kind" => TreeSearchField::Kind,
    "state" => TreeSearchField::State,
    _ => return None,
  };
  Some((field, value))
}

fn node_subtree_matches_search(node: &DevToolsNode, query: &TreeSearchQuery) -> bool {
  node_matches_search(node, query)
    || node
      .children
      .iter()
      .any(|child| node_subtree_matches_search(child, query))
}

fn node_matches_search(node: &DevToolsNode, query: &TreeSearchQuery) -> bool {
  query.terms.iter().all(|term| {
    let matched = node_matches_search_term(node, term);
    if term.negated { !matched } else { matched }
  })
}

fn node_matches_search_term(node: &DevToolsNode, term: &TreeSearchTerm) -> bool {
  match term.field {
    TreeSearchField::Any => {
      string_matches_search(&node.tag, &term.value)
        || string_matches_search(short_tag(&node.tag), &term.value)
        || node
          .key
          .as_deref()
          .is_some_and(|key| string_matches_search(key, &term.value))
        || node
          .text
          .as_deref()
          .is_some_and(|text| string_matches_search(text, &term.value))
    }
    TreeSearchField::Tag => {
      string_matches_search(&node.tag, &term.value) || string_matches_search(short_tag(&node.tag), &term.value)
    }
    TreeSearchField::Key => node
      .key
      .as_deref()
      .is_some_and(|key| string_matches_search(key, &term.value)),
    TreeSearchField::Text => node
      .text
      .as_deref()
      .is_some_and(|text| string_matches_search(text, &term.value)),
    TreeSearchField::Kind => string_matches_search(node_kind_label(node.kind), &term.value),
    TreeSearchField::State => {
      (node.hovered && string_matches_search("hovered", &term.value))
        || (node.active && string_matches_search("active", &term.value))
        || (node.focused && string_matches_search("focused", &term.value))
    }
  }
}

fn string_matches_search(value: &str, query: &str) -> bool {
  value.to_ascii_lowercase().contains(query)
}

fn node_kind_label(kind: super::snapshot::DevToolsNodeKind) -> &'static str {
  match kind {
    super::snapshot::DevToolsNodeKind::Component => "component",
    super::snapshot::DevToolsNodeKind::Element => "element",
  }
}

fn toggle_collapsed(collapsed_nodes: &Signal<Vec<NodeId>>, id: NodeId) {
  collapsed_nodes.update(|nodes| {
    if let Some(index) = nodes.iter().position(|node_id| *node_id == id) {
      nodes.remove(index);
    } else {
      nodes.push(id);
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::IdGenerator;

  #[test]
  fn toggle_collapsed_adds_and_removes_node_id() {
    let id = IdGenerator::new().next();
    let collapsed = Signal::new(Vec::new());

    toggle_collapsed(&collapsed, id);
    assert_eq!(collapsed.get_untracked(), vec![id]);

    toggle_collapsed(&collapsed, id);
    assert!(collapsed.get_untracked().is_empty());
  }

  #[test]
  fn collect_tree_rows_skips_children_below_collapsed_node() {
    let ids = IdGenerator::new();
    let child_id = ids.next();
    let root = DevToolsNode {
      id: ids.next(),
      tag: "Root".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Component,
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
      children: vec![DevToolsNode {
        id: child_id,
        tag: "Child".to_owned(),
        kind: super::super::snapshot::DevToolsNodeKind::Element,
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
        children: vec![DevToolsNode {
          id: ids.next(),
          tag: "Grandchild".to_owned(),
          kind: super::super::snapshot::DevToolsNodeKind::Element,
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
          children: Vec::new(),
        }],
      }],
    };
    let collapsed = Signal::new(vec![child_id]);
    let mut rows = Vec::new();
    let mut row_count = 0;

    collect_tree_rows(
      &root,
      &mut Vec::new(),
      0,
      TreeRowWindow { start: 0, end: 16 },
      &mut row_count,
      &[],
      Signal::new(Vec::new()),
      &collapsed.get_untracked(),
      collapsed,
      None,
      true,
      None,
      None,
      &mut rows,
    );

    assert_eq!(rows.len(), 2);
    assert_eq!(row_count, 2);
  }

  #[test]
  fn collect_tree_rows_only_builds_requested_window() {
    let ids = IdGenerator::new();
    let root = DevToolsNode {
      id: ids.next(),
      tag: "Root".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Component,
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
      children: (0..6)
        .map(|index| DevToolsNode {
          id: ids.next(),
          tag: format!("Child{index}"),
          kind: super::super::snapshot::DevToolsNodeKind::Element,
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
          children: Vec::new(),
        })
        .collect(),
    };
    let collapsed = Signal::new(Vec::new());
    let mut rows = Vec::new();
    let mut row_count = 0;

    collect_tree_rows(
      &root,
      &mut Vec::new(),
      0,
      TreeRowWindow { start: 2, end: 5 },
      &mut row_count,
      &[],
      Signal::new(Vec::new()),
      &collapsed.get_untracked(),
      collapsed,
      None,
      true,
      None,
      None,
      &mut rows,
    );

    assert_eq!(row_count, 7);
    assert_eq!(rows.len(), 3);
  }

  #[test]
  fn collect_tree_rows_searches_component_name_key_and_text() {
    let ids = IdGenerator::new();
    let text_child = DevToolsNode {
      id: ids.next(),
      tag: "Text".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Element,
      key: None,
      attrs: Vec::new(),
      text: Some("Confirm order".to_owned()),
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
      children: Vec::new(),
    };
    let root = DevToolsNode {
      id: ids.next(),
      tag: "CheckoutPanel".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Component,
      key: Some("checkout-root".to_owned()),
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
      children: vec![text_child],
    };

    assert!(node_subtree_matches_search(
      &root,
      &parse_search_query("checkoutpanel").unwrap()
    ));
    assert!(node_subtree_matches_search(
      &root,
      &parse_search_query("key:checkout-root").unwrap()
    ));
    assert!(node_subtree_matches_search(
      &root,
      &parse_search_query("text:\"Confirm order\"").unwrap()
    ));
    assert!(!node_subtree_matches_search(
      &root,
      &parse_search_query("missing").unwrap()
    ));
  }

  #[test]
  fn collect_tree_rows_search_keeps_ancestors_visible() {
    let ids = IdGenerator::new();
    let child_id = ids.next();
    let root = DevToolsNode {
      id: ids.next(),
      tag: "Root".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Component,
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
      children: vec![DevToolsNode {
        id: child_id,
        tag: "Child".to_owned(),
        kind: super::super::snapshot::DevToolsNodeKind::Element,
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
        children: vec![DevToolsNode {
          id: ids.next(),
          tag: "Grandchild".to_owned(),
          kind: super::super::snapshot::DevToolsNodeKind::Element,
          key: Some("target-key".to_owned()),
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
          children: Vec::new(),
        }],
      }],
    };
    let collapsed = Signal::new(vec![child_id]);
    let mut rows = Vec::new();
    let mut row_count = 0;

    collect_tree_rows(
      &root,
      &mut Vec::new(),
      0,
      TreeRowWindow { start: 0, end: 16 },
      &mut row_count,
      &[],
      Signal::new(Vec::new()),
      &collapsed.get_untracked(),
      collapsed,
      parse_search_query("key:target-key").as_ref(),
      true,
      None,
      None,
      &mut rows,
    );

    assert_eq!(row_count, 3);
    assert_eq!(rows.len(), 3);
  }

  #[test]
  fn parsed_search_supports_kind_state_and_negation() {
    let ids = IdGenerator::new();
    let focused_child = DevToolsNode {
      id: ids.next(),
      tag: "Button".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Element,
      key: Some("primary-action".to_owned()),
      attrs: Vec::new(),
      text: Some("Save changes".to_owned()),
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
      focused: true,
      children: Vec::new(),
    };
    let root = DevToolsNode {
      id: ids.next(),
      tag: "SettingsPanel".to_owned(),
      kind: super::super::snapshot::DevToolsNodeKind::Component,
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
      children: vec![focused_child],
    };

    assert!(node_subtree_matches_search(
      &root,
      &parse_search_query("kind:element state:focused").unwrap()
    ));
    assert!(node_subtree_matches_search(
      &root,
      &parse_search_query("text:\"Save changes\" -key:secondary").unwrap()
    ));
    assert!(!node_subtree_matches_search(
      &root,
      &parse_search_query("text:\"Save changes\" -key:primary").unwrap()
    ));
  }
}
