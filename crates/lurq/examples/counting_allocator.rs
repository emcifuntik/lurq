//! Installs `CountingAllocator` and prints the counters of a few recorded
//! passes, headless:
//!
//! `cargo run -p lurq --example counting_allocator --features counting_allocator`
use lurq::{
  app::{
    App, Tree,
    profiler::{CountingAllocator, SampleData, SessionOptions},
  },
  components::{Column, Rect, Row, Text},
};

// The one line an application adds, with lurq's `counting_allocator` feature.
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::system();

fn scene(rows: usize) -> Column {
  let mut column = Column::new().spacing(2.0);
  for index in 0..rows {
    column = column.child(
      Row::new()
        .spacing(4.0)
        .child(Rect::new(12.0, 12.0).background("#3b82f6"))
        .child(Text::new(&format!("Row {index}"))),
    );
  }
  column
}

fn main() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(800, 600);
  tree.set_root(scene(10));
  let handle = tree.profiling_handle();
  let id = handle.start(SessionOptions::default()).expect("profiling session").id;
  for rows in [10, 10, 40, 40] {
    tree.set_root(scene(rows));
    tree.pass_headless(&mut app);
  }
  let report = handle.end(id).expect("profiling report");
  for sample in &report.samples {
    let SampleData::Pass(pass) = &sample.data else {
      continue;
    };
    let Some(counters) = pass.counters else {
      continue;
    };
    let allocations = counters.allocations.map_or_else(
      || "unavailable".to_owned(),
      |allocations| {
        format!(
          "{} allocations, {} B allocated, {} frees, {} B live",
          allocations.allocations, allocations.allocated_bytes, allocations.frees, allocations.live_bytes
        )
      },
    );
    println!(
      "pass #{}: {} nodes, {} B runtime estimate, atlas {} B, text caches {} B; {allocations}",
      sample.sequence,
      counters.nodes,
      counters.memory_bytes,
      counters.caches.glyph_atlas_bytes,
      counters.caches.text_cache_bytes,
    );
  }
}
