use super::{PassCounters, ProfileReport, SampleData, SessionOptions, producer::WindowProfiler};
use crate::{
  app::{App, Tree},
  components::{Column, Rect, Text},
  node::ElementRef,
};

fn counters(report: &ProfileReport) -> Vec<Option<PassCounters>> {
  report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) => Some(pass.counters),
      _ => None,
    })
    .collect()
}

fn element_count(element: ElementRef<'_>) -> usize {
  1 + element.children().iter().map(element_count).sum::<usize>()
}

#[test]
fn a_captured_pass_records_nodes_memory_and_cache_sizes() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(
    Column::new()
      .child(Text::new("Counter fixture"))
      .child(Rect::new(10.0, 10.0))
      .child(Rect::new(10.0, 10.0)),
  );
  // Laid out once outside the session, so the recorded pass draws from caches.
  tree.pass_headless(&mut app);
  let handle = tree.profiling_handle();
  let id = handle.start(SessionOptions::default()).unwrap().id;
  tree.pass_headless(&mut app);
  let report = handle.end(id).unwrap();

  let recorded = counters(&report);
  assert_eq!(recorded.len(), 1);
  let counters = recorded[0].expect("a captured pass carries its counters");
  assert_eq!(counters.nodes, element_count(tree.root().unwrap()));
  assert!(counters.nodes >= 4, "{}", counters.nodes);
  assert!(counters.memory_bytes > 0);
  assert!(counters.caches.text_cache_bytes > 0);
  // The test binary installs no CountingAllocator: unavailable, not zero.
  assert_eq!(counters.allocations, None);
}

#[test]
fn counters_belong_to_the_pass_that_gathered_them() {
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(SessionOptions::default()).unwrap().id;
  let report = crate::app::PassReport {
    required: true,
    ..Default::default()
  };

  let (started, guard) = producer.begin_pass(1);
  assert!(producer.captured());
  producer.set_counters(PassCounters {
    nodes: 7,
    ..PassCounters::default()
  });
  producer.finish_pass(started, &report, "none", None);
  drop(guard);
  // The next pass gathered none: it does not inherit the previous ones.
  let (started, guard) = producer.begin_pass(2);
  producer.finish_pass(started, &report, "none", None);
  drop(guard);

  let recorded = handle.end(id).unwrap();
  let nodes: Vec<_> = counters(&recorded)
    .into_iter()
    .map(|counters| counters.map(|counters| counters.nodes))
    .collect();
  assert_eq!(nodes, vec![Some(7), None]);
  // With no session, a pass is not captured and gathers nothing.
  let (_, guard) = producer.begin_pass(3);
  assert!(!producer.captured());
  drop(guard);
}

#[cfg(any(feature = "serde", feature = "mcp"))]
#[test]
fn the_export_adds_counters_and_marks_missing_allocations_unavailable() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Rect::new(10.0, 10.0)));
  let handle = tree.profiling_handle();
  let id = handle.start(SessionOptions::default()).unwrap().id;
  tree.pass_headless(&mut app);
  let json = handle.end(id).unwrap().to_json();

  assert_eq!(json["schema_version"], 1);
  let counters = &json["samples"][0]["data"]["counters"];
  assert_eq!(counters["nodes"], element_count(tree.root().unwrap()));
  assert!(counters["memory_bytes"].as_u64().unwrap() > 0);
  assert!(counters["caches"]["glyph_atlas_bytes"].is_u64());
  assert_eq!(counters["allocations"]["available"], false);
  assert!(counters["allocations"]["allocations"].is_null());
}
