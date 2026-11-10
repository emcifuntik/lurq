//! Multiline text inputs with padding whose text overflows, to see where their scrollbars sit and how the padding
//! scrolls with the text: an overlay bar, a reserved one, one in an input with a border and rounded corners, and
//! an input scrolled to its end, which the example focuses with the caret after the last line. The first three
//! show their text from its first line. A second row has an input and a virtualized list whose bars show only
//! while the pointer is over them or they scroll (`ScrollBarVisibility::OnHover`).
//!
//! cargo run -p lurq --example text_input_scrollbar --features winit,wgpu,screenshot
//!
//! With `--capture <file.png>` the window opens far off screen, saves its own frame to the file once the last
//! input has scrolled, and closes when the file is written. `--label <text>` replaces the
//! heading, to tell two captures apart. `--hover input` or `--hover list` moves the tree's pointer over the
//! input or the list of the second row before the capture, with no input sent to the desktop.

use std::{
  sync::atomic::{AtomicU64, Ordering},
  time::{Duration, Instant},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Interval, Timeout},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Row, Text, TextInput, TextInputController, VirtualizedList},
  core::Signal,
  layout::{
    scrollbar::{ScrollBarPlacement, ScrollBarStyle, ScrollBarVisibility},
    text_style::TextStyle,
  },
  node::{Element, border::Border},
};

const PADDING: f32 = 16.0;
const FIELD_WIDTH: f32 = 200.0;
const ROWS: usize = 5;
const HOVER_INPUT: &str = "hover-input";
const HOVER_LIST: &str = "hover-list";
const SCROLL_AFTER: Duration = Duration::from_millis(400);
/// After the bars that the first scroll showed have faded.
const HOVER_AFTER: Duration = Duration::from_millis(1800);
const CAPTURE_AFTER: Duration = Duration::from_millis(4500);
/// How often the capture's file is looked at: the window closes once the file is written.
const FILE_CHECK_EVERY: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct Options {
  /// The file the window's frame is saved to; empty to stay open.
  capture: String,
  label: String,
}

struct Page {
  values: [Signal<String>; 5],
  /// Set once the inputs are mounted: the first three then put their caret at their start, which shows their text
  /// from its first line, and the last one takes the focus with its caret at its end.
  placed: Signal<bool>,
  controllers: [TextInputController; 5],
  /// Set when the pointer is due over the second row, which wakes the window's loop for it.
  hover_due: Signal<bool>,
  _timers: Vec<Timeout>,
  _close_when_saved: Option<Interval>,
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

fn card(title: &str, note: &str, shown: impl Into<Element>) -> Column {
  Column::new()
    .spacing(8.0)
    .width(FIELD_WIDTH)
    .child(Text::new(title))
    .child(shown)
    .child(Text::new(note))
}

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct ListRow {
  number: usize,
}

struct ListRowView;

impl Component for ListRowView {
  type Props = ListRow;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let number = ctx.props::<ListRow>().number;
    Row::new()
      .width(FIELD_WIDTH)
      .padding(6.0)
      .child(Text::new(&format!("Row {number} of the list")))
  }
}

impl Component for Page {
  type Props = Options;

  fn create(ctx: &mut Ctx) -> Self {
    let options = ctx.props::<Options>().clone();
    let placed = ctx.signal(false);
    let mut timers = Vec::new();
    let mounted = placed.clone();
    timers.push(ctx.create_timeout(SCROLL_AFTER, move || mounted.set(true)));
    let hover_due = ctx.signal(false);
    let due = hover_due.clone();
    timers.push(ctx.create_timeout(HOVER_AFTER, move || due.set(true)));
    if !options.capture.is_empty() {
      let (window, path) = (ctx.window(), options.capture.clone());
      timers.push(ctx.create_timeout(CAPTURE_AFTER, move || window.screenshot(path.clone())));
    }
    let close_when_saved = (!options.capture.is_empty()).then(|| {
      // The frame is encoded on another thread after the capture; its file is whole once it stops growing.
      let (window, path, seen) = (ctx.window(), options.capture.clone(), AtomicU64::new(0));
      let check = ctx.create_interval(FILE_CHECK_EVERY, move || {
        let size = std::fs::metadata(&path).map_or(0, |file| file.len());
        if size > 0 && seen.swap(size, Ordering::Relaxed) == size {
          window.close();
        }
      });
      check.start();
      check
    });
    for timer in &timers {
      timer.start();
    }
    Self {
      values: std::array::from_fn(|_| ctx.signal(lines())),
      placed,
      controllers: std::array::from_fn(|_| TextInputController::new()),
      hover_due,
      _timers: timers,
      _close_when_saved: close_when_saved,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    if self.placed.get() {
      for controller in self.controllers[..3].iter().chain(&self.controllers[4..]) {
        // An input that is not mounted yet keeps its caret; the next render places it.
        let _ = controller.select(0, 0);
      }
      let end = self.values[3].get_untracked().len();
      if self.controllers[3].select(end, end).is_ok() {
        self.controllers[3].focus(ctx);
      }
    }
    // Read so that the timer's change draws a frame, which the pointer's move comes before.
    self.hover_due.get();
    let label = ctx.props::<Options>().label.clone();
    let overlay = bar(ScrollBarPlacement::Overlay);
    let on_hover = ScrollBarStyle {
      visible: ScrollBarVisibility::OnHover,
      ..overlay.clone()
    };
    let rows: Vec<ListRow> = (1..=40).map(|number| ListRow { number }).collect();
    let list = VirtualizedList::new(ctx, rows)
      .size(FIELD_WIDTH, 124.0)
      .scrollbar(on_hover.clone())
      .mount_keyed::<ListRowView, _, _, _>(|row| row.number, |row| row.clone());
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
      .child(
        Row::new()
          .spacing(24.0)
          .child(card(
            "5. On hover: input",
            "The bar shows while the pointer is over the input or its text scrolls, then fades out.",
            field(&self.values[4], on_hover)
              .id(HOVER_INPUT)
              .controller(self.controllers[4].clone()),
          ))
          .child(card(
            "6. On hover: list",
            "A virtualized list with the same bar.",
            Column::new().id(HOVER_LIST).background("#ffffff").child(list),
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
  if !capture.is_empty() && std::path::Path::new(&capture).exists() {
    // The window closes when the capture's file is written, so a file from an earlier run must not be there.
    std::fs::remove_file(&capture).expect("the earlier capture can be replaced");
  }
  let label = value_of("--label").unwrap_or_else(|| "Multiline text inputs: scrollbar and padding".to_owned());

  let position = if capture.is_empty() {
    (120, 120)
  } else {
    (-12000, -12000)
  };
  let hover = value_of("--hover").map(|target| match target.as_str() {
    "input" => HOVER_INPUT,
    "list" => HOVER_LIST,
    other => panic!("unknown --hover {other}: use input or list"),
  });

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<Page>(&mut app, Options { capture, label });
  println!("pid {}", std::process::id());

  let started_at = Instant::now();
  let mut hover = hover;
  WinitWindow::new(app, tree)
    .with_size(980, 640)
    .on_tick(move |tree, _| {
      if started_at.elapsed() < HOVER_AFTER {
        return;
      }
      let Some(id) = hover.take() else {
        return;
      };
      let scale = tree.scale_factor();
      let bounds = tree
        .get_element_by_id_mut(id)
        .and_then(|element| element.bounds())
        .expect("the second row is laid out");
      // The tree takes the pointer in physical pixels.
      tree.mouse_move(
        (bounds.x + bounds.width / 2.0) * scale,
        (bounds.y + bounds.height / 2.0) * scale,
      );
    })
    .with_position(position.0, position.1)
    .with_start_without_focus(true)
    .with_title("lurq text input scrollbar")
    .run();
}
