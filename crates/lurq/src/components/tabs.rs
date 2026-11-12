//! Tabs: several places of an app side by side, each with a history of its own, one of them shown.
//!
//! [`Tabs`] is the set: the app makes it, puts a router in it for each tab, and moves between them. A tab
//! that is not shown stays as it was, with its screens, their state and their scroll: it is kept out of
//! layout, painting and input until it is shown again.
//!
//! [`TabOutlet`] shows the active tab, and with a [`TabBar`] the bar to choose one with: across the bottom, as a
//! phone's apps have it, or across the top. An app with a strip of tabs of its own draws that from the set and
//! mounts the outlet with no bar.

use std::{
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
  time::Duration,
};

use parking_lot::Mutex;

use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::{BottomBarInset, Ctx},
    window::SafeAreaInsets,
  },
  components::{Column, Router, Row, Stack, Text},
  core::{ElementRef, Signal},
  layout::{
    Alignment, StackAlignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
  router::RouterHandle,
};

const FILL: Dimension = Dimension::Pct(100.0);

/// What a tab's place in the bar shows.
#[derive(Clone, Default)]
pub struct TabItem {
  pub title: String,
  /// A picture over the title, and the one in its place while the tab is the active one.
  pub icon: Option<Element>,
  pub selected_icon: Option<Element>,
  /// The name of one of the platform's symbols, drawn in the bar's colors where the platform has them (iOS,
  /// with the `raster` feature); elsewhere nothing is drawn for it.
  pub symbol: Option<String>,
  pub selected_symbol: Option<String>,
  /// A few characters in a mark at the picture's corner: a count of what waits there.
  pub badge: Option<String>,
}

impl TabItem {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      ..Self::default()
    }
  }

  pub fn icon(mut self, icon: impl Into<Element>) -> Self {
    self.icon = Some(icon.into());
    self
  }

  pub fn selected_icon(mut self, icon: impl Into<Element>) -> Self {
    self.selected_icon = Some(icon.into());
    self
  }

  pub fn symbol(mut self, name: impl Into<String>) -> Self {
    self.symbol = Some(name.into());
    self
  }

  pub fn selected_symbol(mut self, name: impl Into<String>) -> Self {
    self.selected_symbol = Some(name.into());
    self
  }

  pub fn badge(mut self, badge: impl Into<String>) -> Self {
    self.badge = Some(badge.into());
    self
  }
}

struct Tab {
  key: String,
  item: TabItem,
  router: RouterHandle,
  /// How far the tab's own screens have the bar out of view.
  bar_move: Signal<BarMove>,
}

/// How far out of view a tab's screens have the bar, from 0 to 1, and how it gets there: a stack of screens
/// says so as one that hides the bar comes in or goes out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BarMove {
  pub(crate) hidden: f32,
  pub(crate) pace: Duration,
  pub(crate) curve: Easing,
}

impl BarMove {
  pub(crate) const SHOWN: Self = Self {
    hidden: 0.0,
    pace: Duration::ZERO,
    curve: Easing::Linear,
  };
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for BarMove {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// A tab's part of the context: where its screens say how far they have the bar out of view.
#[derive(Clone)]
pub(crate) struct TabBarLink(pub(crate) Signal<BarMove>);

struct TabsInner {
  tabs: Mutex<Vec<Tab>>,
  active: Signal<String>,
  /// Bumped when the set or an item of it changes.
  changed: Signal<u64>,
  bar_hidden: Signal<bool>,
  /// Bumped when the active tab is chosen again at its first screen: its page goes to its top.
  to_top: Signal<u64>,
}

/// A set of tabs, each a router with a history of its own, and which of them is shown. Cloning it gives
/// another handle to the same set. Inside a [`TabOutlet`] a component gets the set it is in with
/// `ctx.use_context::<Tabs>()`.
#[derive(Clone)]
pub struct Tabs {
  inner: Arc<TabsInner>,
}

impl Default for Tabs {
  fn default() -> Self {
    Self::new()
  }
}

impl PartialEq for Tabs {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for Tabs {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// A tab as [`Tabs::list`] tells it.
#[derive(Clone)]
pub struct TabEntry {
  pub key: String,
  pub item: TabItem,
  pub router: RouterHandle,
  pub active: bool,
  bar_move: Signal<BarMove>,
}

impl Tabs {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(TabsInner {
        tabs: Mutex::new(Vec::new()),
        active: Signal::new(String::new()),
        changed: Signal::new(0),
        bar_hidden: Signal::new(false),
        to_top: Signal::new(0),
      }),
    }
  }

  fn bump(&self) {
    self.inner.changed.update(|version| *version += 1);
  }

  /// Adds a tab after the others. `key` tells it from the others; a tab with that key is replaced where it
  /// is. The first tab added is the active one.
  pub fn add(&self, key: impl Into<String>, item: TabItem, router: RouterHandle) -> &Self {
    let at = self.inner.tabs.lock().len();
    self.insert(at, key, item, router)
  }

  /// Adds a tab before the one at `index`.
  pub fn insert(&self, index: usize, key: impl Into<String>, item: TabItem, router: RouterHandle) -> &Self {
    let key = key.into();
    let first = {
      let mut tabs = self.inner.tabs.lock();
      let mut tab = Tab {
        key: key.clone(),
        item,
        router,
        bar_move: Signal::new(BarMove::SHOWN),
      };
      match tabs.iter().position(|tab| tab.key == key) {
        Some(at) => {
          tab.bar_move = tabs[at].bar_move.clone();
          tabs[at] = tab;
        }
        None => {
          let index = index.min(tabs.len());
          tabs.insert(index, tab);
        }
      }
      tabs.len() == 1
    };
    if first {
      self.inner.active.set(key);
    }
    self.bump();
    self
  }

  /// Takes a tab out, with its history and its screens. If it was the active one the tab after it is, or the
  /// one before when it was the last.
  pub fn remove(&self, key: &str) {
    let next = {
      let mut tabs = self.inner.tabs.lock();
      let Some(at) = tabs.iter().position(|tab| tab.key == key) else {
        return;
      };
      tabs.remove(at);
      (self.inner.active.get_untracked() == key).then(|| {
        tabs
          .get(at)
          .or(tabs.last())
          .map(|tab| tab.key.clone())
          .unwrap_or_default()
      })
    };
    if let Some(next) = next {
      self.inner.active.set(next);
    }
    self.bump();
  }

  /// Shows the tab with the key. Does nothing when there is none.
  pub fn select(&self, key: &str) {
    let known = self.inner.tabs.lock().iter().any(|tab| tab.key == key);
    if known && self.inner.active.get_untracked() != key {
      self.inner.active.set(key.to_owned());
    }
  }

  /// What a press on a tab's place in a bar does. Another tab than the active one is shown. The active one
  /// goes back to its first screen, and once there its page scrolls to its top.
  pub fn press(&self, key: &str) {
    if self.inner.active.get_untracked() != key {
      self.select(key);
      return;
    }
    let Some(router) = self.router(key) else {
      return;
    };
    let above = router.stack().len().saturating_sub(1);
    if above > 0 {
      router.back_by(above);
    } else {
      self.inner.to_top.update(|version| *version += 1);
    }
  }

  /// The key of the tab that is shown; a render that reads it is run again when another is.
  pub fn active(&self) -> String {
    self.inner.active.get()
  }

  pub fn active_signal(&self) -> Signal<String> {
    self.inner.active.clone()
  }

  pub fn router(&self, key: &str) -> Option<RouterHandle> {
    self
      .inner
      .tabs
      .lock()
      .iter()
      .find(|tab| tab.key == key)
      .map(|tab| tab.router.clone())
  }

  /// The router of the tab that is shown: where "back" and a link opened from outside the tabs go.
  pub fn active_router(&self) -> Option<RouterHandle> {
    self.router(&self.inner.active.get_untracked())
  }

  /// The tabs in their order; a render that reads them is run again when the set, an item or the active tab
  /// changes.
  pub fn list(&self) -> Vec<TabEntry> {
    let _ = self.inner.changed.get();
    let active = self.inner.active.get();
    self
      .inner
      .tabs
      .lock()
      .iter()
      .map(|tab| TabEntry {
        key: tab.key.clone(),
        item: tab.item.clone(),
        router: tab.router.clone(),
        active: tab.key == active,
        bar_move: tab.bar_move.clone(),
      })
      .collect()
  }

  pub fn len(&self) -> usize {
    self.inner.tabs.lock().len()
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Changes what a tab's place in the bar shows.
  pub fn set_item(&self, key: &str, item: TabItem) {
    if let Some(tab) = self.inner.tabs.lock().iter_mut().find(|tab| tab.key == key) {
      tab.item = item;
    }
    self.bump();
  }

  /// Changes a tab's badge alone; `None` takes it away.
  pub fn set_badge(&self, key: &str, badge: Option<String>) {
    if let Some(tab) = self.inner.tabs.lock().iter_mut().find(|tab| tab.key == key) {
      tab.item.badge = badge;
    }
    self.bump();
  }

  /// Takes the outlet's bar out of view, or brings it back: for a screen that has the whole window.
  pub fn set_bar_hidden(&self, hidden: bool) {
    if self.inner.bar_hidden.get_untracked() != hidden {
      self.inner.bar_hidden.set(hidden);
    }
  }
}

/// Where a [`TabBar`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabBarPlace {
  /// Across the bottom, over the home indicator's room.
  #[default]
  Bottom,
  /// Across the top, under the status bar's room.
  Top,
}

/// How a [`TabOutlet`]'s bar looks. The sizes it starts with are the platform's for a phone.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBar {
  pub place: TabBarPlace,
  pub background: Color,
  /// The line between the bar and the tab.
  pub border: Color,
  /// The active tab's picture and title, and the others'.
  pub tint: Color,
  pub color: Color,
  /// How much what is behind the bar is blurred where the bar's color lets it show: the content of a tab
  /// that scrolls under a bar across the bottom. 0 is no blur.
  pub blur: f32,
  /// How high the bar is, without the room of the home indicator or the status bar it also covers; and in a
  /// window lower than `low_window`, a phone on its side, where a picture and its title stand side by side.
  pub height: f32,
  pub low_height: f32,
  pub low_window: f32,
  pub low_title_size: f32,
  /// How long the bar takes to go out of view and to come back, for the keyboard or when it is told to.
  pub hide_duration: Duration,
  pub title_size: f32,
  pub title_weight: FontWeight,
  /// The point size a tab's symbol is drawn at.
  pub symbol_size: f32,
  /// The room between a picture and the title under it.
  pub spacing: f32,
  pub badge_background: Color,
  pub badge_color: Color,
  pub badge_size: f32,
  /// The widest a tab's place is; in a wider bar the places stand together in the middle.
  pub item_max_width: f32,
  /// The bar goes out of view while the on-screen keyboard is up.
  pub hides_with_keyboard: bool,
}

impl Default for TabBar {
  fn default() -> Self {
    Self {
      place: TabBarPlace::Bottom,
      background: Color::new(249, 249, 249, 224),
      border: Color::new(60, 60, 67, 74),
      tint: Color::from_hex("#007AFF"),
      color: Color::from_hex("#999999"),
      blur: 40.0,
      height: 49.0,
      low_height: 32.0,
      low_window: 480.0,
      low_title_size: 12.0,
      hide_duration: Duration::from_millis(250),
      title_size: 10.0,
      title_weight: FontWeight::Medium,
      symbol_size: 18.0,
      spacing: 2.0,
      badge_background: Color::from_hex("#FF3B30"),
      badge_color: Color::from_hex("#FFFFFF"),
      badge_size: 13.0,
      item_max_width: f32::INFINITY,
      hides_with_keyboard: true,
    }
  }
}

impl TabBar {
  pub fn new() -> Self {
    Self::default()
  }

  /// The platform's dark colors.
  pub fn dark() -> Self {
    Self {
      background: Color::new(22, 22, 22, 204),
      border: Color::new(84, 84, 88, 153),
      tint: Color::from_hex("#0A84FF"),
      color: Color::from_hex("#8E8E93"),
      ..Self::default()
    }
  }

  pub fn place(mut self, place: TabBarPlace) -> Self {
    self.place = place;
    self
  }

  pub fn background(mut self, color: Color) -> Self {
    self.background = color;
    self
  }

  pub fn border(mut self, color: Color) -> Self {
    self.border = color;
    self
  }

  pub fn blur(mut self, radius: f32) -> Self {
    self.blur = radius;
    self
  }

  pub fn tint(mut self, color: Color) -> Self {
    self.tint = color;
    self
  }

  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }
}

/// What a bar of the app's own is told ([`TabOutletProps::bar_with`]).
pub struct TabBarState {
  pub tabs: Tabs,
  /// The tabs in their order, the active one marked.
  pub entries: Vec<TabEntry>,
  /// The window's, for the bar to keep its content clear of the home indicator.
  pub safe_area: SafeAreaInsets,
  /// The window is too low for a bar of full height: a phone on its side.
  pub low: bool,
}

/// Makes a bar of the app's own from the state of the tabs.
pub type TabBarBuilder = Arc<dyn Fn(&TabBarState) -> Element + Send + Sync>;

#[derive(Clone)]
enum Bar {
  Standard(TabBar),
  Custom(TabBarBuilder, TabBarPlace),
}

#[derive(Clone)]
pub struct TabOutletProps {
  tabs: Tabs,
  bar: Option<Bar>,
}

impl PartialEq for TabOutletProps {
  fn eq(&self, other: &Self) -> bool {
    let same_bar = match (&self.bar, &other.bar) {
      (None, None) => true,
      (Some(Bar::Standard(a)), Some(Bar::Standard(b))) => a == b,
      (Some(Bar::Custom(a, at)), Some(Bar::Custom(b, bt))) => Arc::ptr_eq(a, b) && at == bt,
      _ => false,
    };
    self.tabs == other.tabs && same_bar
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for TabOutletProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl TabOutletProps {
  /// The active tab of `tabs`, with no bar.
  pub fn new(tabs: Tabs) -> Self {
    Self { tabs, bar: None }
  }

  /// Draws a bar to choose a tab with.
  pub fn bar(mut self, bar: TabBar) -> Self {
    self.bar = Some(Bar::Standard(bar));
    self
  }

  /// Draws a bar of the app's own: what `builder` returns for the state of the tabs, across the outlet at
  /// `place`, at the height it takes. A press on a tab's place calls [`Tabs::press`].
  pub fn bar_with(
    mut self,
    place: TabBarPlace,
    builder: impl Fn(&TabBarState) -> Element + Send + Sync + 'static,
  ) -> Self {
    self.bar = Some(Bar::Custom(Arc::new(builder), place));
    self
  }
}

/// Shows the active tab of a [`Tabs`], and keeps the others as they were.
pub struct TabOutlet {
  /// The last request to scroll to the top that was passed on.
  to_top: AtomicU64,
  /// The bar as it is laid out, and how high that was: the tabs' content keeps clear of as much.
  bar: ElementRef,
  bar_height: Signal<f32>,
}

impl TabOutlet {
  pub fn mount(ctx: &mut Ctx, props: TabOutletProps) -> Element {
    ctx.mount::<Self>(props)
  }
}

impl Component for TabOutlet {
  type Props = TabOutletProps;

  fn create(ctx: &mut Ctx) -> Self {
    let tabs = ctx.props::<TabOutletProps>().tabs.clone();
    let to_top = AtomicU64::new(tabs.inner.to_top.get_untracked());
    ctx.provide(tabs);
    Self {
      to_top,
      bar: ElementRef::default(),
      bar_height: Signal::new(0.0),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<TabOutletProps>().clone();
    let tabs = props.tabs.clone();
    let entries = tabs.list();
    let hidden = tabs.inner.bar_hidden.get();
    // The active tab chosen again at its first screen: the page goes to its top.
    let to_top = tabs.inner.to_top.get();
    if self.to_top.swap(to_top, Ordering::AcqRel) != to_top {
      ctx.scroll_to_top();
    }
    let window = ctx.window();
    let safe_area = window.safe_area;
    let keyboard_up = safe_area.keyboard > 0.0;
    let low = match &props.bar {
      Some(Bar::Standard(bar)) => window.logical_height() < bar.low_window,
      _ => false,
    };

    let (place, away, hide_duration) = match &props.bar {
      None => (TabBarPlace::Bottom, true, Duration::ZERO),
      Some(Bar::Standard(bar)) => (
        bar.place,
        hidden || (bar.hides_with_keyboard && keyboard_up),
        bar.hide_duration,
      ),
      Some(Bar::Custom(_, place)) => (*place, hidden || keyboard_up, TabBar::default().hide_duration),
    };
    let over = props.bar.is_some() && place == TabBarPlace::Bottom;
    // A bar across the bottom is over the tabs' content, which shows through it: the content keeps clear of
    // the bar's height more than of the window's bottom, while the bar is there.
    let bar_height = self.bar_height.get();
    ctx.provide_if_changed(BottomBarInset(if over && !away {
      (bar_height - safe_area.bottom).max(0.0)
    } else {
      0.0
    }));

    let mut screens = Stack::new().size(FILL, FILL);
    let mut moved = BarMove::SHOWN;
    for entry in &entries {
      let host = ctx.mount_keyed_offstage::<TabHost>(
        &entry.key,
        TabHostProps {
          router: entry.router.clone(),
          bar_move: entry.bar_move.clone(),
        },
        entry.active,
      );
      if entry.active {
        moved = entry.bar_move.get();
      }
      screens = screens.child(
        Stack::new()
          .key(format!("tab-{}", entry.key))
          .size(FILL, FILL)
          .child(host),
      );
    }

    let bar: Option<Element> = match &props.bar {
      None => None,
      _ if away && !over => None,
      Some(Bar::Custom(builder, _)) => Some(builder(&TabBarState {
        tabs: tabs.clone(),
        entries: entries.clone(),
        safe_area,
        low,
      })),
      Some(Bar::Standard(bar)) => Some(standard_bar(bar, &tabs, &entries, safe_area, low)),
    };
    let whole = Stack::new().size(FILL, FILL);
    match (bar, place) {
      (None, _) => whole.child(screens),
      (Some(bar), TabBarPlace::Top) => whole.child(
        Column::new()
          .size(FILL, FILL)
          .child(bar)
          .child(Column::new().width(FILL).flex(1.0).child(screens)),
      ),
      (Some(bar), TabBarPlace::Bottom) => {
        // Out of view the bar is under the window's bottom edge: all the way there for the keyboard or when
        // told to, with a move of its own, and as far as a screen that hides it has come in, with that
        // screen's move.
        let (share, pace, curve) = if away {
          (1.0, hide_duration, Easing::EASE)
        } else if moved.pace.is_zero() && moved.hidden == 0.0 {
          (0.0, hide_duration, Easing::EASE)
        } else {
          (moved.hidden, moved.pace, moved.curve)
        };
        let bar = Row::new()
          .ref_element(self.bar.clone())
          .width(FILL)
          .child(bar)
          .transform(Transform2D::translate(0.0, share * bar_height))
          .transition(Transition::transform().duration(pace).easing(curve));
        whole
          .stack_align(StackAlignment::BottomStart)
          .child(screens)
          .child(if share >= 1.0 { bar.pointer_events_none() } else { bar })
      }
    }
  }

  fn after_layout(&self) {
    if self.bar.is_attached() {
      let height = self.bar.rect().3;
      if (self.bar_height.get_untracked() - height).abs() > 0.25 {
        self.bar_height.set(height);
      }
    }
  }
}

/// A tab's symbol in a color, where the platform draws symbols.
#[allow(unused_variables)]
fn symbol(name: &str, size: f32, color: Color) -> Option<Element> {
  #[cfg(all(target_os = "ios", feature = "raster"))]
  if let Some(symbol) = crate::app::ios_symbol::symbol(name, size, color) {
    return Some(
      crate::components::Image::new(symbol.image)
        .width(symbol.width)
        .height(symbol.height)
        .into(),
    );
  }
  None
}

fn standard_bar(bar: &TabBar, tabs: &Tabs, entries: &[TabEntry], safe_area: SafeAreaInsets, low: bool) -> Element {
  let mut places = Row::new()
    .width(FILL)
    .height(if low { bar.low_height } else { bar.height })
    .justify(Justify::Center)
    .padding_left(safe_area.left)
    .padding_right(safe_area.right);
  for entry in entries {
    let color = if entry.active { bar.tint } else { bar.color };
    let item = &entry.item;
    let picture = match (entry.active, &item.selected_icon, &item.selected_symbol) {
      (true, Some(icon), _) => Some(icon.clone()),
      (true, None, Some(name)) => symbol(name, bar.symbol_size, color),
      // The filled one of the tab's symbol, where the platform has it, as its own bars take.
      (true, None, None) => item
        .symbol
        .as_ref()
        .and_then(|name| symbol(&format!("{name}.fill"), bar.symbol_size, color)),
      _ => None,
    }
    .or_else(|| item.icon.clone())
    .or_else(|| {
      item
        .symbol
        .as_ref()
        .and_then(|name| symbol(name, bar.symbol_size, color))
    });

    let mut content = Column::new().align_items(Alignment::Center).spacing(bar.spacing);
    let badge = item.badge.as_ref().map(|badge| {
      Row::new()
        .min_width(bar.badge_size + 5.0)
        .height(bar.badge_size + 5.0)
        .padding_horizontal(5.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded((bar.badge_size + 5.0) / 2.0)
        .background(bar.badge_background)
        .child(
          Text::styled(
            badge,
            TextStyle {
              font_size: bar.badge_size,
              color: bar.badge_color,
              ..TextStyle::default()
            },
          )
          .nowrap(),
        )
    });
    let title = Text::styled(
      &item.title,
      TextStyle {
        font_size: if low { bar.low_title_size } else { bar.title_size },
        weight: bar.title_weight,
        color,
        ..TextStyle::default()
      },
    )
    .nowrap();
    match (picture, badge) {
      (Some(picture), badge) => {
        // The badge at the picture's upper corner, half over it.
        let mut over = Stack::new().stack_align(StackAlignment::TopEnd).child(picture);
        if let Some(badge) = badge {
          over = over.child(badge.transform(Transform2D::translate(bar.badge_size * 0.7, -bar.badge_size * 0.3)));
        }
        if low {
          // In a low bar the title stands beside the picture.
          let mut row = Row::new().align_items(Alignment::Center).spacing(6.0).child(over);
          if !item.title.is_empty() {
            row = row.child(title);
          }
          content = content.child(row);
        } else {
          content = content.child(over);
          if !item.title.is_empty() {
            content = content.child(title);
          }
        }
      }
      (None, badge) => {
        let mut row = Row::new().align_items(Alignment::Center).spacing(4.0).child(title);
        if let Some(badge) = badge {
          row = row.child(badge);
        }
        content = content.child(row);
      }
    }
    let (tabs, key) = (tabs.clone(), entry.key.clone());
    places = places.child(
      Row::new()
        .id(format!("tab-{}", entry.key))
        .flex(1.0)
        .max_width(bar.item_max_width)
        .height(FILL)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(content)
        .on_click(move |_| tabs.press(&key)),
    );
  }
  let line = Row::new().width(FILL).height(0.5).background(bar.border);
  let column = Column::new()
    .width(FILL)
    .background(bar.background)
    .backdrop_blur(bar.blur);
  match bar.place {
    TabBarPlace::Bottom => column.child(line).child(places).padding_bottom(safe_area.bottom),
    TabBarPlace::Top => column.padding_top(safe_area.top).child(places).child(line),
  }
  .into()
}

#[derive(Clone)]
struct TabHostProps {
  router: RouterHandle,
  bar_move: Signal<BarMove>,
}

impl PartialEq for TabHostProps {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.router.inner, &other.router.inner)
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for TabHostProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// One tab: its router, kept while the tab is not shown.
struct TabHost;

impl Component for TabHost {
  type Props = TabHostProps;

  fn create(ctx: &mut Ctx) -> Self {
    let bar_move = ctx.props::<TabHostProps>().bar_move.clone();
    ctx.provide(TabBarLink(bar_move));
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let router = ctx.props::<TabHostProps>().router.clone();
    Router::mount(ctx, router)
  }
}
