//! What the platform fills a login in with: the other inputs of the login, beside the one typed into.

use lurq::{
  app::{App, Tree},
  components::{Column, ContentKind, TextInput},
  core::Signal,
  node::Element,
};

struct Login {
  tree: Tree,
  app: App,
  name: Signal<String>,
  password: Signal<String>,
  note: Signal<String>,
}

fn login() -> Login {
  let (name, password, note) = (
    Signal::new(String::new()),
    Signal::new(String::new()),
    Signal::new(String::new()),
  );
  let field = |value: &Signal<String>| TextInput::new(value.clone()).single_line().width(240.0).height(32.0);
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(
    Column::new()
      .child(Element::from(field(&name).content(ContentKind::Username)).id("name"))
      .child(Element::from(field(&password).mask()).id("password"))
      .child(Element::from(field(&note)).id("note")),
  );
  let mut app = App::new();
  tree.pass_headless(&mut app);
  Login {
    tree,
    app,
    name,
    password,
    note,
  }
}

fn focus(login: &mut Login, id: &str) {
  login.tree.get_element_by_id_mut(id).expect("the input").focus();
  login.tree.pass_headless(&mut login.app);
}

#[test]
fn a_logins_other_inputs_are_the_focused_ones_peers() {
  let mut login = login();
  assert!(login.tree.autofill_peers().is_empty(), "nothing is typed into");

  focus(&mut login, "password");
  let peers = login.tree.autofill_peers();
  assert_eq!(peers.len(), 1, "the name, and not the note nor the password itself");
  assert_eq!(peers[0].content, ContentKind::Username);
  assert!(!peers[0].secure);

  focus(&mut login, "name");
  let peers = login.tree.autofill_peers();
  assert_eq!(peers.len(), 1);
  assert!(peers[0].secure, "the password");

  focus(&mut login, "note");
  assert!(
    login.tree.autofill_peers().is_empty(),
    "an input that is no part of a login has none"
  );
}

#[test]
fn what_the_platform_fills_a_peer_in_with_is_the_inputs_value() {
  let mut login = login();
  focus(&mut login, "password");
  let name = login.tree.autofill_peers()[0].id;

  login.tree.autofill(name, "ann@example.com");
  assert_eq!(login.name.get_untracked(), "ann@example.com");
  assert_eq!(
    login.password.get_untracked(),
    "",
    "the input typed into is left as it is"
  );
  assert_eq!(login.note.get_untracked(), "");

  login.tree.autofill(name, "bo");
  assert_eq!(login.name.get_untracked(), "bo", "the whole value is replaced");
}
