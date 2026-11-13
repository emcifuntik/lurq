//! Presence in a window: two menus and a dialog whose content enters with the animation it plays from its mount
//! and leaves with `.exit_animation(...)`. A closed layer stays on screen until its exit animations end.
//!
//! cargo run -p lurq --example presence --features winit,wgpu,screenshot
//!
//! Click **File** and **Edit** to switch menus: the one that closes leaves while the other enters. **Dialog**
//! opens a dialog whose backdrop fades while its panel fades and grows. A click on the page counts, also where
//! a layer is still leaving.
//!
//! With `--capture <dir>` the window runs a script on slow animations: it opens and
//! closes the layers, saves a frame at each step to `<dir>` and writes `passes.txt` there with how many layout
//! passes ran while the layers left and after they were gone, then closes.

use std::{
  path::PathBuf,
  sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use lurq::{
  animation::{AnimatableProperty, Animation, Keyframes, KeyframesId},
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Timeout},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Button, Column, Modal, Popup, Rect, Root, Row, Stack, Text},
  core::{ElementRef, Signal},
  layout::StackAlignment,
  node::{BoxShadow, Element, border::Border, color::Color, dimension::Dimension, transform::Decomposed},
};

const MENU_IN: KeyframesId = KeyframesId::new(1);
const MENU_OUT: KeyframesId = KeyframesId::new(2);
const FADE_IN: KeyframesId = KeyframesId::new(3);
const FADE_OUT: KeyframesId = KeyframesId::new(4);
const PANEL_IN: KeyframesId = KeyframesId::new(5);
const PANEL_OUT: KeyframesId = KeyframesId::new(6);
const FILL: Dimension = Dimension::Pct(100.0);
/// The script waits for the window's first frames: the renderer takes a second or two to come up.
const SCRIPT_STARTS_AFTER: Duration = Duration::from_millis(3000);

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct Options {
  /// The directory the script saves its frames to; empty to stay open and be clicked.
  capture: String,
}

struct Page {
  file: Signal<bool>,
  edit: Signal<bool>,
  dialog: Signal<bool>,
  page_clicks: Signal<u32>,
  file_anchor: ElementRef,
  edit_anchor: ElementRef,
  /// How long each animation takes: slow under the script, so a frame can be saved part way.
  duration: u64,
  /// Layout passes so far.
  passes: Arc<AtomicUsize>,
  _script: Vec<Timeout>,
}

fn register_keyframes(tree: &mut Tree) {
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
}

impl Page {
  fn play(&self, keyframes: KeyframesId) -> Animation {
    Animation::new(keyframes).duration_ms(self.duration).linear()
  }

  fn menu(&self, anchor: &ElementRef, open: &Signal<bool>, items: [&str; 3], fill: &str) -> Popup {
    let mut list = Column::new()
      .width(180.0)
      .padding(8.0)
      .spacing(6.0)
      .background(fill)
      .rounded(8.0)
      .border(Border::inside(1.0, "#1f2937"))
      .box_shadow(BoxShadow::new(0.0, 8.0, 24.0, Color::new(0, 0, 0, 80)))
      .animation(self.play(MENU_IN))
      .exit_animation(self.play(MENU_OUT));
    for item in items {
      list = list.child(Text::new(item));
    }
    Popup::new(anchor.clone(), list).open(open.clone())
  }

  fn dialog(&self) -> Modal {
    let close = self.dialog.clone();
    let backdrop = Rect::new(FILL, FILL)
      .background(Color::new(0, 0, 0, 150))
      .animation(self.play(FADE_IN))
      .exit_animation(self.play(FADE_OUT));
    let panel = Column::new()
      .width(320.0)
      .padding(24.0)
      .spacing(16.0)
      .background("#ffffff")
      .rounded(12.0)
      .box_shadow(BoxShadow::new(0.0, 16.0, 48.0, Color::new(0, 0, 0, 100)))
      .animation(self.play(PANEL_IN))
      .exit_animation(self.play(PANEL_OUT))
      .child(Text::new("A dialog"))
      .child(Text::new("Its backdrop fades; its panel fades and grows."))
      .child(Button::new("Close").on_click(move |_| close.set(false)));
    let layer = Stack::new()
      .size(FILL, FILL)
      .stack_align(StackAlignment::Center)
      .child(backdrop)
      .child(panel);
    Modal::new(layer).open(self.dialog.clone()).target(Root)
  }
}

/// The steps of the script: when, in milliseconds from the script's start, and what.
enum Step {
  Set(Signal<bool>, bool),
  Shot(&'static str),
  /// Writes how many layout passes ran since the last note under this label.
  Note(&'static str),
  Close,
}

impl Component for Page {
  type Props = Options;

  fn create(ctx: &mut Ctx) -> Self {
    let capture = ctx.props::<Options>().capture.clone();
    let (file, edit, dialog) = (ctx.signal(false), ctx.signal(false), ctx.signal(false));
    let passes = Arc::new(AtomicUsize::new(0));
    let mut script = Vec::new();
    if !capture.is_empty() {
      let steps = [
        (500, Step::Set(file.clone(), true)),
        (700, Step::Shot("01-menu-entering")),
        (1200, Step::Shot("02-menu-open")),
        (1390, Step::Note("the menu was open")),
        (1400, Step::Set(file.clone(), false)),
        (1400, Step::Set(edit.clone(), true)),
        (1600, Step::Shot("03-one-leaves-one-enters")),
        (2100, Step::Shot("04-second-menu-open")),
        (2290, Step::Note("one menu left as the other entered")),
        (2300, Step::Set(edit.clone(), false)),
        (2500, Step::Shot("05-menu-leaving")),
        (3000, Step::Shot("06-menu-gone")),
        (3190, Step::Note("the menu left")),
        (3200, Step::Set(dialog.clone(), true)),
        (3400, Step::Shot("07-dialog-entering")),
        (3900, Step::Shot("08-dialog-open")),
        (4090, Step::Note("the dialog entered")),
        (4100, Step::Set(dialog.clone(), false)),
        (4300, Step::Shot("09-dialog-leaving")),
        (4800, Step::Shot("10-dialog-gone")),
        (5300, Step::Note("the dialog left")),
        (6800, Step::Note("1.5 s with every layer gone")),
        (7000, Step::Close),
      ];
      let directory = PathBuf::from(&capture);
      let last_note = Arc::new(AtomicUsize::new(0));
      let notes = Arc::new(Mutex::new(String::new()));
      for (at, step) in steps {
        let window = ctx.window();
        let (directory, passes, last_note, notes) =
          (directory.clone(), passes.clone(), last_note.clone(), notes.clone());
        let timeout = ctx.create_timeout(SCRIPT_STARTS_AFTER + Duration::from_millis(at), move || match &step {
          Step::Set(signal, open) => signal.set(*open),
          Step::Shot(name) => window.screenshot(directory.join(format!("{name}.png"))),
          Step::Note(label) => {
            let now = passes.load(Ordering::Relaxed);
            let since = now - last_note.swap(now, Ordering::Relaxed);
            let mut notes = notes.lock().expect("one thread");
            notes.push_str(&format!("{at} ms: {since} passes while {label}\n"));
            std::fs::write(directory.join("passes.txt"), notes.as_bytes()).expect("the notes can be written");
          }
          Step::Close => window.close(),
        });
        timeout.start();
        script.push(timeout);
      }
    }
    Self {
      file,
      edit,
      dialog,
      page_clicks: ctx.signal(0),
      file_anchor: ElementRef::new(),
      edit_anchor: ElementRef::new(),
      duration: if capture.is_empty() { 180 } else { 400 },
      passes,
      _script: script,
    }
  }

  fn after_layout(&self) {
    self.passes.fetch_add(1, Ordering::Relaxed);
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let toggle = |open: &Signal<bool>, other: &Signal<bool>| {
      let (open, other) = (open.clone(), other.clone());
      move |_| {
        other.set(false);
        open.update(|open| *open = !*open);
      }
    };
    let open_dialog = self.dialog.clone();
    let clicks = self.page_clicks.clone();
    let toolbar = Row::new()
      .spacing(8.0)
      .child(
        Button::new("File")
          .ref_element(self.file_anchor.clone())
          .on_click(toggle(&self.file, &self.edit)),
      )
      .child(
        Button::new("Edit")
          .ref_element(self.edit_anchor.clone())
          .on_click(toggle(&self.edit, &self.file)),
      )
      .child(Button::new("Dialog").on_click(move |_| open_dialog.set(true)));
    Column::new()
      .size(FILL, FILL)
      .padding(24.0)
      .spacing(16.0)
      .background("#f4f4f5")
      .child(toolbar)
      .child(Text::new(&format!("page clicks: {}", self.page_clicks.get())))
      .child(
        Column::new()
          .size(FILL, 260.0)
          .padding(16.0)
          .background("#dbeafe")
          .rounded(8.0)
          .on_click(move |_| clicks.update(|count| *count += 1))
          .child(Text::new(
            "The page. A click here counts, also under a layer that is leaving.",
          )),
      )
      .child(self.menu(&self.file_anchor, &self.file, ["New", "Open", "Save"], "#fef3c7"))
      .child(self.menu(&self.edit_anchor, &self.edit, ["Undo", "Cut", "Paste"], "#dcfce7"))
      .child(self.dialog())
  }
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let capture = args
    .iter()
    .position(|arg| arg == "--capture")
    .map(|at| args.get(at + 1).expect("--capture takes a directory").clone())
    .unwrap_or_default();
  if !capture.is_empty() {
    std::fs::create_dir_all(&capture).expect("the capture directory can be made");
  }
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  register_keyframes(&mut tree);
  tree.mount_root::<Page>(&mut app, Options { capture });

  WinitWindow::new(app, tree)
    .with_size(640, 440)
    // On screen under the script too: a window the system counts as hidden draws no frames.
    .with_position(120, 120)
    .with_start_without_focus(true)
    .with_title("lurq presence")
    .run();
}
