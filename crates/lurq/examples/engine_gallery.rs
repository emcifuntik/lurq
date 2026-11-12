//! What the style engine draws, on one page, to look at: borders that follow their corners and dashed ones, a
//! grid, sticky section headers in a list that scrolls, a list whose rows leave with an animation, and text
//! in the theme's foreground.
//!
//! cargo run -p lurq --example engine_gallery --features winit,wgpu,screenshot
//!
//! Scroll the list on the right: each section's header stays at the top until the section ends. Click a task
//! to remove it: it fades and closes up.
//!
//! With `--capture <file.png>` the page scrolls its list part way, removes a task, saves a frame while the
//! task is leaving and closes.

use std::time::Duration;

use lurq::{
  animation::{AnimatableProperty, Animation, Keyframes, KeyframesId},
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Timeout},
    theme::{PaletteColor, TypographyStyle},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Grid, Rect, Row, ScrollVertical, Text},
  core::Signal,
  layout::{
    grid::GridTrack,
    layout_kind::{ScrollState, Sticky},
  },
  node::{Element, border::Border, color::Color},
};

const LEAVE: KeyframesId = KeyframesId::new(1);
const INK: &str = "#1e293b";
const LINE: &str = "#94a3b8";
const ACCENT: &str = "#2563eb";
const CARD: &str = "#ffffff";

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct Options {
  /// The file the page's frame is saved to; empty to stay open.
  capture: String,
}

struct Page {
  tasks: Signal<Vec<&'static str>>,
  scroll: ScrollState,
  /// How long a task takes to leave: slow under the script, so a frame can be saved part way.
  leave: u64,
  _script: Vec<Timeout>,
}

fn heading(text: &str) -> Text {
  Text::new(text).variant(TypographyStyle::Label)
}

fn card(label: &str) -> Column {
  Column::new()
    .size(124.0, 60.0)
    .padding(10.0)
    .background(CARD)
    .rounded(10.0)
    .child(Text::new(label).variant(TypographyStyle::Caption))
}

fn borders() -> Column {
  let line = |width: f32| Border::inside(width, LINE);
  Column::new()
    .spacing(10.0)
    .child(heading("Borders"))
    .child(
      Row::new()
        .spacing(12.0)
        .child(card("one side").border_left(Border::inside(4.0, ACCENT)))
        .child(card("two widths").border_top(line(1.0)).border_bottom(line(5.0)))
        .child(card("outside").border(Border::outside(3.0, LINE)))
        // The last card's border is outside its box: the row leaves it room.
        .padding_right(4.0),
    )
    .child(
      Row::new()
        .spacing(12.0)
        .child(card("dashed").border(line(1.5).dashed()))
        .child(card("dash 2 4").border(Border::inside(2.0, ACCENT).dash(2.0, 4.0)))
        .child(
          Column::new()
            .size(124.0, 60.0)
            .padding(10.0)
            .background(CARD)
            .border_bottom(line(2.0).dashed())
            .child(Text::new("dashed side").variant(TypographyStyle::Caption)),
        ),
    )
}

fn field() -> Rect {
  Rect::new(10.0, 26.0)
    .width(lurq::node::dimension::Dimension::Pct(100.0))
    .background(CARD)
    .rounded(6.0)
    .border(Border::inside(1.0, LINE))
}

fn tile(label: &str, fill: &str) -> Column {
  Column::new()
    .padding(8.0)
    .background(fill)
    .rounded(6.0)
    .child(Text::new(label).variant(TypographyStyle::Caption))
}

fn grids() -> Column {
  let form = Grid::new()
    .columns([GridTrack::Auto, GridTrack::Fr(1.0)])
    .gap(8.0)
    .child(Text::new("Name").variant(TypographyStyle::Label))
    .child(field())
    .child(Text::new("Email address").variant(TypographyStyle::Label))
    .child(field())
    .child(
      Text::new("The labels are as wide as the widest of them.")
        .variant(TypographyStyle::Caption)
        .column_span(2),
    );
  let tiles = Grid::with_columns(3)
    .gap(8.0)
    .child(tile("spans two columns", "#dbeafe").column_span(2))
    .child(tile("spans two rows", "#dcfce7").row_span(2))
    .child(tile("one", "#fef3c7"))
    .child(tile("two", "#fee2e2"));
  Column::new()
    .spacing(10.0)
    .child(heading("Grid"))
    .child(form)
    .child(tiles)
}

fn sticky_list(scroll: &ScrollState) -> Column {
  let mut sections = Column::new();
  for (letter, names) in [
    ("A", ["Ada", "Alan", "Alonzo", "Annie"]),
    ("B", ["Barbara", "Bjarne", "Brian", "Butler"]),
    ("C", ["Claude", "Clarence", "Cleve", "Corrado"]),
    ("D", ["Dana", "David", "Dennis", "Donald"]),
  ] {
    let mut section = Column::new().child(
      Row::new()
        .width(lurq::node::dimension::Dimension::Pct(100.0))
        .padding(6.0)
        .background("#e2e8f0")
        .sticky(Sticky::top(0.0))
        .child(Text::new(letter).variant(TypographyStyle::Caption)),
    );
    for name in names {
      section = section.child(
        Row::new()
          .padding(8.0)
          .child(Text::new(name).variant(TypographyStyle::Label)),
      );
    }
    sections = sections.child(section);
  }
  Column::new().spacing(10.0).child(heading("Sticky headers")).child(
    ScrollVertical::new(sections)
      .with_scroll_state(scroll.clone())
      .size(240.0, 170.0)
      .background(CARD)
      .rounded(8.0),
  )
}

impl Page {
  fn task_list(&self) -> Column {
    let mut list = Column::new().presence().width(240.0);
    for task in self.tasks.get() {
      let tasks = self.tasks.clone();
      list = list.child(
        Row::new()
          .key(task)
          .width(lurq::node::dimension::Dimension::Pct(100.0))
          .height(34.0)
          .padding(8.0)
          .border_bottom(Border::inside(1.0, "#e2e8f0"))
          .exit_animation(Animation::new(LEAVE).duration_ms(self.leave).linear())
          .on_click(move |_| tasks.update(|tasks| tasks.retain(|kept| *kept != task)))
          .child(Text::new(task).variant(TypographyStyle::Label)),
      );
    }
    Column::new()
      .spacing(10.0)
      .child(heading("Presence: click a task"))
      .child(Column::new().background(CARD).rounded(8.0).child(list))
  }
}

impl Component for Page {
  type Props = Options;

  fn create(ctx: &mut Ctx) -> Self {
    let capture = ctx.props::<Options>().capture.clone();
    let tasks = ctx.signal(vec![
      "Write the plan",
      "Draw the icons",
      "Build the engine",
      "Check on a Mac",
    ]);
    let scroll = ScrollState::new();
    let mut script = Vec::new();
    if !capture.is_empty() {
      // The renderer takes a second or two to come up; the script waits for it.
      let (state, list, window, closing) = (scroll.clone(), tasks.clone(), ctx.window(), ctx.window());
      let at = |millis: u64| Duration::from_millis(3000 + millis);
      script.push(ctx.create_timeout(at(0), move || state.set_scroll(0.0, 150.0)));
      script.push(ctx.create_timeout(at(300), move || {
        list.update(|tasks| tasks.retain(|kept| *kept != "Draw the icons"))
      }));
      script.push(ctx.create_timeout(at(600), move || window.screenshot(capture.clone())));
      script.push(ctx.create_timeout(at(2000), move || closing.close()));
      for step in &script {
        step.start();
      }
    }
    Self {
      tasks,
      scroll,
      leave: if script.is_empty() { 240 } else { 1200 },
      _script: script,
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .size(
        lurq::node::dimension::Dimension::Pct(100.0),
        lurq::node::dimension::Dimension::Pct(100.0),
      )
      .padding(24.0)
      .spacing(32.0)
      .background("#f1f5f9")
      .child(Column::new().width(420.0).spacing(22.0).child(borders()).child(grids()))
      .child(
        Column::new()
          .spacing(22.0)
          .child(sticky_list(&self.scroll))
          .child(self.task_list()),
      )
  }
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let capture = args
    .iter()
    .position(|arg| arg == "--capture")
    .map(|at| args.get(at + 1).expect("--capture takes a file").clone())
    .unwrap_or_default();
  if !capture.is_empty() && std::path::Path::new(&capture).exists() {
    std::fs::remove_file(&capture).expect("the earlier capture can be replaced");
  }

  let mut app = App::new();
  // Every text that names no color is drawn in the theme's foreground.
  app
    .theme()
    .set_palette_color(PaletteColor::Foreground, Color::from_hex(INK));
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.register_keyframes(
    Keyframes::new(LEAVE)
      .frame(0.0, |frame| {
        frame.set(AnimatableProperty::Opacity, 1.0);
        frame.set(AnimatableProperty::Height, 34.0);
      })
      // It fades first, and then the tasks after it close up.
      .frame(0.5, |frame| {
        frame.set(AnimatableProperty::Opacity, 0.0);
        frame.set(AnimatableProperty::Height, 34.0);
      })
      .frame(1.0, |frame| {
        frame.set(AnimatableProperty::Opacity, 0.0);
        frame.set(AnimatableProperty::Height, 0.0);
      }),
  );
  tree.mount_root::<Page>(&mut app, Options { capture });

  WinitWindow::new(app, tree)
    .with_size(760, 520)
    // On screen under the script too: a window the system counts as hidden draws no frames.
    .with_position(120, 120)
    .with_start_without_focus(true)
    .with_title("lurq engine gallery")
    .run();
}
