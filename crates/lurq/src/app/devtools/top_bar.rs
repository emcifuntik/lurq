use super::{
  DevToolsBoolCallback, DevToolsDebugOverlayCallback, DevToolsPathCallback, DevToolsTab,
  debug_overlay_path_for_selection, signals,
  style::{
    BORDER, FILL, MUTED, PRIMARY, SELECTED, SURFACE, SURFACE_2, TEXT, badge, icon, text, toolbar_button, toolbar_input,
    toolbar_search_input,
  },
};
use crate::{
  components::{Column, Rect, Row, Spacer},
  core::{Signal, Store},
  layout::{Alignment, text_style::FontWeight},
  node::{CursorIcon, Element, color::Color},
};

pub(crate) fn top_bar(
  _node_count: usize,
  overlay_enabled: bool,
  overlay_enabled_signal: Signal<bool>,
  pick_enabled: bool,
  pick_enabled_signal: Signal<bool>,
  selected_path: Vec<usize>,
  has_selection: bool,
  component_search: Signal<String>,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_overlay_enabled: Option<DevToolsBoolCallback>,
  on_pick_inspected: Option<DevToolsBoolCallback>,
  on_save_node_screenshot: Option<DevToolsPathCallback>,
  active_tab: DevToolsTab,
  active_tab_signal: Signal<DevToolsTab>,
  profiler_actions: Element,
  signals_recording: bool,
  signals_recording_store: Store<signals::SignalsRecordingState>,
) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(icon("bug", 16.0, PRIMARY))
        .child(text("lurq DevTools", 14.0, FontWeight::Bold, TEXT))
        .child(badge("v0.1.0", MUTED, SURFACE_2))
        .width(200.0),
    )
    .child(tab(
      "layers",
      "Components",
      active_tab == DevToolsTab::Components,
      DevToolsTab::Components,
      active_tab_signal.clone(),
    ))
    .child(tab(
      "activity",
      "Profiler",
      active_tab == DevToolsTab::Profiler,
      DevToolsTab::Profiler,
      active_tab_signal.clone(),
    ))
    .child(storage_tab(active_tab, active_tab_signal.clone()))
    .child(tab(
      "zap",
      "Signals",
      active_tab == DevToolsTab::Signals,
      DevToolsTab::Signals,
      active_tab_signal,
    ))
    .child(Spacer::new().flex(1.0))
    .child(match active_tab {
      DevToolsTab::Profiler => profiler_actions,
      DevToolsTab::Signals => signals_actions(signals_recording, signals_recording_store),
      #[cfg(feature = "persistent_storage")]
      DevToolsTab::PersistentStorage => Row::new().align_items(Alignment::Center).into(),
      DevToolsTab::Components => Row::new()
        .align_items(Alignment::Center)
        .child(pick_toggle(pick_enabled, pick_enabled_signal, on_pick_inspected))
        .child(Spacer::new().width(8.0))
        .child(overlay_toggle(
          overlay_enabled,
          overlay_enabled_signal,
          selected_path.clone(),
          has_selection,
          on_debug_overlay_path,
          on_overlay_enabled,
        ))
        .child(Spacer::new().width(8.0))
        .child(screenshot_button(selected_path, has_selection, on_save_node_screenshot))
        .child(Spacer::new().width(10.0))
        .child(toolbar_search_input(
          component_search,
          "Search components...",
          Some("Ctrl+F"),
        ))
        .into(),
    })
    .height(48.0)
    .width(FILL)
    .padding_horizontal(16.0)
    .padding_vertical(0.0)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .into()
}

#[cfg(feature = "persistent_storage")]
fn storage_tab(active_tab: DevToolsTab, active_tab_signal: Signal<DevToolsTab>) -> Element {
  tab(
    "box",
    "Storage",
    active_tab == DevToolsTab::PersistentStorage,
    DevToolsTab::PersistentStorage,
    active_tab_signal,
  )
}

#[cfg(not(feature = "persistent_storage"))]
fn storage_tab(_active_tab: DevToolsTab, _active_tab_signal: Signal<DevToolsTab>) -> Element {
  Element::new()
}

fn signals_actions(recording: bool, recording_store: Store<signals::SignalsRecordingState>) -> Element {
  let color = if recording { "#ef4444" } else { PRIMARY };
  let label = if recording { "Recording..." } else { "Start recording" };
  let record_store = recording_store.clone();
  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(
      toolbar_button(
        "circle",
        label,
        color,
        if recording { "#ef444420" } else { SURFACE_2 },
        if recording { "#ef444460" } else { BORDER },
      )
      .on_click(move |_| {
        record_store.update(move |state| state.set_recording(!recording));
      }),
    )
    .child(
      toolbar_button("trash-2", "Clear", MUTED, SURFACE_2, BORDER).on_click(move |_| {
        recording_store.update(|state| state.clear());
      }),
    )
    .child(toolbar_input("Filter signals...", None))
    .into()
}

fn screenshot_button(
  selected_path: Vec<usize>,
  has_selection: bool,
  on_save_node_screenshot: Option<DevToolsPathCallback>,
) -> Element {
  let color = if has_selection { PRIMARY } else { MUTED };
  toolbar_button("share-2", "Screenshot", color, SURFACE_2, BORDER)
    .on_click(move |_| {
      if has_selection && let Some(on_save_node_screenshot) = &on_save_node_screenshot {
        on_save_node_screenshot(selected_path.clone());
      }
    })
    .into()
}

fn overlay_toggle(
  overlay_enabled: bool,
  overlay_enabled_signal: Signal<bool>,
  selected_path: Vec<usize>,
  has_selection: bool,
  on_debug_overlay_path: Option<DevToolsDebugOverlayCallback>,
  on_overlay_enabled: Option<DevToolsBoolCallback>,
) -> Element {
  let color = if overlay_enabled { PRIMARY } else { MUTED };
  toolbar_button(
    "box",
    "Overlay",
    color,
    if overlay_enabled { SELECTED } else { SURFACE_2 },
    BORDER,
  )
  .on_click(move |_| {
    let next = !overlay_enabled;
    overlay_enabled_signal.set(next);
    if let Some(on_overlay_enabled) = &on_overlay_enabled {
      on_overlay_enabled(next);
    }
    if let Some(on_debug_overlay_path) = &on_debug_overlay_path {
      on_debug_overlay_path(debug_overlay_path_for_selection(
        next,
        selected_path.clone(),
        has_selection,
      ));
    }
  })
  .into()
}

fn pick_toggle(
  pick_enabled: bool,
  pick_enabled_signal: Signal<bool>,
  on_pick_inspected: Option<DevToolsBoolCallback>,
) -> Element {
  let color = if pick_enabled { PRIMARY } else { MUTED };
  toolbar_button(
    "search",
    "Pick",
    color,
    if pick_enabled { SELECTED } else { SURFACE_2 },
    BORDER,
  )
  .on_click(move |_| {
    let next = !pick_enabled;
    pick_enabled_signal.set(next);
    if let Some(on_pick_inspected) = &on_pick_inspected {
      on_pick_inspected(next);
    }
  })
  .into()
}

fn tab(
  icon_name: &str,
  label: &str,
  active: bool,
  target_tab: DevToolsTab,
  active_tab_signal: Signal<DevToolsTab>,
) -> Element {
  let color = if active { TEXT } else { MUTED };
  Column::new()
    .align_items(Alignment::Center)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(6.0)
        .child(icon(icon_name, 14.0, color))
        .child(text(label, 13.0, FontWeight::Medium, color))
        .height(36.0),
    )
    .child(Rect::new(64.0, 2.0).background(if active { PRIMARY } else { "#00000000" }))
    .padding_horizontal(16.0)
    .padding_vertical(0.0)
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| active_tab_signal.set(target_tab))
    .into()
}
