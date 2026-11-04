//! What the layout after a component refresh lays out again. A refreshed
//! component's fresh output takes the layout caches of the nodes it replaces,
//! and an unchanged component put together from its slot's cache matches the
//! live subtree, so the layout lays out only what changed and serves the rest
//! from the caches. A change that loses the caches on the way (LUR-17 moved
//! the fresh output into the live tree and keeps each slot's own output only)
//! lays out whole subtrees again and fails here.
//!
//! The counts are those of `layout_dirty::LAYOUTS` (nodes laid out from
//! scratch) in each pass until the tree settles.

use std::cell::RefCell;

use crate::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::Alignment,
  node::{Element, dimension::Dimension},
};

thread_local! {
  static SIGNALS: RefCell<Vec<Signal<u32>>> = const { RefCell::new(Vec::new()) };
}

/// A signal the component that creates it renders, for the test to change.
fn revision(ctx: &mut Ctx) -> Signal<u32> {
  let revision = ctx.signal(0);
  SIGNALS.with(|signals| signals.borrow_mut().push(revision.clone()));
  revision
}

/// The `index`th signal [`revision`] made since [`mounted`].
fn signal(index: usize) -> Signal<u32> {
  SIGNALS.with(|signals| signals.borrow()[index].clone())
}

fn mounted<C: Component<Props = ()>>(width: u32, height: u32) -> (Tree, App) {
  SIGNALS.with(|signals| signals.borrow_mut().clear());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(width, height);
  tree.mount_root::<C>(&mut app, ());
  settle(&mut tree, &mut app);
  (tree, app)
}

/// The nodes each pass lays out from scratch, until a pass lays out nothing.
fn settle(tree: &mut Tree, app: &mut App) -> Vec<usize> {
  let mut passes = Vec::new();
  for _ in 0..8 {
    crate::core::layout_dirty::LAYOUTS.with(|layouts| layouts.set(0));
    let report = tree.pass_headless(app);
    let layouts = crate::core::layout_dirty::LAYOUTS.with(|layouts| layouts.get());
    if !report.layout_updated && layouts == 0 {
      return passes;
    }
    passes.push(layouts);
  }
  panic!("the tree did not settle: {passes:?}");
}

fn bump(signal: &Signal<u32>) {
  signal.update(|value| *value += 1);
}

// The screen of `benches/component_refresh.rs`, smaller: a root with
// sections of groups of cards.

const SECTIONS: usize = 3;
const GROUPS: usize = 3;
const CARDS: usize = 4;

struct Screen;

impl Component for Screen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new();
    for section in 0..SECTIONS {
      column = column.child(ctx.mount_keyed::<Section>(&format!("s{section}"), section));
    }
    column
  }
}

struct Section {
  revision: Signal<u32>,
}

impl Component for Section {
  type Props = usize;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      revision: revision(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let section = *ctx.props::<usize>();
    let mut groups = Row::new().spacing(8.0);
    for group in 0..GROUPS {
      groups = groups.child(ctx.mount_keyed::<Group>(&format!("g{group}"), (section, group)));
    }
    Column::new()
      .spacing(4.0)
      .child(Text::new(&format!(
        "Section {section} revision {}",
        self.revision.get()
      )))
      .child(groups)
  }
}

struct Group;

impl Component for Group {
  type Props = (usize, usize);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (section, group) = *ctx.props::<(usize, usize)>();
    let mut cards = Column::new().spacing(2.0);
    for card in 0..CARDS {
      cards = cards.child(ctx.mount_keyed::<Card>(&format!("c{card}"), (section, group, card)));
    }
    Column::new()
      .child(Text::new(&format!("Group {section}.{group}")))
      .child(cards)
  }
}

struct Card {
  revision: Signal<u32>,
}

impl Component for Card {
  type Props = (usize, usize, usize);

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      revision: revision(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (section, group, card) = *ctx.props::<(usize, usize, usize)>();
    Column::new()
      .spacing(2.0)
      .padding(4.0)
      .child(
        Row::new()
          .spacing(4.0)
          .align(Alignment::Center)
          .child(Rect::new(12.0, 12.0).background("#3b82f6").rounded(6.0))
          .child(Text::new(&format!("Card {section}.{group}.{card}"))),
      )
      .child(Text::new(&format!("Revision {}", self.revision.get())))
      .child(
        Row::new()
          .spacing(4.0)
          .child(Rect::new(40.0, 16.0).background("#22c55e").rounded(4.0))
          .child(Text::new("actions")),
      )
  }
}

/// The signal of section `section`. Each section creates its signal, then
/// its groups' cards create theirs, before the next section.
fn section_signal(section: usize) -> Signal<u32> {
  signal(section * (1 + GROUPS * CARDS))
}

/// The signal of a card of section `section`, group `group`.
fn card_signal(section: usize, group: usize, card: usize) -> Signal<u32> {
  signal(section * (1 + GROUPS * CARDS) + 1 + group * CARDS + card)
}

/// One card renders again by itself and its text keeps its size: the layout
/// lays out that text and the card's column, which repairs in place, and
/// serves every other card, group and section from their caches.
#[test]
fn a_leaf_refresh_lays_out_only_what_changed() {
  let (mut tree, mut app) = mounted::<Screen>(1600, 1200);
  let card = card_signal(1, 0, 2);

  for _ in 0..3 {
    bump(&card);
    assert_eq!(settle(&mut tree, &mut app), [2]);
  }
}

/// A section renders again with groups and cards whose props did not change:
/// they are put together from their slots' caches, take the live layout
/// caches, and the layout lays out the section's changed text and its column
/// only.
#[test]
fn a_mid_level_refresh_keeps_the_layouts_of_unchanged_children() {
  let (mut tree, mut app) = mounted::<Screen>(1600, 1200);
  let section = section_signal(1);

  for _ in 0..3 {
    bump(&section);
    assert_eq!(settle(&mut tree, &mut app), [2]);
  }
}

// A chat screen as an app builds one: a sidebar of hosts that are handed
// their anchors as slot children (tooltip hosts), and a virtualized list of
// rows whose body is handed to a host that adds the message actions, reading
// a context the root provides on every render; with `markdown`, the body is a
// Markdown component.

const TURNS: usize = 40;

/// A value the root provides anew on every render, as an app's clipboard or
/// theme handle: the components that read it render again.
#[derive(Clone)]
struct Shared(u32);

struct ChatApp;

impl Component for ChatApp {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.provide(Shared(1));
    let sidebar = ctx.mount_keyed::<Sidebar>("sidebar", ());
    let chat = ctx.mount_keyed::<Chat>("chat", ());
    Row::new()
      .child(sidebar)
      .child(Column::new().flex(1.0).height(Dimension::Pct(100.0)).child(chat))
  }
}

struct Sidebar {
  revision: Signal<u32>,
}

impl Component for Sidebar {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      revision: revision(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new()
      .width(200.0)
      .spacing(4.0)
      .child(Text::new(&format!("Projects {}", self.revision.get())));
    for item in 0..8 {
      let label = ctx.mount_keyed::<Label>(&format!("label-{item}"), format!("Item {item}"));
      column = column.child(ctx.mount_keyed_with::<Host>(&format!("host-{item}"), (), vec![label]));
    }
    column
  }
}

/// A host its anchor is handed to, as a tooltip host is.
struct Host;

impl Component for Host {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .align(Alignment::Center)
      .with_children(ctx.children().iter().cloned())
  }
}

struct Label;

impl Component for Label {
  type Props = String;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let text = ctx.props::<String>().clone();
    let shared = ctx.use_context::<Shared>().map_or(0, |shared| shared.0);
    Row::new()
      .spacing(4.0)
      .child(Rect::new(10.0, 10.0))
      .child(Text::new(&format!("{text} {shared}")))
  }
}

/// A chat that opens at the end of its newest row.
struct Chat {
  scroll: crate::layout::layout_kind::ScrollState,
}

impl Component for Chat {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    let scroll = crate::layout::layout_kind::ScrollState::new();
    scroll.scroll_to_bottom_pending();
    Self { scroll }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let list = crate::components::VirtualizedList::new(ctx, 0..TURNS)
      .list_key("chat")
      .with_scroll_state(self.scroll.clone())
      .bootstrap_rows(4)
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .mount_keyed::<Turn, _, _, _>(|turn| *turn, |turn| *turn);
    Column::new()
      .height(Dimension::Pct(100.0))
      .child(Text::new("Chat"))
      .child(list)
  }
}

struct Turn {
  revision: Signal<u32>,
}

impl Component for Turn {
  type Props = usize;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      revision: revision(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let turn = *ctx.props::<usize>();
    let body = ctx.mount_keyed::<Answer>("answer", turn);
    let message = ctx.mount_keyed_with::<Message>("message", turn, vec![body]);
    Column::new()
      .id(format!("turn-{turn}").as_str())
      .spacing(6.0)
      .padding(4.0)
      .child(Text::new(&format!("Message {turn} revision {}", self.revision.get())))
      .child(message)
  }
}

/// A message with its actions, its body handed to it.
struct Message;

impl Component for Message {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let turn = *ctx.props::<usize>();
    let body = ctx.children().to_vec();
    let mut actions = Row::new().spacing(4.0);
    for action in 0..2 {
      let label = ctx.mount_keyed::<Label>(&format!("action-{action}"), format!("Action {turn}.{action}"));
      actions = actions.child(ctx.mount_keyed_with::<Host>(&format!("host-{action}"), (), vec![label]));
    }
    Column::new()
      .spacing(4.0)
      .child(
        Row::new()
          .child(Text::new("Orchestrator"))
          .child(Row::new().flex(1.0))
          .child(actions),
      )
      .with_children(body)
  }
}

struct Answer;

impl Component for Answer {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  #[cfg(feature = "markdown")]
  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let turn = *ctx.props::<usize>();
    let source = format!(
      "Answer {turn}: a reply long enough to wrap over a few lines, with [a link](https://example.com/{turn}).\n\n\
       - first point\n- second point with `code`\n"
    );
    let props = crate::components::MarkdownProps::new(source)
      .width(Dimension::Pct(100.0))
      .selectable(true);
    crate::components::Markdown::mount(ctx, props).id(format!("answer-{turn}").as_str())
  }

  #[cfg(not(feature = "markdown"))]
  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let turn = *ctx.props::<usize>();
    Text::new(&format!(
      "Answer {turn}: a reply long enough to wrap over a few lines of the chat column."
    ))
    .id(format!("answer-{turn}").as_str())
  }
}

/// A row of the chat renders again by itself, and the sidebar renders again
/// over hosts and labels whose props did not change: each layout after them
/// lays out the changed text and the column around it only, and one node more
/// in a row whose body is Markdown, as on the commit before LUR-17
/// (ec739b57). The layout after the chat opened left every row's subtree
/// cached.
#[test]
fn chat_refreshes_keep_the_layouts_of_what_did_not_change() {
  const ROW_LAYOUTS: usize = if cfg!(feature = "markdown") { 3 } else { 2 };
  let (mut tree, mut app) = mounted::<ChatApp>(1200, 900);
  // The sidebar's signal is first, then the rows', from the first one the
  // list mounted.
  let sidebar = signal(0);
  let row = signal(2);

  for _ in 0..3 {
    bump(&row);
    assert_eq!(settle(&mut tree, &mut app), [ROW_LAYOUTS], "a row refresh");
    bump(&sidebar);
    assert_eq!(settle(&mut tree, &mut app), [2], "a sidebar refresh");
  }
}
