//! Styled spans of a [`super::TextInput`]'s text: the inline marks (bold,
//! italic, inline code, strikethrough, links) a rich editor keeps over a plain
//! string. The text stays the input's `Signal<String>`; the spans live in a
//! second signal bound with [`super::TextInput::spans`], as UTF-8 byte ranges
//! into that text. How each mark looks comes from the theme's
//! [`ThemeTextMarks`](crate::app::theme::ThemeTextMarks).
use std::{ops::Range, sync::Arc};

use crate::{
  app::component::{DevtoolsFormatter, DevtoolsInspectable},
  core::Signal,
};

mod ops;
pub(crate) use ops::{insertion_style, normalize, restyle, slice, splice, style_at, summarize};

/// The marks of one run of text. The default is plain text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SpanStyle {
  pub bold: bool,
  pub italic: bool,
  /// Inline code: the theme's monospace face over a background.
  pub code: bool,
  pub strikethrough: bool,
  /// A link and the target the app gave it (a URL, a task key, a path): the
  /// input draws it as a link and reports it when it is clicked
  /// ([`super::TextInput::on_link_click`]); it never opens it itself.
  pub link: Option<Arc<str>>,
}

impl SpanStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn bold(mut self) -> Self {
    self.bold = true;
    self
  }

  pub fn italic(mut self) -> Self {
    self.italic = true;
    self
  }

  pub fn code(mut self) -> Self {
    self.code = true;
    self
  }

  pub fn strikethrough(mut self) -> Self {
    self.strikethrough = true;
    self
  }

  pub fn link(mut self, target: impl Into<Arc<str>>) -> Self {
    self.link = Some(target.into());
    self
  }

  /// Whether the style has `mark`.
  pub fn has(&self, mark: TextMark) -> bool {
    match mark {
      TextMark::Bold => self.bold,
      TextMark::Italic => self.italic,
      TextMark::Code => self.code,
      TextMark::Strikethrough => self.strikethrough,
    }
  }

  /// The style with `mark` set to `on`.
  pub fn with(mut self, mark: TextMark, on: bool) -> Self {
    match mark {
      TextMark::Bold => self.bold = on,
      TextMark::Italic => self.italic = on,
      TextMark::Code => self.code = on,
      TextMark::Strikethrough => self.strikethrough = on,
    }
    self
  }

  pub fn is_plain(&self) -> bool {
    *self == Self::default()
  }
}

/// A mark that is on or off; links carry a target and are set with
/// [`super::TextInputController::set_link`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextMark {
  Bold,
  Italic,
  Code,
  Strikethrough,
}

impl TextMark {
  pub const ALL: [Self; 4] = [Self::Bold, Self::Italic, Self::Code, Self::Strikethrough];
}

/// A styled run of text: `range` is UTF-8 byte offsets into the input's text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StyledSpan {
  pub range: Range<usize>,
  pub style: SpanStyle,
}

impl StyledSpan {
  pub fn new(range: Range<usize>, style: SpanStyle) -> Self {
    Self { range, style }
  }
}

impl DevtoolsInspectable for StyledSpan {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}

/// Text with its spans, as the input reports a change and as copy and paste
/// carry a styled selection. Spans are sorted, do not overlap, cover only
/// styled text (plain text has no span) and never split a character;
/// adjacent spans have different styles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct StyledText {
  pub text: String,
  pub spans: Vec<StyledSpan>,
}

impl StyledText {
  /// `text` with `spans`, put in canonical form: clamped to the text and to
  /// character boundaries, sorted, plain runs dropped, equal neighbours
  /// merged; where spans overlap, the later one wins.
  pub fn new(text: impl Into<String>, spans: Vec<StyledSpan>) -> Self {
    let text = text.into();
    let spans = normalize(&text, spans);
    Self { text, spans }
  }

  /// Text without styles.
  pub fn plain(text: impl Into<String>) -> Self {
    Self {
      text: text.into(),
      spans: Vec::new(),
    }
  }

  /// The style of the character that starts at `offset`; plain past the end.
  pub fn style_at(&self, offset: usize) -> SpanStyle {
    style_at(&self.spans, offset)
  }

  /// Runs of the whole text in order, each with its style; plain text
  /// included. Rebuilding Markdown walks these.
  pub fn runs(&self) -> Vec<(&str, SpanStyle)> {
    ops::runs(&self.spans, self.text.len())
      .into_iter()
      .filter(|(range, _)| !range.is_empty())
      .map(|(range, style)| (&self.text[range], style))
      .collect()
  }
}

impl DevtoolsInspectable for StyledText {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}

/// A change a styled input made, as its `on_styled_change` handlers get it:
/// what the change was, the spans the input now holds (shared, not copied)
/// and access to its text without copying it, so an app can update its model
/// in place. Nothing of the text is copied unless the handler asks.
#[derive(Clone)]
pub struct StyledChange {
  pub(crate) value: Signal<String>,
  pub(crate) spans: Arc<Vec<StyledSpan>>,
  pub(crate) edit: StyledEdit,
}

impl StyledChange {
  /// Runs `f` with the input's whole text. `f` must not write the input's
  /// value.
  pub fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R {
    self.value.with_untracked(|text| f(text))
  }

  /// The text a [`StyledEdit::Text`] edit inserted; `None` for other edits.
  pub fn inserted_text(&self) -> Option<String> {
    let StyledEdit::Text { replaced, inserted } = &self.edit else {
      return None;
    };
    let range = replaced.start..replaced.start + inserted;
    self.with_text(|text| text.get(range).map(str::to_owned))
  }

  pub fn spans(&self) -> &[StyledSpan] {
    &self.spans
  }

  pub fn edit(&self) -> &StyledEdit {
    &self.edit
  }

  /// The text and spans as an owned [`StyledText`] (copies them).
  pub fn to_styled_text(&self) -> StyledText {
    StyledText {
      text: self.value.get_untracked(),
      spans: self.spans.to_vec(),
    }
  }
}

impl std::fmt::Debug for StyledChange {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("StyledChange")
      .field("edit", &self.edit)
      .field("spans", &self.spans)
      .finish_non_exhaustive()
  }
}

/// What a styled input's change did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyledEdit {
  /// `replaced` bytes of the old text gave way to `inserted` bytes starting
  /// at `replaced.start`; spans after it moved by the difference.
  Text { replaced: Range<usize>, inserted: usize },
  /// The marks over `range` changed; the text did not.
  Style { range: Range<usize> },
  /// Undo or redo put back a whole earlier state.
  Replaced,
}

/// Whether a mark covers a range: all of it, some of it, or none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MarkPresence {
  #[default]
  None,
  Some,
  All,
}

/// The marks over the selection, or the style the next typed text takes
/// when nothing is selected, for a toolbar to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionStyle {
  pub bold: MarkPresence,
  pub italic: MarkPresence,
  pub code: MarkPresence,
  pub strikethrough: MarkPresence,
  /// The link target when all of the selection is one link.
  pub link: Option<Arc<str>>,
  /// Whether any of the selection is a link.
  pub has_link: bool,
}

impl SelectionStyle {
  pub fn mark(&self, mark: TextMark) -> MarkPresence {
    match mark {
      TextMark::Bold => self.bold,
      TextMark::Italic => self.italic,
      TextMark::Code => self.code,
      TextMark::Strikethrough => self.strikethrough,
    }
  }

  pub(crate) fn of_style(style: &SpanStyle) -> Self {
    let presence = |on: bool| if on { MarkPresence::All } else { MarkPresence::None };
    Self {
      bold: presence(style.bold),
      italic: presence(style.italic),
      code: presence(style.code),
      strikethrough: presence(style.strikethrough),
      link: style.link.clone(),
      has_link: style.link.is_some(),
    }
  }
}

/// A link a person clicked in a styled input with the platform's shortcut
/// modifier held (Ctrl, or Cmd on macOS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkClick {
  pub target: Arc<str>,
  /// The link's byte range in the text.
  pub range: Range<usize>,
}
