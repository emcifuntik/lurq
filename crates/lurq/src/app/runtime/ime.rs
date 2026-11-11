//! Input method (IME) composition for the focused `TextInput`.
//!
//! A shell forwards the platform's input method events through
//! [`Tree::ime`]; the winit shell does so on Windows and macOS alike. A
//! composition is shown in the focused input (see
//! `node_kind::text_input_composition`) until the input method commits it,
//! which inserts the text like typing.
//!
//! The keys of a composition belong to the input method. Windows reports each
//! key it takes as a press of `Process`, the Enter that confirms a composition
//! included, before the commit; macOS reports none of them, only the release
//! of the key that confirmed. A press of `Process` is withheld from handlers
//! and defaults. Any other press is delivered as usual, marked `composing`
//! while a composition is shown (lurq's own defaults ignore such an `Enter`),
//! so a key the input method passes on (a space after a Korean syllable) is
//! never lost and a composition the platform never ends cannot hold keys back.
//! The releases of withheld presses, of `Process`, of keys while composing and
//! of the key that confirmed a commit reach `on_key_up` handlers marked
//! `composing`.

use super::{Tree, find_node_by_id, find_node_by_path};
use crate::{
  app::events::{ImeCursorArea, ImeEvent, KeyboardEvent},
  core::NodeId,
  node::node_kind::{NodeKind, TextInputState},
};

/// `KeyboardEvent::key` of a key the input method takes (Windows
/// `VK_PROCESSKEY`, the DOM's `"Process"`).
const PROCESS_KEY: &str = "Process";

/// Withheld presses whose releases are still to come; more than this many
/// means releases were lost (focus left the window with keys held).
const MAX_WITHHELD_KEYS: usize = 16;

/// Which key releases belong to a composition.
#[derive(Default)]
pub(super) struct ImeKeys {
  /// Codes of withheld key presses.
  withheld: Vec<String>,
  /// Set by a commit until the next key press: the next release is of the
  /// key that confirmed the composition.
  commit_release: bool,
}

/// What a phone's keyboard is told about the text input it types into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardTraits {
  /// The input is masked: the keyboard keeps what is typed out of its suggestions and out of recordings of
  /// the screen.
  pub secure: bool,
  pub kind: crate::node::node_kind::KeyboardKind,
  pub return_key: crate::node::node_kind::ReturnKey,
  /// What the input holds, for the platform to offer to fill in; never `Auto`.
  pub content: crate::node::node_kind::ContentKind,
}

/// The focused text input as a platform's text system sees it: what a phone's keyboard reads to suggest
/// corrections, and where it puts the caret. Offsets are bytes of `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInputSnapshot {
  /// The text as displayed, a composition in progress included.
  pub text: String,
  /// What is selected, the start first; empty at the caret.
  pub selection: std::ops::Range<usize>,
  /// The composition in progress, if any.
  pub composition: Option<std::ops::Range<usize>>,
  /// The caret as last painted, in logical pixels from the window's top-left corner.
  pub caret: Option<ImeCursorArea>,
  /// Where the input and its text were last painted; `None` before its first frame.
  pub geometry: Option<TextInputGeometry>,
}

/// Where a text input's characters are on the screen, in logical pixels from the window's top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInputGeometry {
  /// The input's box: x, y, width, height.
  pub bounds: (f32, f32, f32, f32),
  /// Every place a caret can be: its byte in the text and the top-left corner of a caret there. A long text
  /// gives its first few thousand.
  pub carets: Vec<(usize, f32, f32)>,
  pub line_height: f32,
}

/// How many caret places a snapshot carries.
const SNAPSHOT_CARETS: usize = 4096;

impl Tree {
  /// The focused text input's text, selection and caret; `None` when no text input has focus.
  pub fn text_input_snapshot(&self) -> Option<TextInputSnapshot> {
    let state = self.focused_text_input_state()?;
    let composition = state.composition_range();
    let text = state.composed_text().unwrap_or_else(|| state.value());
    let selection = match (state.composition_caret(), &composition) {
      (Some(caret), Some(_)) => caret..caret,
      _ => {
        let (caret, anchor) = state.controller_selection();
        let anchor = anchor.unwrap_or(caret);
        anchor.min(caret)..anchor.max(caret)
      }
    };
    Some(TextInputSnapshot {
      text,
      selection,
      composition,
      caret: self.layout_engine.ime_cursor_area(),
      geometry: self.layout_engine.text_input_frame().map(|frame| TextInputGeometry {
        bounds: frame.bounds,
        carets: state
          .caret_table(SNAPSHOT_CARETS)
          .into_iter()
          .map(|(index, x, y)| (index, frame.text_origin.0 + x, frame.text_origin.1 + y))
          .collect(),
        line_height: frame.line_height,
      }),
    })
  }

  /// Replaces a range of the focused text input's value with text, as typing over a selection does: its
  /// `on_input` handlers run and may refuse the change. The caret ends after the text. An autocorrection is
  /// this: the word it corrects, replaced. Does nothing when no text input has focus or the range does not
  /// fall on characters.
  pub fn text_input_replace(&mut self, range: std::ops::Range<usize>, text: &str) {
    self.rebuild_if_dirty();
    let Some(state) = self.focused_text_input_state() else {
      return;
    };
    if !state.controller_select(range.start, range.end) {
      return;
    }
    let event = self.commit_keyboard_event(text);
    if text.is_empty() {
      // Nothing to put in the selection's place: it is deleted.
      if !range.is_empty() {
        state.backspace(&event);
      }
    } else {
      state.insert(text, &event);
    }
    self.needs_redraw = true;
    self.reset_text_input_caret_blink();
    self.apply_reactive_updates_after_event();
  }

  /// Selects a range of the focused text input's value, the caret at its end; an empty range puts the caret.
  pub fn text_input_select(&mut self, range: std::ops::Range<usize>) {
    self.rebuild_if_dirty();
    let Some(state) = self.focused_text_input_state() else {
      return;
    };
    if state.controller_select(range.start, range.end) {
      self.needs_redraw = true;
      self.reset_text_input_caret_blink();
      self.apply_reactive_updates_after_event();
    }
  }

  /// Leaves the focused text input's caret to the platform: a phone draws its own over the window, and handles
  /// to drag at a selection's ends. The input then paints no caret; the selection's highlight stays its own.
  pub fn set_native_text_selection(&mut self, native: bool) {
    self.layout_engine.set_native_text_selection(native);
    self.needs_redraw = true;
  }

  /// Where the platform shows the focused text input's selection: rectangles in the window. The
  /// input paints its highlight there while the platform draws its caret
  /// ([`set_native_text_selection`](Self::set_native_text_selection)), so that it follows a handle as it is
  /// dragged.
  pub fn set_native_selection_rects(&mut self, rects: Vec<(f32, f32, f32, f32)>) {
    self.layout_engine.set_native_selection_rects(rects);
    if let Some(state) = self.focused_text_input_state() {
      state.repaint();
    }
    self.cached_render_list = None;
    self.needs_redraw = true;
  }

  /// What the keyboard should be for the focused text input; `None` when no text input has focus, and there is
  /// nothing to type into.
  pub fn keyboard_traits(&self) -> Option<KeyboardTraits> {
    let state = self.focused_text_input_state()?;
    let (kind, return_key) = state.keyboard();
    Some(KeyboardTraits {
      secure: state.is_masked(),
      kind,
      return_key,
      content: state.content(),
    })
  }

  /// Text from an on-screen keyboard, which gives it whole: a word of an autocorrection, an emoji with all its
  /// parts. It goes into the focused text input as committed text does; a line break in it is the Return key,
  /// which is Enter.
  pub fn keyboard_insert(&mut self, text: &str) {
    let mut lines = text.split('\n');
    if let Some(first) = lines.next()
      && !first.is_empty()
    {
      self.ime(ImeEvent::Commit(first.to_owned()));
    }
    for line in lines {
      self.key_down_with_meta("Enter".to_owned(), "Enter".to_owned(), false, false, false, false);
      self.key_up_with_meta("Enter".to_owned(), "Enter".to_owned(), false, false, false, false);
      if !line.is_empty() {
        self.ime(ImeEvent::Commit(line.to_owned()));
      }
    }
  }

  /// The on-screen keyboard's delete key: Backspace.
  pub fn keyboard_delete_backward(&mut self) {
    self.key_down_with_meta(
      "Backspace".to_owned(),
      "Backspace".to_owned(),
      false,
      false,
      false,
      false,
    );
    self.key_up_with_meta(
      "Backspace".to_owned(),
      "Backspace".to_owned(),
      false,
      false,
      false,
      false,
    );
  }

  /// Applies an input method event to the focused text input: shows or
  /// clears the composition, or inserts the committed text (firing the
  /// input's `on_input` handlers with a `KeyboardEvent` whose `key` is the
  /// text and `composing` is set). Without a focused text input nothing
  /// changes. The winit shell calls this for winit's `Ime` window events; a
  /// custom shell or a test calls it directly.
  pub fn ime(&mut self, event: ImeEvent) {
    self.rebuild_if_dirty();
    if matches!(event, ImeEvent::Commit(_)) {
      self.ime_keys.commit_release = true;
    }
    let Some(state) = self.focused_text_input_state() else {
      return;
    };
    let changed = match event {
      // A new input method session: a composition left from an earlier one
      // (whose end the platform never reported) is stale.
      ImeEvent::Enabled => state.clear_composition(),
      ImeEvent::Preedit { text, cursor } => state.set_composition(text, cursor),
      ImeEvent::Disabled => state.clear_composition(),
      ImeEvent::Commit(text) => {
        let cleared = state.clear_composition();
        let inserted = !text.is_empty() && state.insert(&text, &self.commit_keyboard_event(&text));
        cleared || inserted
      }
    };
    if changed {
      self.needs_redraw = true;
      self.reset_text_input_caret_blink();
    }
    self.apply_reactive_updates_after_event();
  }

  /// Whether the focused text input shows an input method composition.
  pub fn is_composing(&self) -> bool {
    self
      .focused_text_input_state()
      .is_some_and(|state| state.is_composing())
  }

  /// Whether the window should let an input method compose text: a text
  /// input has focus and is not masked (password fields take no
  /// composition, as on both platforms' native fields).
  pub fn ime_allowed(&self) -> bool {
    self.ime_target().is_some()
  }

  /// The focused text input an input method composes into, if
  /// [`Tree::ime_allowed`], identified by the value signal it binds (which,
  /// unlike its node, survives re-renders). A shell resets the platform's
  /// input method when it changes, so a composition does not carry over to
  /// another input.
  /// Brings the focused text input into view in the next pass: a keyboard came up over it.
  pub fn reveal_focused_input(&mut self) {
    if self.focused_text_input_state().is_none() {
      return;
    }
    if let Some(focused) = self.focused_node {
      self.request_scroll_into_view(focused);
      self.needs_redraw = true;
    }
  }

  pub(crate) fn ime_target(&self) -> Option<usize> {
    let state = self.focused_text_input_state()?;
    (!state.is_masked()).then(|| state.value_signal_id())
  }

  /// Where the input method should show its candidate window: the focused
  /// text input's caret as last painted. `None` when [`Tree::ime_allowed`]
  /// is false or the input has not been painted yet.
  pub fn ime_cursor_area(&self) -> Option<ImeCursorArea> {
    if !self.ime_allowed() {
      return None;
    }
    self.layout_engine.ime_cursor_area()
  }

  /// Whether a key press is the input method's (`Process`) and is withheld,
  /// recording it so its release is recognised.
  pub(super) fn withhold_composition_key_down(&mut self, key: &str, code: &str) -> bool {
    self.ime_keys.commit_release = false;
    if key != PROCESS_KEY {
      return false;
    }
    let withheld = &mut self.ime_keys.withheld;
    if !withheld.iter().any(|existing| existing == code) {
      if withheld.len() == MAX_WITHHELD_KEYS {
        withheld.remove(0);
      }
      withheld.push(code.to_owned());
    }
    true
  }

  /// Whether a key release belongs to a composition.
  pub(super) fn is_composition_key_up(&mut self, key: &str, code: &str) -> bool {
    let withheld = match self.ime_keys.withheld.iter().position(|existing| existing == code) {
      Some(index) => {
        self.ime_keys.withheld.remove(index);
        true
      }
      None => false,
    };
    let commit_release = std::mem::take(&mut self.ime_keys.commit_release);
    withheld || commit_release || key == PROCESS_KEY || self.is_composing()
  }

  /// Whether a text input has the focus.
  pub(crate) fn focused_text_input(&self) -> bool {
    self.focused_text_input_state().is_some()
  }

  /// Whether the node is a text input.
  pub(crate) fn is_text_input(&self, id: NodeId) -> bool {
    self
      .root
      .as_ref()
      .and_then(|root| find_node_by_id(root, id))
      .is_some_and(|node| matches!(node.node_kind(), NodeKind::TextInput { .. }))
  }

  fn focused_text_input_state(&self) -> Option<TextInputState> {
    let root = self.root.as_ref()?;
    let node = self
      .focused_path
      .as_deref()
      .and_then(|path| find_node_by_path(root, path))
      .or_else(|| self.focused_node.and_then(|id| find_node_by_id(root, id)))?;
    match node.node_kind() {
      NodeKind::TextInput { state, .. } => Some(state.clone()),
      _ => None,
    }
  }

  fn commit_keyboard_event(&self, text: &str) -> KeyboardEvent {
    let target = self.focused_node.unwrap_or(NodeId::UNASSIGNED);
    let mut event = KeyboardEvent::new(text, "", false, false, false, false, target);
    event.text_input_focused = true;
    event.composing = true;
    event
  }
}
