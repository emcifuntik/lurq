// A pass presents a frame, and a present on a FIFO swapchain can wait for its vblank, holding back what comes
// meanwhile: a timer whose callback changed nothing asks for none, and one that changed what is shown, through
// any handle it can capture, asks for one at once.

use std::time::Duration;

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Timeout},
    theme::PaletteColor,
  },
  components::{Column, Rect, ScrollVertical, Text},
  core::{ElementRef, Signal},
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};

use crate::support::{render_pass_if_needed, render_pass_with_app};

/// Draws the first frame and the passes that frame asks for, until the tree asks for none.
fn settle(tree: &mut Tree, app: &mut App) {
  render_pass_with_app(tree, app);
  for _ in 0..4 {
    if !tree.needs_redraw() {
      return;
    }
    render_pass_if_needed(tree, app);
  }
  assert!(!tree.needs_redraw(), "nothing is left to draw before the timer fires");
}

#[derive(Clone, Copy, Debug, PartialEq, lurq::DevtoolsInspectable)]
enum Change {
  Nothing,
  Signal,
  Scroll,
  ElementBounds,
  Theme,
}

struct TimerChanges {
  label: Signal<&'static str>,
  scroll: ScrollState,
  reference: ElementRef,
  _timeout: Timeout,
}

impl Component for TimerChanges {
  type Props = Change;

  fn create(ctx: &mut Ctx) -> Self {
    let change = *ctx.props::<Change>();
    let label = ctx.signal("idle");
    let scroll = ScrollState::new();
    let reference = ElementRef::new();
    let theme = ctx.theme().clone();
    let timeout = ctx.create_timeout(Duration::ZERO, {
      let label = label.clone();
      let scroll = scroll.clone();
      let reference = reference.clone();
      move || match change {
        Change::Nothing => {}
        Change::Signal => label.set("done"),
        Change::Scroll => scroll.set_scroll(0.0, 50.0),
        Change::ElementBounds => reference.mutable().set_relative_bounds(0.0, 0.0, 10.0, 10.0),
        Change::Theme => theme.set_palette_color(PaletteColor::Accent, Color::from_hex("#c82828")),
      }
    });
    timeout.start();
    Self {
      label,
      scroll,
      reference,
      _timeout: timeout,
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .child(Text::new(self.label.get()))
      .child(Rect::new(40.0, 40.0).ref_element(self.reference.clone()))
      .child(
        ScrollVertical::new(Rect::new(100.0, 400.0))
          .with_scroll_state(self.scroll.clone())
          .size(100.0, 100.0),
      )
  }
}

/// Whether the timer that makes `change` asks for a pass, and whether that pass draws a frame.
fn timer_pass(change: Change) -> (bool, bool) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<TimerChanges>(&mut app, change);
  tree.resize(200, 300);
  settle(&mut tree, &mut app);

  tree.tick_timers_at(std::time::Instant::now() + Duration::from_millis(1));
  let needs_redraw = tree.needs_redraw();
  let drawn = render_pass_if_needed(&mut tree, &mut app).is_some();
  (needs_redraw, drawn)
}

#[test]
fn a_timer_that_changes_nothing_asks_for_no_frame() {
  assert_eq!(timer_pass(Change::Nothing), (false, false));
}

#[test]
fn a_timer_that_changes_what_is_shown_asks_for_a_frame_at_once() {
  for change in [Change::Signal, Change::Scroll, Change::ElementBounds, Change::Theme] {
    assert_eq!(
      timer_pass(change),
      (true, true),
      "a timer that changes {change:?} asks for a frame and the pass draws it"
    );
  }
}

#[cfg(feature = "canvas")]
#[test]
fn a_timer_that_paints_a_canvas_asks_for_a_frame() {
  use lurq::components::Canvas;

  struct CanvasPainter {
    reference: ElementRef,
    _timeout: Timeout,
  }

  impl Component for CanvasPainter {
    type Props = ();

    fn create(ctx: &mut Ctx) -> Self {
      let reference = ElementRef::new();
      let timeout = ctx.create_timeout(Duration::ZERO, {
        let reference = reference.clone();
        move || {
          if let Some(canvas) = reference.as_canvas() {
            canvas.request_paint();
          }
        }
      });
      timeout.start();
      Self {
        reference,
        _timeout: timeout,
      }
    }

    fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
      Canvas::new()
        .ref_element(self.reference.clone())
        .width(40.0)
        .height(40.0)
    }
  }

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<CanvasPainter>(&mut app, ());
  tree.resize(200, 100);
  settle(&mut tree, &mut app);

  tree.tick_timers_at(std::time::Instant::now() + Duration::from_millis(1));
  assert!(tree.needs_redraw(), "the canvas paint asks for a frame");
}
