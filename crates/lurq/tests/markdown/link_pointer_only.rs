//! Markdown links presented as the pointer's only
//! (`MarkdownLinkPresentation::pointer_only`): Tab skips them, a click
//! follows them and leaves the focus where it was, and no key press draws
//! their ring or shows their tooltip.

use std::{
  sync::{Arc, Mutex},
  time::Duration,
};

use lurq::{
  app::{
    Tree,
    events::MouseButton,
    theme::{MarkdownFocusRing, ThemeMarkdown},
  },
  components::{MarkdownLink, MarkdownLinkPresentation, MarkdownProps},
  layout::quad::{Quad, QuadContent},
  node::color::Color,
};

use super::links::{bounds_of, mount, pass, quads};
use crate::support::pointer_click;

const RING: &str = "#ff00ff";
const TIP: &str = "opens the place";

fn key(tree: &mut Tree, key: &str, code: &str) {
  tree.key_down(key.into(), code.into(), false, false, false);
  tree.key_up(key.into(), code.into(), false, false, false);
}

fn focused_text(tree: &Tree) -> Option<String> {
  tree
    .focused_element()
    .and_then(|element| element.text_content().map(str::to_owned))
}

fn has_ring(quads: &[Quad]) -> bool {
  quads.iter().any(|quad| {
    quad
      .border
      .and_then(|border| border.top)
      .is_some_and(|top| top.color == Color::from_hex(RING))
  })
}

fn tooltip_shown(quads: &[Quad]) -> bool {
  quads
    .iter()
    .any(|quad| matches!(&quad.content, QuadContent::Text { text, .. } if text == TIP))
}

/// A document whose links to `/pointer…` are the pointer's only, every link
/// with a tooltip, and the destinations followed.
fn document(text: &str) -> (MarkdownProps, Arc<Mutex<Vec<String>>>) {
  let followed = Arc::new(Mutex::new(Vec::new()));
  let record = followed.clone();
  let props = MarkdownProps::new(text)
    .theme(ThemeMarkdown {
      link_focus_ring: Some(MarkdownFocusRing::new(2.0, Color::from_hex(RING)).offset(1.0)),
      link_tooltip_delay: Duration::ZERO,
      ..ThemeMarkdown::default()
    })
    .link_presentation(|link: &MarkdownLink| {
      let shown = MarkdownLinkPresentation::link().tooltip(TIP);
      if link.destination().starts_with("/pointer") {
        shown.pointer_only()
      } else {
        shown
      }
    })
    .on_link_click(move |link: &MarkdownLink| record.lock().unwrap().push(link.destination().to_owned()))
    .width(500.0);
  (props, followed)
}

#[test]
fn markdown_tab_skips_pointer_only_links() {
  let (props, _) = document("A [first](/pointer/1), a [stop](/keyboard) and a [last](/pointer/2).");
  let (mut app, mut tree) = mount(props);

  let mut order = Vec::new();
  for _ in 0..3 {
    key(&mut tree, "Tab", "Tab");
    pass(&mut app, &mut tree);
    order.push(focused_text(&tree).unwrap_or_default());
  }
  assert_eq!(order, ["stop", "stop", "stop"], "only the keyboard's link is a stop");
}

#[test]
fn markdown_tab_finds_no_stop_among_pointer_only_links() {
  let (props, followed) = document("A [first](/pointer/1) and a [last](/pointer/2).");
  let (mut app, mut tree) = mount(props);

  key(&mut tree, "Tab", "Tab");
  pass(&mut app, &mut tree);
  assert_eq!(focused_text(&tree), None);
  key(&mut tree, "Enter", "Enter");
  assert!(followed.lock().unwrap().is_empty(), "Enter reaches no link");
  let quads = quads(&tree);
  assert!(!has_ring(&quads) && !tooltip_shown(&quads));
}

#[test]
fn markdown_pointer_only_link_is_followed_by_a_click_and_takes_no_focus() {
  let (props, followed) = document("A [first](/pointer/1) and a [stop](/keyboard).");
  let (mut app, mut tree) = mount(props);
  let first = bounds_of(&mut tree, "first");

  pointer_click(&mut tree, first.x + 3.0, first.y + 3.0, MouseButton::Left);
  pass(&mut app, &mut tree);
  assert_eq!(*followed.lock().unwrap(), ["/pointer/1"]);
  assert_eq!(focused_text(&tree), None, "the press leaves the focus where it was");

  // A plain key turns the keyboard's focus looks on: there is nothing of the link's to show. The pointer is moved
  // off the link first, so no tooltip is the pointer's.
  tree.mouse_move(first.x + 3.0, first.y + 200.0);
  key(&mut tree, "ArrowDown", "ArrowDown");
  pass(&mut app, &mut tree);
  let quads = quads(&tree);
  assert!(!has_ring(&quads), "no ring after a press and a key");
  assert!(!tooltip_shown(&quads), "no tooltip from the keyboard");
}

#[test]
fn markdown_pointer_only_link_shows_its_tooltip_under_the_pointer() {
  let (props, _) = document("A [first](/pointer/1).");
  let (mut app, mut tree) = mount(props);
  let first = bounds_of(&mut tree, "first");

  tree.mouse_move(first.x + 3.0, first.y + 3.0);
  pass(&mut app, &mut tree);
  pass(&mut app, &mut tree);
  assert!(tooltip_shown(&quads(&tree)));
}
