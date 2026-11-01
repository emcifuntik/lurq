use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use lurq::{
  app::{App, runtime::Tree},
  components::{Column, Rect, Row, Text},
  layout::{Alignment, quad::QuadContent},
  node::Element,
};

fn sidebar_with_content(item_count: usize) -> Element {
  let sidebar: Element = Column::with(
    0.0,
    Alignment::Start,
    (0..11).map(|i| {
      Row::with(
        0.0,
        Alignment::Center,
        vec![
          Element::from(Rect::new(3.0, 38.0).background("#3b82f6")),
          Element::from(Text::new(if i == 0 { "Layout" } else { "Item" })),
        ],
      )
      .background(if i == 0 { "#1e3a5f" } else { "#1e293b" })
      .width(200.0)
    }),
  )
  .background("#1e293b")
  .into();

  let content: Element = Column::with(
    8.0,
    Alignment::Start,
    (0..item_count).map(|i| {
      Row::with(
        8.0,
        Alignment::Center,
        vec![
          Element::from(Text::new(&format!("Label {i}"))),
          Element::from(Rect::new(60.0, 30.0).background("#334155").rounded(4.0)),
          Element::from(Rect::new(200.0, 30.0).background("#0f172a").rounded(4.0)),
        ],
      )
    }),
  )
  .padding(24.0)
  .into();

  Row::with(0.0, Alignment::Start, vec![sidebar, content])
    .background("#0f172a")
    .size(1200.0, 800.0)
    .into()
}

fn assert_text_laid_out(rt: &Tree) {
  let layout = rt.last_layout().expect("render-list benchmark must run layout");
  assert!(rt.resolve_quads(layout).iter().any(|quad| {
    matches!(quad.content, QuadContent::Text { .. }) && quad.width > 0.0 && quad.height > 0.0
  }));
}

fn bench_full_pass(c: &mut Criterion) {
  let mut group = c.benchmark_group("full_pass");

  for count in [10, 50, 100, 200] {
    group.bench_with_input(BenchmarkId::new("sidebar_content", count), &count, |b, &count| {
      let mut app = App::new();
      let mut rt = Tree::new();
      rt.resize(1200, 800);
      rt.set_root(sidebar_with_content(count));
      rt.pass_headless(&mut app);
      b.iter(|| {
        rt.set_root(sidebar_with_content(count));
        rt.pass_headless(&mut app)
      });
      assert_text_laid_out(&rt);
    });
  }

  group.finish();
}

fn bench_rebuild_no_change(c: &mut Criterion) {
  let mut group = c.benchmark_group("rebuild_no_change");

  for count in [10, 50, 100] {
    group.bench_with_input(BenchmarkId::new("sidebar_content", count), &count, |b, &count| {
      let mut app = App::new();
      let mut rt = Tree::new();
      rt.resize(1200, 800);
      rt.set_root(sidebar_with_content(count));
      rt.pass_headless(&mut app);
      b.iter(|| {
        rt.pass_headless(&mut app)
      });
      assert_text_laid_out(&rt);
    });
  }

  group.finish();
}

criterion_group!(benches, bench_full_pass, bench_rebuild_no_change);
criterion_main!(benches);
