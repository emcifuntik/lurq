use lurq::{
  app::{App, Tree, events::ImeEvent},
  components::{InputEditError, TextInput, TextInputController},
  core::Signal,
};

fn input(text: &str) -> (Tree, App, Signal<String>, TextInputController) {
  let source = Signal::new(text.to_owned());
  let controller = TextInputController::new();
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.set_root(
    TextInput::new(source.clone())
      .id("editor")
      .controller(controller.clone()),
  );
  tree.pass_headless(&mut app);
  (tree, app, source, controller)
}

fn key(tree: &mut Tree, key: &str, code: &str, shift: bool) {
  let (ctrl, meta) = (cfg!(not(target_vendor = "apple")), cfg!(target_vendor = "apple"));
  tree.key_down_with_meta(key.into(), code.into(), shift, ctrl, false, meta);
}

#[test]
fn input_controller_selection_replacement_is_one_undo_and_redo() {
  let (mut tree, mut app, source, controller) = input("before selected after");
  tree.get_element_by_id_mut("editor").unwrap().focus();
  controller.select(7, 15).unwrap();
  assert_eq!(controller.selection().unwrap().range(), 7..15);
  assert!(controller.replace_selection("{{task.title}}").unwrap());
  assert_eq!(source.get(), "before {{task.title}} after");
  tree.pass_headless(&mut app);
  key(&mut tree, "z", "KeyZ", false);
  assert_eq!(source.get(), "before selected after");
  assert_eq!(controller.selection().unwrap().range(), 7..15);
  key(&mut tree, "z", "KeyZ", true);
  assert_eq!(source.get(), "before {{task.title}} after");
}

#[test]
fn input_controller_retains_caret_through_keyed_rerender_and_blur() {
  let (mut tree, mut app, source, controller) = input("αβ end");
  tree.get_element_by_id_mut("editor").unwrap().focus();
  controller.select(2, 2).unwrap();
  tree.set_root(
    TextInput::new(source.clone())
      .id("editor")
      .controller(controller.clone()),
  );
  tree.pass_headless(&mut app);
  tree.get_element_by_id_mut("editor").unwrap().blur();
  assert!(controller.replace_selection("{{task.title}}").unwrap());
  assert_eq!(source.get(), "α{{task.title}}β end");
  assert_eq!(controller.selection().unwrap().caret, 16);
}

#[test]
fn input_controller_prevented_edit_preserves_source_selection_and_history() {
  let source = Signal::new("left right".to_owned());
  let controller = TextInputController::new();
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.set_root(
    TextInput::new(source.clone())
      .id("editor")
      .controller(controller.clone())
      .on_input(|e: lurq::app::events::TextInputEvent| e.prevent_default()),
  );
  tree.pass_headless(&mut app);
  tree.get_element_by_id_mut("editor").unwrap().focus();
  controller.select(5, 10).unwrap();
  assert!(!controller.replace_selection("{{task.title}}").unwrap());
  assert_eq!(source.get(), "left right");
  assert_eq!(controller.selection().unwrap().range(), 5..10);
  key(&mut tree, "z", "KeyZ", false);
  assert_eq!(source.get(), "left right");
}

#[test]
fn input_controller_rejects_invalid_utf8_and_reconciles_external_value() {
  let (_tree, _app, source, controller) = input("aé字z");
  assert_eq!(controller.select(2, 2), Err(InputEditError::InvalidSelection));
  assert_eq!(controller.select(90, 90), Err(InputEditError::InvalidSelection));
  controller.select(1, 6).unwrap();
  assert!(controller.replace_selection("é").unwrap());
  assert_eq!(source.get(), "aéz");
  source.set("x".to_owned());
  assert!(controller.replace_selection("{{task.title}}").unwrap());
  assert_eq!(source.get(), "x{{task.title}}");
}

#[test]
fn input_controller_refuses_active_ime_without_discarding_preedit() {
  let (mut tree, _app, source, controller) = input("before");
  tree.get_element_by_id_mut("editor").unwrap().focus();
  tree.ime(ImeEvent::Preedit {
    text: "かな".into(),
    cursor: Some((0, 6)),
  });
  assert_eq!(
    controller.replace_selection("{{task.title}}"),
    Err(InputEditError::Composing)
  );
  assert_eq!(source.get(), "before");
  assert_eq!(
    tree
      .get_element_by_id_mut("editor")
      .unwrap()
      .as_text_input()
      .unwrap()
      .composition()
      .as_deref(),
    Some("かな")
  );
  tree.ime(ImeEvent::Commit("かな".into()));
  assert_eq!(source.get(), "beforeかな");
}

#[test]
fn input_controller_does_not_expose_masked_selection() {
  let source = Signal::new("private".to_owned());
  let controller = TextInputController::new();
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.set_root(TextInput::new(source).mask().controller(controller.clone()));
  tree.pass_headless(&mut app);
  assert_eq!(controller.selection(), Err(InputEditError::Masked));
  assert_eq!(controller.replace_selection("x"), Err(InputEditError::Masked));
}

#[test]
fn input_controller_without_attached_input_returns_error() {
  assert_eq!(
    TextInputController::new().replace_selection("x"),
    Err(InputEditError::NotMounted)
  );
}
