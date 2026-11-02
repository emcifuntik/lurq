//! Paragraphs in a scrolled `VirtualizedList`, and a paragraph stretched
//! taller than its text by the app's own list view.

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{MarkdownProps, Row, VirtualizedList},
  layout::Alignment,
  markdown::MarkdownBlock,
  node::Element,
};

use super::{super::links::MarkdownRoot, page, paragraph::REPORTED};
use crate::support::{
  TestSurface, render_pass_with_app,
  selection::{GLYPH_PADDING, INK_SLACK, Page, rows_of},
};

const OTHER: &str = "A shorter reply with `inline code` and *emphasis*, wrapped over two lines at this width maybe.";
const REPLIES: usize = 60;

/// The list's replies, each starting with its number, so a drawn row tells
/// which reply it belongs to.
fn replies() -> Vec<(usize, String)> {
  (0..REPLIES)
    .map(|index| {
      let body = if index % 2 == 0 { REPORTED } else { OTHER };
      (index, format!("Reply {index}: {body}"))
    })
    .collect()
}

struct ListRoot;

impl Component for ListRoot {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    VirtualizedList::new(ctx, replies())
      .size(600.0, 700.0)
      .mount_keyed::<MarkdownRoot, _, _, _>(
        |(index, _)| *index,
        |(_, text)| MarkdownProps::new(text.as_str()).selectable(true).width(560.3),
      )
  }
}

/// Whether the list has the reply `index` mounted.
fn mounted(tree: &mut Tree, index: usize) -> bool {
  let start = format!("Reply {index}:");
  tree
    .find_element(|element| element.text_content().is_some_and(|text| text.starts_with(&start)))
    .is_some()
}

/// Sixty replies in a list 700 px tall at scale 1.25, scrolled into its
/// middle: the rows drawn were mounted by the scrolling and laid out at the
/// fractional width 560.3. A drag over each drawn row copies the words on
/// it and highlights nothing but them.
#[test]
fn drags_over_the_drawn_rows_of_a_scrolled_virtualized_list_copy_and_highlight_their_glyphs() {
  let scale = 1.25_f32;
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(scale);
  tree.resize((640.0 * scale) as u32, (760.0 * scale) as u32);
  tree.mount_root::<ListRoot>(&mut app, ());
  // The first pass mounts every reply to measure it; the second keeps the
  // ones in view.
  tree.pass(&mut app, &TestSurface);
  tree.pass(&mut app, &TestSurface);
  assert!(
    mounted(&mut tree, 0) && !mounted(&mut tree, REPLIES - 1),
    "the list mounts the replies in view only"
  );
  for _ in 0..7 {
    tree.scroll(300.0 * scale, 300.0 * scale, 0.0, -437.0, ScrollPhase::Scroll);
    tree.pass(&mut app, &TestSurface);
  }
  assert!(!mounted(&mut tree, 0), "the first reply is scrolled out and unmounted");

  let mut page = Page::without_inks(app, tree, scale);
  let corpus = replies()
    .into_iter()
    .map(|(_, text)| text)
    .collect::<Vec<_>>()
    .join(" ")
    .replace(['`', '*'], "");
  let glyphs = render_pass_with_app(&mut page.tree, &mut page.app)
    .glyphs
    .into_iter()
    .filter(|glyph| glyph.width > 0.0)
    .collect();
  let rows = rows_of(glyphs);
  let count = rows.iter().map(|(row, _)| row.index).max().map_or(0, |last| last + 1);
  // A row cut by the view's edge is left out: a press on it hits nothing.
  let in_view =
    |glyph: &crate::support::GlyphSnapshot| glyph.y > 60.0 * scale && glyph.y + glyph.height < 650.0 * scale;
  let slack = INK_SLACK * scale;
  let mut checked = 0;
  let mut failures = Vec::new();
  for index in 0..count {
    let on_row: Vec<_> = rows.iter().filter(|(row, _)| row.index == index).collect();
    if !on_row.iter().all(|(_, glyph)| in_view(glyph)) {
      continue;
    }
    checked += 1;
    let (row, first) = on_row[0];
    let (_, last) = on_row[on_row.len() - 1];
    let (left, right) = (first.x + GLYPH_PADDING, last.x + last.width - GLYPH_PADDING);
    page.drag((left, row.center), (right, row.center));
    let words = page.selected().unwrap_or_default();
    let drawn = if corpus.contains(words.trim()) {
      page.glyphs_drawn_for(&words)
    } else {
      0
    };
    if drawn != on_row.len() {
      failures.push(format!(
        "row {index}: copied {words:?}, drawn as {drawn} glyphs, for {} glyphs",
        on_row.len()
      ));
    }
    let highlights = page.highlights();
    let elsewhere = |rect: &crate::support::RectSnapshot| {
      rect.y > row.center
        || row.center > rect.y + rect.height
        || rect.x < left - slack
        || rect.x + rect.width > right + slack
    };
    if highlights.is_empty() || highlights.iter().any(elsewhere) {
      failures.push(format!(
        "row {index}: highlighted {highlights:?} for the glyphs {left}..{right} at y {}",
        row.center
      ));
    }
  }
  assert!(checked > 10, "{checked} rows are drawn whole in the view");
  assert!(
    failures.is_empty(),
    "{} of {checked} rows:\n{}",
    failures.len(),
    failures.join("\n")
  );
}

/// The document's one list item, drawn by the app in a row 120 px tall that
/// stretches the paragraph to its height: its text is drawn centered in the
/// paragraph's box, and a selection must meet it there.
#[test]
fn a_drag_over_a_paragraph_stretched_taller_than_its_text_copies_its_glyphs() {
  let props = MarkdownProps::new("- an item with `inline code` in a stretched row").list_view(|list| {
    let MarkdownBlock::Paragraph(inlines) = list.items().first()?.blocks.first()? else {
      return None;
    };
    Some(
      Row::new()
        .height(120.0)
        .align_items(Alignment::Stretch)
        .child(list.inlines(inlines, list.style()))
        .into(),
    )
  });
  let mut page = page(props, 400.0);
  let bounds = page
    .tree
    .find_element(|element| element.text_content().is_some_and(|text| text.starts_with("an item")))
    .expect("the paragraph")
    .bounds();
  assert!(bounds.height > 100.0, "the paragraph is stretched: {bounds:?}");
  assert!(
    page.inks[0].y > bounds.y + 40.0,
    "its text is drawn centered, not at its top"
  );

  let (from, to) = page.inks_of("inline code", 0);
  page.drag_inks(from, to);
  assert_eq!(page.selected().as_deref(), Some("inline code"));
  page.assert_highlight_covers(from, to);
}
