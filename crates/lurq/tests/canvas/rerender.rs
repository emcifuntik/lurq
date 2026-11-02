use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseEvent, render_engine::RenderEngine},
  canvas::{CanvasError, CanvasHandle},
  components::{Canvas, Column, Overlay, Rect, Text},
  core::{ElementRef, Signal},
  layout::{Constraints, Size, render_list::RenderList},
  node::Element,
};
use raw_window_handle::{DisplayHandle, WindowHandle};

use super::{setup, support};

/// A GPU canvas whose click handler draws and changes state in one go, the
/// way a chart paints a selection and shows its label.
struct Selectable {
  reference: ElementRef,
  clicks: Signal<u32>,
}

impl Component for Selectable {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      reference: ctx.element_ref(),
      clicks: ctx.signal(0),
    }
  }

  fn render(&self, _: &mut Ctx) -> impl Into<Element> {
    let (reference, clicks) = (self.reference.clone(), self.clicks.clone());
    Column::new()
      .child(
        Canvas::new()
          .ref_element(self.reference.clone())
          .id("gpu")
          .width(64.0)
          .height(64.0)
          .on_click(move |_: MouseEvent| {
            let canvas = reference.as_canvas().expect("the canvas is attached while clicked");
            canvas.context_2d().fill_rect(0.0, 0.0, 16.0, 16.0);
            clicks.update(|count| *count += 1);
          }),
      )
      .child(Text::new(&format!("{} clicks", self.clicks.get())))
  }
}

/// A GPU renderer stand-in that records the queued bytes of each canvas it is
/// asked to prepare. It encodes nothing, so the queue stays as it was.
struct Recorder(Arc<Mutex<Vec<usize>>>);

impl RenderEngine for Recorder {
  fn resize(&mut self, _: u32, _: u32) {}
  fn prepare_canvases(&mut self, canvases: &[CanvasHandle]) {
    let mut prepared = self.0.lock().unwrap();
    for canvas in canvases {
      prepared.push(canvas.status().pending_bytes);
    }
  }
  fn render(&mut self, _: &RenderList, _: WindowHandle<'_>, _: DisplayHandle<'_>) -> bool {
    true
  }
}

#[test]
fn drawing_queued_by_a_handler_survives_the_rerender_it_causes() {
  let prepared = Arc::new(Mutex::new(Vec::new()));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(256, 256);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(256.0, 256.0))));
  let recorder = prepared.clone();
  tree.set_render_engine_factory(move || Box::new(Recorder(recorder.clone())));
  tree.mount_root::<Selectable>(&mut app, ());
  tree.pass(&mut app, &support::TestSurface);
  let canvas = tree.get_element_by_id("gpu").unwrap().as_canvas().unwrap();
  prepared.lock().unwrap().clear();
  let queued = canvas.status().pending_bytes;

  support::pointer_click(&mut tree, 32.0, 32.0, lurq::app::events::MouseButton::Left);
  let drawn = canvas.status();
  assert!(drawn.pending_bytes > queued, "the click queued drawing");
  tree.pass(&mut app, &support::TestSurface);

  // The re-render carried the surface over and the renderer received the
  // drawing the handler queued before it; the surface stayed attached.
  assert_eq!(*prepared.lock().unwrap(), [drawn.pending_bytes]);
  assert!(canvas.is_attached());
  assert_eq!(canvas.status().content_revision, drawn.content_revision);
  let rebound = tree.get_element_by_id("gpu").unwrap().as_canvas().unwrap();
  assert_eq!(rebound.surface_id(), canvas.surface_id());
  assert!(
    tree
      .find_element(|node| node.text_content() == Some("1 clicks"))
      .is_some()
  );
}

#[test]
fn a_canvas_the_rerender_removes_is_still_detached() {
  let (mut app, mut tree, reference) = setup(64.0, 64.0);
  let canvas = reference.as_canvas().unwrap();
  canvas.context_2d().fill_rect(0.0, 0.0, 8.0, 8.0);
  tree.set_root(Column::new().child(Text::new("no canvas")));
  assert!(!canvas.is_attached());
  assert!(reference.as_canvas().is_none());
  tree.pass(&mut app, &support::TestSurface);
  assert!(!canvas.is_attached());
}

/// A GPU canvas in an overlay, opened either through the signal itself or by
/// re-rendering with its value (`by_render`). `#toggle` flips it, `#bump`
/// re-renders the component without touching the overlay.
struct OverlayCanvas {
  anchor: ElementRef,
  reference: ElementRef,
  open: Signal<bool>,
  bumps: Signal<u32>,
  by_render: bool,
}

/// The canvas ref comes from the test: overlay content is not reachable
/// through tree lookups.
#[derive(Clone)]
struct OverlayProps {
  by_render: bool,
  reference: ElementRef,
}

impl lurq::app::component::DevtoolsInspectable for OverlayProps {}

impl PartialEq for OverlayProps {
  fn eq(&self, other: &Self) -> bool {
    self.by_render == other.by_render
  }
}

impl Component for OverlayCanvas {
  type Props = OverlayProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<OverlayProps>().clone();
    Self {
      anchor: ctx.element_ref(),
      reference: props.reference,
      open: ctx.signal(true),
      bumps: ctx.signal(0),
      by_render: props.by_render,
    }
  }

  fn render(&self, _: &mut Ctx) -> impl Into<Element> {
    let (open, bumps) = (self.open.clone(), self.bumps.clone());
    let canvas = Canvas::new()
      .ref_element(self.reference.clone())
      .id("oc")
      .width(40.0)
      .height(40.0);
    let overlay = if self.by_render {
      Overlay::new(canvas)
        .anchor(self.anchor.clone())
        .open_when(self.open.get())
    } else {
      Overlay::new(canvas).anchor(self.anchor.clone()).open(self.open.clone())
    };
    Column::new()
      .child(
        Rect::new(20.0, 20.0)
          .id("toggle")
          .ref_element(self.anchor.clone())
          .on_click(move |_| open.update(|open| *open = !*open)),
      )
      .child(
        Rect::new(20.0, 20.0)
          .id("bump")
          .on_click(move |_| bumps.update(|count| *count += 1)),
      )
      .child(Text::new(&format!("{} bumps", self.bumps.get())))
      .child(overlay)
  }
}

fn overlay_tree(by_render: bool) -> (App, Tree, ElementRef, Arc<Mutex<Vec<usize>>>) {
  let prepared = Arc::new(Mutex::new(Vec::new()));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(256, 256);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(256.0, 256.0))));
  let recorder = prepared.clone();
  tree.set_render_engine_factory(move || Box::new(Recorder(recorder.clone())));
  let reference = ElementRef::new();
  let props = OverlayProps {
    by_render,
    reference: reference.clone(),
  };
  tree.mount_root::<OverlayCanvas>(&mut app, props);
  tree.pass(&mut app, &support::TestSurface);
  (app, tree, reference, prepared)
}

fn click(tree: &mut Tree, app: &mut App, id: &str) {
  tree.get_element_by_id_mut(id).unwrap().click();
  tree.pass(app, &support::TestSurface);
}

fn assert_detached(canvas: &CanvasHandle) {
  assert!(!canvas.is_attached());
  let queued = canvas.status().pending_bytes;
  canvas.context_2d().fill_rect(0.0, 0.0, 4.0, 4.0);
  assert_eq!(
    canvas.status().pending_bytes,
    queued,
    "a detached canvas queues nothing"
  );
  assert!(matches!(canvas.snapshot().try_take(), Some(Err(CanvasError::Detached))));
}

#[test]
fn closing_an_overlay_detaches_its_canvas_and_reopening_binds_a_new_one() {
  for by_render in [false, true] {
    let (mut app, mut tree, reference, _) = overlay_tree(by_render);
    let first = reference.as_canvas().unwrap();
    assert!(first.is_attached());

    click(&mut tree, &mut app, "toggle");
    assert!(reference.as_canvas().is_none(), "by_render={by_render}");
    assert_detached(&first);

    click(&mut tree, &mut app, "toggle");
    let reopened = reference.as_canvas().unwrap();
    assert!(reopened.is_attached());
    assert_ne!(
      reopened.surface_id(),
      first.surface_id(),
      "a closed surface is never retargeted"
    );
    assert_detached(&first);
  }
}

#[test]
fn an_open_overlay_keeps_its_canvas_and_queued_drawing_across_rerenders() {
  let (mut app, mut tree, reference, prepared) = overlay_tree(false);
  let canvas = reference.as_canvas().unwrap();
  canvas.context_2d().fill_rect(0.0, 0.0, 8.0, 8.0);
  let queued = canvas.status().pending_bytes;
  prepared.lock().unwrap().clear();
  click(&mut tree, &mut app, "bump");
  assert!(
    tree
      .find_element(|node| node.text_content() == Some("1 bumps"))
      .is_some()
  );
  assert!(canvas.is_attached());
  assert_eq!(reference.as_canvas().unwrap().surface_id(), canvas.surface_id());
  assert_eq!(*prepared.lock().unwrap(), [queued]);
}
