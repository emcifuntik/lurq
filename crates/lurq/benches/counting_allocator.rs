//! What `CountingAllocator` adds to an allocation, against the system
//! allocator it wraps, in the same binary and session:
//!
//! `cargo bench -p lurq --bench counting_allocator --features counting_allocator`
//!
//! Neither is installed as the global allocator; each is called directly, so
//! both run the same calls with and without the counting.
//!
//! - `alloc_free/<allocator>`: one 64-byte allocation freed at once, on one thread.
//! - `alloc_free_4_threads/<allocator>`: the same on four threads at once, which share the one atomic the wrapper
//!   writes (live bytes); each thread's counts are thread-locals.
//! - `vec_growth/<allocator>`: a `Vec<u64>` grown to 1024 items by doubling, then freed (reallocations).
use std::{
  alloc::{GlobalAlloc, Layout, System},
  hint::black_box,
  time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use lurq::app::profiler::CountingAllocator;

static COUNTING: CountingAllocator = CountingAllocator::system();

fn alloc_free(allocator: &impl GlobalAlloc, layout: Layout) {
  // SAFETY: a valid non-zero layout, freed once with the same layout.
  unsafe {
    let ptr = allocator.alloc(layout);
    black_box(ptr);
    allocator.dealloc(ptr, layout);
  }
}

fn vec_growth(allocator: &impl GlobalAlloc) {
  let mut layout = Layout::array::<u64>(1).unwrap();
  // SAFETY: each realloc gets the block and layout the previous call returned;
  // the last block is freed once with its layout.
  unsafe {
    let mut ptr = allocator.alloc(layout);
    while layout.size() < 1024 * 8 {
      let grown = Layout::array::<u64>(layout.size() / 8 * 2).unwrap();
      ptr = allocator.realloc(ptr, layout, grown.size());
      layout = grown;
    }
    black_box(ptr);
    allocator.dealloc(ptr, layout);
  }
}

fn on_threads(threads: usize, iters: u64, work: impl Fn() + Sync) -> Duration {
  let start = Instant::now();
  std::thread::scope(|scope| {
    for _ in 0..threads {
      scope.spawn(|| {
        for _ in 0..iters {
          work();
        }
      });
    }
  });
  start.elapsed()
}

fn bench_counting_allocator(c: &mut Criterion) {
  let small = Layout::from_size_align(64, 8).unwrap();
  let mut group = c.benchmark_group("allocator");
  group.bench_function("alloc_free/system", |b| b.iter(|| alloc_free(&System, small)));
  group.bench_function("alloc_free/counting", |b| b.iter(|| alloc_free(&COUNTING, small)));
  group.bench_function("alloc_free_4_threads/system", |b| {
    b.iter_custom(|iters| on_threads(4, iters, || alloc_free(&System, small)))
  });
  group.bench_function("alloc_free_4_threads/counting", |b| {
    b.iter_custom(|iters| on_threads(4, iters, || alloc_free(&COUNTING, small)))
  });
  group.bench_function("vec_growth/system", |b| b.iter(|| vec_growth(&System)));
  group.bench_function("vec_growth/counting", |b| b.iter(|| vec_growth(&COUNTING)));
  group.finish();
}

criterion_group!(benches, bench_counting_allocator);
criterion_main!(benches);
