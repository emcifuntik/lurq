use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    App, Tree,
    events::{ImeEvent, KeyboardEvent, TextInputEvent},
  },
  components::{Button, Column, TextInput},
  core::Signal,
  layout::quad::QuadContent,
  node::Element,
};

const INPUT: &str = "input";

/// Key events an `on_key_down` / `on_key_up` handler saw, as
/// `(key, code, composing)`.
type Seen = Arc<Mutex<Vec<(String, String, bool)>>>;

struct Fixture {
  tree: Tree,
  app: App,
  value: Signal<String>,
  downs: Seen,
  ups: Seen,
  inputs: Arc<Mutex<Vec<(String, String, bool)>>>,
}

impl Fixture {
  fn new(initial: &str, configure: impl FnOnce(TextInput) -> TextInput) -> Self {
    let value = Signal::new(initial.to_owned());
    let downs = Seen::default();
    let ups = Seen::default();
    let inputs: Arc<Mutex<Vec<(String, String, bool)>>> = Arc::default();
    let (down_log, up_log, input_log) = (downs.clone(), ups.clone(), inputs.clone());
    let input = TextInput::new(value.clone())
      .width(240.0)
      .height(32.0)
      .placeholder("Message")
      .on_key_down(move |event: KeyboardEvent| {
        down_log
          .lock()
          .unwrap()
          .push((event.key.clone(), event.code.clone(), event.composing));
      })
      .on_key_up(move |event: KeyboardEvent| {
        up_log
          .lock()
          .unwrap()
          .push((event.key.clone(), event.code.clone(), event.composing));
      })
      .on_input(move |event: TextInputEvent| {
        input_log.lock().unwrap().push((
          event.keyboard.key.clone(),
          event.new_value().to_owned(),
          event.keyboard.composing,
        ));
      });
    let mut tree = Tree::new();
    tree.set_root(Element::from(configure(input)).id(INPUT));
    let mut app = App::new();
    tree.pass_headless(&mut app);
    tree.get_element_by_id_mut(INPUT).expect("input has an id").focus();
    tree.pass_headless(&mut app);
    Self {
      tree,
      app,
      value,
      downs,
      ups,
      inputs,
    }
  }

  fn preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
    self.tree.ime(ImeEvent::Preedit {
      text: text.to_owned(),
      cursor,
    });
    self.tree.pass_headless(&mut self.app);
  }

  fn commit(&mut self, text: &str) {
    self.tree.ime(ImeEvent::Preedit {
      text: String::new(),
      cursor: None,
    });
    self.tree.ime(ImeEvent::Commit(text.to_owned()));
    self.tree.pass_headless(&mut self.app);
  }

  fn press(&mut self, key: &str, code: &str) {
    self.tree.key_down(key.to_owned(), code.to_owned(), false, false, false);
  }

  fn release(&mut self, key: &str, code: &str) {
    self.tree.key_up(key.to_owned(), code.to_owned(), false, false, false);
  }

  fn composition(&mut self) -> Option<String> {
    self
      .tree
      .get_element_by_id_mut(INPUT)
      .and_then(|element| element.as_text_input())
      .and_then(|input| input.composition())
  }

  fn shown(&mut self) -> Option<String> {
    self
      .tree
      .get_element_by_id_mut(INPUT)
      .and_then(|element| element.text_content().map(str::to_owned))
  }

  fn downs(&self) -> Vec<(String, String, bool)> {
    self.downs.lock().unwrap().clone()
  }

  fn ups(&self) -> Vec<(String, String, bool)> {
    self.ups.lock().unwrap().clone()
  }

  /// One-pixel-tall rects painted in the input's text colour below the
  /// glyphs: the composition underline.
  fn underlines(&self) -> Vec<(f32, f32)> {
    self
      .tree
      .painted_quads()
      .iter()
      .filter(|quad| quad.height == 1.0 && quad.width > 1.0)
      .filter(|quad| matches!(quad.content, QuadContent::Rect { .. }))
      .map(|quad| (quad.x, quad.width))
      .collect()
  }
}

fn key(key: &str, code: &str, composing: bool) -> (String, String, bool) {
  (key.to_owned(), code.to_owned(), composing)
}

#[test]
fn preedit_is_shown_at_the_caret_without_changing_the_value() {
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほ", Some((6, 6)));

  assert!(fixture.tree.is_composing());
  assert_eq!(fixture.composition().as_deref(), Some("にほ"));
  assert_eq!(
    fixture.shown().as_deref(),
    Some("abにほ"),
    "shown inline after the caret"
  );
  assert_eq!(fixture.value.get(), "ab", "the value waits for the commit");
  assert!(
    fixture.inputs.lock().unwrap().is_empty(),
    "no on_input before the commit"
  );
  let underlines = fixture.underlines();
  assert_eq!(underlines.len(), 1, "the composition is underlined");
}

#[test]
fn preedit_replaces_the_placeholder_and_the_selection() {
  let mut empty = Fixture::new("", |input| input);
  assert_eq!(empty.shown().as_deref(), Some("Message"));
  empty.preedit("か", None);
  assert_eq!(empty.shown().as_deref(), Some("か"), "the placeholder gives way");

  let mut selected = Fixture::new("hello", |input| input);
  selected
    .tree
    .get_element_by_id_mut(INPUT)
    .and_then(|element| element.as_text_input())
    .expect("text input")
    .select_all();
  selected.preedit("世界", None);
  assert_eq!(
    selected.shown().as_deref(),
    Some("世界"),
    "shown in place of the selection"
  );
  selected.commit("世界");
  assert_eq!(selected.value.get(), "世界", "the commit replaces the selection");
}

#[test]
fn commit_inserts_the_text_through_on_input() {
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほん", None);
  fixture.commit("日本");

  assert_eq!(fixture.value.get(), "ab日本");
  assert!(!fixture.tree.is_composing());
  assert_eq!(fixture.composition(), None);
  assert_eq!(fixture.shown().as_deref(), Some("ab日本"));
  assert!(fixture.underlines().is_empty());
  assert_eq!(
    *fixture.inputs.lock().unwrap(),
    [("日本".to_owned(), "ab日本".to_owned(), true)],
    "on_input sees the committed text, marked composing"
  );
  // Typing goes on after the committed text.
  fixture.press("!", "Digit1");
  assert_eq!(fixture.value.get(), "ab日本!");
}

#[test]
fn cancelled_composition_leaves_the_value() {
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほ", None);
  fixture.tree.ime(ImeEvent::Preedit {
    text: String::new(),
    cursor: None,
  });
  fixture.tree.ime(ImeEvent::Disabled);
  fixture.tree.pass_headless(&mut fixture.app);
  assert!(!fixture.tree.is_composing());
  assert_eq!(fixture.shown().as_deref(), Some("ab"));
  assert_eq!(fixture.value.get(), "ab");
}

#[test]
fn enter_during_composition_reaches_no_key_handler_or_default() {
  // A multi-line input inserts a newline on Enter; an app sends on it.
  let mut fixture = Fixture::new("ab", TextInput::multiline);
  fixture.preedit("にほん", None);

  // Windows: the Enter that confirms arrives as a `Process` press before the
  // commit.
  fixture.press("Process", "Enter");
  assert!(
    fixture.downs().is_empty(),
    "the confirming Enter reaches no on_key_down"
  );
  assert_eq!(fixture.value.get(), "ab", "no newline from the confirming Enter");
  fixture.commit("日本");
  fixture.release("Enter", "Enter");

  assert_eq!(fixture.value.get(), "ab日本");
  assert_eq!(
    fixture.ups(),
    [key("Enter", "Enter", true)],
    "the release is marked composing"
  );

  // After the composition Enter is an ordinary key again.
  fixture.press("Enter", "Enter");
  fixture.release("Enter", "Enter");
  assert_eq!(fixture.downs(), [key("Enter", "Enter", false)]);
  assert_eq!(fixture.ups().last(), Some(&key("Enter", "Enter", false)));
  assert_eq!(fixture.value.get(), "ab日本\n");
}

#[test]
fn release_after_a_commit_without_a_reported_press_is_composing() {
  // macOS reports neither the composition's key presses nor the Enter that
  // confirms it, only that Enter's release after the commit.
  let mut fixture = Fixture::new("", |input| input);
  fixture.preedit("é", None);
  fixture.commit("é");
  fixture.release("Enter", "Enter");
  assert_eq!(fixture.ups(), [key("Enter", "Enter", true)]);

  fixture.press("a", "KeyA");
  fixture.release("a", "KeyA");
  assert_eq!(fixture.downs(), [key("a", "KeyA", false)]);
  assert_eq!(fixture.ups().last(), Some(&key("a", "KeyA", false)));
  assert_eq!(fixture.value.get(), "éa");
}

#[test]
fn process_key_is_never_an_ordinary_key() {
  // The press that starts a composition on Windows comes before the preedit.
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.press("Process", "KeyN");
  fixture.release("Process", "KeyN");
  assert!(fixture.downs().is_empty());
  assert_eq!(fixture.ups(), [key("Process", "KeyN", true)]);
  assert_eq!(fixture.value.get(), "ab");
}

#[test]
fn blur_cancels_the_composition() {
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほ", None);
  fixture.tree.get_element_by_id_mut(INPUT).expect("input").blur();
  fixture.tree.pass_headless(&mut fixture.app);
  assert!(!fixture.tree.is_composing());
  assert_eq!(fixture.shown().as_deref(), Some("ab"));
  fixture.tree.ime(ImeEvent::Commit("日本".to_owned()));
  assert_eq!(
    fixture.value.get(),
    "ab",
    "a commit without a focused input changes nothing"
  );
}

#[test]
fn masked_input_takes_no_composition() {
  let mut fixture = Fixture::new("pw", TextInput::mask);
  assert!(!fixture.tree.ime_allowed());
  assert_eq!(fixture.tree.ime_cursor_area(), None);
  fixture.preedit("にほ", None);
  assert!(!fixture.tree.is_composing());
  assert_eq!(fixture.value.get(), "pw");
}

#[test]
fn cursor_area_is_where_a_moved_input_is_painted() {
  let mut plain = Fixture::new("ab", |input| input);
  plain.tree.painted_quads();
  let laid_out = plain.tree.ime_cursor_area().expect("painted focused input");

  let mut moved = Fixture::new("ab", |input| {
    input.transform(lurq::node::transform::Transform2D::translate(40.0, 300.0))
  });
  moved.tree.painted_quads();
  let painted = moved.tree.ime_cursor_area().expect("painted focused input");
  assert!((painted.x - laid_out.x - 40.0).abs() < 0.5, "{painted:?} {laid_out:?}");
  assert!((painted.y - laid_out.y - 300.0).abs() < 0.5, "{painted:?} {laid_out:?}");
}

#[test]
fn cursor_area_follows_the_caret_inside_the_composition() {
  let mut fixture = Fixture::new("ab", |input| input);
  assert!(fixture.tree.ime_allowed());
  let bounds = fixture
    .tree
    .get_element_by_id_mut(INPUT)
    .and_then(|element| element.bounds())
    .expect("laid out");
  fixture.tree.painted_quads();
  let before = fixture.tree.ime_cursor_area().expect("painted focused input");
  assert!(before.x > bounds.x && before.x < bounds.x + bounds.width);
  assert!(before.y >= bounds.y && before.y + before.height <= bounds.y + bounds.height);

  // The input method's cursor after the first of two characters.
  fixture.preedit("にほ", Some((3, 3)));
  fixture.tree.painted_quads();
  let first = fixture.tree.ime_cursor_area().expect("composing").x;
  fixture.preedit("にほ", Some((6, 6)));
  fixture.tree.painted_quads();
  let second = fixture.tree.ime_cursor_area().expect("composing").x;
  assert!(before.x < first && first < second, "{} < {first} < {second}", before.x);
}

#[test]
fn key_passed_on_while_composing_arrives_marked() {
  // A Korean input method can pass Space on while a syllable is still shown;
  // it must not be lost.
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("한", None);
  fixture.press(" ", "Space");
  assert_eq!(
    fixture.downs(),
    [key(" ", "Space", true)],
    "delivered, marked composing"
  );
  fixture.commit("한");
  assert!(fixture.value.get().contains(' '), "the space was inserted");
  assert!(fixture.value.get().contains('한'));

  // After the commit Space is ordinary and lands after the committed text.
  fixture.press(" ", "Space");
  assert_eq!(fixture.downs().last(), Some(&key(" ", "Space", false)));
  assert!(fixture.value.get().ends_with("한 "), "{}", fixture.value.get());
}

#[test]
fn stuck_composition_holds_no_key_back() {
  // A platform that never reports the composition's end must not leave the
  // input withholding keys.
  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほ", None);
  fixture.press("Escape", "Escape");
  assert_eq!(
    fixture.downs(),
    [key("Escape", "Escape", true)],
    "Escape reaches handlers"
  );

  let mut fixture = Fixture::new("ab", |input| input);
  fixture.preedit("にほ", None);
  // A new input method session ends a composition left from the last one.
  fixture.tree.ime(ImeEvent::Enabled);
  fixture.tree.pass_headless(&mut fixture.app);
  assert!(!fixture.tree.is_composing());
  assert_eq!(fixture.shown().as_deref(), Some("ab"));
  fixture.press("a", "KeyA");
  assert_eq!(fixture.downs(), [key("a", "KeyA", false)]);
  assert_eq!(fixture.value.get(), "aba");
}

#[test]
fn enter_passed_on_while_composing_reaches_handlers_but_no_default() {
  let mut fixture = Fixture::new("ab", TextInput::multiline);
  fixture.preedit("にほん", None);
  fixture.press("Enter", "Enter");
  assert_eq!(fixture.downs(), [key("Enter", "Enter", true)], "delivered, marked");
  assert_eq!(fixture.value.get(), "ab", "no newline while composing");
  assert!(fixture.tree.is_composing());

  fixture.commit("日本");
  fixture.press("Enter", "Enter");
  assert_eq!(fixture.value.get(), "ab日本\n", "an Enter after the commit is ordinary");
}

#[test]
fn enter_during_a_composition_activates_no_button() {
  let clicks = Arc::new(Mutex::new(0));
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.set_root(
    Column::new()
      .child(Element::from(TextInput::new(Signal::new(String::new())).width(240.0)).id(INPUT))
      .child(
        Element::from(Button::new("Send").on_click({
          let clicks = clicks.clone();
          move |_| *clicks.lock().unwrap() += 1
        }))
        .id("send"),
      ),
  );
  tree.pass_headless(&mut app);
  tree.get_element_by_id_mut(INPUT).expect("input").focus();
  tree.pass_headless(&mut app);
  tree.ime(ImeEvent::Preedit {
    text: "にほん".to_owned(),
    cursor: None,
  });
  tree.pass_headless(&mut app);

  tree.key_down("Enter".to_owned(), "Enter".to_owned(), false, false, false);
  assert_eq!(*clicks.lock().unwrap(), 0);

  // A button cannot hold focus during a composition (moving focus ends it),
  // so the button's own Enter is an ordinary key.
  tree.get_element_by_id_mut("send").expect("button").focus();
  tree.pass_headless(&mut app);
  assert!(!tree.is_composing());
  tree.key_down("Enter".to_owned(), "Enter".to_owned(), false, false, false);
  assert_eq!(*clicks.lock().unwrap(), 1);
}
