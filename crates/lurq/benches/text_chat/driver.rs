//! The chat screen in a tree of its own, and the scripted steps of the text
//! scenarios: each step changes something and runs the pass that draws it.
//! Shared by the CPU benchmark (a no-op renderer) and the GPU probe (the
//! wgpu and DX12 renderers on a hidden window).
use lurq::app::{App, PassReport, Tree, events::ScrollPhase, render_engine::RenderEngine};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use super::screen::{ChatProps, ChatScreen, DRAFT_ID, SIDE_WIDTH, VIEWPORT};

/// Messages of a channel that is not scrolled: enough to fill the list.
pub const CHANNEL_MESSAGES: usize = 200;
pub const SCROLL_MESSAGES: usize = 10_000;
/// One wheel step, in logical pixels: about nine new rows.
pub const SCROLL_STEP: f32 = 600.0;
/// The window narrows by this much a frame, then widens again.
pub const RESIZE_STEP: u32 = 2;
pub const RESIZE_MIN_WIDTH: u32 = VIEWPORT.0 - 320;
const TYPED: &str = "sounds good, I will check the build tonight ";

pub struct Chat<S> {
  pub app: App,
  pub tree: Tree,
  pub props: ChatProps,
  surface: S,
  width: u32,
  narrowing: bool,
  hover_low: bool,
  scroll_down: bool,
  typed: usize,
  deleting: bool,
  next_channel: u64,
  scale: f32,
}

impl<S: HasWindowHandle + HasDisplayHandle> Chat<S> {
  /// Mounted on `surface` with `engine` to draw it, and no pass run yet.
  pub fn new(messages: usize, surface: S, engine: impl Fn() -> Box<dyn RenderEngine> + 'static) -> Self {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.set_render_engine_factory(engine);
    // After the engine exists: it takes its size from the resize.
    tree.resize(VIEWPORT.0, VIEWPORT.1);
    let props = ChatProps::new(0, messages);
    tree.mount_root::<ChatScreen>(&mut app, props.clone());
    Self {
      app,
      tree,
      props,
      surface,
      width: VIEWPORT.0,
      narrowing: true,
      hover_low: false,
      scroll_down: true,
      typed: 0,
      deleting: false,
      next_channel: 1,
      scale: 1.0,
    }
  }

  /// Draws until the list has measured its rows and nothing is left to do.
  pub fn warm(&mut self) {
    for _ in 0..4 {
      self.pass();
    }
  }

  pub fn pass(&mut self) -> PassReport {
    self.tree.request_redraw();
    self.tree.pass(&mut self.app, &self.surface)
  }

  pub fn idle(&mut self) {
    self.pass();
  }

  /// The pointer goes to another message row.
  pub fn hover(&mut self) {
    self.hover_low = !self.hover_low;
    let y = if self.hover_low { 1400.0 } else { 700.0 };
    self.tree.mouse_move(SIDE_WIDTH + 240.0, y);
    self.pass();
  }

  /// One wheel step; the direction turns at either end of the list.
  pub fn scroll(&mut self) {
    let before = self.props.scroll.scroll_y();
    let delta = if self.scroll_down { -SCROLL_STEP } else { SCROLL_STEP };
    self
      .tree
      .scroll(SIDE_WIDTH + 240.0, 700.0, 0.0, delta, ScrollPhase::Scroll);
    self.pass();
    if (self.props.scroll.scroll_y() - before).abs() < 1.0 {
      self.scroll_down = !self.scroll_down;
    }
  }

  /// The props of a channel never shown before; making them is not part of
  /// the frame.
  pub fn new_channel(&mut self) -> ChatProps {
    let channel = self.next_channel;
    self.next_channel += 1;
    self.props.switched_to(channel, CHANNEL_MESSAGES)
  }

  pub fn show(&mut self, props: ChatProps) {
    self.props = props.clone();
    self.tree.update_root_props::<ChatScreen>(props);
    self.pass();
  }

  /// The window `RESIZE_STEP` narrower than the frame before, down to
  /// `RESIZE_MIN_WIDTH`, then as much wider again.
  pub fn resize(&mut self) {
    if self.width <= RESIZE_MIN_WIDTH {
      self.narrowing = false;
    } else if self.width >= VIEWPORT.0 {
      self.narrowing = true;
    }
    let width = if self.narrowing {
      self.width - RESIZE_STEP
    } else {
      self.width + RESIZE_STEP
    };
    self.resize_to(width);
  }

  pub fn resize_to(&mut self, width: u32) {
    self.width = width;
    let (width, height) = self.physical_size();
    self.tree.resize(width, height);
    self.pass();
  }

  /// The window's size in physical pixels.
  pub fn physical_size(&self) -> (u32, u32) {
    let physical = |logical: u32| (logical as f32 * self.scale).round() as u32;
    (physical(self.width), physical(VIEWPORT.1))
  }

  pub fn focus_input(&mut self) {
    self
      .tree
      .get_element_by_id_mut(DRAFT_ID)
      .expect("the message input")
      .focus();
    self.pass();
  }

  /// Types the next character of a sentence; at its end, deletes them one
  /// by one.
  pub fn type_key(&mut self) {
    if self.typed == TYPED.len() {
      self.deleting = true;
    } else if self.typed == 0 {
      self.deleting = false;
    }
    if self.deleting {
      self.typed -= 1;
      self
        .tree
        .key_down("Backspace".into(), "Backspace".into(), false, false, false);
    } else {
      let key = &TYPED[self.typed..self.typed + 1];
      self.typed += 1;
      let code = if key == " " { "Space" } else { "KeyA" };
      self.tree.key_down(key.into(), code.into(), false, false, false);
    }
    self.pass();
  }

  /// The window keeps its logical size: the frame is drawn again at `scale`.
  pub fn set_scale(&mut self, scale: f32) {
    self.scale = scale;
    self.tree.set_scale_factor(scale);
    self.resize_to(self.width);
  }
}
