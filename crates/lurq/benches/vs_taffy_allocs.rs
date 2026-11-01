//! Heap allocations per layout, lurq against taffy, on the `vs_taffy` trees.
//! Prints a table; no timing (the counting allocator would skew it).

use std::{
  alloc::{GlobalAlloc, Layout, System},
  sync::atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{App, Tree},
  layout::{Constraints, Size as LSize},
};

#[path = "vs_taffy/common.rs"]
mod common;

use common::{count as count_nodes, *};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(layout.size(), Ordering::Relaxed);
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(new_size, Ordering::Relaxed);
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations and bytes of `f`.
fn measure(f: impl FnOnce()) -> (usize, usize) {
  let (a, b) = (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
  f();
  (ALLOCS.load(Ordering::Relaxed) - a, BYTES.load(Ordering::Relaxed) - b)
}

/// Allocations and bytes of one run of `f`, after a warm-up run.
fn count(mut f: impl FnMut()) -> (usize, usize) {
  f();
  measure(f)
}

/// What `f` reports of one run, after a warm-up run: for a phase of a run,
/// `f` measures that phase only.
fn count_phase(mut f: impl FnMut() -> (usize, usize)) -> (usize, usize) {
  f();
  f()
}

fn main() {
  // `cargo bench` passes `--bench`; nothing to filter here.
  let mut scenarios = scenarios();
  for d in [8, 32, 96] {
    scenarios.push((format!("deep_chain/{d}"), deep_chain(d)));
  }
  println!(
    "{:22}{:>8} {:>14} | {:>10}{:>10} | {:>10}{:>10} | {:>10}{:>10}",
    "scenario", "nodes", "mode", "lurq al", "taffy al", "lurq al/n", "taffy al/n", "lurq KiB", "taffy KiB"
  );
  for (name, spec) in &scenarios {
    let nodes = count_nodes(spec);
    let mut app = App::new();

    let mut tree = Tree::new();
    tree.resize(VIEW_W as u32, VIEW_H as u32);
    tree.set_root(lurq_element(spec));
    tree.bench_layout_only_shared(&mut app, true);
    let mut built = taffy_build(spec);
    taffy_compute(&mut built);

    let mut rows: Vec<(&str, (usize, usize), (usize, usize))> = Vec::new();

    rows.push((
      "build",
      count(|| {
        let mut t = Tree::new();
        t.resize(VIEW_W as u32, VIEW_H as u32);
        t.set_root(lurq_element(spec));
        std::mem::forget(t);
      }),
      count(|| {
        let b = taffy_build(spec);
        std::mem::forget(b);
      }),
    ));
    // `cold` and `rebuild` are the whole iteration `vs_taffy` times, element
    // and node building included; their `layout` rows are the layout alone.
    rows.push((
      "cold",
      count(|| {
        let mut t = Tree::new();
        t.resize(VIEW_W as u32, VIEW_H as u32);
        t.set_root(lurq_element(spec));
        t.bench_layout_only_shared(&mut app, false);
        std::mem::forget(t);
      }),
      count(|| {
        let mut b = taffy_build(spec);
        taffy_compute(&mut b);
        std::mem::forget(b);
      }),
    ));
    rows.push((
      "cold layout",
      count_phase(|| {
        let mut t = Tree::new();
        t.resize(VIEW_W as u32, VIEW_H as u32);
        t.set_root(lurq_element(spec));
        let counted = measure(|| {
          t.bench_layout_only_shared(&mut app, false);
        });
        std::mem::forget(t);
        counted
      }),
      count_phase(|| {
        let mut b = taffy_build(spec);
        let counted = measure(|| taffy_compute(&mut b));
        std::mem::forget(b);
        counted
      }),
    ));
    // On their own trees: the rows below need `tree` and `built` as first laid out.
    let mut rebuilt_tree = lurq_tree(spec, &mut app);
    let mut rebuilt = taffy_build(spec);
    taffy_compute(&mut rebuilt);
    rows.push((
      "rebuild",
      count(|| {
        rebuilt_tree.set_root(lurq_element(spec));
        rebuilt_tree.bench_layout_only_shared(&mut app, false);
      }),
      count(|| {
        taffy_rebuild(&mut rebuilt, spec);
        taffy_compute(&mut rebuilt);
      }),
    ));
    rows.push((
      "rebuild layout",
      count_phase(|| {
        rebuilt_tree.set_root(lurq_element(spec));
        measure(|| {
          rebuilt_tree.bench_layout_only_shared(&mut app, false);
        })
      }),
      count_phase(|| {
        taffy_rebuild(&mut rebuilt, spec);
        measure(|| taffy_compute(&mut rebuilt))
      }),
    ));
    rows.push((
      "forced",
      count(|| {
        tree.bench_layout_only_shared(&mut app, true);
      }),
      count(|| {
        for &leaf in &built.leaves {
          built.tree.mark_dirty(leaf).unwrap();
        }
        taffy_compute(&mut built);
      }),
    ));
    let mut i = 0;
    let mut j = 0;
    rows.push((
      "resize",
      count(|| {
        i += 1;
        let w = WIDTHS[i % WIDTHS.len()];
        tree.set_layout_constraints_override(Some(Constraints::tight(LSize::new(w, VIEW_H))));
        tree.bench_layout_only_shared(&mut app, false);
      }),
      count(|| {
        j += 1;
        taffy_set_root_width(&mut built, WIDTHS[j % WIDTHS.len()]);
        taffy_compute(&mut built);
      }),
    ));
    tree.set_layout_constraints_override(None);
    tree.bench_layout_only_shared(&mut app, false);
    rows.push((
      "noop",
      count(|| {
        tree.bench_layout_only_shared(&mut app, false);
      }),
      count(|| taffy_compute(&mut built)),
    ));

    for (mode, (la, lb), (ta, tb)) in rows {
      println!(
        "{:22}{:>8} {:>14} | {:>10}{:>10} | {:>10.1}{:>10.1} | {:>10.1}{:>10.1}",
        name,
        nodes,
        mode,
        la,
        ta,
        la as f64 / nodes as f64,
        ta as f64 / nodes as f64,
        lb as f64 / 1024.0,
        tb as f64 / 1024.0
      );
    }
    std::mem::forget(tree);
    std::mem::forget(rebuilt_tree);
  }
}
