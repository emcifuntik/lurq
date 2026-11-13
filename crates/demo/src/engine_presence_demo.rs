//! Presence by hand: menus and a dialog that enter and leave, and a list whose items leave when removed.

use lurq::{
  animation::{AnimatableProperty, Animation, Keyframes, KeyframesId},
  app::{Tree, component::Component, ctx::Ctx},
  components::{Column, Modal, Popup, Rect, Root, Row, Stack},
  core::{ElementRef, Signal},
  layout::{Alignment, StackAlignment, text_style::FontWeight},
  node::{BoxShadow, CursorIcon, Element, color::Color, transform::Decomposed},
};

use crate::{
  engine_kit::{FILL, body, card, chip, controls, counted, note, page, slider, toggle},
  style::{BORDER, SURFACE_DARK, TEXT, text},
};

const MENU_IN: KeyframesId = KeyframesId::new(40);
const MENU_OUT: KeyframesId = KeyframesId::new(41);
const FADE_IN: KeyframesId = KeyframesId::new(42);
const FADE_OUT: KeyframesId = KeyframesId::new(43);
const PANEL_IN: KeyframesId = KeyframesId::new(44);
const PANEL_OUT: KeyframesId = KeyframesId::new(45);
/// A row that fades.
const ROW_FADE: KeyframesId = KeyframesId::new(46);
/// A row that fades and then gives its room to the rows after it.
const ROW_CLOSE: KeyframesId = KeyframesId::new(47);
const ROW_HEIGHT: f32 = 36.0;

pub(crate) fn register_keyframes(tree: &mut Tree) {
  let slide = |opacity: f32, y: f32| {
    move |frame: &mut lurq::animation::KeyframeBuilder| {
      frame.set(AnimatableProperty::Opacity, opacity);
      frame.set(AnimatableProperty::OffsetY, y);
    }
  };
  let grow = |opacity: f32, scale: f32| {
    move |frame: &mut lurq::animation::KeyframeBuilder| {
      frame.set(AnimatableProperty::Opacity, opacity);
      frame.set(
        AnimatableProperty::Transform,
        Decomposed::IDENTITY.with_scale(scale, scale),
      );
    }
  };
  let fade = |opacity: f32| {
    move |frame: &mut lurq::animation::KeyframeBuilder| {
      frame.set(AnimatableProperty::Opacity, opacity);
    }
  };
  let row = |opacity: f32, height: f32| {
    move |frame: &mut lurq::animation::KeyframeBuilder| {
      frame.set(AnimatableProperty::Opacity, opacity);
      frame.set(AnimatableProperty::Height, height);
    }
  };
  tree.register_keyframes(
    Keyframes::new(MENU_IN)
      .frame(0.0, slide(0.0, -10.0))
      .frame(1.0, slide(1.0, 0.0)),
  );
  tree.register_keyframes(
    Keyframes::new(MENU_OUT)
      .frame(0.0, slide(1.0, 0.0))
      .frame(1.0, slide(0.0, -10.0)),
  );
  tree.register_keyframes(Keyframes::new(FADE_IN).frame(0.0, fade(0.0)).frame(1.0, fade(1.0)));
  tree.register_keyframes(Keyframes::new(FADE_OUT).frame(0.0, fade(1.0)).frame(1.0, fade(0.0)));
  tree.register_keyframes(
    Keyframes::new(PANEL_IN)
      .frame(0.0, grow(0.0, 0.9))
      .frame(1.0, grow(1.0, 1.0)),
  );
  tree.register_keyframes(
    Keyframes::new(PANEL_OUT)
      .frame(0.0, grow(1.0, 1.0))
      .frame(1.0, grow(0.0, 0.9)),
  );
  tree.register_keyframes(Keyframes::new(ROW_FADE).frame(0.0, fade(1.0)).frame(1.0, fade(0.0)));
  tree.register_keyframes(
    Keyframes::new(ROW_CLOSE)
      .frame(0.0, row(1.0, ROW_HEIGHT))
      .frame(0.5, row(0.0, ROW_HEIGHT))
      .frame(1.0, row(0.0, 0.0)),
  );
}

pub(crate) struct PresenceDemo {
  file: Signal<bool>,
  edit: Signal<bool>,
  dialog: Signal<bool>,
  tools: Signal<bool>,
  page_clicks: Signal<u32>,
  /// How long an animation takes, in milliseconds: slow, to see a layer on its way.
  duration: Signal<i32>,
  tasks: Signal<Vec<(u32, String)>>,
  tools_items: Signal<Vec<&'static str>>,
  next_task: Signal<u32>,
  /// Whether a removed row gives its room away as it leaves, or only fades.
  closes_up: Signal<bool>,
  with_presence: Signal<bool>,
  renders: Signal<u32>,
  file_anchor: ElementRef,
  edit_anchor: ElementRef,
  tools_anchor: ElementRef,
}

impl PresenceDemo {
  fn play(&self, keyframes: KeyframesId) -> Animation {
    Animation::new(keyframes)
      .duration_ms(self.duration.get().max(1) as u64)
      .linear()
  }

  fn menu_panel(&self) -> Column {
    Column::new()
      .width(190.0)
      .padding(6.0)
      .spacing(2.0)
      .background("#f8fafc")
      .rounded(8.0)
      .box_shadow(BoxShadow::new(0.0, 10.0, 28.0, Color::new(0, 0, 0, 110)))
      .animation(self.play(MENU_IN))
      .exit_animation(self.play(MENU_OUT))
  }

  fn menu_item(caption: &str) -> Row {
    Row::new()
      .width(FILL)
      .padding_horizontal(10.0)
      .height(30.0)
      .align_items(Alignment::Center)
      .rounded(5.0)
      .hovered(|style| style.background("#e2e8f0"))
      .child(text(caption, 13.0, FontWeight::Normal, "#0f172a"))
  }

  fn layers(&self) -> Element {
    let opener = |caption: &str, anchor: &ElementRef, open: &Signal<bool>, other: &Signal<bool>| {
      let (open, other) = (open.clone(), other.clone());
      chip(caption, open.get(), move || {
        other.set(false);
        open.update(|open| *open = !*open);
      })
      // Focusable, as a menu bar's buttons are: a press on it while the other menu is open closes that one and
      // still reaches it, so one press switches menus.
      .tab_index(0)
      .ref_element(anchor.clone())
    };
    let menu = |anchor: &ElementRef, open: &Signal<bool>, items: [&str; 4]| {
      let mut panel = self.menu_panel();
      for item in items {
        panel = panel.child(Self::menu_item(item));
      }
      Popup::new(anchor.clone(), panel).open(open.clone()).offset(0.0, 6.0)
    };
    let open_dialog = self.dialog.clone();
    let clicks = self.page_clicks.clone();
    card("Menus and a dialog")
      .child(slider("Duration, ms", &self.duration, 60, 2000))
      .child(controls([
        Element::from(opener("File", &self.file_anchor, &self.file, &self.edit)),
        opener("Edit", &self.edit_anchor, &self.edit, &self.file).into(),
        chip("Dialog", false, move || open_dialog.set(true)).into(),
        counted("clicks on the area below", self.page_clicks.get()).into(),
      ]))
      .child(
        Column::new()
          .width(FILL)
          .height(150.0)
          .padding(14.0)
          .background(SURFACE_DARK)
          .rounded(8.0)
          .cursor(CursorIcon::Pointer)
          .on_click(move |_| clicks.update(|count| *count += 1))
          .child(body(
            "Click here. A click where a menu is still leaving counts: the menu takes nothing.",
          )),
      )
      .child(note(
        "Open File, then click Edit: File leaves while Edit enters. Close a menu with Escape or a click outside. With \
         a long duration, press Tab while a menu leaves: focus does not go into it.",
      ))
      .child(menu(&self.file_anchor, &self.file, ["New", "Open…", "Save", "Close"]))
      .child(menu(&self.edit_anchor, &self.edit, ["Undo", "Cut", "Copy", "Paste"]))
      .child(self.dialog_layer())
      .into()
  }

  fn dialog_layer(&self) -> Modal {
    let close = self.dialog.clone();
    let backdrop = Rect::new(FILL, FILL)
      .background(Color::new(0, 0, 0, 150))
      .animation(self.play(FADE_IN))
      .exit_animation(self.play(FADE_OUT));
    let panel = Column::new()
      .width(340.0)
      .padding(24.0)
      .spacing(14.0)
      .background("#f8fafc")
      .rounded(12.0)
      .box_shadow(BoxShadow::new(0.0, 18.0, 50.0, Color::new(0, 0, 0, 120)))
      .animation(self.play(PANEL_IN))
      .exit_animation(self.play(PANEL_OUT))
      .child(text("A dialog", 18.0, FontWeight::Bold, "#0f172a"))
      .child(text(
        "Its backdrop fades. Its panel fades and grows. Each leaves with its own animation.",
        13.0,
        FontWeight::Normal,
        "#334155",
      ))
      .child(chip("Close", true, move || close.set(false)).tab_index(0));
    Modal::new(
      Stack::new()
        .size(FILL, FILL)
        .stack_align(StackAlignment::Center)
        .child(backdrop)
        .child(panel),
    )
    .open(self.dialog.clone())
    .target(Root)
  }

  fn task_row(&self, id: u32, title: &str) -> Row {
    let tasks = self.tasks.clone();
    let leave = if self.closes_up.get() { ROW_CLOSE } else { ROW_FADE };
    Row::new()
      .key(format!("task-{id}"))
      .width(FILL)
      .height(ROW_HEIGHT)
      .padding_horizontal(12.0)
      .align_items(Alignment::Center)
      .border_bottom(lurq::node::border::Border::inside(1.0, BORDER))
      .cursor(CursorIcon::Pointer)
      .hovered(|style| style.background("#1e293b"))
      .exit_animation(self.play(leave))
      .on_click(move |_| tasks.update(|tasks| tasks.retain(|(kept, _)| *kept != id)))
      .child(text(title, 13.0, FontWeight::Normal, TEXT))
  }

  fn list(&self) -> Element {
    let mut list = Column::new().width(FILL);
    if self.with_presence.get() {
      list = list.presence();
    }
    for (id, title) in self.tasks.get() {
      list = list.child(self.task_row(id, &title));
    }
    let (tasks, next) = (self.tasks.clone(), self.next_task.clone());
    let add = move || {
      let id = next.get();
      next.set(id + 1);
      tasks.update(|tasks| tasks.insert(tasks.len().min(1), (id, format!("Task {id}"))));
    };
    let (first, all, renders) = (self.tasks.clone(), self.tasks.clone(), self.renders.clone());
    card("A list: click a task to remove it")
      .child(controls([
        Element::from(chip("Add a task", false, add)),
        chip("Remove the first", false, move || {
          first.update(|tasks| {
            if !tasks.is_empty() {
              tasks.remove(0);
            }
          })
        })
        .into(),
        chip("Remove every other", false, move || {
          all.update(|tasks| {
            let mut index = 0;
            tasks.retain(|_| {
              index += 1;
              index % 2 == 0
            });
          })
        })
        .into(),
        chip("Render again", false, move || renders.update(|count| *count += 1)).into(),
        counted("renders", self.renders.get()).into(),
      ]))
      .child(controls([
        toggle("the list has presence", &self.with_presence),
        toggle("a row gives its room away", &self.closes_up),
      ]))
      .child(
        Column::new()
          .width(360.0)
          .background(SURFACE_DARK)
          .rounded(8.0)
          .border_inside(1.0, Color::from_hex(BORDER))
          .child(list),
      )
      .child(note(
        "A removed task stays where it was until its animation ends, and takes no click. Remove several at once, add \
         one while another leaves, and render again while one leaves: it goes on from where it was. Without presence \
         a removed task is gone at once.",
      ))
      .into()
  }

  fn list_in_a_menu(&self) -> Element {
    let open = self.tools.clone();
    let mut items = Column::new().presence().width(FILL);
    for item in self.tools_items.get() {
      let kept = self.tools_items.clone();
      items = items.child(
        Self::menu_item(item)
          .key(item)
          .cursor(CursorIcon::Pointer)
          .exit_animation(self.play(ROW_CLOSE))
          .on_click(move |_| kept.update(|items| items.retain(|other| *other != item))),
      );
    }
    let restore = self.tools_items.clone();
    card("A list in a menu")
      .child(controls([
        Element::from(
          chip("Tools", open.get(), move || open.update(|open| *open = !*open)).ref_element(self.tools_anchor.clone()),
        ),
        chip("Put the items back", false, move || {
          restore.set(vec!["Format", "Lint", "Test", "Bench", "Publish"])
        })
        .into(),
      ]))
      .child(note(
        "Open Tools and click an item: it leaves, and the menu gets shorter once it is gone, not before.",
      ))
      .child(
        Popup::new(self.tools_anchor.clone(), self.menu_panel().child(items))
          .open(self.tools.clone())
          .offset(0.0, 6.0),
      )
      .into()
  }
}

impl Component for PresenceDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let tasks = [
      "Write the plan",
      "Draw the icons",
      "Build the engine",
      "Check on a Mac",
      "Ship it",
    ];
    Self {
      file: ctx.signal(false),
      edit: ctx.signal(false),
      dialog: ctx.signal(false),
      tools: ctx.signal(false),
      page_clicks: ctx.signal(0),
      duration: ctx.signal(400),
      tasks: ctx.signal(
        tasks
          .into_iter()
          .enumerate()
          .map(|(index, title)| (index as u32, title.to_owned()))
          .collect(),
      ),
      tools_items: ctx.signal(vec!["Format", "Lint", "Test", "Bench", "Publish"]),
      next_task: ctx.signal(tasks.len() as u32),
      closes_up: ctx.signal(true),
      with_presence: ctx.signal(true),
      renders: ctx.signal(0),
      file_anchor: ElementRef::new(),
      edit_anchor: ElementRef::new(),
      tools_anchor: ElementRef::new(),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    // Read here so that "Render again" renders this component.
    let _ = self.renders.get();
    page(
      "Presence",
      "Make the duration long and watch a layer or a row on its way out. What is leaving is only painted: it takes no \
       click, no focus and no Tab.",
    )
    .child(self.layers())
    .child(self.list())
    .child(self.list_in_a_menu())
  }
}
