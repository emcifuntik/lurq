//! Styled text inputs: spans over the value, edited, drawn and copied with
//! it. Every test name starts with `styled_`.
use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree},
  components::{SpanStyle, StyledChange, StyledEdit, StyledSpan, StyledText, TextInput, TextInputController},
  core::Signal,
  layout::text_style::TextStyle,
};

use crate::support::{RenderSnapshot, render_pass_with_app};

mod clipboard;
mod editing;
mod paint;

pub(super) const ID: &str = "editor";

pub(super) fn span(range: std::ops::Range<usize>, style: SpanStyle) -> StyledSpan {
  StyledSpan::new(range, style)
}

pub(super) fn bold() -> SpanStyle {
  SpanStyle::new().bold()
}

/// A focused multiline styled input and what it reported.
pub(super) struct Editor {
  pub(super) tree: Tree,
  pub(super) app: App,
  pub(super) value: Signal<String>,
  pub(super) spans: Signal<Vec<StyledSpan>>,
  pub(super) controller: TextInputController,
  /// Each change as the handler saw it: the text and spans then, and the edit.
  pub(super) changes: Arc<Mutex<Vec<(StyledText, StyledEdit)>>>,
}

impl Editor {
  pub(super) fn new(text: &str, spans: Vec<StyledSpan>) -> Self {
    Self::with(text, spans, |input| input)
  }

  /// An editor whose input `configure` adds to.
  pub(super) fn with(text: &str, spans: Vec<StyledSpan>, configure: impl Fn(TextInput) -> TextInput) -> Self {
    let value = Signal::new(text.to_owned());
    let spans = Signal::new(spans);
    let controller = TextInputController::new();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let log = changes.clone();
    let input = TextInput::styled(
      value.clone(),
      TextStyle {
        font_size: 14.0,
        line_height: 1.4,
        ..TextStyle::default()
      },
    )
    .id(ID)
    .spans(spans.clone())
    .controller(controller.clone())
    .on_styled_change(move |change: &StyledChange| {
      log
        .lock()
        .unwrap()
        .push((change.to_styled_text(), change.edit().clone()));
    })
    .multiline()
    .width(420.0);
    let mut tree = Tree::new();
    tree.resize(800, 600);
    tree.set_root(configure(input));
    let mut editor = Self {
      tree,
      app: App::new(),
      value,
      spans,
      controller,
      changes,
    };
    editor.render();
    editor.tree.get_element_by_id_mut(ID).unwrap().focus();
    editor.render();
    editor
  }

  pub(super) fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  pub(super) fn key(&mut self, key: &str) {
    self.tree.key_down(key.into(), key.into(), false, false, false);
  }

  /// Ctrl+key, or Cmd+key on macOS.
  pub(super) fn shortcut(&mut self, key: &str, code: &str, shift: bool) {
    let (ctrl, meta) = (cfg!(not(target_os = "macos")), cfg!(target_os = "macos"));
    self
      .tree
      .key_down_with_meta(key.into(), code.into(), shift, ctrl, false, meta);
  }

  pub(super) fn type_text(&mut self, text: &str) {
    for ch in text.chars() {
      self.tree.key_down(ch.to_string(), "Key".into(), false, false, false);
    }
  }

  pub(super) fn select(&mut self, anchor: usize, caret: usize) {
    self.controller.select(anchor, caret).unwrap();
  }

  pub(super) fn styled(&self) -> StyledText {
    self.controller.styled_text().unwrap().expect("a styled input")
  }

  pub(super) fn spans(&self) -> Vec<StyledSpan> {
    self.styled().spans
  }
}
