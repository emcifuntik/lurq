//! Editing a long text input, plain and styled: one frame (event plus a full
//! pass through layout, paint and the render list) per measured step.
//!
//! cargo bench -p lurq --features markdown --bench styled_input
//!
//! Each text is about 4,000 characters, as one paragraph or as 50 paragraphs
//! of a multiline input; the styled ones have a bold, code or link span every
//! few words. `edit` replaces six characters with a number not typed before
//! (one frame, nothing cached); `caret_move` moves the caret right and back (two frames);
//! `selection_drag` drags across the text (two frames); `toggle_bold` bolds
//! and unbolds a selection starting somewhere new each time (two frames).
use std::num::NonZeroIsize;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use lurq::{
  app::{App, Tree, events::MouseButton, render_engine::RenderEngine},
  components::{SpanStyle, StyledSpan, TextInput, TextInputController, TextMark},
  core::Signal,
  layout::render_list::RenderList,
};
use raw_window_handle::{
  DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};

const WORDS: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ";
const PARAGRAPHS: usize = 50;
const ID: &str = "editor";

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
    true
  }
}

#[derive(Clone, Copy)]
enum Shape {
  /// One paragraph of about 4,000 characters.
  Paragraph,
  /// 50 paragraphs of about 80 characters.
  Paragraphs,
}

fn text(shape: Shape) -> String {
  match shape {
    Shape::Paragraph => WORDS.repeat(PARAGRAPHS),
    Shape::Paragraphs => vec![WORDS.trim_end(); PARAGRAPHS].join("\n"),
  }
}

/// A span every third word: bold, code or a link in turn.
fn spans(text: &str) -> Vec<StyledSpan> {
  let mut spans = Vec::new();
  let mut start = 0;
  for (index, word) in text.split(' ').enumerate() {
    let range = start..start + word.len();
    start += word.len() + 1;
    if index % 3 != 0 || word.contains('\n') {
      continue;
    }
    let style = match index % 9 {
      0 => SpanStyle::new().bold(),
      3 => SpanStyle::new().code(),
      _ => SpanStyle::new().link("https://example.com"),
    };
    spans.push(StyledSpan::new(range, style));
  }
  spans
}

struct Editor {
  tree: Tree,
  app: App,
  controller: TextInputController,
  middle: usize,
  edits: u32,
}

impl Editor {
  fn new(shape: Shape, styled: bool) -> Self {
    let text = text(shape);
    let middle = text.len() / 2 + 1;
    let controller = TextInputController::new();
    let mut input = TextInput::new(Signal::new(text.clone()))
      .id(ID)
      .controller(controller.clone())
      .multiline()
      .width(860.0);
    if styled {
      input = input.spans(Signal::new(spans(&text)));
    }
    let mut tree = Tree::new();
    tree.resize(1200, 4000);
    tree.set_render_engine_factory(|| Box::new(NoopRenderEngine));
    tree.set_root(input);
    let mut editor = Self {
      tree,
      app: App::new(),
      controller,
      middle,
      edits: 0,
    };
    editor.pass();
    editor.tree.get_element_by_id_mut(ID).unwrap().focus();
    editor.controller.select(middle, middle).unwrap();
    editor.pass();
    editor
  }

  fn pass(&mut self) {
    self.tree.request_redraw();
    self.tree.pass(&mut self.app, &BenchSurface);
  }

  fn key(&mut self, key: &str, shift: bool) {
    self.tree.key_down(key.into(), key.into(), shift, false, false);
    self.pass();
  }

  /// Replaces six characters in the middle with a number never typed
  /// before, so no layout of the text is cached: one edit, one frame.
  fn edit(&mut self) {
    self.edits += 1;
    self.controller.select(self.middle, self.middle + 6).unwrap();
    self
      .controller
      .replace_selection(&format!("{:06}", self.edits % 1_000_000))
      .unwrap();
    self.pass();
  }

  fn caret_move(&mut self) {
    self.key("ArrowRight", false);
    self.key("ArrowLeft", false);
  }

  fn selection_drag(&mut self) {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    let (x, y) = (bounds.x + 20.0, bounds.y + 30.0);
    self.tree.mouse_down(x, y, MouseButton::Left);
    self.tree.mouse_move(x + 300.0, y + 200.0);
    self.pass();
    self.tree.mouse_move(x + 400.0, y + 260.0);
    self.pass();
    self.tree.mouse_up(x + 400.0, y + 260.0, MouseButton::Left);
  }

  /// Bolds and unbolds a 400-byte selection that starts somewhere new each
  /// time, so no layout of it is cached (two frames).
  fn toggle_bold(&mut self) {
    self.edits += 1;
    let start = self.middle - 600 + (self.edits as usize * 7) % 1000;
    self.controller.select(start, start + 400).unwrap();
    self.controller.toggle_mark(TextMark::Bold).unwrap();
    self.pass();
    self.controller.toggle_mark(TextMark::Bold).unwrap();
    self.pass();
  }
}

fn bench(c: &mut Criterion) {
  let mut group = c.benchmark_group("styled_input");
  group.sample_size(30);
  let cases = [("paragraph", Shape::Paragraph), ("50_paragraphs", Shape::Paragraphs)];
  for (name, shape) in cases {
    for styled in [false, true] {
      let label = format!("{name}_{}", if styled { "styled" } else { "plain" });
      let mut editor = Editor::new(shape, styled);
      group.bench_function(BenchmarkId::new("edit", &label), |b| b.iter(|| editor.edit()));
      group.bench_function(BenchmarkId::new("caret_move", &label), |b| {
        b.iter(|| editor.caret_move())
      });
      group.bench_function(BenchmarkId::new("selection_drag", &label), |b| {
        b.iter(|| editor.selection_drag())
      });
      if styled {
        group.bench_function(BenchmarkId::new("toggle_bold", &label), |b| {
          b.iter(|| editor.toggle_bold())
        });
      }
    }
  }
  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
