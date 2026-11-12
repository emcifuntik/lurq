use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, Rect, Text, TextInput},
  core::Signal,
  node::{Element, color::Color},
};

use crate::support::run_pass;

#[derive(Debug, lurq::DevtoolsInspectable)]
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

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct PanelProps {
  #[devtools_ignore]
  // One slot for the two signals of this test.
  #[allow(clippy::type_complexity)]
  signals_out: Shared<Mutex<Option<(Signal<String>, Signal<u32>)>>>,
}

struct Panel {
  value: Signal<String>,
  generation: Signal<u32>,
}

impl Component for Panel {
  type Props = PanelProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(String::from("initial"));
    let generation = ctx.signal(0u32);
    *props.signals_out.0.lock().unwrap() = Some((value.clone(), generation.clone()));
    Self { value, generation }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let generation = self.generation.get();
    Column::new()
      .child(Text::new(&format!("generation {generation}")))
      .child(Rect::new(30.0, 30.0).id("panel").background("#22c55e"))
      .child(TextInput::new(self.value.clone()).id("field"))
  }
}

#[test]
fn direct_style_mutation_is_transient_but_signal_backed_value_persists() {
  let signals_out = Arc::new(Mutex::new(None));
  let mut tree = Tree::new();
  tree.mount_root::<Panel>(
    &mut App::new(),
    PanelProps {
      signals_out: Shared(signals_out.clone()),
    },
  );
  run_pass(&mut tree);

  // Direct transient mutation of a declared style sticks as long as the
  // owning component does not re-render.
  tree.get_element_by_id_mut("panel").unwrap().set_background("#ef4444");
  run_pass(&mut tree);
  assert_eq!(
    tree.get_element_by_id("panel").unwrap().color(),
    Some(Color::from_hex("#ef4444")),
    "without a re-render the mutation sticks"
  );

  // A signal-backed value write through the typed handle.
  tree
    .get_element_by_id_mut("field")
    .unwrap()
    .as_text_input()
    .unwrap()
    .set_value("typed by test");

  // Force the owning component to re-render.
  let (value, generation) = signals_out.lock().unwrap().clone().unwrap();
  generation.set(1);
  run_pass(&mut tree);

  assert_eq!(
    tree.get_element_by_id("panel").unwrap().color(),
    Some(Color::from_hex("#22c55e")),
    "re-render replaces the node, reverting the transient style mutation"
  );
  assert_eq!(value.get(), "typed by test");
  assert_eq!(
    tree.get_element_by_id("field").unwrap().text_content(),
    Some("typed by test"),
    "signal-backed input value survives the re-render"
  );
}
