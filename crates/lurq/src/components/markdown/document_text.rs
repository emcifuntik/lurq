//! The rendered text of a Markdown document, and highlights over it.
//!
//! A document draws its words in many text leaves: a paragraph, a heading, a
//! table cell, a code block, each text piece between images. Its rendered
//! text is their text in document order, one line feed between two leaves.
//! What the document draws around its words (list markers, footnote labels,
//! a code block's language, a link's isolate marks) is not part of it. A
//! highlight is given in bytes of that text; each leaf takes the part over
//! its own text, in its own bytes.

use std::{cell::RefCell, ops::Range, sync::Arc};

use crate::node::{Element, Node, node_kind::NodeKind, text_highlight::TextHighlight};

pub(super) struct DocumentText<'a> {
  text: RefCell<String>,
  leaves: std::cell::Cell<usize>,
  highlights: &'a [TextHighlight],
}

impl<'a> DocumentText<'a> {
  pub(super) fn new(highlights: &'a [TextHighlight]) -> Self {
    Self {
      text: RefCell::new(String::new()),
      leaves: std::cell::Cell::new(0),
      highlights,
    }
  }

  pub(super) fn into_text(self) -> String {
    self.text.into_inner()
  }

  /// `element`, a text leaf of the document's words, appended to its text.
  pub(super) fn leaf(&self, element: impl Into<Element>) -> Element {
    let mut element = element.into();
    self.append(&mut element.node);
    element
  }

  /// Appends a text leaf's words to the document's text and gives the leaf
  /// the highlights over them.
  pub(super) fn append(&self, node: &mut Node) {
    let content = node.text_content().unwrap_or_default();
    let start = {
      let mut text = self.text.borrow_mut();
      if self.leaves.get() > 0 {
        text.push('\n');
      }
      self.leaves.set(self.leaves.get() + 1);
      let start = text.len();
      text.push_str(content);
      start
    };
    let end = start + content.len();
    let local: Vec<TextHighlight> = self
      .highlights
      .iter()
      .filter(|highlight| highlight.range.start < end && highlight.range.end > start)
      .map(|highlight| {
        let range = highlight.range.start.max(start) - start..highlight.range.end.min(end) - start;
        TextHighlight {
          range: content_to_value(node, range),
          ..highlight.clone()
        }
      })
      .collect();
    if !local.is_empty() {
      node.set_text_highlights(Arc::<[TextHighlight]>::from(local));
    }
  }
}

/// `range` of a leaf's text content in bytes of the text its carets index:
/// a rich text's spans with the presentation ones (a link's isolate marks)
/// among them; the content itself for a plain text.
fn content_to_value(node: &Node, range: Range<usize>) -> Range<usize> {
  let NodeKind::RichText { spans, generated, .. } = node.node_kind() else {
    return range;
  };
  if generated.is_empty() {
    return range;
  }
  // Each span of the content: where it starts in the content and in the value.
  let mut content = 0;
  let mut value = 0;
  let mut parts = Vec::new();
  for (index, span) in spans.iter().enumerate() {
    if !generated.iter().any(|generated| generated.contains(&index)) {
      parts.push((content, value, span.text.len()));
      content += span.text.len();
    }
    value += span.text.len();
  }
  // A start at a span's end goes to the next span's start, past the marks
  // between them; an end at a span's start stays at the last one's end.
  let start = parts
    .iter()
    .find(|(at, _, len)| range.start >= *at && range.start < at + len)
    .map_or(value, |(at, value, _)| value + range.start - at);
  let end = parts
    .iter()
    .rev()
    .find(|(at, _, len)| range.end > *at && range.end <= at + len)
    .map_or(start, |(at, value, _)| value + range.end - at);
  start..end.max(start)
}
