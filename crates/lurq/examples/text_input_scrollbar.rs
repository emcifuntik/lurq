//! Multiline text inputs with padding whose text overflows, to see where their scrollbars sit and how the padding
//! scrolls with the text: an overlay bar, a reserved one, one in an input with a border and rounded corners, and
//! an input scrolled to its end, which the example focuses with the caret after the last line. The first three
//! show their text from its first line.
//!
//! cargo run -p lurq --example text_input_scrollbar --features winit,wgpu,screenshot
//!
//! With `--capture <file.png>` the window opens far off screen, saves its own frame to the file once the last
//! input has scrolled, and closes a few seconds later, when the file is written. `--label <text>` replaces the
//! heading, to tell two captures apart.

use std::time::Duration;

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Timeout},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Row, Text, TextInput, TextInputController},
  core::Signal,
  layout::{
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::TextStyle,
  },
  node::{Element, border::Border},
};

const PADDING: f32 = 16.0;
const FIELD_WIDTH: f32 = 200.0;
const ROWS: usize = 5;
const SCROLL_AFTER: Duration = Duration::from_millis(400);
const CAPTURE_AFTER: Duration = Duration::from_millis(1500);
const CLOSE_AFTER: Duration = Duration::from_millis(6000);

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct Options {
  /// The file the window's frame is saved to; empty to stay open.
  capture: String,
  label: String,
}

struct Page {
  values: [Signal<String>; 4],
  /// Set once the inputs are mounted: the first three then put their caret at their start, which shows their text
  /// from its first line, and the last one takes the focus with its caret at its end.
  placed: Signal<bool>,
  controllers: [TextInputController; 4],
  _timers: Vec<Timeout>,
}

fn lines() -> String {
  (1..=14)
    .map(|line| format!("{line:>2}  line of text in the input"))
    .collect::<Vec<_>>()
    .join("\n")
}

fn bar(placement: ScrollBarPlacement) -> ScrollBarStyle {
  ScrollBarStyle {
    thumb_color: "#2563eb".into(),
    track_color: "#bfdbfe".into(),
    width: 6.0,
    placement,
    ..ScrollBarStyle::default()
  }
}

fn field(value: &Signal<String>, style: ScrollBarStyle) -> TextInput {
  TextInput::styled(
    value.clone(),
    TextStyle {
      font_size: 13.0,
      line_height: 1.4,
      ..TextStyle::default()
    },
  )
  .rows_exact(ROWS)
  .width(FIELD_WIDTH)
  .padding(PADDING)
  .background("#ffffff")
  .scrollbar(style)
}

fn card(title: &str, note: &str, input: TextInput) -> Column {
  Column::new()
    .spacing(8.0)
    .width(FIELD_WIDTH)
    .child(Text::new(title))
    .child(input)
    .child(Text::new(note))
}

impl Component for Page {
  type Props = Options;

  fn create(ctx: &mut Ctx) -> Self {
    let options = ctx.props::<Options>().clone();
    let placed = ctx.signal(false);
    let mut timers = Vec::new();
    let mounted = placed.clone();
    timers.push(ctx.create_timeout(SCROLL_AFTER, move || mounted.set(true)));
    if !options.capture.is_empty() {
      let (window, path) = (ctx.window(), options.capture.clone());
      timers.push(ctx.create_timeout(CAPTURE_AFTER, move || window.screenshot(path.clone())));
      let window = ctx.window();
      timers.push(ctx.create_timeout(CLOSE_AFTER, move || window.close()));
    }
    for timer in &timers {
      timer.start();
    }
    Self {
      values: std::array::from_fn(|_| ctx.signal(lines())),
      placed,
      controllers: std::array::from_fn(|_| TextInputController::new()),
      _timers: timers,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    if self.placed.get() {
      for controller in &self.controllers[..3] {
        // An input that is not mounted yet keeps its caret; the next render places it.
        let _ = controller.select(0, 0);
      }
      let end = self.values[3].get_untracked().len();
      if self.controllers[3].select(end, end).is_ok() {
        self.controllers[3].focus(ctx);
      }
    }
    let label = ctx.props::<Options>().label.clone();
    let overlay = bar(ScrollBarPlacement::Overlay);
    Column::new()
      .padding(24.0)
      .spacing(16.0)
      .background("#e5e7eb")
      .child(Text::new(&label))
      .child(Text::new(&format!(
        "Each input: {ROWS} rows, padding {PADDING}, scrollbar width 6, edge inset 2, end inset 2 (12 with the rounded corners)."
      )))
      .child(
        Row::new()
          .spacing(24.0)
          .child(card(
            "1. Overlay",
            "The bar runs over the right padding, 2 px from the edge.",
            field(&self.values[0], overlay.clone()).controller(self.controllers[0].clone()),
          ))
          .child(card(
            "2. Reserved",
            "The bar has its lane at the edge; the padding stays between it and the text.",
            field(&self.values[1], bar(ScrollBarPlacement::Reserved)).controller(self.controllers[1].clone()),
          ))
          .child(card(
            "3. Border, rounded",
            "The bar sits just inside the border; its track ends clear of the corners.",
            field(&self.values[2], overlay.clone().insets(2.0, 12.0))
              .border(Border::inside(2.0, "#111827"))
              .rounded(12.0)
              .controller(self.controllers[2].clone()),
          ))
          .child(card(
            "4. Scrolled to the end",
            "The top padding scrolled away with the text; the bottom padding shows under the last line.",
            field(&self.values[3], overlay).controller(self.controllers[3].clone()),
          )),
      )
  }
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let value_of = |flag: &str| {
    args.iter().position(|arg| arg == flag).map(|at| {
      args
        .get(at + 1)
        .unwrap_or_else(|| panic!("{flag} takes a value"))
        .clone()
    })
  };
  let capture = value_of("--capture").unwrap_or_default();
  let label = value_of("--label").unwrap_or_else(|| "Multiline text inputs: scrollbar and padding".to_owned());

  let position = if capture.is_empty() {
    (120, 120)
  } else {
    (-12000, -12000)
  };

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<Page>(&mut app, Options { capture, label });
  println!("pid {}", std::process::id());

  WinitWindow::new(app, tree)
    .with_size(980, 360)
    .with_position(position.0, position.1)
    .with_start_without_focus(true)
    .with_title("lurq text input scrollbar")
    .run();
}
