//! A row of a list that a finger swipes aside for what can be done with it, as a phone's lists have.
//!
//! [`SwipeRow`] goes around a row's content. Swiped towards the leading edge it shows its trailing actions,
//! and the other way its leading ones; let go far enough it stays open, a press on an action does it and
//! closes the row, and a swipe all the way does the first action at once.

use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;

use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::Ctx,
    events::{GesturePhase, PanEvent},
    window::Haptic,
  },
  components::{Row, Stack, Text},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
};

const FILL: Dimension = Dimension::Pct(100.0);
/// No finger has the row.
const NO_DRAG: f32 = f32::MIN;

/// Something to do with a row, shown beside it when it is swiped.
#[derive(Clone)]
pub struct SwipeAction {
  pub title: String,
  /// What stands over the title, or in its place when there is no title.
  pub icon: Option<Element>,
  pub background: Color,
  pub color: Color,
  pub on_press: Arc<dyn Fn() + Send + Sync>,
}

impl SwipeAction {
  pub fn new(title: impl Into<String>, on_press: impl Fn() + Send + Sync + 'static) -> Self {
    Self {
      title: title.into(),
      icon: None,
      background: Color::from_hex("#8E8E93"),
      color: Color::from_hex("#FFFFFF"),
      on_press: Arc::new(on_press),
    }
  }

  /// An action that takes the row away: the platform's red.
  pub fn destructive(title: impl Into<String>, on_press: impl Fn() + Send + Sync + 'static) -> Self {
    Self::new(title, on_press).background(Color::from_hex("#FF3B30"))
  }

  pub fn background(mut self, color: Color) -> Self {
    self.background = color;
    self
  }

  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  pub fn icon(mut self, icon: impl Into<Element>) -> Self {
    self.icon = Some(icon.into());
    self
  }
}

/// How a [`SwipeRow`] looks and moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwipeRowStyle {
  /// How wide each action is.
  pub action_width: f32,
  pub title_size: f32,
  /// A row swiped this share of its width does its first action at once; more than 1 never does.
  pub full_swipe: f32,
  /// How far ahead of where a finger let the row go it is taken to be going, in seconds of its speed.
  pub throw: f32,
  /// How long the row takes to open, close or go back. It follows a spring.
  pub duration: Duration,
  /// Whether the device ticks when a swipe goes far enough to do the first action.
  pub haptic: bool,
}

impl Default for SwipeRowStyle {
  fn default() -> Self {
    Self {
      action_width: 74.0,
      title_size: 15.0,
      full_swipe: 0.6,
      throw: 0.15,
      duration: Duration::from_millis(350),
      haptic: true,
    }
  }
}

#[derive(Clone, Default)]
pub struct SwipeRowProps {
  leading: Vec<SwipeAction>,
  trailing: Vec<SwipeAction>,
  style: SwipeRowStyle,
  open: Option<Signal<i8>>,
}

impl PartialEq for SwipeRowProps {
  fn eq(&self, _other: &Self) -> bool {
    // The actions are closures: a row is made again with its parent.
    false
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for SwipeRowProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl SwipeRowProps {
  pub fn new() -> Self {
    Self::default()
  }

  /// The actions shown at the row's trailing edge, the first of them at the edge: the one a swipe all the
  /// way does.
  pub fn trailing(mut self, actions: impl IntoIterator<Item = SwipeAction>) -> Self {
    self.trailing = actions.into_iter().collect();
    self
  }

  /// The actions shown at the row's leading edge, the first of them at the edge.
  pub fn leading(mut self, actions: impl IntoIterator<Item = SwipeAction>) -> Self {
    self.leading = actions.into_iter().collect();
    self
  }

  pub fn style(mut self, style: SwipeRowStyle) -> Self {
    self.style = style;
    self
  }

  /// Which side of the row is open, to read and to set: 0 for neither, -1 for the trailing actions and 1 for
  /// the leading ones. A list that lets one row be open at a time closes the others with it.
  pub fn open(mut self, open: Signal<i8>) -> Self {
    self.open = Some(open);
    self
  }
}

struct Held {
  /// Where the fingers were when they took the row, and where the row was then.
  from: f32,
  base: f32,
}

/// A row of a list with actions a swipe shows.
pub struct SwipeRow {
  /// Which side is open, for a row whose props name no signal.
  open: Signal<i8>,
  /// How far a finger has the row from where it rests, or [`NO_DRAG`].
  drag: Signal<f32>,
  held: Arc<Mutex<Option<Held>>>,
  /// The speed a finger let the row go with, towards where it is going.
  velocity: Arc<Mutex<f64>>,
  /// The row as it is laid out: how wide it is, and how high, which its actions are too.
  row: ElementRef,
  height: Signal<f32>,
}

impl SwipeRow {
  /// Around `content`, which is as high as the row is and paints its own background.
  pub fn mount(ctx: &mut Ctx, props: SwipeRowProps, content: impl Into<Element>) -> Element {
    ctx.mount_with::<Self>(props, vec![content.into()])
  }

  /// The same, told apart from its siblings by `key`: a row of a list that changes.
  pub fn mount_keyed(ctx: &mut Ctx, key: &str, props: SwipeRowProps, content: impl Into<Element>) -> Element {
    ctx.mount_keyed_with::<Self>(key, props, vec![content.into()])
  }
}

impl Component for SwipeRow {
  type Props = SwipeRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      open: Signal::new(0),
      drag: Signal::new(NO_DRAG),
      held: Arc::default(),
      velocity: Arc::default(),
      row: ElementRef::default(),
      height: Signal::new(0.0),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<SwipeRowProps>().clone();
    let style = props.style;
    let open = props.open.clone().unwrap_or_else(|| self.open.clone());
    let side = open.get();
    let drag = self.drag.get();
    let height = self.height.get();
    let (leading, trailing) = (
      props.leading.len() as f32 * style.action_width,
      props.trailing.len() as f32 * style.action_width,
    );
    // Where the content rests, and where a finger has it: past the actions it gives way less.
    let rest = match side {
      1 => leading,
      -1 => -trailing,
      _ => 0.0,
    };
    let dragged = drag != NO_DRAG;
    let at = if dragged { drag } else { rest };
    let pace = if dragged { Duration::ZERO } else { style.duration };
    let curve = Easing::Spring {
      velocity: *self.velocity.lock(),
    };

    let close = {
      let open = open.clone();
      move || {
        if open.get_untracked() != 0 {
          open.set(0);
        }
      }
    };
    // The actions of a side, the first at the edge, as wide together as the content is out of the way.
    let actions = |list: &[SwipeAction], at_end: bool, shown: f32| {
      let mut row = Row::new().height(FILL);
      let ordered: Vec<&SwipeAction> = if at_end {
        list.iter().rev().collect()
      } else {
        list.iter().collect()
      };
      let each = if list.is_empty() {
        0.0
      } else {
        (shown / list.len() as f32).max(style.action_width)
      };
      for action in ordered {
        let (press, close) = (action.on_press.clone(), close.clone());
        let mut inside = crate::components::Column::new()
          .align_items(Alignment::Center)
          .spacing(4.0);
        if let Some(icon) = &action.icon {
          inside = inside.child(icon.clone());
        }
        if !action.title.is_empty() {
          inside = inside.child(
            Text::styled(
              &action.title,
              TextStyle {
                font_size: style.title_size,
                weight: FontWeight::Medium,
                color: action.color,
                ..TextStyle::default()
              },
            )
            .nowrap(),
          );
        }
        row = row.child(
          Row::new()
            .width(each)
            .height(FILL)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .background(action.background)
            .child(inside)
            .on_click(move |_| {
              close();
              press();
            }),
        );
      }
      row
    };

    let mut content = Stack::new()
      .width(FILL)
      .transform(Transform2D::translate(at, 0.0))
      .transition(Transition::transform().duration(pace).easing(curve));
    for child in ctx.children() {
      content = content.child(child.clone());
    }
    let content = content
      // A press on an open row closes it, and is not the row's own.
      .on_click({
        let close = close.clone();
        let open = open.clone();
        move |event: crate::app::events::MouseEvent| {
          if open.get_untracked() != 0 {
            event.stop_propagation();
            close();
          }
        }
      })
      .on_pan({
        let (held, drag, velocity) = (self.held.clone(), self.drag.clone(), self.velocity.clone());
        let (open, row) = (open.clone(), self.row.clone());
        let (first_leading, first_trailing) = (
          props.leading.first().map(|action| action.on_press.clone()),
          props.trailing.first().map(|action| action.on_press.clone()),
        );
        let window = ctx.window();
        move |event: PanEvent| match event.phase {
          GesturePhase::Start => {
            if event.fingers != 1 || event.translation_x.abs() <= event.translation_y.abs() {
              return;
            }
            // A row with nothing on the side it is swiped to is not the row's to take.
            let towards = if event.translation_x < 0.0 { trailing } else { leading };
            if towards <= 0.0 && rest == 0.0 {
              return;
            }
            // The row's: the finger does not scroll the list, nor click.
            event.prevent_default();
            event.stop_propagation();
            *held.lock() = Some(Held {
              from: event.translation_x,
              base: rest,
            });
            drag.set(rest);
          }
          GesturePhase::Change => {
            let held = held.lock();
            if let Some(held) = &*held {
              event.stop_propagation();
              let x = held.base + event.translation_x - held.from;
              // Past its actions the row gives way less; with none on that side, hardly at all.
              let limit = if x < 0.0 { trailing } else { leading };
              let over = (x.abs() - limit).max(0.0);
              let give = if limit > 0.0 { 1.0 } else { 0.15 };
              drag.set(x.signum() * (x.abs().min(limit) + over * give));
            }
          }
          GesturePhase::End => {
            let Some(held) = held.lock().take() else {
              return;
            };
            event.stop_propagation();
            let x = held.base + event.translation_x - held.from;
            let width = row.width().max(1.0);
            let thrown = x + event.velocity_x * style.throw;
            // All the way: the first action of that side, at once.
            let all_the_way = x.abs() > width * style.full_swipe;
            let goes: i8 = if thrown < -trailing / 2.0 && trailing > 0.0 {
              -1
            } else if thrown > leading / 2.0 && leading > 0.0 {
              1
            } else {
              0
            };
            let target = match goes {
              1 => leading,
              -1 => -trailing,
              _ => 0.0,
            };
            let left = target - x;
            *velocity.lock() = if left.abs() > 1.0 {
              f64::from((event.velocity_x * style.duration.as_secs_f32() / left).max(0.0))
            } else {
              0.0
            };
            drag.set(NO_DRAG);
            if all_the_way && goes != 0 {
              let first = if goes < 0 { &first_trailing } else { &first_leading };
              if open.get_untracked() != 0 {
                open.set(0);
              }
              if style.haptic {
                window.haptic(Haptic::Medium);
              }
              if let Some(first) = first {
                first();
              }
            } else if open.get_untracked() != goes {
              open.set(goes);
            }
          }
        }
      });

    Stack::new()
      .ref_element(self.row.clone())
      .width(FILL)
      .clip()
      .child(
        // As high as the content was last laid out: the row is as high as its content, and its actions
        // fill it.
        Row::new()
          .width(FILL)
          .height(height)
          .child(actions(&props.leading, false, at.max(0.0)))
          .child(Row::new().flex(1.0))
          .child(actions(&props.trailing, true, (-at).max(0.0))),
      )
      .child(content)
  }
  fn after_layout(&self) {
    if self.row.is_attached() {
      let height = self.row.rect().3;
      if (self.height.get_untracked() - height).abs() > 0.25 {
        self.height.set(height);
      }
    }
  }
}
