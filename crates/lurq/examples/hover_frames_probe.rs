//! A scrolling list the pointer sweeps over, to measure the frames a hover costs. With `plain` the rows have no
//! hover style, so the pointer moving over them changes nothing on screen; with `hovered` each row has one, so
//! every row the pointer comes onto has to be drawn; with `virtualized` the rows have none and are those of a
//! `VirtualizedList`, which keeps an element ref on each. The pointer goes down and up the first 20 rows, half a
//! second each way, as 125 moves a second through `Tree::mouse_move`, for the given number of sweeps (default
//! 20). The sweeps are recorded in a profiling session; the example prints its pid, closes itself and prints the
//! pointer moves it made, the frames that were presented meanwhile and how far apart they came.
//!
//! The window opens far off screen, where the real pointer cannot come over it; `--visible` opens it on the
//! primary display. A run in which the real pointer came over the window says so, and does not count. The shell
//! polls its loop while `on_tick` is set, so this measures frames, not CPU time.
//!
//! cargo run -p lurq --release --example hover_frames_probe --features winit,wgpu,perf_profile -- plain 20

use std::{
  sync::atomic::{AtomicBool, Ordering},
  time::{Duration, Instant},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Interval},
    profiler::{InputKind, ProfilingHandle, RecordingOptions, SampleData, SessionId},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Rect, Row, ScrollVertical, Text, VirtualizedList},
  layout::Alignment,
  node::Element,
};

const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 600.0;
const HEADING_HEIGHT: f32 = 40.0;
const ROW_HEIGHT: f32 = 28.0;
/// More rows than fit, so the list scrolls and shows its scrollbar.
const ROWS: usize = 40;
/// The rows the pointer sweeps over: the ones in view.
const SWEPT_ROWS: usize = 20;
/// Where the pointer runs: through the rows' text, away from the scrollbar.
const POINTER_X: f32 = 200.0;
/// How long the pointer takes from the first swept row to the last, or back.
const SWEEP: Duration = Duration::from_millis(500);
/// What a mouse commonly reports in a second.
const MOVES_PER_SECOND: f64 = 125.0;
/// The window's first frames are over before the sweeps start.
const SETTLE: Duration = Duration::from_secs(1);
/// The frames the last moves asked for are presented before the recording ends.
const DRAIN: Duration = Duration::from_millis(200);

/// Set once the results are printed; the page's interval then closes the window.
static FINISHED: AtomicBool = AtomicBool::new(false);

/// What the list is made of.
#[derive(Clone, Copy, Debug, PartialEq, lurq::DevtoolsInspectable)]
enum Rows {
  Plain,
  Hovered,
  Virtualized,
}

impl Rows {
  fn name(self) -> &'static str {
    match self {
      Self::Plain => "plain",
      Self::Hovered => "hovered",
      Self::Virtualized => "virtualized",
    }
  }
}

/// A row of the list: a dot and two texts, with some of the row's own height above and below them.
fn row(index: usize, hover_style: bool) -> Row {
  let row = Row::new()
    .id(format!("row-{index}"))
    .width(WIDTH)
    .height(ROW_HEIGHT)
    .padding_horizontal(12.0)
    .spacing(10.0)
    .align_items(Alignment::Center)
    .child(Rect::new(16.0, 16.0).background("#2563eb").rounded(8.0))
    .child(Text::new(&format!("Row {index}")).width(120.0))
    .child(Text::new("a line of text that stays as it is"));
  if hover_style {
    row.hovered(|style| style.background("#1f2937"))
  } else {
    row
  }
}

/// A row of the virtualized list.
struct PlainRow;

impl Component for PlainRow {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    row(*ctx.props::<usize>(), false)
  }
}

struct RowsPage {
  _close: Interval,
}

impl Component for RowsPage {
  type Props = Rows;

  fn create(ctx: &mut Ctx) -> Self {
    let window = ctx.window();
    // Changes nothing shown, so it asks for no frame of its own.
    let close = ctx.create_interval(Duration::from_millis(50), move || {
      if FINISHED.load(Ordering::Relaxed) {
        window.close();
      }
    });
    close.start();
    Self { _close: close }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let rows = *ctx.props::<Rows>();
    let (width, height) = (WIDTH, HEIGHT - HEADING_HEIGHT);
    let list: Element = match rows {
      Rows::Plain | Rows::Hovered => {
        let mut column = Column::new().spacing(0.0);
        for index in 0..ROWS {
          column = column.child(row(index, rows == Rows::Hovered));
        }
        ScrollVertical::new(column).size(width, height).into()
      }
      Rows::Virtualized => VirtualizedList::new(ctx, 0..ROWS)
        .size(width, height)
        .mount_keyed::<PlainRow, _, _, _>(|index| *index, |index| *index),
    };
    Column::new()
      .spacing(0.0)
      .child(
        Row::new()
          .height(HEADING_HEIGHT)
          .padding_horizontal(12.0)
          .align_items(Alignment::Center)
          .child(Text::new(match rows {
            Rows::Plain => "Rows without a hover style",
            Rows::Hovered => "Rows with a hover style",
            Rows::Virtualized => "Rows without a hover style, in a virtualized list",
          })),
      )
      .child(list)
  }
}

/// The pointer's sweeps, driven from the shell's tick.
struct Sweeps {
  profiling: ProfilingHandle,
  rows: Rows,
  sweeps: u32,
  opened: Instant,
  stage: Stage,
  /// Pointer moves made, and how many of them came onto another row.
  moves: u64,
  row_changes: u64,
  last_row: Option<usize>,
}

enum Stage {
  Settling,
  Sweeping {
    session: SessionId,
    started: Instant,
    /// The top of the first swept row and the bottom of the last, in logical pixels.
    span: (f32, f32),
    /// The moves due so far, made or skipped.
    due: u64,
  },
  Draining {
    session: SessionId,
    since: Instant,
  },
  Finished,
}

impl Sweeps {
  fn tick(&mut self, tree: &mut Tree) {
    match self.stage {
      Stage::Settling => {
        if self.opened.elapsed() < SETTLE {
          return;
        }
        let top = row_bounds(tree, 0).y;
        let last = row_bounds(tree, SWEPT_ROWS - 1);
        let session = self
          .profiling
          .start_recording(RecordingOptions::default())
          .expect("a profiling session")
          .id;
        self.stage = Stage::Sweeping {
          session,
          started: Instant::now(),
          span: (top, last.y + last.height),
          due: 0,
        };
      }
      Stage::Sweeping {
        session,
        started,
        span,
        due,
      } => {
        let elapsed = started.elapsed();
        if elapsed >= SWEEP * self.sweeps {
          self.stage = Stage::Draining {
            session,
            since: Instant::now(),
          };
          return;
        }
        // One move for each interval of the mouse's rate. A tick that comes late makes the latest one only, as
        // the OS merges the moves an application did not take in time.
        let now_due = (elapsed.as_secs_f64() * MOVES_PER_SECOND) as u64 + 1;
        if now_due == due {
          return;
        }
        self.stage = Stage::Sweeping {
          session,
          started,
          span,
          due: now_due,
        };
        let y = sweep_position(span, (now_due - 1) as f64 / MOVES_PER_SECOND);
        let row = ((y - span.0) / ROW_HEIGHT) as usize;
        self.moves += 1;
        if self.last_row.replace(row) != Some(row) {
          self.row_changes += 1;
        }
        let scale = tree.scale_factor();
        tree.mouse_move(POINTER_X * scale, y * scale);
      }
      Stage::Draining { session, since } => {
        if since.elapsed() < DRAIN {
          return;
        }
        self.report(session);
        self.stage = Stage::Finished;
        FINISHED.store(true, Ordering::Relaxed);
      }
      Stage::Finished => {}
    }
  }

  fn report(&self, session: SessionId) {
    let report = self.profiling.end(session).expect("the recording to end");
    let mut frames = 0_u64;
    let mut unpresented = 0_u64;
    // The frames of each sweep; the ones presented after the last move count for the last sweep.
    let mut per_sweep = vec![0_u32; self.sweeps as usize];
    let mut intervals = Vec::new();
    let mut refresh = None;
    let (mut pointer_inputs, mut left_a_frame) = (0_u64, 0_u64);
    for sample in &report.samples {
      match &sample.data {
        SampleData::Pass(pass) if pass.rendered => {
          frames += 1;
          let sweep = ((sample.completed_ms - report.started_ms) / (SWEEP.as_secs_f64() * 1000.0)) as usize;
          per_sweep[sweep.min(self.sweeps as usize - 1)] += 1;
          if let Some(present) = pass.present {
            intervals.extend(present.interval);
            refresh = present.refresh_interval.or(refresh);
          }
        }
        SampleData::Pass(_) => unpresented += 1,
        SampleData::InputDispatch(input) if input.kind == InputKind::Pointer => {
          pointer_inputs += 1;
          left_a_frame += u64::from(input.presented_frame_id.is_some());
        }
        _ => {}
      }
    }
    per_sweep.sort();

    let seconds = (SWEEP * self.sweeps).as_secs_f64();
    println!(
      "{}: {} sweeps in {seconds:.1} s, {} pointer moves, {} of them onto another row",
      self.rows.name(),
      self.sweeps,
      self.moves,
      self.row_changes
    );
    println!(
      "frames presented: {frames}, {:.1} a second; a sweep: median {}, least {}, most {}; passes that presented \
       nothing: {unpresented}",
      frames as f64 / seconds,
      per_sweep[per_sweep.len() / 2],
      per_sweep[0],
      per_sweep[per_sweep.len() - 1]
    );
    println!("pointer moves that left a frame to draw: {left_a_frame} of {pointer_inputs}");
    if pointer_inputs != self.moves {
      println!(
        "{} pointer moves were not the probe's: the real pointer came over the window, so this run does not count",
        pointer_inputs.saturating_sub(self.moves)
      );
    }
    if !intervals.is_empty() {
      intervals.sort();
      let ms = |at: f64| intervals[((intervals.len() - 1) as f64 * at) as usize].as_secs_f64() * 1000.0;
      println!(
        "frame intervals: median {:.2} ms, p1 {:.2} ms, p99 {:.2} ms, max {:.2} ms; display refresh {}",
        ms(0.5),
        ms(0.01),
        ms(0.99),
        ms(1.0),
        refresh.map_or("unknown".to_owned(), |refresh| format!(
          "{:.2} ms",
          refresh.as_secs_f64() * 1000.0
        ))
      );
    }
  }
}

fn row_bounds(tree: &mut Tree, index: usize) -> lurq::core::ElementRect {
  tree
    .get_element_by_id_mut(&format!("row-{index}"))
    .and_then(|row| row.bounds())
    .expect("the row is laid out")
}

/// Where the pointer is `seconds` into the sweeps: down the span in one [`SWEEP`], back up in the next.
fn sweep_position((top, bottom): (f32, f32), seconds: f64) -> f32 {
  let sweeps = seconds / SWEEP.as_secs_f64();
  let along = if (sweeps as u64).is_multiple_of(2) {
    sweeps.fract()
  } else {
    1.0 - sweeps.fract()
  };
  // Half a pixel inside the span at either end.
  top + 0.5 + along as f32 * (bottom - top - 1.0)
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let rows = match args.first().map(String::as_str) {
    Some("plain") | None => Rows::Plain,
    Some("hovered") => Rows::Hovered,
    Some("virtualized") => Rows::Virtualized,
    Some(other) => panic!("unknown rows {other}: use plain, hovered or virtualized"),
  };
  let sweeps = args
    .get(1)
    .filter(|value| !value.starts_with("--"))
    .map(|value| value.parse().expect("the number of sweeps"))
    .unwrap_or(20);
  assert!(sweeps > 0, "at least one sweep");
  let position = if args.iter().any(|arg| arg == "--visible") {
    (120, 120)
  } else {
    (-12000, -12000)
  };

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<RowsPage>(&mut app, rows);
  println!("pid {}", std::process::id());

  let mut driver = Sweeps {
    profiling: tree.profiling_handle(),
    rows,
    sweeps,
    opened: Instant::now(),
    stage: Stage::Settling,
    moves: 0,
    row_changes: 0,
    last_row: None,
  };
  WinitWindow::new(app, tree)
    .with_size(WIDTH as u32, HEIGHT as u32)
    .with_position(position.0, position.1)
    .with_start_without_focus(true)
    .with_title("lurq hover frames probe")
    .on_tick(move |tree, _| driver.tick(tree))
    .run();
}
