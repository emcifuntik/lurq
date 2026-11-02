use super::EventControl;
use crate::core::NodeId;

/// Which way a history swipe goes: fingers to the right go back, as in a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDirection {
  Back,
  Forward,
}

/// A trackpad swipe between pages: `on_history_swipe`. Two fingers moved across the window sideways and lifted, and
/// nothing under the pointer scrolled with them, so the app may go back or forward in its history as a browser does.
/// `x` and `y` are logical pixels where the swipe ended. The event bubbles from the innermost element under the
/// pointer to its ancestors; a handler that moves its history calls `stop_propagation` so an outer one does not move
/// too.
#[derive(Debug, Clone)]
pub struct HistorySwipeEvent {
  pub direction: HistoryDirection,
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

impl HistorySwipeEvent {
  pub fn prevent_default(&self) {
    self.control.prevent_default();
  }

  pub fn default_prevented(&self) -> bool {
    self.control.default_prevented()
  }

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
