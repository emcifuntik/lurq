//! A paste into a text input delivers one `PasteEvent` to the input and its
//! ancestors before the text is inserted, and a synthetic Ctrl+V never
//! reads the OS clipboard.

use std::{
  path::PathBuf,
  sync::{Arc, Mutex},
};

use lurq::{
  app::{
    Tree,
    events::{ClipboardContent, ClipboardImage, MouseButton, PasteEvent, is_synthetic_input},
    synthetic_input::{self, SyntheticInput, SyntheticModifiers},
  },
  components::{Column, TextInput},
  core::Signal,
};

use crate::support::{pointer_click, run_pass};

/// A clipboard of the test's own, counting what a paste reads from it.
struct TestClipboard {
  text_reads: Mutex<usize>,
  full_reads: Mutex<usize>,
}

impl TestClipboard {
  fn install(runtime: &mut Tree) -> Arc<Self> {
    let clipboard = Arc::new(Self {
      text_reads: Mutex::new(0),
      full_reads: Mutex::new(0),
    });
    runtime.set_clipboard_reader(Some(clipboard.clone()));
    clipboard
  }

  fn reads(&self) -> (usize, usize) {
    (*self.text_reads.lock().unwrap(), *self.full_reads.lock().unwrap())
  }
}

impl lurq::app::ClipboardReader for TestClipboard {
  fn read_text(&self) -> Option<String> {
    *self.text_reads.lock().unwrap() += 1;
    Some("secret".to_owned())
  }

  fn read(&self) -> Option<ClipboardContent> {
    *self.full_reads.lock().unwrap() += 1;
    Some(ClipboardContent::text("secret").with_files([PathBuf::from("C:/shots/a.png")]))
  }
}

fn focused_input(value: &Signal<String>) -> Tree {
  let mut runtime = Tree::new();
  runtime.set_root(TextInput::new(value.clone()).id("input"));
  focus(runtime)
}

fn focus(mut runtime: Tree) -> Tree {
  run_pass(&mut runtime);
  let (x, y) = runtime
    .get_element_by_id_mut("input")
    .expect("input")
    .bounds()
    .expect("laid out")
    .center();
  pointer_click(&mut runtime, x, y, MouseButton::Left);
  runtime
}

#[test]
fn a_paste_inserts_the_text_at_the_caret() {
  let value = Signal::new("ab".to_owned());
  let mut runtime = focused_input(&value);
  runtime.key_down("End".into(), "End".into(), false, false, false);

  assert!(runtime.paste(ClipboardContent::text("cd")));

  assert_eq!(value.get(), "abcd");
}

#[test]
fn the_paste_event_runs_before_the_insertion() {
  let value = Signal::new("old".to_owned());
  let seen = Arc::new(Mutex::new(Vec::new()));
  let mut runtime = Tree::new();
  let log = seen.clone();
  let observed = value.clone();
  runtime.set_root(
    TextInput::new(value.clone())
      .id("input")
      .on_paste(move |event: PasteEvent| {
        log
          .lock()
          .unwrap()
          .push((observed.get(), event.text().map(str::to_owned)));
      }),
  );
  let mut runtime = focus(runtime);
  runtime.key_down("End".into(), "End".into(), false, false, false);

  runtime.paste(ClipboardContent::text("new"));

  assert_eq!(
    seen.lock().unwrap().as_slice(),
    &[("old".to_owned(), Some("new".to_owned()))],
    "the handler sees the value before the paste"
  );
  assert_eq!(value.get(), "oldnew");
}

#[test]
fn prevent_default_keeps_the_text_out_of_the_input() {
  let value = Signal::new(String::new());
  let mut runtime = Tree::new();
  runtime.set_root(
    TextInput::new(value.clone())
      .id("input")
      .on_paste(|event: PasteEvent| event.prevent_default()),
  );
  let mut runtime = focus(runtime);

  assert!(
    runtime.paste(ClipboardContent::text("kept out")),
    "a prevented paste is handled"
  );

  assert_eq!(value.get(), "");
}

#[test]
fn an_image_and_files_reach_the_handler_and_insert_nothing() {
  let value = Signal::new(String::new());
  let seen = Arc::new(Mutex::new(None));
  let mut runtime = Tree::new();
  let log = seen.clone();
  runtime.set_root(
    TextInput::new(value.clone())
      .id("input")
      .on_paste(move |event: PasteEvent| {
        let image = event.image().map(|image| (image.width(), image.height()));
        *log.lock().unwrap() = Some((event.kinds(), image, event.files().to_vec()));
        event.prevent_default();
      }),
  );
  let mut runtime = focus(runtime);
  let image = ClipboardImage::from_rgba(2, 1, vec![255; 8]).expect("2x1 RGBA");
  let content = ClipboardContent::image(image).with_files([PathBuf::from("C:/shots/a.png")]);

  runtime.paste(content);

  let (kinds, image, files) = seen.lock().unwrap().clone().expect("the handler ran");
  assert!(kinds.image && kinds.files && !kinds.text);
  assert_eq!(image, Some((2, 1)));
  assert_eq!(files, vec![PathBuf::from("C:/shots/a.png")]);
  assert_eq!(value.get(), "");
}

#[test]
fn a_paste_without_text_or_handler_is_not_handled() {
  let value = Signal::new(String::new());
  let mut runtime = focused_input(&value);

  assert!(!runtime.paste(ClipboardContent::files([PathBuf::from("a.txt")])));
  assert!(!runtime.paste(ClipboardContent::text("")));
  assert_eq!(value.get(), "");
}

#[test]
fn the_paste_event_bubbles_from_the_input_to_its_ancestors() {
  let value = Signal::new(String::new());
  let order = Arc::new(Mutex::new(Vec::new()));
  let mut runtime = Tree::new();
  let (outer, inner) = (order.clone(), order.clone());
  runtime.set_root(
    Column::new()
      .on_paste(move |event: PasteEvent| {
        outer
          .lock()
          .unwrap()
          .push(("column", event.target_id == event.input_id));
      })
      .child(
        TextInput::new(value.clone())
          .id("input")
          .on_paste(move |event: PasteEvent| {
            inner.lock().unwrap().push(("input", event.target_id == event.input_id));
          }),
      ),
  );
  let mut runtime = focus(runtime);

  runtime.paste(ClipboardContent::text("x"));

  assert_eq!(order.lock().unwrap().as_slice(), &[("input", true), ("column", false)]);
  assert_eq!(value.get(), "x");
}

#[test]
fn stop_propagation_keeps_the_paste_from_the_ancestors() {
  let value = Signal::new(String::new());
  let column_saw = Arc::new(Mutex::new(false));
  let mut runtime = Tree::new();
  let flag = column_saw.clone();
  runtime.set_root(
    Column::new()
      .on_paste(move |_: PasteEvent| *flag.lock().unwrap() = true)
      .child(
        TextInput::new(value.clone())
          .id("input")
          .on_paste(|event: PasteEvent| event.stop_propagation()),
      ),
  );
  let mut runtime = focus(runtime);

  runtime.paste(ClipboardContent::text("x"));

  assert!(!*column_saw.lock().unwrap());
  assert_eq!(value.get(), "x", "stopping the propagation keeps the default");
}

#[test]
fn a_paste_without_a_focused_input_does_nothing() {
  let value = Signal::new(String::new());
  let mut runtime = Tree::new();
  runtime.set_root(TextInput::new(value.clone()).id("input"));
  run_pass(&mut runtime);

  assert!(!runtime.paste(ClipboardContent::text("nowhere")));
  assert_eq!(value.get(), "");
}

#[test]
fn a_synthetic_paste_carries_its_content_and_is_marked_synthetic() {
  let value = Signal::new(String::new());
  let synthetic = Arc::new(Mutex::new(None));
  let mut runtime = Tree::new();
  let flag = synthetic.clone();
  runtime.set_root(
    TextInput::new(value.clone())
      .id("input")
      .on_paste(move |_: PasteEvent| {
        *flag.lock().unwrap() = Some(is_synthetic_input());
      }),
  );
  let mut runtime = focus(runtime);

  synthetic_input::apply(&mut runtime, &SyntheticInput::paste(ClipboardContent::text("agent")));

  assert_eq!(*synthetic.lock().unwrap(), Some(true));
  assert_eq!(value.get(), "agent");
}

/// The person's clipboard is theirs: a synthetic Ctrl+V (an MCP client's
/// `key` action) is refused instead of reading it, whatever it holds.
#[test]
fn a_synthetic_ctrl_v_does_not_read_the_clipboard() {
  let value = Signal::new(String::new());
  let seen = Arc::new(Mutex::new(0));
  let mut runtime = Tree::new();
  let clipboard = TestClipboard::install(&mut runtime);
  let count = seen.clone();
  runtime.set_root(
    TextInput::new(value.clone())
      .id("input")
      .on_paste(move |_: PasteEvent| {
        *count.lock().unwrap() += 1;
      }),
  );
  let mut runtime = focus(runtime);
  let ctrl = SyntheticModifiers::default().ctrl();
  let meta = SyntheticModifiers::default().meta();
  let shift = SyntheticModifiers::default().shift();

  for input in [
    SyntheticInput::key_down("v").with_modifiers(ctrl),
    SyntheticInput::key_down("v").with_modifiers(meta),
    SyntheticInput::key_down("Insert").with_modifiers(shift),
  ] {
    synthetic_input::apply(&mut runtime, &input);
  }

  assert_eq!(clipboard.reads(), (0, 0), "the clipboard was never read");
  assert_eq!(*seen.lock().unwrap(), 0, "no paste event without content");
  assert_eq!(value.get(), "");
}

/// `paste_from_os_clipboard` trusts neither its argument nor its caller: a
/// request made while synthetic input is delivered is refused either way.
#[test]
fn a_paste_request_under_synthetic_input_is_refused_whatever_its_argument_says() {
  let value = Signal::new(String::new());
  let mut runtime = Tree::new();
  let clipboard = TestClipboard::install(&mut runtime);
  runtime.set_root(TextInput::new(value.clone()).id("input"));
  let mut runtime = focus(runtime);

  assert!(!runtime.paste_from_os_clipboard(true));
  assert!(!lurq::app::events::with_synthetic_input(
    || runtime.paste_from_os_clipboard(false)
  ));
  assert_eq!(clipboard.reads(), (0, 0));
  assert_eq!(value.get(), "");

  assert!(runtime.paste_from_os_clipboard(false));
  assert_eq!(value.get(), "secret");
}

/// A person's Ctrl+V reads the clipboard: only its text when no handler
/// looks at the paste, as before paste events, and everything when one does.
#[test]
fn a_ctrl_v_from_the_os_reads_as_much_of_the_clipboard_as_the_paste_needs() {
  let value = Signal::new(String::new());
  let mut runtime = Tree::new();
  let clipboard = TestClipboard::install(&mut runtime);
  runtime.set_root(TextInput::new(value.clone()).id("input"));
  let mut runtime = focus(runtime);
  let shortcut = if cfg!(target_vendor = "apple") {
    (false, true)
  } else {
    (true, false)
  };

  runtime.key_down_with_meta("v".into(), "KeyV".into(), false, shortcut.0, false, shortcut.1);
  assert_eq!(value.get(), "secret");
  assert_eq!(clipboard.reads(), (1, 0), "text only: nothing looks at the paste");

  let seen = Arc::new(Mutex::new(Vec::new()));
  let mut runtime = Tree::new();
  let clipboard = TestClipboard::install(&mut runtime);
  let log = seen.clone();
  runtime.set_root(
    Column::new()
      .on_paste(move |event: PasteEvent| log.lock().unwrap().push(event.files().len()))
      .child(TextInput::new(value.clone()).id("input")),
  );
  let mut runtime = focus(runtime);
  runtime.key_down("Insert".into(), "Insert".into(), true, false, false);
  assert_eq!(
    clipboard.reads(),
    (0, 1),
    "everything: an ancestor's handler looks at the paste"
  );
  assert_eq!(seen.lock().unwrap().as_slice(), &[1]);
}
