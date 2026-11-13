use std::num::NonZeroIsize;

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group};
use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, render_engine::RenderEngine},
  components::{Column, Markdown, MarkdownProps, Text},
  layout::{render_list::RenderList, text_style::TextStyle},
  markdown::parse_markdown,
  node::Element,
};
use raw_window_handle::{
  DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};

const README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
const VIEWPORT_WIDTH: f32 = 1200.0;
const TALL_VIEWPORT_HEIGHT: f32 = 20_000.0;
const REALISTIC_VIEWPORT_HEIGHT: f32 = 800.0;
const MARKDOWN_WIDTH: f32 = 860.0;

struct MarkdownRoot;

impl Component for MarkdownRoot {
  type Props = MarkdownProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Markdown::mount(ctx, ctx.props::<Self::Props>().clone())
  }
}

struct LongTextRoot;

impl Component for LongTextRoot {
  type Props = String;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Text::styled(ctx.props::<Self::Props>(), TextStyle::default()).width(MARKDOWN_WIDTH)
  }
}

struct FlowLongTextRoot;

impl Component for FlowLongTextRoot {
  type Props = String;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new().child(Text::styled(ctx.props::<Self::Props>(), TextStyle::default()).width(MARKDOWN_WIDTH))
  }
}

struct TwinTextsRoot;

impl Component for TwinTextsRoot {
  type Props = Vec<String>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new();
    for text in ctx.props::<Self::Props>() {
      column = column.child(Text::styled(text, TextStyle::default()).width(MARKDOWN_WIDTH));
    }
    column
  }
}

struct BenchSurface;

impl HasWindowHandle for BenchSurface {
  fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
    let handle = Win32WindowHandle::new(NonZeroIsize::new(1).unwrap());
    Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
  }
}

impl HasDisplayHandle for BenchSurface {
  fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
    Ok(unsafe { DisplayHandle::borrow_raw(WindowsDisplayHandle::new().into()) })
  }
}

struct NoopRenderEngine;

impl RenderEngine for NoopRenderEngine {
  fn resize(&mut self, _width: u32, _height: u32) {}

  fn render(&mut self, list: &RenderList, _window: WindowHandle<'_>, _display: DisplayHandle<'_>) -> bool {
    black_box(list.glyphs.len());
    black_box(list.atlas.version);
    true
  }
}

fn tree() -> Tree {
  tree_with_viewport_height(TALL_VIEWPORT_HEIGHT)
}

fn realistic_viewport_tree() -> Tree {
  tree_with_viewport_height(REALISTIC_VIEWPORT_HEIGHT)
}

fn tree_with_viewport_height(height: f32) -> Tree {
  let mut tree = Tree::new();
  tree.resize(VIEWPORT_WIDTH as u32, height as u32);
  tree.set_render_engine_factory(|| Box::new(NoopRenderEngine));
  tree
}

fn run_pass(tree: &mut Tree, app: &mut App) {
  tree.request_redraw();
  tree.pass(app, &BenchSurface);
}

fn readme_source(max_lines: usize) -> String {
  if max_lines == usize::MAX {
    return README.to_owned();
  }
  README.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}

fn readme_props(max_lines: usize) -> MarkdownProps {
  MarkdownProps::new(readme_source(max_lines)).width(MARKDOWN_WIDTH)
}

fn long_text_source() -> String {
  let mut source = String::with_capacity(README.len() * 24);
  for _ in 0..24 {
    source.push_str(README);
    source.push('\n');
  }
  source
}

fn unique_long_text_source() -> String {
  use std::fmt::Write;
  let mut source = String::new();
  for copy in 0..24 {
    for (line, text) in README.lines().enumerate() {
      writeln!(source, "{copy:02}/{line:03}: {text}").unwrap();
    }
  }
  source
}

/// Paragraphs of one length that differ in one byte, where the fingerprint of
/// a long text does not look: it reads the first, the middle and the last 128
/// bytes of a text over 384 bytes. `twins` is false for the same paragraphs
/// with the byte inside the first of those samples.
fn paragraphs_alike(twins: bool) -> Vec<String> {
  let differs_at = if twins { 200 } else { 100 };
  (0..8)
    .map(|index| {
      let mut text = "The quick brown fox jumps over the lazy dog. ".repeat(14);
      text.replace_range(differs_at..differs_at + 1, &index.to_string());
      text
    })
    .collect()
}

#[cfg(feature = "perf_profile")]
fn print_text_profile_once() {
  if std::env::var_os("LURQ_TEXT_PROFILE").is_none() {
    return;
  }

  let mut app = App::new();
  let mut tree = tree();
  tree.mount_root::<MarkdownRoot>(&mut app, readme_props(usize::MAX));
  run_pass(&mut tree, &mut app);
  eprintln!("[text_pipeline_profile tall] {}", tree.profile());

  let mut app = App::new();
  let mut tree = realistic_viewport_tree();
  tree.mount_root::<MarkdownRoot>(&mut app, readme_props(usize::MAX));
  run_pass(&mut tree, &mut app);
  eprintln!("[text_pipeline_profile realistic] {}", tree.profile());

  let mut app = App::new();
  let mut tree = realistic_viewport_tree();
  tree.mount_root::<LongTextRoot>(&mut app, long_text_source());
  run_pass(&mut tree, &mut app);
  eprintln!("[text_pipeline_profile long_text_realistic] {}", tree.profile());

  let mut app = App::new();
  let mut tree = realistic_viewport_tree();
  tree.mount_root::<FlowLongTextRoot>(&mut app, long_text_source());
  run_pass(&mut tree, &mut app);
  eprintln!("[text_pipeline_profile flow_long_text_realistic] {}", tree.profile());
}

#[cfg(not(feature = "perf_profile"))]
fn print_text_profile_once() {}

fn bench_text_pipeline(c: &mut Criterion) {
  print_text_profile_once();
  let mut group = c.benchmark_group("text_pipeline");

  for lines in [32, 128, usize::MAX] {
    let label = if lines == usize::MAX {
      "all".to_owned()
    } else {
      lines.to_string()
    };

    group.bench_with_input(
      BenchmarkId::new("parse_readme_markdown", &label),
      &lines,
      |b, &lines| {
        let source = readme_source(lines);
        b.iter(|| {
          black_box(parse_markdown(black_box(&source)));
        });
      },
    );

    group.bench_with_input(
      BenchmarkId::new("cold_readme_markdown_first_pass", &label),
      &lines,
      |b, &lines| {
        b.iter_batched(
          || {
            let mut app = App::new();
            let mut tree = tree();
            tree.mount_root::<MarkdownRoot>(&mut app, readme_props(lines));
            (app, tree)
          },
          |(mut app, mut tree)| {
            run_pass(&mut tree, &mut app);
          },
          BatchSize::SmallInput,
        );
      },
    );

    group.bench_with_input(
      BenchmarkId::new("warm_readme_markdown_cached_pass", &label),
      &lines,
      |b, &lines| {
        let mut app = App::new();
        let mut tree = tree();
        tree.mount_root::<MarkdownRoot>(&mut app, readme_props(lines));
        run_pass(&mut tree, &mut app);
        b.iter(|| {
          run_pass(&mut tree, &mut app);
        });
      },
    );
  }

  group.bench_function("cold_readme_markdown_realistic_viewport/all", |b| {
    b.iter_batched(
      || {
        let mut app = App::new();
        let mut tree = realistic_viewport_tree();
        tree.mount_root::<MarkdownRoot>(&mut app, readme_props(usize::MAX));
        (app, tree)
      },
      |(mut app, mut tree)| {
        run_pass(&mut tree, &mut app);
      },
      BatchSize::SmallInput,
    );
  });

  group.bench_function("warm_readme_markdown_realistic_viewport/all", |b| {
    let mut app = App::new();
    let mut tree = realistic_viewport_tree();
    tree.mount_root::<MarkdownRoot>(&mut app, readme_props(usize::MAX));
    run_pass(&mut tree, &mut app);
    b.iter(|| {
      run_pass(&mut tree, &mut app);
    });
  });

  group.bench_function("cold_long_text_realistic_viewport/all", |b| {
    b.iter_batched(
      || {
        let mut app = App::new();
        let mut tree = realistic_viewport_tree();
        tree.mount_root::<LongTextRoot>(&mut app, long_text_source());
        (app, tree)
      },
      |(mut app, mut tree)| {
        run_pass(&mut tree, &mut app);
      },
      BatchSize::SmallInput,
    );
  });

  group.bench_function("warm_long_text_realistic_viewport/all", |b| {
    let mut app = App::new();
    let mut tree = realistic_viewport_tree();
    tree.mount_root::<LongTextRoot>(&mut app, long_text_source());
    run_pass(&mut tree, &mut app);
    b.iter(|| {
      run_pass(&mut tree, &mut app);
    });
  });

  group.bench_function("cold_flow_long_text_realistic_viewport/all", |b| {
    b.iter_batched(
      || {
        let mut app = App::new();
        let mut tree = realistic_viewport_tree();
        tree.mount_root::<FlowLongTextRoot>(&mut app, long_text_source());
        (app, tree)
      },
      |(mut app, mut tree)| {
        run_pass(&mut tree, &mut app);
      },
      BatchSize::SmallInput,
    );
  });

  group.bench_function("warm_flow_long_text_realistic_viewport/all", |b| {
    let mut app = App::new();
    let mut tree = realistic_viewport_tree();
    tree.mount_root::<FlowLongTextRoot>(&mut app, long_text_source());
    run_pass(&mut tree, &mut app);
    b.iter(|| {
      run_pass(&mut tree, &mut app);
    });
  });

  group.bench_function("remount_flow_long_text_same_app/all", |b| {
    let mut app = App::new();
    let source = long_text_source();
    b.iter(|| {
      let mut tree = realistic_viewport_tree();
      tree.mount_root::<FlowLongTextRoot>(&mut app, source.clone());
      run_pass(&mut tree, &mut app);
    });
  });

  group.bench_function("cold_unique_long_text_realistic_viewport/all", |b| {
    b.iter_batched(
      || {
        let mut app = App::new();
        let mut tree = realistic_viewport_tree();
        tree.mount_root::<LongTextRoot>(&mut app, unique_long_text_source());
        (app, tree)
      },
      |(mut app, mut tree)| run_pass(&mut tree, &mut app),
      BatchSize::SmallInput,
    );
  });

  // Eight long paragraphs drawn again: once told apart by their fingerprints,
  // once sharing one.
  for (name, twins) in [
    ("warm_long_paragraphs_alike", false),
    ("warm_long_paragraphs_with_one_fingerprint", true),
  ] {
    group.bench_function(name, |b| {
      let mut app = App::new();
      let mut tree = realistic_viewport_tree();
      tree.mount_root::<TwinTextsRoot>(&mut app, paragraphs_alike(twins));
      run_pass(&mut tree, &mut app);
      b.iter(|| {
        run_pass(&mut tree, &mut app);
      });
    });
  }

  for (name, skip, selectable) in [
    ("edit_document_paragraph", 0, false),
    ("resize_document", 8, false),
    ("scroll_document", 16, false),
    ("edit_selectable_document", 0, true),
    ("resize_selectable_document", 8, true),
    ("scroll_selectable_document", 16, true),
  ] {
    group.bench_function(name, |b| {
      b.iter_batched(
        || {
          let mut app = App::new();
          let mut tree = realistic_viewport_tree();
          tree.resize(860, 800);
          let mut scenario = scenario::Scenario::new().with_selectable(selectable);
          scenario.mount(&mut tree, &mut app);
          run_pass(&mut tree, &mut app);
          for _ in 0..skip {
            scenario.advance(&mut tree, false);
            run_pass(&mut tree, &mut app);
          }
          scenario.advance(&mut tree, false);
          (app, tree)
        },
        |(mut app, mut tree)| run_pass(&mut tree, &mut app),
        BatchSize::PerIteration,
      );
    });
  }
  group.finish();
}

criterion_group!(benches, bench_text_pipeline);

#[cfg(feature = "perf_profile")]
#[path = "text_pipeline/metrics.rs"]
mod metrics;

#[path = "text_pipeline/scenario.rs"]
mod scenario;

#[cfg(feature = "perf_profile")]
#[path = "text_pipeline/selection.rs"]
mod selection;

fn main() {
  if let Some(path) = std::env::var_os("LURQ_TEXT_SELECTION") {
    #[cfg(feature = "perf_profile")]
    {
      selection::run(path.as_ref());
      return;
    }
    #[cfg(not(feature = "perf_profile"))]
    panic!("LURQ_TEXT_SELECTION={path:?} requires --features perf_profile");
  }
  if let Some(path) = std::env::var_os("LURQ_TEXT_INTERACTIONS") {
    #[cfg(feature = "perf_profile")]
    {
      metrics::run_interactions(path.as_ref());
      return;
    }
    #[cfg(not(feature = "perf_profile"))]
    panic!("LURQ_TEXT_INTERACTIONS={path:?} requires --features perf_profile");
  }
  if let Some(path) = std::env::var_os("LURQ_TEXT_METRICS") {
    #[cfg(feature = "perf_profile")]
    {
      metrics::run(path.as_ref());
      return;
    }
    #[cfg(not(feature = "perf_profile"))]
    panic!("LURQ_TEXT_METRICS={path:?} requires --features perf_profile");
  }
  benches();
  Criterion::default().configure_from_args().final_summary();
}
