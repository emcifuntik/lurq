use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    Tree,
    component::Component,
    ctx::Ctx,
    events::{DragEvent, MouseButton, MouseButtonMask},
  },
  components::{
    DragContainer, DragContainerProps, DragOverridePolicy, Draggable, DraggableProps, DropMissBehavior, DropZone,
    DropZoneProps, Rect, Row,
  },
  core::{ElementRefMut, Signal},
  node::{Element, color::Color},
};

use crate::support::run_pass;

const DRAG_COLOR: Color = Color::new(59, 130, 246, 255);

#[test]
fn drag_move_continues_after_pointer_leaves_source_bounds() {
  let moves = Arc::new(Mutex::new(Vec::new()));
  let captured = moves.clone();

  let mut runtime = Tree::new();
  runtime.set_root(Rect::new(100.0, 100.0).on_drag_move(move |event: DragEvent| {
    captured
      .lock()
      .unwrap()
      .push((event.delta_x, event.delta_y, event.total_delta_x, event.total_delta_y));
  }));

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(200.0, 220.0);

  assert_eq!(*moves.lock().unwrap(), vec![(190.0, 210.0, 190.0, 210.0)]);
}

#[test]
fn drag_end_fires_even_when_pointer_is_released_outside_source_bounds() {
  let ends = Arc::new(Mutex::new(Vec::new()));
  let captured = ends.clone();

  let mut runtime = Tree::new();
  runtime.set_root(Rect::new(100.0, 100.0).on_drag_end(move |event: DragEvent| {
    captured
      .lock()
      .unwrap()
      .push((event.x, event.y, event.total_delta_x, event.total_delta_y));
  }));

  run_pass(&mut runtime);
  runtime.mouse_down(20.0, 25.0, MouseButton::Left);
  runtime.mouse_up(160.0, 175.0, MouseButton::Left);

  assert_eq!(*ends.lock().unwrap(), vec![(160.0, 175.0, 140.0, 150.0)]);
}

#[test]
fn drag_reports_incremental_and_total_deltas() {
  let moves = Arc::new(Mutex::new(Vec::new()));
  let captured = moves.clone();

  let mut runtime = Tree::new();
  runtime.set_root(Rect::new(100.0, 100.0).on_drag_move(move |event: DragEvent| {
    captured
      .lock()
      .unwrap()
      .push((event.delta_x, event.delta_y, event.total_delta_x, event.total_delta_y));
  }));

  run_pass(&mut runtime);
  runtime.mouse_down(5.0, 8.0, MouseButton::Left);
  runtime.mouse_move(15.0, 18.0);
  runtime.mouse_move(35.0, 23.0);

  assert_eq!(
    *moves.lock().unwrap(),
    vec![(10.0, 10.0, 10.0, 10.0), (20.0, 5.0, 30.0, 15.0)]
  );
}

#[test]
fn drag_start_button_mask_filters_pointer_buttons() {
  let starts = Arc::new(Mutex::new(Vec::new()));
  let captured = starts.clone();

  let mut runtime = Tree::new();
  runtime.set_root(
    Rect::new(100.0, 100.0)
      .start_drag_buttons(MouseButtonMask::LEFT)
      .on_drag_start(move |event: DragEvent| captured.lock().unwrap().push(event.button)),
  );

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Right);
  runtime.mouse_move(30.0, 30.0);
  runtime.mouse_up(30.0, 30.0, MouseButton::Right);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);

  assert_eq!(*starts.lock().unwrap(), vec![MouseButton::Left]);
}

#[test]
fn drop_dispatches_to_hit_drop_target_on_release() {
  let drops = Arc::new(Mutex::new(Vec::new()));

  let mut runtime = Tree::new();
  runtime.mount_root::<DropDispatch>(&mut lurq::app::App::new(), Shared(drops.clone()));

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_up(90.0, 20.0, MouseButton::Left);

  assert_eq!(*drops.lock().unwrap(), vec![(90.0, 20.0, 80.0, 10.0, true, true)]);
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Shared<T>(Arc<T>);

impl<T> PartialEq for Shared<T> {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

impl<T> std::fmt::Debug for Shared<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_tuple("Shared").field(&(Arc::as_ptr(&self.0) as usize)).finish()
  }
}

struct DropDispatch {
  // A log of the fields of each drop event, compared whole.
  #[allow(clippy::type_complexity)]
  drops: Arc<Mutex<Vec<(f32, f32, f32, f32, bool, bool)>>>,
}

impl Component for DropDispatch {
  type Props = Shared<Mutex<Vec<(f32, f32, f32, f32, bool, bool)>>>;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      drops: ctx.props::<Self::Props>().0.clone(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let drop_zone = DropZone::mount(
      ctx,
      DropZoneProps::new().on_drop({
        let drops = self.drops.clone();
        move |event| {
          drops.lock().unwrap().push((
            event.x,
            event.y,
            event.total_delta_x,
            event.total_delta_y,
            event.source_id.is_assigned(),
            event.target_id.is_assigned(),
          ));
        }
      }),
      Rect::new(80.0, 50.0),
    );

    Row::new()
      .spacing(20.0)
      .child(Rect::new(50.0, 50.0).on_drag_move(|_| {}))
      .child(drop_zone)
  }
}

struct DragRerender {
  status: Signal<&'static str>,
  moves: Arc<Mutex<Vec<f32>>>,
}

impl Component for DragRerender {
  type Props = Shared<Mutex<Vec<f32>>>;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      status: ctx.signal("Idle"),
      moves: ctx.props::<Self::Props>().0.clone(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let _status = self.status.get();

    let draggable = Draggable::mount(
      ctx,
      DraggableProps::new()
        .on_drag_start({
          let status = self.status.clone();
          move |_| status.set("Dragging")
        })
        .on_drag_move({
          let moves = self.moves.clone();
          move |event: &DragEvent| moves.lock().unwrap().push(event.delta_x)
        }),
      Rect::new(50.0, 50.0).background(DRAG_COLOR).absolute_position(0.0, 0.0),
    );

    DragContainer::mount(
      ctx,
      DragContainerProps::new(),
      lurq::components::Stack::new().size(240.0, 80.0).child(draggable),
    )
  }
}

#[test]
fn signal_driven_rerender_does_not_cancel_active_drag() {
  let moves = Arc::new(Mutex::new(Vec::new()));
  let mut runtime = Tree::new();
  runtime.mount_root::<DragRerender>(&mut lurq::app::App::new(), Shared(moves.clone()));

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(20.0, 10.0);
  runtime.mouse_move(180.0, 10.0);

  assert_eq!(*moves.lock().unwrap(), vec![10.0, 160.0]);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 170.0);
}

struct BoundedDrag;

impl Component for BoundedDrag {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let draggable = Draggable::mount(
      ctx,
      DraggableProps::new(),
      Rect::new(50.0, 50.0).background(DRAG_COLOR).absolute_position(0.0, 0.0),
    );

    DragContainer::mount(
      ctx,
      DragContainerProps::new(),
      lurq::components::Stack::new().size(240.0, 80.0).child(draggable),
    )
  }
}

#[test]
fn drag_container_clamps_draggable_to_container_bounds() {
  let mut runtime = Tree::new();
  runtime.mount_root::<BoundedDrag>(&mut lurq::app::App::new(), ());

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(400.0, 100.0);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 190.0);
  assert_eq!(dragged.bounds().y, 30.0);
}

struct ExternalRefDrag;

impl Component for ExternalRefDrag {
  type Props = Shared<ElementRefMut>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let element_ref = (*ctx.props::<Self::Props>().0).clone();
    let draggable = Draggable::mount(
      ctx,
      DraggableProps::new()
        .element_ref(element_ref)
        .override_policy(DragOverridePolicy::Clear),
      Rect::new(50.0, 50.0).background(DRAG_COLOR).absolute_position(0.0, 0.0),
    );

    lurq::components::Stack::new().size(240.0, 80.0).child(draggable)
  }
}

#[test]
fn external_element_ref_drives_drag_and_clear_policy_restores_layout_position() {
  let element_ref = ElementRefMut::new();
  let mut runtime = Tree::new();
  runtime.mount_root::<ExternalRefDrag>(&mut lurq::app::App::new(), Shared(Arc::new(element_ref.clone())));

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(40.0, 30.0);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 30.0);
  assert_eq!(dragged.bounds().y, 20.0);
  assert_eq!(element_ref.bounds().relative_x, 30.0);

  // The drop handler would commit the position into app state here; the
  // Clear policy hands layout authority back at drag end.
  runtime.mouse_up(40.0, 30.0, MouseButton::Left);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 0.0);
  assert_eq!(dragged.bounds().y, 0.0);
}

#[test]
fn drag_that_moved_suppresses_the_synthesized_click() {
  let clicks = Arc::new(Mutex::new(0u32));
  let captured = clicks.clone();

  let mut runtime = Tree::new();
  runtime.set_root(
    Rect::new(100.0, 100.0)
      .on_drag_move(|_| {})
      .on_click(move |_| *captured.lock().unwrap() += 1),
  );

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(40.0, 30.0);
  runtime.mouse_up(40.0, 30.0, MouseButton::Left);

  assert_eq!(*clicks.lock().unwrap(), 0);
}

#[test]
fn stationary_press_on_a_drag_source_still_clicks() {
  let clicks = Arc::new(Mutex::new(0u32));
  let captured = clicks.clone();

  let mut runtime = Tree::new();
  runtime.set_root(
    Rect::new(100.0, 100.0)
      .on_drag_move(|_| {})
      .on_click(move |_| *captured.lock().unwrap() += 1),
  );

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_up(10.0, 10.0, MouseButton::Left);

  assert_eq!(*clicks.lock().unwrap(), 1);
}

struct PayloadDrag {
  received: Arc<Mutex<Vec<(String, f32)>>>,
}

impl Component for PayloadDrag {
  type Props = Shared<Mutex<Vec<(String, f32)>>>;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      received: ctx.props::<Self::Props>().0.clone(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let draggable = Draggable::mount(
      ctx,
      DraggableProps::new().payload("widget-7".to_owned()),
      Rect::new(50.0, 50.0),
    );

    let drop_zone = DropZone::mount(
      ctx,
      DropZoneProps::new().on_drop({
        let received = self.received.clone();
        move |event| {
          let payload = event
            .payload::<String>()
            .cloned()
            .unwrap_or_else(|| "<missing>".to_owned());
          received.lock().unwrap().push((payload, event.total_delta_x));
        }
      }),
      Rect::new(80.0, 50.0),
    );

    Row::new().spacing(20.0).child(draggable).child(drop_zone)
  }
}

#[test]
fn drop_event_delivers_the_drag_sources_payload() {
  let received = Arc::new(Mutex::new(Vec::new()));
  let mut runtime = Tree::new();
  runtime.mount_root::<PayloadDrag>(&mut lurq::app::App::new(), Shared(received.clone()));

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_up(90.0, 20.0, MouseButton::Left);

  assert_eq!(*received.lock().unwrap(), vec![("widget-7".to_owned(), 80.0)]);
}

struct FollowerDrag {
  revert_on_miss: bool,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct FollowerDragProps {
  #[devtools_ignore]
  follower: ElementRefMut,
  revert_on_miss: bool,
}

impl std::fmt::Debug for FollowerDragProps {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("FollowerDragProps")
      .field("revert_on_miss", &self.revert_on_miss)
      .finish()
  }
}

impl PartialEq for FollowerDragProps {
  fn eq(&self, other: &Self) -> bool {
    self.revert_on_miss == other.revert_on_miss && std::ptr::eq(&self.follower, &other.follower)
  }
}

impl Component for FollowerDrag {
  type Props = FollowerDragProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      revert_on_miss: ctx.props::<Self::Props>().revert_on_miss,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let follower = ctx.props::<Self::Props>().follower.clone();
    let mut props = DraggableProps::new().follower(follower.clone());
    if self.revert_on_miss {
      props = props.drop_miss_behavior(DropMissBehavior::RevertToDragStart);
    }
    let draggable = Draggable::mount(
      ctx,
      props,
      Rect::new(50.0, 50.0).background(DRAG_COLOR).absolute_position(0.0, 0.0),
    );

    lurq::components::Stack::new()
      .size(400.0, 280.0)
      .child(draggable)
      .child(
        Rect::new(40.0, 40.0)
          .absolute_position(150.0, 10.0)
          .ref_element(follower.clone()),
      )
  }
}

#[test]
fn followers_move_in_lockstep_with_the_dragged_element() {
  let follower = ElementRefMut::new();
  let mut runtime = Tree::new();
  runtime.mount_root::<FollowerDrag>(
    &mut lurq::app::App::new(),
    FollowerDragProps {
      follower: follower.clone(),
      revert_on_miss: false,
    },
  );

  run_pass(&mut runtime);
  assert_eq!(follower.bounds().relative_x, 150.0);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(40.0, 30.0);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 30.0);
  assert_eq!(dragged.bounds().y, 20.0);
  assert_eq!(follower.bounds().relative_x, 180.0);
  assert_eq!(follower.bounds().relative_y, 30.0);
}

#[test]
fn followers_revert_with_the_dragged_element_on_a_missed_drop() {
  let follower = ElementRefMut::new();
  let mut runtime = Tree::new();
  runtime.mount_root::<FollowerDrag>(
    &mut lurq::app::App::new(),
    FollowerDragProps {
      follower: follower.clone(),
      revert_on_miss: true,
    },
  );

  run_pass(&mut runtime);
  runtime.mouse_down(10.0, 10.0, MouseButton::Left);
  runtime.mouse_move(40.0, 30.0);
  runtime.mouse_up(40.0, 30.0, MouseButton::Left);

  run_pass(&mut runtime);
  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 0.0);
  assert_eq!(dragged.bounds().y, 0.0);
  assert_eq!(follower.bounds().relative_x, 150.0);
  assert_eq!(follower.bounds().relative_y, 10.0);
}

struct DemoBoundedDrag {
  status: Signal<&'static str>,
}

impl Component for DemoBoundedDrag {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      status: ctx.signal("Idle"),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let _status = self.status.get();
    let draggable = Draggable::mount(
      ctx,
      DraggableProps::new().on_drag_start({
        let status = self.status.clone();
        move |_| status.set("Dragging")
      }),
      Row::new()
        .align_items(lurq::layout::Alignment::Center)
        .justify(lurq::layout::layout_kind::Justify::Center)
        .size(120.0, 80.0)
        .background(DRAG_COLOR)
        .absolute_position(20.0, 40.0),
    );

    DragContainer::mount(
      ctx,
      DragContainerProps::new(),
      lurq::components::Stack::new().size(400.0, 280.0).child(draggable),
    )
  }
}

#[test]
fn drag_container_clamp_survives_drag_start_rerender() {
  let mut runtime = Tree::new();
  runtime.mount_root::<DemoBoundedDrag>(&mut lurq::app::App::new(), ());

  run_pass(&mut runtime);
  runtime.mouse_down(30.0, 50.0, MouseButton::Left);
  run_pass(&mut runtime);
  runtime.mouse_move(700.0, 500.0);
  run_pass(&mut runtime);
  runtime.mouse_move(900.0, 700.0);

  let dragged = runtime
    .find_element(|element| element.color() == Some(DRAG_COLOR))
    .unwrap();
  assert_eq!(dragged.bounds().x, 280.0);
  assert_eq!(dragged.bounds().y, 200.0);
}
