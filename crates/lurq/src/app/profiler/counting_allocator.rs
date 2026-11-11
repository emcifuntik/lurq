//! A global allocator wrapper that counts allocations for the profiler.
//!
//! A library cannot install `#[global_allocator]`; the application does, in one
//! line (see [`CountingAllocator`]). Counting uses thread-locals and relaxed
//! atomics only: no lock, no allocation and no I/O inside the allocator.
use std::{
  alloc::{GlobalAlloc, Layout, System},
  cell::Cell,
  sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::AllocationSnapshot;

/// Counts every allocation, free and byte of the allocator `A` it wraps, so a
/// profiling session reports allocations per frame and live heap bytes.
///
/// Install it in the application's binary crate, with the
/// `counting_allocator` Cargo feature of lurq enabled:
///
/// ```ignore
/// #[global_allocator]
/// static ALLOCATOR: lurq::app::profiler::CountingAllocator = lurq::app::profiler::CountingAllocator::system();
/// ```
///
/// Each thread counts its own allocations, frees and bytes in thread-locals; a
/// pass reports those of the thread that ran it. Live bytes, the bytes
/// allocated and not yet freed by every thread, are relaxed atomics spread over
/// a few cache lines so that threads rarely share one. A `realloc` counts as
/// one allocation of the new size and one free of the old. Without the wrapper
/// installed, recorded passes report allocations as unavailable
/// (`PassCounters::allocations` is `None`), never as zero.
pub struct CountingAllocator<A = System> {
  inner: A,
}

impl CountingAllocator<System> {
  /// Wraps the system allocator.
  pub const fn system() -> Self {
    Self::new(System)
  }
}

impl<A> CountingAllocator<A> {
  /// Wraps `inner`, the allocator that does the work.
  pub const fn new(inner: A) -> Self {
    Self { inner }
  }
}

/// Set by the first allocation through a wrapper; never written again.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Live-byte shards: a thread adds to and subtracts from its own, and live
/// bytes are their sum. Wrapping arithmetic: a block may be freed on another
/// thread, or was allocated before the wrapper saw it, so one shard alone can
/// go below zero.
const SHARDS: usize = 16;

#[repr(align(128))]
struct Shard(AtomicUsize);

static LIVE_BYTES: [Shard; SHARDS] = [const { Shard(AtomicUsize::new(0)) }; SHARDS];
static NEXT_SHARD: AtomicUsize = AtomicUsize::new(0);

struct ThreadCounts {
  allocations: Cell<u64>,
  frees: Cell<u64>,
  allocated_bytes: Cell<u64>,
  freed_bytes: Cell<u64>,
  /// The thread's live-byte shard; `SHARDS` until its first count.
  shard: Cell<usize>,
}

thread_local! {
  // Const-initialized and without Drop: access never allocates or registers a
  // destructor, so it is safe inside the allocator, even while a thread exits.
  static THREAD: ThreadCounts = const {
    ThreadCounts {
      allocations: Cell::new(0),
      frees: Cell::new(0),
      allocated_bytes: Cell::new(0),
      freed_bytes: Cell::new(0),
      shard: Cell::new(SHARDS),
    }
  };
}

#[inline]
fn add(cell: &Cell<u64>, value: u64) {
  cell.set(cell.get().wrapping_add(value));
}

impl ThreadCounts {
  #[inline]
  fn shard(&self) -> &'static AtomicUsize {
    let mut shard = self.shard.get();
    if shard == SHARDS {
      shard = NEXT_SHARD.fetch_add(1, Ordering::Relaxed) % SHARDS;
      self.shard.set(shard);
    }
    &LIVE_BYTES[shard].0
  }

  #[inline]
  fn allocated(&self, size: usize) {
    add(&self.allocations, 1);
    add(&self.allocated_bytes, size as u64);
  }

  #[inline]
  fn freed(&self, size: usize) {
    add(&self.frees, 1);
    add(&self.freed_bytes, size as u64);
  }
}

/// Counts an allocation of `allocated` bytes and a free of `freed` bytes,
/// either of them absent, with one thread-local access.
#[inline]
fn note(allocated: Option<usize>, freed: Option<usize>) {
  if allocated.is_some() && !INSTALLED.load(Ordering::Relaxed) {
    INSTALLED.store(true, Ordering::Relaxed);
  }
  let change = allocated.unwrap_or(0).wrapping_sub(freed.unwrap_or(0));
  let counted = THREAD.try_with(|counts| {
    if let Some(size) = allocated {
      counts.allocated(size);
    }
    if let Some(size) = freed {
      counts.freed(size);
    }
    counts.shard().fetch_add(change, Ordering::Relaxed);
  });
  // A thread past its thread-locals still changes live bytes.
  if counted.is_err() {
    LIVE_BYTES[0].0.fetch_add(change, Ordering::Relaxed);
  }
}

// SAFETY: every call is forwarded to `inner` unchanged; the wrapper only counts.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
  #[inline]
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    // SAFETY: the caller's contract is passed on.
    let ptr = unsafe { self.inner.alloc(layout) };
    if !ptr.is_null() {
      note(Some(layout.size()), None);
    }
    ptr
  }

  #[inline]
  unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
    // SAFETY: the caller's contract is passed on.
    let ptr = unsafe { self.inner.alloc_zeroed(layout) };
    if !ptr.is_null() {
      note(Some(layout.size()), None);
    }
    ptr
  }

  #[inline]
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    // SAFETY: the caller's contract is passed on.
    unsafe { self.inner.dealloc(ptr, layout) };
    note(None, Some(layout.size()));
  }

  #[inline]
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    // SAFETY: the caller's contract is passed on.
    let new = unsafe { self.inner.realloc(ptr, layout, new_size) };
    if !new.is_null() {
      note(Some(new_size), Some(layout.size()));
    }
    new
  }
}

/// The calling thread's counts since it started, and live bytes now; None
/// until a `CountingAllocator` has served an allocation.
pub(crate) fn snapshot() -> Option<AllocationSnapshot> {
  if !INSTALLED.load(Ordering::Relaxed) {
    return None;
  }
  let live_bytes = LIVE_BYTES
    .iter()
    .fold(0_usize, |sum, shard| sum.wrapping_add(shard.0.load(Ordering::Relaxed)));
  THREAD
    .try_with(|counts| AllocationSnapshot {
      allocations: counts.allocations.get(),
      frees: counts.frees.get(),
      allocated_bytes: counts.allocated_bytes.get(),
      freed_bytes: counts.freed_bytes.get(),
      // Relaxed shards read one by one may be mid-update: an estimate, never
      // below zero.
      live_bytes: (live_bytes as isize).max(0) as u64,
    })
    .ok()
}

// Tested in tests/counting_allocator.rs, a binary that installs the wrapper:
// using it here would mark it installed for every other test of the library.
