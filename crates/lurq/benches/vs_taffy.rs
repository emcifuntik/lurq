//! Box layout, lurq against taffy, on the same trees.
//!
//! Each scenario is one `Spec` built into both engines. Text is left out:
//! taffy measures no text, so a text leaf would time cosmic-text, not layout.
//!
//! Modes, per engine:
//! - `build`: building the tree only (lurq: `set_root` on one `Tree`, over the previous root; taffy: new nodes).
//! - `cold`: a new tree every iteration, building it plus its first layout. Dropping the tree is left out of the time
//!   in both engines.
//! - `rebuild`: a freshly built tree over one already laid out, plus layout (lurq: `set_root` on one `Tree`, which
//!   keeps what the previous root's caches allow; taffy has no equivalent, the closest is `clear`, new nodes and a
//!   compute, which keeps only the storage). Dropping the previous tree is part of the time in both engines.
//! - `forced`: every node laid out again on a tree already laid out (lurq: `force_dirty`; taffy: `mark_dirty` on every
//!   leaf).
//! - `resize`: the root's width cycles through three values, so caches miss wherever a width reaches.
//! - `noop`: layout again with nothing changed.
//!
//! The phases of lurq's `build` and `rebuild`, each timed alone:
//! - `phase_element`: building the element tree (`lurq_element`); dropping it is left out.
//! - `phase_set_root`: `set_root` of a built element tree on a new `Tree`, with no tree to carry state from.
//! - `phase_set_root_over`: `set_root` over a laid-out tree, which carries its state over and drops it.
//! - `phase_drop`: dropping a laid-out tree (taffy: its `TaffyTree`).

use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use lurq::{
  app::{App, Tree},
  layout::{Constraints, Size as LSize},
};

#[path = "vs_taffy/common.rs"]
mod common;

use common::*;

// ---- agreement ----

/// Reports where the two engines place leaves differently: a gap measured on
/// different layouts would not be a gap.
fn check_agreement(name: &str, spec: &Spec) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(VIEW_W as u32, VIEW_H as u32);
  tree.set_root(lurq_element(spec));
  let lurq = tree.bench_layout_only_shared(&mut app, true).expect("root");
  let mut lurq_leaves = Vec::new();
  lurq_leaf_positions(&lurq, 0.0, 0.0, &mut lurq_leaves);

  let mut built = taffy_build(spec);
  taffy_compute(&mut built);
  let mut taffy_leaves = Vec::new();
  // The root's own location is 0,0 in both.
  taffy_leaf_positions(&built.tree, built.root, 0.0, 0.0, &mut taffy_leaves);

  let mismatches = lurq_leaves
    .iter()
    .zip(&taffy_leaves)
    .filter(|(a, b)| {
      (a.0 - b.0).abs() > 0.5 || (a.1 - b.1).abs() > 0.5 || (a.2 - b.2).abs() > 0.5 || (a.3 - b.3).abs() > 0.5
    })
    .count();
  let first = lurq_leaves.iter().zip(&taffy_leaves).find(|(a, b)| {
    (a.0 - b.0).abs() > 0.5 || (a.1 - b.1).abs() > 0.5 || (a.2 - b.2).abs() > 0.5 || (a.3 - b.3).abs() > 0.5
  });
  eprintln!(
    "[agree] {name}: nodes={} leaves lurq={} taffy={} mismatched={} root lurq={:?} {}",
    count(spec),
    lurq_leaves.len(),
    taffy_leaves.len(),
    mismatches,
    (lurq.size.width, lurq.size.height),
    first
      .map(|(a, b)| format!("first: lurq={a:?} taffy={b:?}"))
      .unwrap_or_default(),
  );
}

// ---- benches ----

/// `VS_TAFFY_PROBE="<engine> <depth> <stack bytes>"` lays out one deep
/// chain on a thread with that stack and exits; an overflow aborts the
/// process, so a script bisects the deepest chain that fits.
///
/// Engines: `lurq` and `taffy` build and lay out the chain. The phases of
/// `lurq` alone: `lurq-element` runs only the recursive `lurq_element`;
/// `lurq-set-root` builds the elements in a loop and runs only `set_root`;
/// `lurq-layout` builds them in a loop, so only `set_root` and the layout
/// recurse.
fn probe(arg: &str) -> ! {
  let mut parts = arg.split_whitespace();
  let engine = parts.next().unwrap().to_owned();
  let depth: usize = parts.next().unwrap().parse().unwrap();
  let stack: usize = parts.next().unwrap().parse().unwrap();
  std::thread::Builder::new()
    .stack_size(stack)
    .spawn(move || {
      let spec = deep_chain(depth);
      if engine == "taffy" {
        let mut built = taffy_build(&spec);
        taffy_compute(&mut built);
        return;
      }
      let element = if engine == "lurq" || engine == "lurq-element" {
        lurq_element(&spec)
      } else {
        lurq_chain_element(depth)
      };
      // Dropping a deep tree or spec recurses as well; keep it out of the probe.
      std::mem::forget(spec);
      if engine == "lurq-element" {
        std::mem::forget(element);
        return;
      }
      let mut app = App::new();
      let mut tree = Tree::new();
      tree.resize(VIEW_W as u32, VIEW_H as u32);
      tree.set_root(element);
      if engine != "lurq-set-root" {
        tree.bench_layout_only_shared(&mut app, true);
      }
      std::mem::forget(tree);
    })
    .unwrap()
    .join()
    .unwrap();
  std::process::exit(0)
}

/// `lurq_element(&deep_chain(depth))`, built bottom-up in a loop.
fn lurq_chain_element(depth: usize) -> lurq::node::Element {
  use lurq::{
    components::{Column, Rect, Row},
    layout::Alignment,
  };
  let mut element = Rect::new(10.0, 10.0).into();
  for i in 0..depth {
    element = if i % 2 == 0 {
      Row::with(0.0, Alignment::Start, [element]).padding(1.0).into()
    } else {
      Column::with(0.0, Alignment::Start, [element]).padding(1.0).into()
    };
  }
  element
}

fn bench(c: &mut Criterion) {
  if let Ok(arg) = std::env::var("VS_TAFFY_PROBE") {
    probe(&arg);
  }
  let scenarios = scenarios();
  for (name, spec) in &scenarios {
    check_agreement(name, spec);
  }

  let mut group = c.benchmark_group("vs_taffy");
  group.sample_size(20);
  group.warm_up_time(Duration::from_millis(300));
  group.measurement_time(Duration::from_secs(2));

  for (name, spec) in &scenarios {
    let mut app = App::new();

    // build
    group.bench_function(BenchmarkId::new("lurq/build", name), |b| {
      let mut tree = Tree::new();
      tree.resize(VIEW_W as u32, VIEW_H as u32);
      b.iter(|| tree.set_root(lurq_element(spec)));
    });
    group.bench_function(BenchmarkId::new("taffy/build", name), |b| {
      b.iter(|| black_box(taffy_build(spec)));
    });

    // phases
    group.bench_function(BenchmarkId::new("lurq/phase_element", name), |b| {
      b.iter_with_large_drop(|| lurq_element(spec));
    });
    group.bench_function(BenchmarkId::new("lurq/phase_set_root", name), |b| {
      b.iter_batched(
        || {
          let mut tree = Tree::new();
          tree.resize(VIEW_W as u32, VIEW_H as u32);
          (tree, lurq_element(spec))
        },
        |(mut tree, element)| {
          tree.set_root(element);
          tree
        },
        BatchSize::LargeInput,
      );
    });
    group.bench_function(BenchmarkId::new("lurq/phase_set_root_over", name), |b| {
      b.iter_batched(
        || (lurq_tree(spec, &mut app), lurq_element(spec)),
        |(mut tree, element)| {
          tree.set_root(element);
          tree
        },
        BatchSize::LargeInput,
      );
    });
    group.bench_function(BenchmarkId::new("lurq/phase_drop", name), |b| {
      b.iter_batched(|| lurq_tree(spec, &mut app), drop, BatchSize::LargeInput);
    });
    group.bench_function(BenchmarkId::new("taffy/phase_drop", name), |b| {
      b.iter_batched(
        || {
          let mut built = taffy_build(spec);
          taffy_compute(&mut built);
          built
        },
        drop,
        BatchSize::LargeInput,
      );
    });

    // cold
    group.bench_function(BenchmarkId::new("lurq/cold", name), |b| {
      b.iter_with_large_drop(|| {
        let mut tree = Tree::new();
        tree.resize(VIEW_W as u32, VIEW_H as u32);
        tree.set_root(lurq_element(spec));
        let layout = tree.bench_layout_only_shared(&mut app, false);
        (tree, layout)
      });
    });
    group.bench_function(BenchmarkId::new("taffy/cold", name), |b| {
      b.iter_with_large_drop(|| {
        let mut built = taffy_build(spec);
        taffy_compute(&mut built);
        built
      });
    });

    // rebuild
    group.bench_function(BenchmarkId::new("lurq/rebuild", name), |b| {
      let mut tree = lurq_tree(spec, &mut app);
      b.iter(|| {
        tree.set_root(lurq_element(spec));
        black_box(tree.bench_layout_only_shared(&mut app, false));
      });
    });
    group.bench_function(BenchmarkId::new("taffy/rebuild", name), |b| {
      let mut built = taffy_build(spec);
      taffy_compute(&mut built);
      b.iter(|| {
        taffy_rebuild(&mut built, spec);
        taffy_compute(&mut built);
      });
    });

    // forced
    group.bench_function(BenchmarkId::new("lurq/forced", name), |b| {
      let mut tree = lurq_tree(spec, &mut app);
      b.iter(|| black_box(tree.bench_layout_only_shared(&mut app, true)));
    });
    group.bench_function(BenchmarkId::new("taffy/forced", name), |b| {
      let mut built = taffy_build(spec);
      taffy_compute(&mut built);
      b.iter(|| {
        for &leaf in &built.leaves {
          built.tree.mark_dirty(leaf).unwrap();
        }
        taffy_compute(&mut built);
      });
    });

    // resize
    group.bench_function(BenchmarkId::new("lurq/resize", name), |b| {
      let mut tree = lurq_tree(spec, &mut app);
      let mut i = 0;
      b.iter(|| {
        i += 1;
        let w = WIDTHS[i % WIDTHS.len()];
        tree.set_layout_constraints_override(Some(Constraints::tight(LSize::new(w, VIEW_H))));
        black_box(tree.bench_layout_only_shared(&mut app, false));
      });
    });
    group.bench_function(BenchmarkId::new("taffy/resize", name), |b| {
      let mut built = taffy_build(spec);
      taffy_compute(&mut built);
      let mut i = 0;
      b.iter(|| {
        i += 1;
        taffy_set_root_width(&mut built, WIDTHS[i % WIDTHS.len()]);
        taffy_compute(&mut built);
      });
    });

    // noop
    group.bench_function(BenchmarkId::new("lurq/noop", name), |b| {
      let mut tree = lurq_tree(spec, &mut app);
      b.iter(|| black_box(tree.bench_layout_only_shared(&mut app, false)));
    });
    group.bench_function(BenchmarkId::new("taffy/noop", name), |b| {
      let mut built = taffy_build(spec);
      taffy_compute(&mut built);
      b.iter(|| taffy_compute(&mut built));
    });
  }

  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
