//! A component that renders again keeps the runtime state of the unchanged
//! components inside it: their output is put together from what their slots
//! kept, and the live tree's state is carried over to it.

use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Column, Rect, ScrollVertical, Text, TextInput},
  core::{Signal, SignalValue},
  node::{Element, color::Color},
};

/// Where the components hand their signals and render counts to the test.
#[derive(Clone, Default)]
struct Probe {
  top: Arc<Mutex<Option<Signal<i32>>>>,
  section: Arc<Mutex<Option<Signal<i32>>>>,
  panel: Arc<Mutex<Option<Signal<i32>>>>,
  leaf: Arc<Mutex<Option<Signal<i32>>>>,
  section_renders: Arc<AtomicUsize>,
  panel_renders: Arc<AtomicUsize>,
}

impl PartialEq for Probe {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.panel, &other.panel)
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Probe {}

impl Probe {
  fn signal<T: SignalValue>(slot: &Arc<Mutex<Option<Signal<T>>>>) -> Signal<T> {
    slot.lock().unwrap().clone().expect("the component made its signal")
  }

  fn keep(slot: &Arc<Mutex<Option<Signal<i32>>>>, signal: &Signal<i32>) {
    *slot.lock().unwrap() = Some(signal.clone());
  }
}

/// Renders again on its own signal; mounts a `Section`, which stays clean.
struct Top {
  tick: Signal<i32>,
}

impl Component for Top {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let tick = ctx.signal(1);
    Probe::keep(&ctx.props::<Probe>().top, &tick);
    Self { tick }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Probe>().clone();
    Column::new()
      .child(Text::new(&format!("top-{}", self.tick.get())))
      .child(ctx.mount::<Section>(probe))
  }
}

/// The mid-level component: renders again on its own signal and mounts a
/// `Panel` whose props do not change, and a `Holder`.
struct Section {
  revision: Signal<i32>,
}

impl Component for Section {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let revision = ctx.signal(1);
    Probe::keep(&ctx.props::<Probe>().section, &revision);
    Self { revision }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Probe>().clone();
    probe.section_renders.fetch_add(1, Ordering::Relaxed);
    Column::new()
      .child(Text::new(&format!("section-{}", self.revision.get())))
      .child(ctx.mount::<Panel>(probe.clone()).id("panel"))
      .child(ctx.mount::<Holder>(probe))
  }
}

/// A scroll area and a text input, with a count of its own.
struct Panel {
  count: Signal<i32>,
  value: Signal<String>,
}

impl Component for Panel {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let count = ctx.signal(1);
    Probe::keep(&ctx.props::<Probe>().panel, &count);
    Self {
      count,
      value: ctx.signal("hello".to_owned()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.props::<Probe>().panel_renders.fetch_add(1, Ordering::Relaxed);
    let mut rows = Column::new();
    for index in 0..20 {
      rows = rows.child(Rect::new(100.0, 50.0).id(format!("row-{index}")).background(Color::new(
        index as u8,
        0,
        0,
        255,
      )));
    }
    Column::new()
      .child(Text::new(&format!("panel-{}", self.count.get())))
      .child(ScrollVertical::new(rows).id("scroll").height(200.0))
      .child(TextInput::new(self.value.clone()).id("input"))
  }
}

/// Mounts a `Leaf` and hands it to a `Wrapper` as a slot child: the leaf is
/// the holder's component, but it sits in the wrapper's output.
struct Holder;

impl Component for Holder {
  type Props = Probe;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Probe>().clone();
    let leaf = ctx.mount::<Leaf>(probe).id("leaf");
    ctx.mount_with::<Wrapper>((), vec![leaf])
  }
}

struct Wrapper;

impl Component for Wrapper {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new().with_children(ctx.children().to_vec())
  }
}

struct Leaf {
  count: Signal<i32>,
}

impl Component for Leaf {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let count = ctx.signal(1);
    Probe::keep(&ctx.props::<Probe>().leaf, &count);
    Self { count }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Text::new(&format!("leaf-{}", self.count.get()))
  }
}

struct Fixture {
  tree: Tree,
  app: App,
  probe: Probe,
}

impl Fixture {
  fn new() -> Self {
    let probe = Probe::default();
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(400, 1200);
    tree.mount_root::<Top>(&mut app, probe.clone());
    let mut fixture = Self { tree, app, probe };
    fixture.pass();
    fixture.pass();
    fixture
  }

  fn pass(&mut self) {
    self.tree.request_redraw();
    self.tree.pass_headless(&mut self.app);
  }

  fn top_of(&mut self, id: &str) -> f32 {
    self
      .tree
      .get_element_by_id_mut(id)
      .and_then(|element| element.bounds())
      .unwrap_or_else(|| panic!("#{id} should be laid out"))
      .y
  }

  fn scroll_offset(&mut self) -> f32 {
    self.top_of("scroll") - self.top_of("row-0")
  }

  fn scroll_by(&mut self, delta: f32) {
    let top = self.top_of("scroll");
    self.tree.scroll(10.0, top + 10.0, 0.0, -delta, ScrollPhase::Scroll);
    self.pass();
    self.pass();
  }

  fn key(&mut self, key: &str, code: &str, shift: bool) {
    self.tree.key_down(key.to_owned(), code.to_owned(), shift, false, false);
  }

  fn input_value(&mut self) -> String {
    self
      .tree
      .get_element_by_id_mut("input")
      .and_then(|element| element.as_text_input().map(|input| input.value()))
      .expect("text input")
  }

  fn focused_id(&self) -> Option<String> {
    self.tree.focused_element()?.id().map(str::to_owned)
  }

  fn has_text(&mut self, text: &str) -> bool {
    self
      .tree
      .find_element(|element| element.text_content() == Some(text))
      .is_some()
  }

  /// Scrolls the panel, focuses its input and selects "lo" at its end.
  fn edit_panel(&mut self) {
    self.scroll_by(150.0);
    assert_eq!(self.scroll_offset(), 150.0);
    self.tree.get_element_by_id_mut("input").expect("input").focus();
    self.key("End", "End", false);
    self.key("ArrowLeft", "ArrowLeft", true);
    self.key("ArrowLeft", "ArrowLeft", true);
    self.pass();
  }

  /// The panel kept its scroll offset, focus and selection: typing replaces
  /// the selected "lo".
  fn assert_panel_state_kept(&mut self) {
    assert_eq!(self.scroll_offset(), 150.0, "scroll offset");
    assert_eq!(self.focused_id().as_deref(), Some("input"), "focus");
    self.key("Z", "KeyZ", false);
    self.pass();
    assert_eq!(self.input_value(), "helZ", "caret and selection");
  }
}

#[test]
fn a_mid_level_refresh_keeps_scroll_text_input_and_focus_inside_an_unchanged_child() {
  let mut fixture = Fixture::new();
  fixture.edit_panel();
  let panel_renders = fixture.probe.panel_renders.load(Ordering::Relaxed);

  Probe::signal(&fixture.probe.section).set(2);
  fixture.pass();

  assert_eq!(
    fixture.probe.panel_renders.load(Ordering::Relaxed),
    panel_renders,
    "the panel did not render again"
  );
  assert!(fixture.has_text("section-2"));
  fixture.assert_panel_state_kept();
}

#[test]
fn a_refresh_above_a_clean_component_shows_the_fresh_render_of_a_child_that_rendered_by_itself() {
  let mut fixture = Fixture::new();
  fixture.edit_panel();
  Probe::signal(&fixture.probe.panel).set(2);
  fixture.pass();
  assert!(fixture.has_text("panel-2"));
  let section_renders = fixture.probe.section_renders.load(Ordering::Relaxed);

  // `Top` renders again; `Section` is clean, so its output is put together
  // from the slots, with the panel's own render since.
  Probe::signal(&fixture.probe.top).set(2);
  fixture.pass();

  assert_eq!(
    fixture.probe.section_renders.load(Ordering::Relaxed),
    section_renders,
    "the section did not render again"
  );
  assert!(fixture.has_text("top-2"));
  assert!(
    fixture.has_text("panel-2"),
    "the panel's own render, not the one the section saw"
  );
  assert!(fixture.tree.get_element_by_id("panel").is_some(), "the outer id stays");
  fixture.assert_panel_state_kept();
}

#[test]
fn a_slot_child_that_rendered_by_itself_stays_fresh_when_a_clean_ancestor_is_put_together() {
  let mut fixture = Fixture::new();
  assert!(fixture.has_text("leaf-1"));

  Probe::signal(&fixture.probe.leaf).set(2);
  fixture.pass();
  assert!(fixture.has_text("leaf-2"));

  // The wrapper's kept output holds the leaf as it was handed to it.
  Probe::signal(&fixture.probe.top).set(2);
  fixture.pass();

  assert!(fixture.has_text("leaf-2"));
  assert!(!fixture.has_text("leaf-1"));
  assert!(fixture.tree.get_element_by_id("leaf").is_some(), "the outer id stays");

  Probe::signal(&fixture.probe.leaf).set(3);
  fixture.pass();
  Probe::signal(&fixture.probe.section).set(2);
  fixture.pass();
  assert!(fixture.has_text("leaf-3"));
}
