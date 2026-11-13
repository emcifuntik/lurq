use lurq::{
  app::ctx::Ctx,
  layout::{Alignment, text_style::FontWeight},
  node::{CursorIcon, Element},
};

use crate::style::{DemoTheme, text};

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub(crate) enum DemoTab {
  AtlasUpload,
  DynamicKeyframes,
  DynamicImages,
  Layout,
  Sizing,
  Position,
  Dnd,
  Animation,
  Transform,
  Scroll,
  Visual,
  Text,
  Markdown,
  PersistentStorage,
  Inputs,
  Events,
  Reactivity,
  Components,
  Context,
  Borders,
  Grid,
  Sticky,
  Presence,
  Style,
}

impl DemoTab {
  pub(crate) fn label(self) -> &'static str {
    match self {
      Self::AtlasUpload => "Atlas",
      Self::DynamicKeyframes => "Keyframes",
      Self::DynamicImages => "GIF/WebP",
      Self::Layout => "Layout",
      Self::Sizing => "Sizing",
      Self::Position => "Position",
      Self::Dnd => "DnD",
      Self::Animation => "Animation",
      Self::Transform => "Transform",
      Self::Scroll => "Scroll",
      Self::Visual => "Visual",
      Self::Text => "Text",
      Self::Markdown => "Markdown",
      Self::PersistentStorage => "Storage",
      Self::Inputs => "Inputs",
      Self::Events => "Events",
      Self::Reactivity => "React.",
      Self::Components => "Comps.",
      Self::Context => "Context",
      Self::Borders => "Borders",
      Self::Grid => "Grid",
      Self::Sticky => "Sticky",
      Self::Presence => "Presence",
      Self::Style => "Style",
    }
  }

  pub(crate) fn path(self) -> &'static str {
    match self {
      Self::AtlasUpload => "/atlas-upload",
      Self::DynamicKeyframes => "/dynamic-keyframes",
      Self::DynamicImages => "/dynamic-images",
      Self::Layout => "/",
      Self::Sizing => "/sizing",
      Self::Position => "/position",
      Self::Dnd => "/dnd",
      Self::Animation => "/animation",
      Self::Transform => "/transform",
      Self::Scroll => "/scroll",
      Self::Visual => "/visual",
      Self::Text => "/text",
      Self::Markdown => "/markdown",
      Self::PersistentStorage => "/persistent-storage",
      Self::Inputs => "/inputs",
      Self::Events => "/events",
      Self::Reactivity => "/reactivity",
      Self::Components => "/components",
      Self::Context => "/context",
      Self::Borders => "/borders",
      Self::Grid => "/grid",
      Self::Sticky => "/sticky",
      Self::Presence => "/presence",
      Self::Style => "/style",
    }
  }

  pub(crate) fn from_path(path: &str) -> Self {
    match path {
      "/atlas-upload" => Self::AtlasUpload,
      "/dynamic-keyframes" => Self::DynamicKeyframes,
      "/dynamic-images" => Self::DynamicImages,
      "/sizing" => Self::Sizing,
      "/position" => Self::Position,
      "/dnd" => Self::Dnd,
      "/animation" => Self::Animation,
      "/transform" => Self::Transform,
      "/scroll" => Self::Scroll,
      "/visual" => Self::Visual,
      "/text" => Self::Text,
      "/markdown" => Self::Markdown,
      "/persistent-storage" => Self::PersistentStorage,
      "/inputs" => Self::Inputs,
      "/events" => Self::Events,
      "/reactivity" => Self::Reactivity,
      "/components" => Self::Components,
      "/context" => Self::Context,
      "/borders" => Self::Borders,
      "/grid" => Self::Grid,
      "/sticky" => Self::Sticky,
      "/presence" => Self::Presence,
      "/style" => Self::Style,
      _ => Self::Layout,
    }
  }
}

pub(crate) fn sidebar(ctx: &mut Ctx, selected: DemoTab, theme: DemoTheme) -> Element {
  let palette = theme.palette();
  let items = [
    DemoTab::AtlasUpload,
    DemoTab::DynamicImages,
    DemoTab::DynamicKeyframes,
    DemoTab::Layout,
    DemoTab::Sizing,
    DemoTab::Position,
    DemoTab::Dnd,
    DemoTab::Animation,
    DemoTab::Transform,
    DemoTab::Scroll,
    DemoTab::Visual,
    DemoTab::Text,
    DemoTab::Markdown,
    DemoTab::PersistentStorage,
    DemoTab::Inputs,
    DemoTab::Events,
    DemoTab::Reactivity,
    DemoTab::Components,
    DemoTab::Context,
    DemoTab::Borders,
    DemoTab::Grid,
    DemoTab::Sticky,
    DemoTab::Presence,
    DemoTab::Style,
  ];

  let mut children = Vec::new();
  for tab in items {
    children.push(sidebar_item(ctx, tab, tab == selected, theme));
  }

  lurq::components::Column::new()
    .child(
      lurq::components::Column::new()
        .spacing(2.0)
        .child(text("lurq engine demo", 12.0, FontWeight::Bold, palette.text))
        .child(text(selected.label(), 10.0, FontWeight::Medium, palette.text_muted))
        .padding_horizontal(16.0)
        .padding_vertical(10.0)
        .width(200.0)
        .height(56.0),
    )
    .with_children(children)
    .width(200.0)
    .background(palette.surface_dark)
    .into()
}

fn sidebar_item(ctx: &mut Ctx, tab: DemoTab, selected: bool, theme: DemoTheme) -> Element {
  let palette = theme.palette();
  let item = lurq::components::Row::new()
    .align_items(Alignment::Center)
    .child(lurq::components::Rect::new(3.0, 38.0).background(if selected { palette.primary } else { "#00000000" }))
    .child(lurq::components::Spacer::new().width(13.0))
    .child(text(
      tab.label(),
      11.0,
      if selected { FontWeight::Bold } else { FontWeight::Medium },
      if selected { palette.text } else { palette.text_muted },
    ))
    .width(200.0)
    .height(38.0)
    .cursor(CursorIcon::Pointer)
    .background(if selected { palette.nav_selected } else { "#00000000" });

  lurq::components::Link::build_empty(ctx, tab.path()).child(item).into()
}
