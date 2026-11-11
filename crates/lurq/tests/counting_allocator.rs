//! `CountingAllocator` installed as the global allocator, as an application
//! installs it: recorded passes then report their allocations.
use lurq::{
  app::{
    App, Tree,
    profiler::{CountingAllocator, SampleData, SessionOptions},
  },
  components::{Column, Rect, Text},
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::system();

#[test]
fn recorded_passes_report_allocations_with_the_allocator_installed() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(Column::new().child(Text::new("Before")));
  tree.pass_headless(&mut app);

  let handle = tree.profiling_handle();
  let id = handle.start(SessionOptions::default()).unwrap().id;
  // A new root: the recorded pass lays it out, which allocates.
  let mut column = Column::new();
  for index in 0..20 {
    column = column
      .child(Text::new(&format!("Row {index}")))
      .child(Rect::new(10.0, 10.0));
  }
  tree.set_root(column);
  tree.pass_headless(&mut app);
  let report = handle.end(id).unwrap();

  let allocations: Vec<_> = report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) => pass.counters.and_then(|counters| counters.allocations),
      _ => None,
    })
    .collect();
  assert_eq!(allocations.len(), 1, "the pass reports its allocations");
  let pass = allocations[0];
  assert!(pass.allocations > 0, "{pass:?}");
  assert!(pass.allocated_bytes > 0, "{pass:?}");
  assert!(pass.live_bytes > 0, "{pass:?}");
}

#[test]
fn a_pass_counts_its_thread_and_not_others() {
  use std::{
    alloc::{GlobalAlloc, Layout},
    sync::{
      Arc,
      atomic::{AtomicU64, Ordering},
    },
  };

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Rect::new(10.0, 10.0)));
  tree.pass_headless(&mut app);
  let handle = tree.profiling_handle();
  let id = handle.start(SessionOptions::default()).unwrap().id;
  // Another thread allocates 1 MiB blocks without pause while the pass runs;
  // `u64::MAX` tells it to stop.
  let blocks = Arc::new(AtomicU64::new(0));
  let busy = std::thread::spawn({
    let blocks = blocks.clone();
    move || {
      let layout = Layout::from_size_align(1 << 20, 8).unwrap();
      while blocks.load(Ordering::Relaxed) != u64::MAX {
        // SAFETY: a valid non-zero layout, freed once with the same layout.
        unsafe { ALLOCATOR.dealloc(ALLOCATOR.alloc(layout), layout) };
        let _ = blocks.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
          (count != u64::MAX).then(|| count + 1)
        });
      }
    }
  });
  while blocks.load(Ordering::Relaxed) == 0 {
    std::hint::spin_loop();
  }
  tree.request_redraw();
  tree.pass_headless(&mut app);
  let during_pass = blocks.swap(u64::MAX, Ordering::Relaxed);
  busy.join().unwrap();
  assert!(during_pass > 0);
  let report = handle.end(id).unwrap();

  let SampleData::Pass(pass) = &report.samples[0].data else {
    panic!("a pass sample");
  };
  let allocations = pass.counters.unwrap().allocations.unwrap();
  // The pass of a one-rect tree allocates far less than one block.
  assert!(allocations.allocated_bytes < 1 << 20, "{allocations:?}");
}
