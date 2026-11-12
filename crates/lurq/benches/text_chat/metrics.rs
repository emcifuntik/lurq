//! One CSV row per frame of each chat scenario, with the frame's phases, the
//! text caches' counters and, when the binary is built with
//! `counting_allocator`, the pass's allocations. No window or GPU is created.
//!
//! ```powershell
//! cargo bench -p lurq --bench text_chat --features perf_profile,counting_allocator --no-run
//! $env:LURQ_CHAT_METRICS = "target/text-chat/metrics.csv"; <the bench executable>
//! python scripts/summarize-text-chat.py target/text-chat/metrics.csv
//! ```
//!
//! Every scenario runs `LURQ_CHAT_METRICS_RUNS` times (3 by default), each
//! run in a new app, twice over: once untouched for the wall time and the
//! phase timers, once inside a profiling session for the allocations and the
//! cache sizes, which a session measures at the end of each pass.
use std::{
  fs::File,
  io::{BufWriter, Write},
  path::Path,
  time::{Duration, Instant},
};

use lurq::app::profiler::{FrameProfile, PassCounters, RecordingOptions, SampleData};

use super::*;

const FRAMES: usize = 60;
const SCROLL_FRAMES: usize = 120;
/// New apps a cold scenario runs in, one frame each.
const COLD_APPS: usize = 5;

/// The scripted frames of a scenario: `frames` calls of `step` on a chat that
/// `setup` prepared. `step` runs the pass that draws what it changed.
struct Scenario {
  name: &'static str,
  frames: usize,
  setup: fn() -> Chat,
  step: fn(&mut Chat, usize),
}

fn settled() -> Chat {
  warmed(CHANNEL_MESSAGES)
}

fn first_frame_drawn() -> Chat {
  let mut chat = chat(CHANNEL_MESSAGES);
  chat.pass();
  chat
}

fn focused() -> Chat {
  let mut chat = settled();
  chat.focus_input();
  chat
}

fn two_channels() -> Chat {
  let mut chat = settled();
  let (first, second) = (chat.new_channel(), chat.new_channel());
  chat.show(first);
  chat.show(second);
  chat
}

fn two_scales() -> Chat {
  let mut chat = settled();
  chat.set_scale(1.5);
  chat.set_scale(1.0);
  chat
}

const SCENARIOS: &[Scenario] = &[
  Scenario {
    name: "cold_first_frame",
    frames: 1,
    setup: || chat(CHANNEL_MESSAGES),
    step: |chat, _| {
      chat.pass();
    },
  },
  Scenario {
    name: "cold_second_frame",
    frames: 1,
    setup: first_frame_drawn,
    step: |chat, _| {
      chat.pass();
    },
  },
  Scenario {
    name: "idle_frame",
    frames: FRAMES,
    setup: settled,
    step: |chat, _| chat.idle(),
  },
  Scenario {
    name: "hover",
    frames: FRAMES,
    setup: settled,
    step: |chat, _| chat.hover(),
  },
  Scenario {
    name: "scroll_10k",
    frames: SCROLL_FRAMES,
    setup: || warmed(SCROLL_MESSAGES),
    step: |chat, _| chat.scroll(),
  },
  Scenario {
    name: "switch_channel_new",
    frames: FRAMES,
    setup: settled,
    step: |chat, _| {
      let props = chat.new_channel();
      chat.show(props);
    },
  },
  Scenario {
    name: "switch_channel_back",
    frames: FRAMES,
    setup: two_channels,
    step: |chat, frame| {
      let props = chat.props.switched_to(1 + (frame as u64 % 2), CHANNEL_MESSAGES);
      chat.show(props);
    },
  },
  Scenario {
    name: "resize_width",
    frames: FRAMES,
    setup: settled,
    step: |chat, _| chat.resize(),
  },
  Scenario {
    name: "typing",
    frames: FRAMES,
    setup: focused,
    step: |chat, _| chat.type_key(),
  },
  Scenario {
    name: "scale_factor_first_150",
    frames: 1,
    setup: settled,
    step: |chat, _| chat.set_scale(1.5),
  },
  Scenario {
    name: "scale_factor_toggle",
    frames: FRAMES,
    setup: two_scales,
    step: |chat, frame| chat.set_scale(if frame % 2 == 0 { 1.5 } else { 1.0 }),
  },
];

fn runs() -> usize {
  let runs: usize = std::env::var("LURQ_CHAT_METRICS_RUNS")
    .map(|value| {
      value
        .parse()
        .expect("LURQ_CHAT_METRICS_RUNS must be a positive integer")
    })
    .unwrap_or(3);
  assert!(runs > 0, "LURQ_CHAT_METRICS_RUNS must be positive");
  runs
}

fn us(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1_000_000.0
}

fn header(out: &mut impl Write) {
  writeln!(
    out,
    "scenario,run,frame,wall_us,layout_us,quads_us,glyphs_us,render_us,measure_us,shape_us,wrap_us,raster_shape_us,\
     extents_us,swash_us,atlas_pack_us,append_us,rich_shape_us,glyph_count,quad_count,measure_hits,measure_misses,\
     glyph_hits,glyph_misses,shaped_paragraphs,wrapped_paragraphs,plain_buffer_hits,plain_buffer_misses,\
     swash_requests,atlas_packs,layout_recalculated,allocations,allocated_bytes,text_cache_bytes,atlas_bytes,\
     memory_bytes"
  )
  .unwrap();
}

fn record(
  out: &mut impl Write,
  scenario: &str,
  run: usize,
  frame: usize,
  wall: Duration,
  profile: &FrameProfile,
  counters: Option<PassCounters>,
) {
  let glyphs = &profile.glyph_engine;
  write!(out, "{scenario},{run},{frame}").unwrap();
  for duration in [
    wall,
    profile.layout,
    profile.quad_resolve,
    profile.glyph_rasterize,
    profile.gpu_submit,
    glyphs.shape_text,
    glyphs.plain_paragraph_shape,
    glyphs.plain_paragraph_layout,
    glyphs.raster_set_text,
    glyphs.vertical_extents,
    glyphs.swash_lookup,
    glyphs.atlas_pack,
    glyphs.append_cached,
    glyphs.shape_rich_text,
  ] {
    write!(out, ",{:.3}", us(duration)).unwrap();
  }
  for count in [
    profile.glyph_count,
    profile.quad_count,
    profile.text_measure_cache_hits,
    profile.text_measure_cache_misses,
    profile.glyph_cache_hits,
    profile.glyph_cache_misses,
    glyphs.plain_shaped_paragraphs,
    glyphs.plain_laid_out_paragraphs,
    glyphs.plain_buffer_hits,
    glyphs.plain_buffer_misses,
    glyphs.swash_requests,
    glyphs.atlas_packs,
    usize::from(profile.layout_recalculated),
  ] {
    write!(out, ",{count}").unwrap();
  }
  // Empty cells where nothing measured: no counting allocator in this build.
  let allocations = counters.and_then(|counters| counters.allocations);
  let cell = |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
  writeln!(
    out,
    ",{},{},{},{},{}",
    cell(allocations.map(|allocations| allocations.allocations)),
    cell(allocations.map(|allocations| allocations.allocated_bytes)),
    cell(counters.map(|counters| counters.caches.text_cache_bytes as u64)),
    cell(counters.map(|counters| counters.caches.glyph_atlas_bytes as u64)),
    cell(counters.map(|counters| counters.memory_bytes as u64)),
  )
  .unwrap();
}

/// The frames of one run, timed with nothing else measuring.
fn timed_frames(scenario: &Scenario) -> Vec<(Duration, FrameProfile)> {
  let mut chat = (scenario.setup)();
  (0..scenario.frames)
    .map(|frame| {
      let start = Instant::now();
      (scenario.step)(&mut chat, frame);
      (start.elapsed(), chat.tree.profile().clone())
    })
    .collect()
}

/// The same frames inside a profiling session, for what only a session counts.
fn counted_frames(scenario: &Scenario) -> Vec<Option<PassCounters>> {
  let mut chat = (scenario.setup)();
  let handle = chat.tree.profiling_handle();
  // A recording keeps every sample; a bounded session stops at 240.
  let session = handle
    .start_recording(RecordingOptions::default())
    .expect("a profiling session")
    .id;
  for frame in 0..scenario.frames {
    (scenario.step)(&mut chat, frame);
  }
  let report = handle.end(session).expect("the session's report");
  let counters: Vec<_> = report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) if pass.rendered => Some(pass.counters),
      _ => None,
    })
    .collect();
  // A step runs one rendered pass; anything else would misalign the rows.
  assert_eq!(counters.len(), scenario.frames, "{}: passes per step", scenario.name);
  counters
}

pub(super) fn run(path: &Path) {
  let runs = runs();
  if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
    std::fs::create_dir_all(parent).expect("create the metrics directory");
  }
  let mut out = BufWriter::new(File::create(path).expect("create the metrics CSV"));
  header(&mut out);
  eprintln!(
    "Chat text metrics: {runs} runs per scenario; viewport {}x{}; {CHANNEL_MESSAGES} messages per channel, \
     {SCROLL_MESSAGES} in the scrolled list; debug_assertions={}; counting_allocator={}",
    screen::VIEWPORT.0,
    screen::VIEWPORT.1,
    cfg!(debug_assertions),
    cfg!(feature = "counting_allocator"),
  );
  for run in 0..runs {
    for scenario in SCENARIOS {
      // A cold frame needs a new app each time; the others repeat in one.
      let apps = if scenario.frames == 1 { COLD_APPS } else { 1 };
      for app in 0..apps {
        let timed = timed_frames(scenario);
        let counted = counted_frames(scenario);
        for (frame, ((wall, profile), counters)) in timed.iter().zip(counted).enumerate() {
          record(&mut out, scenario.name, run, app + frame, *wall, profile, counters);
        }
      }
    }
  }
  out.flush().unwrap();
  eprintln!("Chat text metrics saved to {}", path.display());
}
