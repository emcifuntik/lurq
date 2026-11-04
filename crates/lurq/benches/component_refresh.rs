//! What a component refresh costs on a screen of nested components: a root
//! with 5 sections, each with 5 groups of 9 cards (256 component instances
//! over about 3,000 nodes), as a heavy app screen holds them.
//!
//! - `leaf_change`: one card's own signal changes; only it renders again.
//! - `mid_level`: one section's own signal changes; it renders again and its groups and cards, whose props did not
//!   change, do not.
//! - `leaf_change_layout`, `mid_level_layout`: the same, each followed by the layout of the refreshed tree
//!   (`Tree::bench_layout_only_shared`), which lays out the changed nodes and serves the rest from the caches the
//!   refresh kept (pinned by `app::runtime::refresh_layout_tests`).
//! - `first_mount`: the whole screen is created and rendered.
//!
//! A refresh runs through `Tree::get_element_by_id`, which settles dirty
//! components first; the id it looks up is on the root, so the lookup itself
//! stops at the first node.

use std::cell::RefCell;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::Alignment,
  node::Element,
};

const SECTIONS: usize = 5;
const GROUPS: usize = 5;
const CARDS: usize = 9;

thread_local! {
  static SECTION_SIGNALS: RefCell<Vec<Signal<u32>>> = const { RefCell::new(Vec::new()) };
  static CARD_SIGNALS: RefCell<Vec<Signal<u32>>> = const { RefCell::new(Vec::new()) };
}

struct Root;

impl Component for Root {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new().id("bench-root");
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
    let revision = ctx.signal(0);
    SECTION_SIGNALS.with(|signals| signals.borrow_mut().push(revision.clone()));
    Self { revision }
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
    let revision = ctx.signal(0);
    CARD_SIGNALS.with(|signals| signals.borrow_mut().push(revision.clone()));
    Self { revision }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (section, group, card) = *ctx.props::<(usize, usize, usize)>();
    let revision = self.revision.get();
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
      .child(Text::new(&format!("Body of the card, revision {revision}")))
      .child(
        Row::new()
          .spacing(4.0)
          .child(Rect::new(40.0, 16.0).background("#22c55e").rounded(4.0))
          .child(Rect::new(40.0, 16.0).background("#ef4444").rounded(4.0))
          .child(Text::new("actions")),
      )
      .child(
        Column::new()
          .child(Text::new("first line"))
          .child(Text::new("second line")),
      )
  }
}

fn mounted() -> (Tree, App) {
  SECTION_SIGNALS.with(|signals| signals.borrow_mut().clear());
  CARD_SIGNALS.with(|signals| signals.borrow_mut().clear());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(1600, 1200);
  tree.mount_root::<Root>(&mut app, ());
  tree.pass_headless(&mut app);
  (tree, app)
}

fn refresh(tree: &mut Tree) {
  assert!(tree.get_element_by_id("bench-root").is_some());
}

/// The refresh, then the layout the next pass runs over the refreshed tree.
fn refresh_and_layout(tree: &mut Tree, app: &mut App) {
  refresh(tree);
  assert!(tree.bench_layout_only_shared(app, false).is_some());
}

fn leaf_signal() -> Signal<u32> {
  CARD_SIGNALS.with(|signals| signals.borrow()[CARDS * GROUPS * 2 + CARDS * 2 + 4].clone())
}

fn mid_level_signal() -> Signal<u32> {
  SECTION_SIGNALS.with(|signals| signals.borrow()[2].clone())
}

fn bench_component_refresh(c: &mut Criterion) {
  let mut group = c.benchmark_group("component_refresh");

  let (mut tree, _app) = mounted();
  let card = leaf_signal();
  group.bench_function("leaf_change", |b| {
    b.iter(|| {
      card.update(|value| *value += 1);
      refresh(&mut tree);
    });
  });
  drop(tree);

  let (mut tree, _app) = mounted();
  let section = mid_level_signal();
  group.bench_function("mid_level", |b| {
    b.iter(|| {
      section.update(|value| *value += 1);
      refresh(&mut tree);
    });
  });
  drop(tree);

  // The same refreshes with the layout after each, which serves what the
  // refresh did not change from the caches it kept.
  let (mut tree, mut app) = mounted();
  let card = leaf_signal();
  group.bench_function("leaf_change_layout", |b| {
    b.iter(|| {
      card.update(|value| *value += 1);
      refresh_and_layout(&mut tree, &mut app);
    });
  });
  drop(tree);

  let (mut tree, mut app) = mounted();
  let section = mid_level_signal();
  group.bench_function("mid_level_layout", |b| {
    b.iter(|| {
      section.update(|value| *value += 1);
      refresh_and_layout(&mut tree, &mut app);
    });
  });
  drop(tree);

  group.bench_function("first_mount", |b| {
    b.iter_batched(
      || {
        SECTION_SIGNALS.with(|signals| signals.borrow_mut().clear());
        CARD_SIGNALS.with(|signals| signals.borrow_mut().clear());
        (Tree::new(), App::new())
      },
      |(mut tree, mut app)| {
        tree.mount_root::<Root>(&mut app, ());
        (tree, app)
      },
      BatchSize::LargeInput,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_component_refresh);
criterion_main!(benches);
