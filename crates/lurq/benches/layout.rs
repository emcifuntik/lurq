use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use lurq::{
  app::{App, runtime::Tree},
  components::{Column, Rect, Row, Text},
  layout::Alignment,
  node::Element,
};

fn build_flat_rects(count: usize) -> Element {
  Column::with(
    0.0,
    Alignment::Start,
    (0..count).map(|_| Rect::new(100.0, 20.0).background("#334155")),
  )
  .into()
}

fn build_nested_rows(depth: usize, items_per_level: usize) -> Element {
  fn nest(depth: usize, items: usize) -> Element {
    if depth == 0 {
      return Rect::new(40.0, 20.0).background("#3b82f6").into();
    }
    Row::with(4.0, Alignment::Start, (0..items).map(|_| nest(depth - 1, items))).into()
  }
  nest(depth, items_per_level)
}

fn build_text_heavy(count: usize) -> Element {
  Column::with(
    2.0,
    Alignment::Start,
    (0..count).map(|i| Text::new(&format!("Item {i}: The quick brown fox jumps over the lazy dog"))),
  )
  .into()
}

fn build_mixed_dashboard() -> Element {
  Column::with(
    8.0,
    Alignment::Start,
    (0..10).map(|_| {
      Row::with(
        8.0,
        Alignment::Start,
        (0..5).map(|_| {
          Column::with(
            4.0,
            Alignment::Start,
            vec![
              Element::from(Text::new("Metric")),
              Element::from(Rect::new(120.0, 60.0).background("#1e293b").rounded(8.0)),
              Element::from(Text::new("12,345")),
            ],
          )
        }),
      )
    }),
  )
  .into()
}

fn bench_layout_compute(c: &mut Criterion) {
  let mut group = c.benchmark_group("layout_compute");
  // set_root preserves node caches; compute must lay every node out again.

  for count in [10, 50, 100, 500, 1000] {
    group.bench_with_input(BenchmarkId::new("flat_rects", count), &count, |b, &count| {
      let mut app = App::new();
      let mut rt = Tree::new();
      rt.resize(1200, 800);
      b.iter(|| {
        rt.set_root(build_flat_rects(count));
        rt.bench_layout_only(&mut app, true)
      });
    });
  }

  for count in [10, 50, 100, 500] {
    group.bench_with_input(BenchmarkId::new("text_heavy", count), &count, |b, &count| {
      let mut app = App::new();
      let mut rt = Tree::new();
      rt.resize(1200, 800);
      let mut compute = || {
        rt.set_root(build_text_heavy(count));
        rt.bench_layout_only(&mut app, true)
      };
      // Also runs in Criterion's test mode: rebuilding alone never measures text.
      let layout = compute().expect("text benchmark must run layout");
      assert_eq!(layout.children.len(), count);
      assert!(layout.children.iter().all(|child| {
        child.result.size.width > 0.0 && child.result.size.height > 0.0
      }));
      drop(layout);
      b.iter(compute);
    });
  }

  for (depth, items) in [(3, 3), (4, 3), (5, 2), (3, 5)] {
    let label = format!("d{depth}_i{items}");
    group.bench_with_input(
      BenchmarkId::new("nested_rows", &label),
      &(depth, items),
      |b, &(d, i)| {
        let mut app = App::new();
        let mut rt = Tree::new();
        rt.resize(1200, 800);
        b.iter(|| {
          rt.set_root(build_nested_rows(d, i));
          rt.bench_layout_only(&mut app, true)
        });
      },
    );
  }

  group.bench_function("mixed_dashboard", |b| {
    let mut app = App::new();
    let mut rt = Tree::new();
    rt.resize(1200, 800);
    b.iter(|| {
      rt.set_root(build_mixed_dashboard());
      rt.bench_layout_only(&mut app, true)
    });
  });

  group.finish();
}

fn bench_layout_cached(c: &mut Criterion) {
  let mut group = c.benchmark_group("layout_cached");

  for count in [100, 500, 1000] {
    group.bench_with_input(BenchmarkId::new("flat_rects_rebuild", count), &count, |b, &count| {
      let mut app = App::new();
      let mut rt = Tree::new();
      rt.resize(1200, 800);
      rt.set_root(build_flat_rects(count));
      rt.bench_layout_only(&mut app, false);
      b.iter(|| {
        rt.bench_layout_only(&mut app, false)
      });
    });
  }

  group.finish();
}

criterion_group!(benches, bench_layout_compute, bench_layout_cached);
criterion_main!(benches);
