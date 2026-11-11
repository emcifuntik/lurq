//! Input method (IME) plumbing between winit and a window's [`Tree`], the
//! same on Windows and macOS: winit's `Ime` events become [`ImeEvent`]s, and
//! the window lets an input method compose only while a text input that takes
//! composition has focus, with its candidate window at that input's caret.

use winit::{
  dpi::{LogicalPosition, LogicalSize},
  event::Ime,
  window::Window,
};

use crate::app::{
  Tree,
  events::{ImeCursorArea, ImeEvent},
};

pub(super) fn to_ime_event(ime: Ime) -> ImeEvent {
  match ime {
    Ime::Enabled => ImeEvent::Enabled,
    Ime::Preedit(text, cursor) => ImeEvent::Preedit { text, cursor },
    Ime::Commit(text) => ImeEvent::Commit(text),
    Ime::Disabled => ImeEvent::Disabled,
  }
}

/// What the window was last told, so it is told only of changes.
#[derive(Default)]
pub(super) struct ImeSync {
  target: Option<usize>,
  area: Option<ImeCursorArea>,
}

impl ImeSync {
  pub(super) fn apply(&mut self, window: &Window, tree: &Tree) {
    // On iOS the shell's own view takes the keyboard, with the input's traits; see `ios::TextInput`. winit's
    // view would bring up a keyboard that knows nothing of the input.
    #[cfg(target_os = "ios")]
    let target = None;
    #[cfg(not(target_os = "ios"))]
    let target = tree.ime_target();
    if target != self.target {
      // Turning the input method off ends a composition left in the input
      // that lost focus (macOS drops its marked text, Windows detaches the
      // input context); it is turned on again for the newly focused input.
      if self.target.is_some() {
        window.set_ime_allowed(false);
      }
      if target.is_some() {
        window.set_ime_allowed(true);
      }
      self.target = target;
      self.area = None;
    }
    if target.is_none() {
      return;
    }
    let area = tree.ime_cursor_area();
    if area == self.area {
      return;
    }
    if let Some(area) = area {
      window.set_ime_cursor_area(
        LogicalPosition::new(f64::from(area.x), f64::from(area.y)),
        LogicalSize::new(f64::from(area.width), f64::from(area.height)),
      );
    }
    self.area = area;
  }
}
