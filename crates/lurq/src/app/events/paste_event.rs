use std::sync::Arc;

use super::{ClipboardContent, ClipboardImage, ClipboardKinds, EventControl};
use crate::core::NodeId;

/// A paste into a text input, before the text is inserted: Ctrl+V (Cmd+V on
/// macOS), Shift+Insert, a `WindowHandle::paste_from_clipboard` request (an
/// app's context menu) and a synthetic paste all make one.
///
/// The event carries what the clipboard holds. A handler on the input, or on
/// an ancestor it bubbles to, takes an image or files and calls
/// [`prevent_default`](Self::prevent_default) to stop the insertion, turns a
/// long text into an attachment the same way, or leaves plain text to paste
/// as it always did. Only the text is inserted by default; an image or files
/// without a handler do nothing.
#[derive(Clone)]
pub struct PasteEvent {
  content: Arc<ClipboardContent>,
  /// The node whose handlers run now: the focused input, or an ancestor the
  /// event bubbled to.
  pub target_id: NodeId,
  /// The focused text input the paste goes into.
  pub input_id: NodeId,
  pub(crate) control: EventControl,
}

impl PasteEvent {
  pub(crate) fn new(content: Arc<ClipboardContent>, input_id: NodeId) -> Self {
    Self {
      content,
      target_id: input_id,
      input_id,
      control: EventControl::new(),
    }
  }

  pub(crate) fn at(&self, target_id: NodeId) -> Self {
    Self {
      content: Arc::clone(&self.content),
      target_id,
      input_id: self.input_id,
      control: self.control.clone(),
    }
  }

  /// Everything the clipboard holds.
  pub fn content(&self) -> &ClipboardContent {
    &self.content
  }

  /// Which kinds the clipboard holds: text, an image, files.
  pub fn kinds(&self) -> ClipboardKinds {
    self.content.kinds()
  }

  /// The text that the default inserts, if the clipboard holds some.
  pub fn text(&self) -> Option<&str> {
    self.content.text_content()
  }

  pub fn image(&self) -> Option<&ClipboardImage> {
    self.content.image_content()
  }

  pub fn files(&self) -> &[std::path::PathBuf] {
    self.content.file_paths()
  }

  /// Stops the default: no text is inserted into the input.
  pub fn prevent_default(&self) {
    self.control.prevent_default();
  }

  pub fn default_prevented(&self) -> bool {
    self.control.default_prevented()
  }

  /// Stops the event from bubbling to the input's ancestors.
  pub fn stop_propagation(&self) {
    self.control.stop_propagation();
  }

  pub fn propagation_stopped(&self) -> bool {
    self.control.propagation_stopped()
  }

  pub fn stop_immediate_propagation(&self) {
    self.control.stop_immediate_propagation();
  }

  pub fn immediate_propagation_stopped(&self) -> bool {
    self.control.immediate_propagation_stopped()
  }
}

impl std::fmt::Debug for PasteEvent {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PasteEvent")
      .field("kinds", &self.kinds())
      .field("target_id", &self.target_id)
      .field("input_id", &self.input_id)
      .finish()
  }
}
