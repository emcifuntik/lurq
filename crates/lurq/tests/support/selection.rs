//! Selecting text with the pointer, checked against the glyphs the tree
//! draws: a drag from the left edge of one drawn glyph to the right edge of
//! another selects exactly the text between them, and highlights nothing but
//! those glyphs. A glyph stands for the characters it draws, which a
//! ligature makes several (`clusters`).

// Loaded through `#[path]`, this file finds its modules beside it.
#[path = "selection/clusters.rs"]
mod clusters;

use std::cell::{Cell, RefCell};

use lurq::{
  app::{App, Tree, events::MouseButton},
  core::ElementRect,
  node::color::Color,
};

use self::clusters::{Painted, Probes};
use super::{GlyphSnapshot, RectSnapshot, TestSurface, render_pass_with_app};

/// The theme's default selection highlight.
const HIGHLIGHT: Color = Color::new(191, 219, 254, 255);
/// How far a highlight may reach past the ink of the glyphs it covers, in
/// logical pixels: a glyph's advance is wider than its ink by its side
/// bearings.
/// How far past a glyph's ink a highlight may reach on a row, in logical pixels.
pub const INK_SLACK: f32 = 3.0;
/// The transparent border around every glyph bitmap, in physical pixels
/// (the glyph atlas pads each glyph so sampling never bleeds).
pub const GLYPH_PADDING: f32 = 2.0;
/// Two texts closer than this, in logical pixels, are joined without a
/// space when copied (`selected_text_lines::EPSILON`).
const JOIN_GAP: f32 = 1.0;

/// A drawn glyph and the characters of the page's text it stands for: one,
/// or several that a ligature draws as one glyph. A character drawn as
/// several glyphs has one ink over all of them.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
  /// The glyph's row, from the top of its text.
  pub row: usize,
  pub left: f32,
  pub right: f32,
  /// The middle of the glyph's row, in physical pixels.
  pub y: f32,
  /// Where its characters sit in the text (`Page::text`).
  pub start: usize,
  pub end: usize,
}

/// One text of the tree: where it sits in `Page::text` and on screen.
#[derive(Clone, Debug)]
struct TextNode {
  text: String,
  offset: usize,
  bounds: ElementRect,
}

impl TextNode {
  /// Whether `other` is drawn beside this text on one line, as copying
  /// joins texts: their boxes share some height and no width.
  fn beside(&self, other: &Self) -> bool {
    let overlap = |start: f32, end: f32, other_start: f32, other_end: f32| {
      start < other_end - JOIN_GAP && other_start < end - JOIN_GAP
    };
    let (a, b) = (&self.bounds, &other.bounds);
    overlap(a.y, a.y + a.height, b.y, b.y + b.height) && !overlap(a.x, a.x + a.width, b.x, b.x + b.width)
  }
}

/// A mounted tree and the pointer over it, in physical pixels.
pub struct Page {
  pub app: App,
  pub tree: Tree,
  pub scale: f32,
  /// The texts of the tree as inspection shows them, joined by line feeds.
  pub text: String,
  /// The drawn glyphs that stand for the non-whitespace characters of
  /// `text`, in reading order: text by text, row by row, left to right.
  pub inks: Vec<Ink>,
  nodes: Vec<TextNode>,
  /// Colors of glyphs that are not the text's (a link's trailing runs),
  /// left out of `inks`.
  ignored: Vec<[f32; 4]>,
  probes: Probes,
}

impl Page {
  /// Reads the glyphs of a tree that has had its first pass.
  pub fn new(app: App, tree: Tree, scale: f32) -> Self {
    Self::ignoring(app, tree, scale, [])
  }

  /// Reads the glyphs of a tree that has had its first pass, leaving out
  /// the glyphs drawn in the `ignored` colors: presentation, not text (a
  /// link's trailing runs).
  pub fn ignoring(app: App, tree: Tree, scale: f32, ignored: impl IntoIterator<Item = Color>) -> Self {
    let mut page = Self::without_inks(app, tree, scale);
    page.ignored = ignored.into_iter().map(|color| color.to_linear_f32_array()).collect();
    page.read_glyphs();
    page
  }

  /// A page whose glyphs are not read: for a text that draws only part of
  /// itself (a scrolled input), whose rows a test reads itself.
  pub fn without_inks(app: App, tree: Tree, scale: f32) -> Self {
    Self {
      app,
      tree,
      scale,
      text: String::new(),
      inks: Vec::new(),
      nodes: Vec::new(),
      ignored: Vec::new(),
      probes: Probes::new(scale),
    }
  }

  /// Draws the tree and reads its texts and their glyphs again, after it
  /// changed.
  pub fn read_glyphs(&mut self) {
    let snapshot = render_pass_with_app(&mut self.tree, &mut self.app);
    self.nodes = text_nodes(&mut self.tree);
    self.text = self
      .nodes
      .iter()
      .map(|node| node.text.as_str())
      .collect::<Vec<_>>()
      .join("\n");
    let ignored = |glyph: &GlyphSnapshot| {
      self
        .ignored
        .iter()
        .any(|color| color.iter().zip(glyph.color).all(|(a, b)| (a - b).abs() < 0.01))
    };
    let mut glyphs: Vec<GlyphSnapshot> = snapshot
      .glyphs
      .into_iter()
      .filter(|glyph| glyph.width > 0.0 && !ignored(glyph))
      .collect();
    glyphs.sort_by(|a, b| a.order.cmp(&b.order));
    let mut glyphs = glyphs.into_iter();
    let mut painted = Painted::new(&self.tree);
    let mut inks = Vec::new();
    for node in &self.nodes {
      let stretches = painted.stretches(&node.text);
      let clusters = self.probes.clusters(&mut self.app, &node.text, &stretches);
      let count = clusters.iter().map(|cluster| cluster.glyphs).sum();
      let node_glyphs: Vec<GlyphSnapshot> = glyphs.by_ref().take(count).collect();
      assert_eq!(
        node_glyphs.len(),
        count,
        "every non-whitespace character of {:?} is drawn",
        node.text
      );
      let mut drawn = rows_of(node_glyphs).into_iter();
      for cluster in clusters.into_iter().filter(|cluster| cluster.glyphs > 0) {
        let cluster_glyphs: Vec<_> = drawn.by_ref().take(cluster.glyphs).collect();
        let (row, _) = cluster_glyphs[0];
        inks.push(Ink {
          row: row.index,
          left: cluster_glyphs.iter().map(|(_, glyph)| glyph.x).fold(f32::MAX, f32::min) + GLYPH_PADDING,
          right: cluster_glyphs
            .iter()
            .map(|(_, glyph)| glyph.x + glyph.width)
            .fold(f32::MIN, f32::max)
            - GLYPH_PADDING,
          y: row.center,
          start: node.offset + cluster.bytes.start,
          end: node.offset + cluster.bytes.end,
        });
      }
    }
    let left_over = glyphs.count();
    assert_eq!(
      left_over, 0,
      "{left_over} drawn glyphs belong to no text of {:?}",
      self.text
    );
    self.inks = inks;
  }

  /// How many glyphs `text` draws where the tree paints it, each
  /// stretch in its own style: one per non-whitespace character, less where
  /// a ligature joins several.
  pub fn glyphs_drawn_for(&mut self, text: &str) -> usize {
    let stretches = Painted::new(&self.tree).find(text);
    let clusters = self.probes.clusters(&mut self.app, text, &stretches);
    clusters.iter().map(|cluster| cluster.glyphs).sum()
  }

  pub fn pass(&mut self) {
    self.tree.request_redraw();
    self.tree.pass(&mut self.app, &TestSurface);
  }

  pub fn press(&mut self, x: f32, y: f32) {
    self.tree.mouse_down(x, y, MouseButton::Left);
  }

  pub fn shift_press(&mut self, x: f32, y: f32) {
    self
      .tree
      .mouse_down_with_modifiers(x, y, MouseButton::Left, true, false, false);
  }

  pub fn release(&mut self, x: f32, y: f32) {
    self.tree.mouse_up(x, y, MouseButton::Left);
    self.pass();
  }

  pub fn shift_release(&mut self, x: f32, y: f32) {
    self
      .tree
      .mouse_up_with_modifiers(x, y, MouseButton::Left, true, false, false);
    self.pass();
  }

  pub fn click(&mut self, x: f32, y: f32) {
    self.press(x, y);
    self.release(x, y);
  }

  pub fn shift_click(&mut self, x: f32, y: f32) {
    self.shift_press(x, y);
    self.shift_release(x, y);
  }

  /// A drag from `from` to `to`, released there.
  pub fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
    self.press(from.0, from.1);
    self.tree.mouse_move(to.0, to.1);
    self.release(to.0, to.1);
  }

  /// The point before the glyph `ink`: its left edge, on its row.
  pub fn before(&self, ink: usize) -> (f32, f32) {
    let ink = self.inks[ink];
    (ink.left, ink.y)
  }

  /// The point after the glyph `ink`: its right edge, on its row.
  pub fn after(&self, ink: usize) -> (f32, f32) {
    let ink = self.inks[ink];
    (ink.right, ink.y)
  }

  /// A drag from the left edge of the glyph `from` to the right edge of the
  /// glyph `to`, in reading order.
  pub fn drag_inks(&mut self, from: usize, to: usize) {
    self.drag(self.before(from), self.after(to));
  }

  pub fn selected(&self) -> Option<String> {
    self.tree.selected_text()
  }

  /// The text between the glyphs `from` and `to`, both included, as copying
  /// joins it: texts beside each other with a space between them, texts
  /// below each other on lines of their own.
  pub fn text_between(&self, from: usize, to: usize) -> String {
    let (start, end) = (self.inks[from].start, self.inks[to].end);
    let mut copied = String::new();
    let mut previous: Option<&TextNode> = None;
    for node in &self.nodes {
      let node_end = node.offset + node.text.len();
      if node_end < start || node.offset > end {
        continue;
      }
      if let Some(previous) = previous {
        copied.push(if previous.beside(node) { ' ' } else { '\n' });
      }
      copied.push_str(&node.text[start.saturating_sub(node.offset)..end.min(node_end) - node.offset]);
      previous = Some(node);
    }
    copied
  }

  /// The glyph the text `needle` starts at, at its `nth` occurrence.
  pub fn ink_at(&self, needle: &str, nth: usize) -> usize {
    let start = self
      .text
      .match_indices(needle)
      .nth(nth)
      .unwrap_or_else(|| panic!("{needle:?} is not in {:?}", self.text))
      .0;
    self
      .inks
      .iter()
      .position(|ink| ink.start == start)
      .unwrap_or_else(|| panic!("{needle:?} starts at a drawn glyph"))
  }

  /// The glyphs `from..=to` of the text `needle`'s `nth` occurrence.
  pub fn inks_of(&self, needle: &str, nth: usize) -> (usize, usize) {
    let from = self.ink_at(needle, nth);
    let end = self.inks[from].start + needle.len();
    let to = self
      .inks
      .iter()
      .rposition(|ink| ink.end <= end)
      .expect("the text ends at a drawn glyph");
    (from, to)
  }

  pub fn highlights(&mut self) -> Vec<RectSnapshot> {
    render_pass_with_app(&mut self.tree, &mut self.app)
      .rects
      .into_iter()
      .filter(|rect| rect.color == HIGHLIGHT && rect.width > 0.0 && rect.height > 0.0)
      .collect()
  }

  /// Checks that the highlight covers the glyphs `from..=to` and nothing
  /// else: on every row, it reaches no further than their ink, and a row
  /// without a selected glyph has none.
  pub fn assert_highlight_covers(&mut self, from: usize, to: usize) {
    if let Some(mismatch) = self.highlight_mismatch(from, to) {
      panic!("{mismatch}");
    }
  }

  /// How the highlight fails to cover the glyphs `from..=to` and nothing
  /// else (see [`Self::assert_highlight_covers`]), or `None` when it does.
  pub fn highlight_mismatch(&mut self, from: usize, to: usize) -> Option<String> {
    let selected: Vec<Ink> = self.inks[from..=to].to_vec();
    let slack = INK_SLACK * self.scale;
    let highlights = self.highlights();
    for rect in &highlights {
      let on_row: Vec<&Ink> = selected
        .iter()
        .filter(|ink| rect.y <= ink.y && ink.y <= rect.y + rect.height)
        .collect();
      if on_row.is_empty() {
        return Some(format!("a highlight on a row with no selected glyph: {rect:?}"));
      }
      let left = on_row.iter().map(|ink| ink.left).fold(f32::MAX, f32::min);
      let right = on_row.iter().map(|ink| ink.right).fold(f32::MIN, f32::max);
      if rect.x < left - slack || rect.x + rect.width > right + slack {
        return Some(format!(
          "the highlight at y {} ({}..{}) reaches past the selected glyphs' ink ({left}..{right})",
          rect.y,
          rect.x,
          rect.x + rect.width
        ));
      }
    }
    selected
      .iter()
      .find(|ink| {
        !highlights
          .iter()
          .any(|rect| rect.y <= ink.y && ink.y <= rect.y + rect.height)
      })
      .map(|ink| format!("no highlight on the row at y {}, which has selected glyphs", ink.y))
  }
}

/// A row of drawn glyphs: its index in its text and the middle of its glyphs.
#[derive(Clone, Copy)]
pub struct Row {
  pub index: usize,
  pub center: f32,
}

/// The glyphs of one text by row and, in a row, from left to right. Glyphs
/// are on one row when their ink overlaps vertically: a row's glyphs share
/// its baseline, so a comma's or a descender's ink overlaps an x-height
/// letter's, while the next row's tallest letter starts below the lowest
/// descender of this one.
pub fn rows_of(mut glyphs: Vec<GlyphSnapshot>) -> Vec<(Row, GlyphSnapshot)> {
  let top = |glyph: &GlyphSnapshot| glyph.y + GLYPH_PADDING;
  let bottom = |glyph: &GlyphSnapshot| glyph.y + glyph.height - GLYPH_PADDING;
  glyphs.sort_by(|a, b| top(a).total_cmp(&top(b)));
  let mut rows: Vec<(f32, Vec<GlyphSnapshot>)> = Vec::new();
  for glyph in glyphs {
    match rows.last_mut() {
      Some((row_bottom, row)) if top(&glyph) < *row_bottom => {
        *row_bottom = row_bottom.max(bottom(&glyph));
        row.push(glyph);
      }
      _ => rows.push((bottom(&glyph), vec![glyph])),
    }
  }
  let mut ordered = Vec::new();
  for (index, (_, mut row)) in rows.into_iter().enumerate() {
    row.sort_by(|a, b| a.x.total_cmp(&b.x));
    let middle = row.iter().map(|glyph| (top(glyph) + bottom(glyph)) / 2.0).sum::<f32>() / row.len() as f32;
    ordered.extend(row.into_iter().map(move |glyph| (Row { index, center: middle }, glyph)));
  }
  ordered
}

/// The texts of the tree in tree order, with where they sit in the page's
/// text and on screen. A link is part of its paragraph's text, not a text.
fn text_nodes(tree: &mut Tree) -> Vec<TextNode> {
  let mut nodes: Vec<TextNode> = Vec::new();
  loop {
    let skip = nodes.len();
    let seen = Cell::new(0);
    let text = RefCell::new(String::new());
    let found = tree.find_element(|element| {
      let is_text = element.tag_name() != "Link" && element.text_content().is_some_and(|text| !text.is_empty());
      if !is_text {
        return false;
      }
      let index = seen.get();
      seen.set(index + 1);
      if index != skip {
        return false;
      }
      *text.borrow_mut() = element.text_content().unwrap_or_default().to_owned();
      true
    });
    let Some(element) = found else {
      return nodes;
    };
    let offset = nodes.last().map_or(0, |node| node.offset + node.text.len() + 1);
    nodes.push(TextNode {
      text: text.into_inner(),
      offset,
      bounds: element.bounds(),
    });
  }
}

/// Every pair of glyphs `(from, to)` with `from <= to`, `step` apart, so a
/// sweep covers a text's starts and ends without trying them all.
pub fn pairs(count: usize, step: usize) -> impl Iterator<Item = (usize, usize)> {
  (0..count)
    .step_by(step)
    .flat_map(move |from| (from..count).step_by(step).map(move |to| (from, to)))
}

/// The rows of `page` that copy other text than they draw, as messages
/// naming `page_name`: every row is dragged from its first glyph to its
/// last.
pub fn rows_copying_other_text(page: &mut Page, page_name: &str) -> Vec<String> {
  let rows = page.inks.iter().map(|ink| ink.row).max().unwrap_or(0) + 1;
  let mut failures = Vec::new();
  for row in 0..rows {
    let first = page
      .inks
      .iter()
      .position(|ink| ink.row == row)
      .expect("a glyph on the row");
    let last = page
      .inks
      .iter()
      .rposition(|ink| ink.row == row)
      .expect("a glyph on the row");
    page.drag_inks(first, last);
    let expected = page.text_between(first, last);
    let copied = page.selected().unwrap_or_default();
    if copied != expected {
      failures.push(format!("{page_name} row {row}: copied {copied:?}, drawn {expected:?}"));
    }
  }
  failures
}

/// The rows of `page` whose highlight does not cover their glyphs, and
/// only them (see [`Page::highlight_mismatch`]), as messages naming
/// `page_name`: every row is dragged from its first glyph to its last.
pub fn rows_highlighted_elsewhere(page: &mut Page, page_name: &str) -> Vec<String> {
  let rows = page.inks.iter().map(|ink| ink.row).max().unwrap_or(0) + 1;
  let mut failures = Vec::new();
  for row in 0..rows {
    let first = page
      .inks
      .iter()
      .position(|ink| ink.row == row)
      .expect("a glyph on the row");
    let last = page
      .inks
      .iter()
      .rposition(|ink| ink.row == row)
      .expect("a glyph on the row");
    page.drag_inks(first, last);
    if let Some(mismatch) = page.highlight_mismatch(first, last) {
      failures.push(format!("{page_name} row {row}: {mismatch}"));
    }
  }
  failures
}

/// Checks that every drag over the glyphs `(from, to)` of `pairs` copies
/// the text they cover.
pub fn assert_drags_copy_their_glyphs(page: &mut Page, pairs: impl Iterator<Item = (usize, usize)>) {
  let mut failures = Vec::new();
  let mut count = 0;
  for (from, to) in pairs {
    count += 1;
    page.drag_inks(from, to);
    let expected = page.text_between(from, to);
    let copied = page.selected().unwrap_or_default();
    if copied != expected {
      failures.push(format!("{from}..={to}: copied {copied:?}, drawn {expected:?}"));
    }
  }
  assert!(
    failures.is_empty(),
    "{} of {count} drags copied other text:\n{}",
    failures.len(),
    failures.join("\n")
  );
}
