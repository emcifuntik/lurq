//! Presence: the content of a layer enters with the animation it plays from
//! its mount, and leaves with its exit animations. A closed layer stays on
//! screen, taking no input, until the longest of them ends.

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{Duration, Instant},
};

use lurq::{
  animation::{AnimatableProperty, AnimatableValue, Animation, AnimationDirection, Keyframes, KeyframesId},
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
  },
  components::{Button, Column, Modal, Popup, Rect, Root, ScrollVertical, Stack},
  core::{ElementRef, Signal},
  layout::{
    Constraints, Size,
    quad::{Quad, QuadContent},
  },
  node::{Element, color::Color},
};

use crate::support::pointer_click;

const FADE_IN: u16 = 71;
const FADE_OUT: u16 = 72;
const FADE: u16 = 73;

const PANEL_A: Color = Color::new(200, 30, 30, 255);
const PANEL_B: Color = Color::new(30, 30, 200, 255);
const BACKDROP: Color = Color::new(10, 10, 10, 255);

fn fade(id: u16, from: f32, to: f32) -> Keyframes {
  Keyframes::new(KeyframesId::new(id))
    .frame(0.0, |frame| {
      frame.set(AnimatableProperty::Opacity, AnimatableValue::Float(from));
    })
    .frame(1.0, |frame| {
      frame.set(AnimatableProperty::Opacity, AnimatableValue::Float(to));
    })
}

fn enter() -> Animation {
  Animation::new(KeyframesId::new(FADE_IN)).duration_ms(200).linear()
}

fn exit() -> Animation {
  Animation::new(KeyframesId::new(FADE_OUT)).duration_ms(200).linear()
}

type Scene = Arc<dyn Fn() -> Element + Send + Sync>;

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Props {
  #[devtools_ignore]
  scene: Scene,
}

impl PartialEq for Props {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.scene, &other.scene)
  }
}

/// Renders the scene a test gives it, again whenever a signal the scene
/// reads changes.
struct Page;

impl Component for Page {
  type Props = Props;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    (ctx.props::<Props>().scene)()
  }
}

/// A page whose animations run on a clock the test moves.
struct Stage {
  app: App,
  tree: Tree,
  start: Instant,
}

impl Stage {
  fn new(scene: impl Fn() -> Element + Send + Sync + 'static) -> Self {
    Self::with_root::<Page>(Props { scene: Arc::new(scene) })
  }

  fn with_root<C: Component>(props: C::Props) -> Self {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.register_keyframes(fade(FADE_IN, 0.0, 1.0));
    tree.register_keyframes(fade(FADE_OUT, 1.0, 0.0));
    tree.register_keyframes(fade(FADE, 0.0, 1.0));
    tree.set_layout_constraints_override(Some(Constraints::tight(Size::new(400.0, 300.0))));
    tree.mount_root::<C>(&mut app, props);
    let mut stage = Self {
      app,
      tree,
      start: Instant::now(),
    };
    stage.at(0);
    stage
  }

  /// Runs the passes of the frame `millis` after the start.
  fn at(&mut self, millis: u64) {
    self
      .tree
      .set_animation_clock(Some(self.start + Duration::from_millis(millis)));
    for _ in 0..2 {
      self.tree.request_redraw();
      self.tree.pass_headless(&mut self.app);
    }
  }

  fn quads(&self) -> Vec<Quad> {
    self.tree.resolve_quads(self.tree.last_layout().unwrap())
  }

  /// The opacity of each rectangle painted in `color`, in paint order.
  fn opacities(&self, color: Color) -> Vec<f32> {
    self
      .quads()
      .iter()
      .filter(|quad| matches!(&quad.content, QuadContent::Rect { color: fill, .. } if *fill == color))
      .map(|quad| quad.opacity)
      .collect()
  }

  /// Where each rectangle painted in `color` is in the paint order.
  fn paint_order(&self, color: Color) -> Vec<usize> {
    self
      .quads()
      .iter()
      .enumerate()
      .filter(|(_, quad)| matches!(&quad.content, QuadContent::Rect { color: fill, .. } if *fill == color))
      .map(|(index, _)| index)
      .collect()
  }

  fn center_of(&mut self, id: &str) -> (f32, f32) {
    self
      .tree
      .get_element_by_id_mut(id)
      .and_then(|element| element.bounds())
      .unwrap_or_else(|| panic!("#{id} should be laid out"))
      .center()
  }

  fn tab(&mut self) {
    self
      .tree
      .key_down("Tab".to_owned(), "Tab".to_owned(), false, false, false);
    self
      .tree
      .key_up("Tab".to_owned(), "Tab".to_owned(), false, false, false);
  }

  fn focused_id(&self) -> Option<String> {
    self
      .tree
      .focused_element()
      .and_then(|element| element.id().map(str::to_owned))
  }
}

fn assert_close(actual: &[f32], expected: &[f32]) {
  assert_eq!(
    actual.len(),
    expected.len(),
    "painted {actual:?}, expected {expected:?}"
  );
  for (actual_value, expected_value) in actual.iter().zip(expected) {
    assert!(
      (actual_value - expected_value).abs() < 0.01,
      "painted {actual:?}, expected {expected:?}"
    );
  }
}

/// What a popup shows: a panel that fades in and out, and a button.
fn panel(name: &str, color: Color, animated: bool, clicks: &Arc<AtomicUsize>) -> Column {
  let clicks = clicks.clone();
  let mut fill = Rect::new(120.0, 40.0).background(color);
  if animated {
    fill = fill.animation(enter()).exit_animation(exit());
  }
  Column::new().child(fill).child(
    Button::new("Inside")
      .id(format!("inside-{name}"))
      .size(120.0, 30.0)
      .tab_index(0)
      .on_click(move |_| {
        clicks.fetch_add(1, Ordering::SeqCst);
      }),
  )
}

/// A page with one popup under its trigger, over a button.
struct OnePopup {
  open: Signal<bool>,
  inside_clicks: Arc<AtomicUsize>,
  behind_clicks: Arc<AtomicUsize>,
  stage: Stage,
}

fn one_popup(animated: bool) -> OnePopup {
  let open = Signal::new(false);
  let anchor = ElementRef::new();
  let inside_clicks = Arc::new(AtomicUsize::new(0));
  let behind_clicks = Arc::new(AtomicUsize::new(0));
  let stage = Stage::new({
    let (open, inside_clicks, behind_clicks) = (open.clone(), inside_clicks.clone(), behind_clicks.clone());
    move || {
      let behind_clicks = behind_clicks.clone();
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), panel("a", PANEL_A, animated, &inside_clicks)).open(open.clone()))
        .child(
          Button::new("Behind")
            .id("behind")
            .size(300.0, 200.0)
            .tab_index(0)
            .on_click(move |_| {
              behind_clicks.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .into()
    }
  });
  OnePopup {
    open,
    inside_clicks,
    behind_clicks,
    stage,
  }
}

#[test]
fn a_layer_enters_with_the_animation_its_content_plays() {
  let mut page = one_popup(true);
  assert_close(&page.stage.opacities(PANEL_A), &[]);

  page.open.set(true);
  page.stage.at(0);
  assert_close(&page.stage.opacities(PANEL_A), &[0.0]);

  page.stage.at(100);
  assert_close(&page.stage.opacities(PANEL_A), &[0.5]);

  page.stage.at(200);
  assert_close(&page.stage.opacities(PANEL_A), &[1.0]);
}

#[test]
fn a_closed_layer_stays_until_its_exit_animation_ends() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);

  page.open.set(false);
  page.stage.at(300);
  assert_close(&page.stage.opacities(PANEL_A), &[1.0]);

  page.stage.at(400);
  assert_close(&page.stage.opacities(PANEL_A), &[0.5]);
  assert!(
    page.stage.tree.has_active_timeline(),
    "frames go on while the layer leaves"
  );

  page.stage.at(499);
  assert_eq!(page.stage.opacities(PANEL_A).len(), 1);

  page.stage.at(500);
  assert_close(&page.stage.opacities(PANEL_A), &[]);
  assert!(!page.stage.tree.has_active_timeline(), "and stop once it is gone");
  assert_eq!(page.stage.tree.next_timeline_change(), None);
}

#[test]
fn a_layer_without_an_exit_animation_is_gone_with_the_pass_that_closes_it() {
  let mut page = one_popup(false);
  page.open.set(true);
  page.stage.at(0);
  assert_close(&page.stage.opacities(PANEL_A), &[1.0]);

  page.open.set(false);
  page.stage.at(0);

  assert_close(&page.stage.opacities(PANEL_A), &[]);
  assert!(!page.stage.tree.has_active_timeline());
}

#[test]
fn a_leaving_layer_is_dropped_by_the_first_pass_after_its_time() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);
  page.open.set(false);
  page.stage.at(300);

  // No frame in between: the pass that comes late still drops the layer.
  page.stage.at(5000);

  assert_close(&page.stage.opacities(PANEL_A), &[]);
  assert!(!page.stage.tree.has_active_timeline());
}

#[test]
fn a_leaving_layer_takes_no_pointer() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);
  let inside = page.stage.center_of("inside-a");

  pointer_click(&mut page.stage.tree, inside.0, inside.1, MouseButton::Left);
  page.stage.at(300);
  assert_eq!(page.inside_clicks.load(Ordering::SeqCst), 1, "open, it takes the click");

  page.open.set(false);
  page.stage.at(300);
  page.stage.at(400);
  pointer_click(&mut page.stage.tree, inside.0, inside.1, MouseButton::Left);
  page.stage.at(400);

  assert_eq!(page.inside_clicks.load(Ordering::SeqCst), 1);
  assert_eq!(
    page.behind_clicks.load(Ordering::SeqCst),
    1,
    "the click goes to what is under the leaving layer"
  );
}

#[test]
fn a_leaving_layer_keeps_no_focus_and_tab_passes_it_by() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);
  page.stage.tree.get_element_by_id_mut("inside-a").unwrap().focus();
  page.stage.at(300);
  assert_eq!(page.stage.focused_id().as_deref(), Some("inside-a"));

  page.open.set(false);
  page.stage.at(300);
  page.stage.at(400);
  assert_eq!(page.stage.opacities(PANEL_A).len(), 1, "the layer is still painted");
  assert_eq!(page.stage.focused_id(), None, "the focus that was in it is let go");

  for _ in 0..3 {
    page.stage.tab();
    page.stage.at(400);
    assert_eq!(page.stage.focused_id().as_deref(), Some("behind"));
  }
}

#[test]
fn nothing_in_a_leaving_layer_is_found() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);
  assert!(page.stage.tree.get_element_by_id("inside-a").is_some());

  page.open.set(false);
  page.stage.at(300);
  page.stage.at(400);

  assert!(page.stage.tree.get_element_by_id("inside-a").is_none());
  assert!(
    page
      .stage
      .tree
      .find_element(|element| element.text_content() == Some("Inside"))
      .is_none()
  );
}

#[test]
fn a_layer_opened_again_while_it_leaves_enters_over_the_one_leaving() {
  let mut page = one_popup(true);
  page.open.set(true);
  page.stage.at(0);
  page.stage.at(300);
  page.open.set(false);
  page.stage.at(300);
  page.stage.at(350);

  page.open.set(true);
  page.stage.at(350);
  assert_close(&page.stage.opacities(PANEL_A), &[0.75, 0.0]);

  page.stage.at(450);
  assert_close(&page.stage.opacities(PANEL_A), &[0.25, 0.5]);
  let inside = page.stage.center_of("inside-a");
  pointer_click(&mut page.stage.tree, inside.0, inside.1, MouseButton::Left);
  page.stage.at(450);
  assert_eq!(
    page.inside_clicks.load(Ordering::SeqCst),
    1,
    "the open one takes the click"
  );

  page.stage.at(550);
  assert_close(&page.stage.opacities(PANEL_A), &[1.0]);
}

#[test]
fn an_exit_animation_of_the_keyframes_the_content_entered_with_plays_from_its_start() {
  let open = Signal::new(false);
  let anchor = ElementRef::new();
  let mut stage = Stage::new({
    let open = open.clone();
    move || {
      let fade = Animation::new(KeyframesId::new(FADE)).duration_ms(200).linear();
      let content = Rect::new(120.0, 40.0)
        .background(PANEL_A)
        .animation(fade.clone())
        .exit_animation(fade.direction(AnimationDirection::Reverse));
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), content).open(open.clone()))
        .into()
    }
  });
  open.set(true);
  stage.at(0);
  stage.at(300);

  open.set(false);
  stage.at(300);
  stage.at(350);

  assert_close(&stage.opacities(PANEL_A), &[0.75]);
}

#[test]
fn a_popup_opened_by_what_closes_another_enters_while_the_other_leaves() {
  let open_a = Signal::new(false);
  let open_b = Signal::new(false);
  let anchor_a = ElementRef::new();
  let anchor_b = ElementRef::new();
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let (open_a, open_b) = (open_a.clone(), open_b.clone());
    move || {
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor_a.clone()))
        .child(Popup::new(anchor_a.clone(), panel("a", PANEL_A, true, &clicks)).open(open_a.clone()))
        .child(Rect::new(150.0, 30.0).ref_element(anchor_b.clone()))
        .child(Popup::new(anchor_b.clone(), panel("b", PANEL_B, true, &clicks)).open(open_b.clone()))
        .into()
    }
  });
  open_a.set(true);
  stage.at(0);
  stage.at(300);

  open_a.set(false);
  open_b.set(true);
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[1.0]);
  assert_close(&stage.opacities(PANEL_B), &[0.0]);

  stage.at(400);
  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  assert_close(&stage.opacities(PANEL_B), &[0.5]);
  assert!(stage.tree.get_element_by_id("inside-a").is_none());
  assert!(stage.tree.get_element_by_id("inside-b").is_some());

  stage.at(500);
  assert_close(&stage.opacities(PANEL_A), &[]);
  assert_close(&stage.opacities(PANEL_B), &[1.0]);
}

#[test]
fn a_layer_that_stays_open_keeps_its_state_when_one_under_it_closes() {
  let open_a = Signal::new(false);
  let open_b = Signal::new(false);
  let anchor_a = ElementRef::new();
  let anchor_b = ElementRef::new();
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let (open_a, open_b) = (open_a.clone(), open_b.clone());
    move || {
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor_a.clone()))
        .child(Popup::new(anchor_a.clone(), panel("a", PANEL_A, true, &clicks)).open(open_a.clone()))
        .child(Rect::new(150.0, 30.0).ref_element(anchor_b.clone()))
        .child(Popup::new(anchor_b.clone(), panel("b", PANEL_B, true, &clicks)).open(open_b.clone()))
        .into()
    }
  });
  open_a.set(true);
  open_b.set(true);
  stage.at(0);
  stage.at(300);
  stage.tree.get_element_by_id_mut("inside-b").unwrap().focus();
  stage.at(300);

  open_a.set(false);
  stage.at(300);
  stage.at(400);

  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  assert_close(&stage.opacities(PANEL_B), &[1.0]);
  assert_eq!(
    stage.focused_id().as_deref(),
    Some("inside-b"),
    "it is the layer it was: it neither enters again nor loses its focus"
  );
  assert!(
    stage.paint_order(PANEL_A)[0] < stage.paint_order(PANEL_B)[0],
    "the one leaving stays under it, where it was"
  );
}

/// A dialog whose backdrop and panel leave each with its own animation.
fn dialog(open: &Signal<bool>) -> Element {
  let backdrop = Rect::new(400.0, 300.0)
    .background(BACKDROP)
    .exit_animation(Animation::new(KeyframesId::new(FADE_OUT)).duration_ms(100).linear());
  let panel = Rect::new(200.0, 100.0).background(PANEL_A).exit_animation(exit());
  let layer = Stack::new().child(backdrop).child(
    Column::new()
      .child(panel)
      .child(Button::new("Close").id("close").tab_index(0)),
  );
  Column::new()
    .child(Button::new("Page").id("page").size(100.0, 30.0).tab_index(0))
    .child(Modal::new(layer).open(open.clone()).target(Root))
    .into()
}

#[test]
fn each_part_of_a_layer_leaves_with_its_own_animation_and_holds_its_last_pose() {
  let open = Signal::new(true);
  let mut stage = Stage::new({
    let open = open.clone();
    move || dialog(&open)
  });
  stage.at(300);
  assert_close(&stage.opacities(BACKDROP), &[1.0]);

  open.set(false);
  stage.at(300);
  stage.at(350);
  assert_close(&stage.opacities(BACKDROP), &[0.5]);
  assert_close(&stage.opacities(PANEL_A), &[0.75]);

  stage.at(450);
  assert_close(&stage.opacities(BACKDROP), &[0.0]);
  assert_close(&stage.opacities(PANEL_A), &[0.25]);

  stage.at(500);
  assert_close(&stage.opacities(BACKDROP), &[]);
  assert_close(&stage.opacities(PANEL_A), &[]);
}

#[test]
fn a_modal_that_leaves_does_not_trap_tab() {
  let open = Signal::new(true);
  let mut stage = Stage::new({
    let open = open.clone();
    move || dialog(&open)
  });
  stage.at(300);
  stage.tab();
  stage.at(300);
  assert_eq!(
    stage.focused_id().as_deref(),
    Some("close"),
    "open, it is the Tab scope"
  );

  open.set(false);
  stage.at(300);
  stage.at(350);
  assert_eq!(stage.opacities(PANEL_A).len(), 1);
  assert_eq!(stage.focused_id(), None);

  stage.tab();
  stage.at(350);
  assert_eq!(stage.focused_id().as_deref(), Some("page"));
}

#[test]
fn a_popup_inside_a_modal_leaves_over_the_modal() {
  let open = Signal::new(false);
  let anchor = ElementRef::new();
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let open = open.clone();
    move || {
      let layer = Stack::new().child(Rect::new(400.0, 300.0).background(BACKDROP)).child(
        Column::new()
          .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
          .child(Popup::new(anchor.clone(), panel("a", PANEL_A, true, &clicks)).open(open.clone())),
      );
      Column::new().child(Modal::new(layer).target(Root)).into()
    }
  });
  open.set(true);
  stage.at(0);
  stage.at(300);

  open.set(false);
  stage.at(300);
  stage.at(400);

  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  assert!(stage.paint_order(BACKDROP)[0] < stage.paint_order(PANEL_A)[0]);
}

#[test]
fn a_layer_whose_page_is_replaced_leaves_too() {
  let shown = Signal::new(true);
  let anchor = ElementRef::new();
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let shown = shown.clone();
    move || {
      if !shown.get() {
        return Column::new().child(Rect::new(150.0, 30.0)).into();
      }
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), panel("a", PANEL_A, true, &clicks)))
        .into()
    }
  });
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[1.0]);

  shown.set(false);
  stage.at(300);
  stage.at(400);
  assert_close(&stage.opacities(PANEL_A), &[0.5]);

  stage.at(500);
  assert_close(&stage.opacities(PANEL_A), &[]);
}

#[test]
fn a_popup_hidden_with_its_anchor_does_not_leave_and_comes_back_with_it() {
  let open = Signal::new(true);
  let anchor = ElementRef::new();
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let open = open.clone();
    move || {
      let list = Column::new()
        .child(Rect::new(300.0, 60.0))
        .child(Rect::new(120.0, 20.0).ref_element(anchor.clone()))
        .child(Rect::new(300.0, 600.0));
      Column::new()
        .child(Rect::new(300.0, 40.0))
        .child(ScrollVertical::new(list).size(300.0, 200.0))
        .child(Popup::new(anchor.clone(), panel("a", PANEL_A, true, &clicks)).open(open.clone()))
        .into()
    }
  });
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[1.0]);

  // The anchor goes under the header: the popup is open and not drawn.
  stage.tree.scroll(280.0, 200.0, 0.0, -120.0, ScrollPhase::Scroll);
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[]);
  assert!(open.get_untracked());

  stage.tree.scroll(280.0, 200.0, 0.0, 120.0, ScrollPhase::Scroll);
  stage.at(300);
  assert_eq!(stage.opacities(PANEL_A).len(), 1, "back with its anchor");
}

#[test]
fn layers_declared_at_one_place_with_different_keys_are_different_layers() {
  let second = Signal::new(false);
  let anchor = ElementRef::new();
  let mut stage = Stage::new({
    let second = second.clone();
    move || {
      let (key, color) = if second.get() { ("b", PANEL_B) } else { ("a", PANEL_A) };
      let content = Rect::new(120.0, 40.0)
        .background(color)
        .animation(enter())
        .exit_animation(exit())
        .key(key);
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), content))
        .into()
    }
  });
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[1.0]);

  second.set(true);
  stage.at(300);
  stage.at(400);

  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  assert_close(&stage.opacities(PANEL_B), &[0.5]);
}

#[test]
fn what_has_no_exit_animation_goes_on_as_it_was_while_its_layer_leaves() {
  const SPINNER: Color = Color::new(30, 200, 30, 255);
  let open = Signal::new(true);
  let anchor = ElementRef::new();
  let mut stage = Stage::new({
    let open = open.clone();
    move || {
      let spinner = Rect::new(20.0, 20.0).background(SPINNER).animation(
        Animation::new(KeyframesId::new(FADE))
          .duration_ms(1000)
          .linear()
          .infinite(),
      );
      let content = Column::new()
        .child(Rect::new(120.0, 40.0).background(PANEL_A).exit_animation(exit()))
        .child(spinner);
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), content).open(open.clone()))
        .into()
    }
  });
  stage.at(300);
  assert_close(&stage.opacities(SPINNER), &[0.3]);

  open.set(false);
  stage.at(300);
  stage.at(400);

  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  assert_close(&stage.opacities(SPINNER), &[0.4]);
}

/// What the popup of [`Host`] shows: a component of its own, which renders again when `tick` changes.
#[derive(Clone, lurq::DevtoolsInspectable)]
struct HostProps {
  #[devtools_ignore]
  open: Signal<bool>,
  #[devtools_ignore]
  tick: Signal<u32>,
}

impl PartialEq for HostProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

struct Inner;

impl Component for Inner {
  type Props = HostProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let _ = ctx.props::<HostProps>().tick.get();
    Rect::new(120.0, 40.0).background(PANEL_A).exit_animation(exit())
  }
}

/// A page that mounts [`Inner`] for its popup's content in every render, open or closed.
struct Host {
  anchor: ElementRef,
}

impl Component for Host {
  type Props = HostProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      anchor: ElementRef::new(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<HostProps>().clone();
    let content = ctx.mount::<Inner>(props.clone());
    Column::new()
      .child(Rect::new(150.0, 30.0).ref_element(self.anchor.clone()))
      .child(Popup::new(self.anchor.clone(), content).open(props.open))
  }
}

#[test]
fn a_component_that_renders_again_does_not_reach_into_the_layer_it_left() {
  let (open, tick) = (Signal::new(true), Signal::new(0));
  let mut stage = Stage::with_root::<Host>(HostProps {
    open: open.clone(),
    tick: tick.clone(),
  });
  stage.at(300);
  assert_close(&stage.opacities(PANEL_A), &[1.0]);

  open.set(false);
  stage.at(300);
  stage.at(400);
  assert_close(&stage.opacities(PANEL_A), &[0.5]);

  // The component is still mounted, for the closed popup. Its fresh output is not the leaving layer's.
  tick.set(1);
  stage.at(400);
  assert_close(&stage.opacities(PANEL_A), &[0.5]);
  stage.at(450);
  assert_close(&stage.opacities(PANEL_A), &[0.25]);

  stage.at(500);
  assert_close(&stage.opacities(PANEL_A), &[]);
}

// ---------------------------------------------------------------------------
// Children of a container with presence.

const ITEM_A: Color = Color::new(200, 130, 30, 255);
const ITEM_B: Color = Color::new(30, 200, 130, 255);
const ITEM_C: Color = Color::new(130, 30, 200, 255);
const SHRINK: u16 = 74;

impl Stage {
  /// Where each rectangle painted in `color` starts from the top.
  fn tops(&self, color: Color) -> Vec<f32> {
    self
      .quads()
      .iter()
      .filter(|quad| matches!(&quad.content, QuadContent::Rect { color: fill, .. } if *fill == color))
      .map(|quad| quad.y)
      .collect()
  }
}

fn list_item(key: &str, color: Color) -> Rect {
  Rect::new(120.0, 40.0)
    .background(color)
    .exit_animation(exit())
    .key(key.to_owned())
}

/// A column of the items A, B and C, each 40 tall, with B there while `shown` says so.
fn list(shown: &Signal<bool>, presence: bool) -> Stage {
  let shown = shown.clone();
  Stage::new(move || {
    let mut column = Column::new().child(list_item("a", ITEM_A));
    if presence {
      column = column.presence();
    }
    if shown.get() {
      column = column.child(list_item("b", ITEM_B));
    }
    column.child(list_item("c", ITEM_C)).into()
  })
}

#[test]
fn a_child_removed_from_a_container_with_presence_stays_until_its_exit_animation_ends() {
  let shown = Signal::new(true);
  let mut stage = list(&shown, true);
  assert_eq!(stage.tops(ITEM_C), [80.0]);

  shown.set(false);
  stage.at(0);
  assert_close(&stage.opacities(ITEM_B), &[1.0]);
  assert_eq!(stage.tops(ITEM_B), [40.0], "where it was among its siblings");
  assert_eq!(
    stage.tops(ITEM_C),
    [80.0],
    "and in flow: the one after it has not moved"
  );

  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5]);
  assert!(stage.tree.has_active_timeline());

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[]);
  assert_eq!(stage.tops(ITEM_C), [40.0], "gone, and the one after it takes its place");
  assert!(!stage.tree.has_active_timeline());
}

#[test]
fn without_presence_a_removed_child_is_gone_with_the_render_that_removes_it() {
  let shown = Signal::new(true);
  let mut stage = list(&shown, false);

  shown.set(false);
  stage.at(0);

  assert_close(&stage.opacities(ITEM_B), &[]);
  assert_eq!(stage.tops(ITEM_C), [40.0]);
  assert!(!stage.tree.has_active_timeline());
}

#[test]
fn a_child_with_no_key_is_not_kept() {
  let shown = Signal::new(true);
  let mut stage = Stage::new({
    let shown = shown.clone();
    move || {
      let mut column = Column::new()
        .presence()
        .child(Rect::new(120.0, 40.0).background(ITEM_A));
      if shown.get() {
        column = column.child(Rect::new(120.0, 40.0).background(ITEM_B).exit_animation(exit()));
      }
      column.into()
    }
  });

  shown.set(false);
  stage.at(0);

  assert_close(&stage.opacities(ITEM_B), &[]);
}

#[test]
fn a_render_while_a_child_leaves_keeps_it_leaving() {
  let shown = Signal::new(true);
  let tick = Signal::new(0);
  let mut stage = Stage::new({
    let (shown, tick) = (shown.clone(), tick.clone());
    move || {
      let _ = tick.get();
      let mut column = Column::new().presence().child(list_item("a", ITEM_A));
      if shown.get() {
        column = column.child(list_item("b", ITEM_B));
      }
      column.child(list_item("c", ITEM_C)).into()
    }
  });
  shown.set(false);
  stage.at(0);
  stage.at(50);

  tick.set(1);
  stage.at(50);
  assert_close(&stage.opacities(ITEM_B), &[0.75]);
  assert_eq!(stage.tops(ITEM_B), [40.0]);
  assert_eq!(stage.tops(ITEM_C), [80.0]);

  tick.set(2);
  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5]);

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[]);
  assert_eq!(stage.tops(ITEM_C), [40.0]);
}

#[test]
fn a_leaving_child_takes_no_input_and_is_not_found() {
  let shown = Signal::new(true);
  let clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let (shown, clicks) = (shown.clone(), clicks.clone());
    move || {
      let clicks = clicks.clone();
      let mut column = Column::new().presence();
      if shown.get() {
        column = column.child(
          Button::new("Row")
            .id("row")
            .size(120.0, 40.0)
            .tab_index(0)
            .exit_animation(exit())
            .on_click(move |_| {
              clicks.fetch_add(1, Ordering::SeqCst);
            }),
        );
      }
      column
        .child(Button::new("Next").id("next").size(120.0, 40.0).tab_index(0))
        .into()
    }
  });
  stage.tree.get_element_by_id_mut("row").unwrap().focus();
  stage.at(0);
  assert_eq!(stage.focused_id().as_deref(), Some("row"));

  shown.set(false);
  stage.at(0);
  stage.at(100);
  assert!(stage.tree.get_element_by_id("row").is_none());
  assert_eq!(stage.focused_id(), None, "the focus it had is let go");

  pointer_click(&mut stage.tree, 60.0, 20.0, MouseButton::Left);
  stage.at(100);
  assert_eq!(clicks.load(Ordering::SeqCst), 0);

  stage.tab();
  stage.at(100);
  assert_eq!(stage.focused_id().as_deref(), Some("next"));
}

#[test]
fn a_child_added_again_while_it_leaves_is_a_new_one() {
  let shown = Signal::new(true);
  let mut stage = list(&shown, true);
  shown.set(false);
  stage.at(0);
  stage.at(100);

  shown.set(true);
  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5, 1.0]);

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[1.0]);
  assert_eq!(stage.tops(ITEM_C), [80.0]);
}

#[test]
fn an_exit_animation_of_its_height_lets_the_siblings_close_the_gap() {
  let shown = Signal::new(true);
  let mut stage = Stage::new({
    let shown = shown.clone();
    move || {
      let mut column = Column::new().presence().child(list_item("a", ITEM_A));
      if shown.get() {
        column = column.child(
          Rect::new(120.0, 40.0)
            .background(ITEM_B)
            .exit_animation(Animation::new(KeyframesId::new(SHRINK)).duration_ms(200).linear())
            .key("b"),
        );
      }
      column.child(list_item("c", ITEM_C)).into()
    }
  });
  stage.tree.register_keyframes(
    Keyframes::new(KeyframesId::new(SHRINK))
      .frame(0.0, |frame| {
        frame.set(AnimatableProperty::Height, AnimatableValue::Float(40.0));
      })
      .frame(1.0, |frame| {
        frame.set(AnimatableProperty::Height, AnimatableValue::Float(0.0));
      }),
  );

  shown.set(false);
  stage.at(0);
  assert_eq!(stage.tops(ITEM_C), [80.0]);
  stage.at(100);
  assert_eq!(stage.tops(ITEM_C), [60.0], "half of its height is left");
  stage.at(150);
  assert_eq!(stage.tops(ITEM_C), [50.0]);
  stage.at(200);
  assert_eq!(stage.tops(ITEM_C), [40.0]);
}

#[test]
fn the_sibling_after_a_leaving_child_keeps_its_own_state() {
  let shown = Signal::new(true);
  let tick = Signal::new(0);
  let mut stage = Stage::new({
    let (shown, tick) = (shown.clone(), tick.clone());
    move || {
      let _ = tick.get();
      let mut column = Column::new().presence();
      if shown.get() {
        column = column.child(list_item("b", ITEM_B));
      }
      // A scroll container with no key and no id: it is matched by its place among its siblings.
      column
        .child(
          ScrollVertical::new(
            Column::new()
              .child(Rect::new(120.0, 30.0))
              .child(Rect::new(120.0, 100.0).background(ITEM_C)),
          )
          .size(120.0, 60.0),
        )
        .into()
    }
  });
  // The child before it is removed and leaves; the scroll container is scrolled while it does.
  shown.set(false);
  stage.at(0);
  stage.tree.scroll(60.0, 70.0, 0.0, -30.0, ScrollPhase::Scroll);
  stage.at(0);
  assert_eq!(stage.tops(ITEM_C), [40.0], "scrolled by 30: 40 + 30 - 30");

  // A render while the child leaves matches the container with itself, not with the child that is leaving
  // in the place before it.
  tick.set(1);
  stage.at(50);
  assert_eq!(stage.tops(ITEM_C), [40.0], "still scrolled");

  tick.set(2);
  stage.at(200);
  assert_eq!(
    stage.tops(ITEM_C),
    [0.0],
    "and after: it moved up by 40, scrolled as it was"
  );
}

/// The list as a component of its own, which renders again by itself when `open` changes.
struct ListInside;

impl Component for ListInside {
  type Props = HostProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new().presence().child(list_item("a", ITEM_A));
    if ctx.props::<HostProps>().open.get() {
      column = column.child(list_item("b", ITEM_B));
    }
    column.child(list_item("c", ITEM_C))
  }
}

struct ListHost;

impl Component for ListHost {
  type Props = HostProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<HostProps>().clone();
    Column::new()
      .child(Rect::new(120.0, 10.0))
      .child(ctx.mount::<ListInside>(props))
  }
}

#[test]
fn a_child_removed_by_a_component_that_renders_by_itself_leaves_too() {
  let (shown, tick) = (Signal::new(true), Signal::new(0));
  let mut stage = Stage::with_root::<ListHost>(HostProps {
    open: shown.clone(),
    tick,
  });
  assert_eq!(stage.tops(ITEM_C), [90.0]);

  shown.set(false);
  stage.at(0);
  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5]);
  assert_eq!(stage.tops(ITEM_C), [90.0]);

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[]);
  assert_eq!(stage.tops(ITEM_C), [50.0]);
}

#[test]
fn a_child_removed_from_a_list_in_a_popup_leaves_too() {
  let shown = Signal::new(true);
  let anchor = ElementRef::new();
  let mut stage = Stage::new({
    let shown = shown.clone();
    move || {
      let mut menu = Column::new().presence().child(list_item("a", ITEM_A));
      if shown.get() {
        menu = menu.child(list_item("b", ITEM_B));
      }
      Column::new()
        .child(Rect::new(150.0, 30.0).ref_element(anchor.clone()))
        .child(Popup::new(anchor.clone(), menu))
        .into()
    }
  });
  assert_close(&stage.opacities(ITEM_B), &[1.0]);

  shown.set(false);
  stage.at(0);
  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5]);

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[]);
}

#[test]
fn a_child_removed_from_a_list_in_a_dialog_leaves_too() {
  let shown = Signal::new(true);
  let mut stage = Stage::new({
    let shown = shown.clone();
    move || {
      let mut tasks = Column::new().presence().child(list_item("a", ITEM_A));
      if shown.get() {
        tasks = tasks.child(list_item("b", ITEM_B));
      }
      let tasks = tasks.child(list_item("c", ITEM_C));
      Column::new().child(Modal::new(tasks).target(Root)).into()
    }
  });
  assert_eq!(stage.tops(ITEM_C), [80.0]);

  shown.set(false);
  stage.at(0);
  stage.at(100);
  assert_close(&stage.opacities(ITEM_B), &[0.5]);
  assert_eq!(stage.tops(ITEM_C), [80.0]);

  stage.at(200);
  assert_close(&stage.opacities(ITEM_B), &[]);
  assert_eq!(stage.tops(ITEM_C), [40.0]);
}
