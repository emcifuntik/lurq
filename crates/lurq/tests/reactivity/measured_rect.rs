// A component whose render depends on its own measured size can converge:
// `ElementRef::observe_rect` fires during quad resolution with the measured
// rect, the observer writes a signal, the dirty component re-renders on the
// next pass with the real size. Without the observer this never converges —
// the reactive flush runs entirely before layout, so a render-time bounds
// read always sees the previous frame (zero on the first).

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  core::{ElementRefMut, Signal},
  node::{Element, dimension::Dimension},
};

use crate::support::render_pass_with_app;

const PROBE_HEIGHT: f32 = 7.0;

#[derive(Clone, Copy, PartialEq)]
struct NoProps;

impl lurq::app::component::DevtoolsInspectable for NoProps {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

// Renders a probe rect at half its own measured width, 5 wide until measured.
struct MeasureHost {
  measured: Signal<(f32, f32)>,
  target: ElementRefMut,
}

impl Component for MeasureHost {
  type Props = NoProps;

  fn create(ctx: &mut Ctx) -> Self {
    let measured = ctx.signal((0.0f32, 0.0f32));
    let target = ElementRefMut::new();
    let size = measured.clone();
    target.observe_rect(move |rect| {
      let next = (rect.width, rect.height);
      if size.get() != next {
        size.set(next);
      }
    });
    Self { measured, target }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let (w, _) = self.measured.get();
    let probe = if w > 0.0 { w / 2.0 } else { 5.0 };
    lurq::components::Row::new()
      .width(Dimension::full())
      .height(Dimension::full())
      .ref_element(self.target.clone())
      .child(lurq::components::Rect::new(probe, PROBE_HEIGHT).background(lurq::node::color::Color::from_hex("#c82828")))
  }
}

fn probe_width(snapshot: &crate::support::RenderSnapshot) -> f32 {
  snapshot
    .rects
    .iter()
    .find(|rect| (rect.height - PROBE_HEIGHT).abs() < 0.25)
    .expect("probe rect rendered")
    .width
}

#[test]
fn observed_rect_feeds_a_render_that_depends_on_measured_size() {
  // One persistent App: a fresh App per pass flips the theme version and
  // force-dirties everything, masking whether the observer really drove the
  // re-render.
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MeasureHost>(&mut app, NoProps);
  tree.resize(400, 300);

  // First pass renders the 5.0 fallback, then measures 400x300 at paint.
  let first = render_pass_with_app(&mut tree, &mut app);
  assert_eq!(probe_width(&first), 5.0);

  // The observer's signal write left the component dirty — with no further
  // input, the next pass must re-render with the measured width.
  assert!(
    tree.needs_redraw(),
    "paint-time signal write must schedule a follow-up pass"
  );
  let second = render_pass_with_app(&mut tree, &mut app);
  assert_eq!(probe_width(&second), 200.0);

  // Converged: an unchanged rect must not fire the observer again.
  render_pass_with_app(&mut tree, &mut app);
  assert!(!tree.needs_redraw(), "steady state must not keep re-rendering");
}
