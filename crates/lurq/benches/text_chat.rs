//! Text work of a chat client's screen: about 40 visible messages of one to
//! three wrapped lines with author names and timestamps, a channel list and a
//! member list (`text_chat/screen.rs`). One frame (the event plus a full pass
//! through layout, quads and the glyph list) per measured step, with a no-op
//! renderer; `examples/text_chat_gpu_probe.rs` runs the same steps through
//! the wgpu and DX12 renderers.
//!
//! cargo bench -p lurq --bench text_chat
//!
//! - `cold_first_frame`: the first pass of a new app and tree (fonts loaded, nothing shaped or rasterised). The list
//!   shows its first rows.
//! - `cold_first_frame_warmed`: the same after `App::warm_text_styles` prepared the screen's two font faces, as an
//!   application does on another thread while its window is created.
//! - `cold_second_frame`: the pass after it, which fills the list.
//! - `idle_frame`: a redraw with nothing changed.
//! - `hover`: the pointer moves to another message row, whose background changes; no text changes.
//! - `scroll_10k`: a wheel step of 600 px through 10,000 messages, so new rows enter every frame.
//! - `switch_channel_new`: every message replaced by text never shown before.
//! - `switch_channel_back`: every message replaced by those of a channel shown before.
//! - `resize_width`: the window 2 px narrower (later wider) than the frame before.
//! - `typing`: one character typed into (or deleted from) the message input.
//! - `scale_factor_first_150`: the first frame at 150% of a screen drawn at 100%.
//! - `scale_factor_toggle`: 100% to 150% and back, both drawn before.
//!
//! With `--features perf_profile` and `LURQ_CHAT_METRICS=<csv path>` the
//! binary writes one row per frame instead (phases, cache counters and, with
//! `counting_allocator`, allocations); see `text_chat/metrics.rs`.
use std::{
  cell::RefCell,
  num::NonZeroIsize,
  time::{Duration, Instant},
};

use criterion::{BatchSize, Criterion, black_box, criterion_group};
use driver::{CHANNEL_MESSAGES, SCROLL_MESSAGES};
use lurq::{app::render_engine::RenderEngine, layout::render_list::RenderList};
use raw_window_handle::{
  DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};

#[path = "text_chat/driver.rs"]
mod driver;

#[path = "text_chat/screen.rs"]
mod screen;

#[cfg(feature = "perf_profile")]
#[path = "text_chat/metrics.rs"]
mod metrics;

// Passes report their allocations to a profiling session (`metrics.rs`).
#[cfg(feature = "counting_allocator")]
#[global_allocator]
static ALLOCATOR: lurq::app::profiler::CountingAllocator = lurq::app::profiler::CountingAllocator::system();

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

type Chat = driver::Chat<BenchSurface>;

/// The screen mounted with the no-op renderer, and no pass run yet.
fn chat(messages: usize) -> Chat {
  Chat::new(messages, BenchSurface, || Box::new(NoopRenderEngine))
}

fn warmed(messages: usize) -> Chat {
  let mut chat = chat(messages);
  chat.warm();
  chat
}

/// Times `step` alone; `prepare` runs before each step, outside the timing.
fn timed<T>(iters: u64, mut prepare: impl FnMut() -> T, mut step: impl FnMut(T)) -> Duration {
  let mut total = Duration::ZERO;
  for _ in 0..iters {
    let input = prepare();
    let start = Instant::now();
    step(input);
    total += start.elapsed();
  }
  total
}

/// A new chat whose fonts were prepared before its first frame.
fn fonts_warmed() -> Chat {
  let chat = chat(CHANNEL_MESSAGES);
  chat.app.warm_text_styles(&screen::font_styles());
  chat
}

fn bench_text_chat(c: &mut Criterion) {
  let mut group = c.benchmark_group("text_chat");

  group.bench_function("cold_first_frame", |b| {
    b.iter_batched(
      || chat(CHANNEL_MESSAGES),
      |mut chat| {
        chat.pass();
        chat
      },
      BatchSize::PerIteration,
    );
  });
  group.bench_function("cold_first_frame_warmed", |b| {
    b.iter_batched(
      fonts_warmed,
      |mut chat| {
        chat.pass();
        chat
      },
      BatchSize::PerIteration,
    );
  });
  group.bench_function("cold_second_frame", |b| {
    b.iter_batched(
      || {
        let mut chat = chat(CHANNEL_MESSAGES);
        chat.pass();
        chat
      },
      |mut chat| {
        chat.pass();
        chat
      },
      BatchSize::PerIteration,
    );
  });

  let mut chat = warmed(CHANNEL_MESSAGES);
  group.bench_function("idle_frame", |b| b.iter(|| chat.idle()));
  group.bench_function("hover", |b| b.iter(|| chat.hover()));
  group.bench_function("resize_width", |b| b.iter(|| chat.resize()));
  chat.resize_to(screen::VIEWPORT.0);
  chat.focus_input();
  group.bench_function("typing", |b| b.iter(|| chat.type_key()));
  group.bench_function("switch_channel_new", |b| {
    b.iter_custom(|iters| {
      let chat = RefCell::new(&mut chat);
      timed(
        iters,
        || chat.borrow_mut().new_channel(),
        |props| chat.borrow_mut().show(props),
      )
    });
  });
  let (first, second) = (chat.new_channel(), chat.new_channel());
  chat.show(first.clone());
  chat.show(second.clone());
  let mut back = [first, second].into_iter().cycle();
  group.bench_function("switch_channel_back", |b| {
    b.iter_custom(|iters| {
      let chat = RefCell::new(&mut chat);
      timed(iters, || back.next().unwrap(), |props| chat.borrow_mut().show(props))
    });
  });
  chat.set_scale(1.5);
  chat.set_scale(1.0);
  let mut scales = [1.5, 1.0].into_iter().cycle();
  group.bench_function("scale_factor_toggle", |b| {
    b.iter(|| chat.set_scale(scales.next().unwrap()));
  });
  drop(chat);

  group.bench_function("scale_factor_first_150", |b| {
    b.iter_batched(
      || warmed(CHANNEL_MESSAGES),
      |mut chat| {
        chat.set_scale(1.5);
        chat
      },
      BatchSize::PerIteration,
    );
  });

  let mut chat = warmed(SCROLL_MESSAGES);
  group.bench_function("scroll_10k", |b| b.iter(|| chat.scroll()));
  group.finish();
}

criterion_group!(benches, bench_text_chat);

fn main() {
  if let Some(path) = std::env::var_os("LURQ_CHAT_METRICS") {
    #[cfg(feature = "perf_profile")]
    {
      metrics::run(path.as_ref());
      return;
    }
    #[cfg(not(feature = "perf_profile"))]
    panic!("LURQ_CHAT_METRICS={path:?} requires --features perf_profile");
  }
  benches();
  Criterion::default().configure_from_args().final_summary();
}
