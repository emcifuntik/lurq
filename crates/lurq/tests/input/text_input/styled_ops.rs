//! Span arithmetic of styled text inputs: edits move spans by byte offsets,
//! restyling splits and merges them, typing picks a style.
use super::*;
use crate::components::{MarkPresence, StyledText};

fn bold() -> SpanStyle {
  SpanStyle::new().bold()
}

fn link(target: &str) -> SpanStyle {
  SpanStyle::new().link(target)
}

fn span(range: Range<usize>, style: SpanStyle) -> StyledSpan {
  StyledSpan::new(range, style)
}

#[test]
fn styled_normalize_clamps_sorts_merges_and_lets_later_spans_win() {
  let text = "aé bold";
  let spans = vec![
    span(3..5, bold()),
    span(5..20, bold()),
    span(2..3, SpanStyle::new().italic()),
    span(1..1, bold()),
    span(0..1, SpanStyle::default()),
  ];
  let normalized = normalize(text, spans);
  // 2 is inside "é" (1..3): the italic span floors to 1..3, the bold ones merge and clamp to the end.
  assert_eq!(
    normalized,
    vec![span(1..3, SpanStyle::new().italic()), span(3..8, bold())]
  );
  let overlapping = normalize("abcdef", vec![span(0..4, bold()), span(2..6, SpanStyle::new().code())]);
  assert_eq!(
    overlapping,
    vec![span(0..2, bold()), span(2..6, SpanStyle::new().code())]
  );
}

#[test]
fn styled_splice_moves_spans_after_an_edit_and_keeps_those_before() {
  let spans = vec![span(0..4, bold()), span(10..14, link("x"))];
  // Insert 3 plain bytes at 6: the link moves by 3.
  assert_eq!(
    splice(&spans, 6..6, 3, &SpanStyle::default()),
    vec![span(0..4, bold()), span(13..17, link("x"))]
  );
  // Delete 2..12: the bold keeps 0..2, the link keeps its last 2 bytes, now at 2..4.
  assert_eq!(
    splice(&spans, 2..12, 0, &SpanStyle::default()),
    vec![span(0..2, bold()), span(2..4, link("x"))]
  );
  // Typing bold at the end of a bold run grows it.
  assert_eq!(splice(&spans, 4..4, 2, &bold())[0], span(0..6, bold()));
}

#[test]
fn styled_restyle_splits_runs_and_merges_equal_neighbours() {
  let spans = vec![span(0..4, bold())];
  let italic = restyle(&spans, 10, 2..6, |style| style.italic = true);
  assert_eq!(
    italic,
    vec![
      span(0..2, bold()),
      span(2..4, bold().italic()),
      span(4..6, SpanStyle::new().italic()),
    ]
  );
  let back = restyle(&italic, 10, 2..6, |style| style.italic = false);
  assert_eq!(back, spans);
}

#[test]
fn styled_summary_reports_all_some_or_none_and_a_common_link() {
  let spans = vec![span(0..4, bold().link("a")), span(4..8, link("a"))];
  let summary = summarize(&spans, 10, 0..8);
  assert_eq!(summary.bold, MarkPresence::Some);
  assert_eq!(summary.italic, MarkPresence::None);
  assert_eq!(summary.link.as_deref(), Some("a"));
  let wider = summarize(&spans, 10, 0..10);
  assert_eq!(wider.link, None);
  assert!(wider.has_link);
  assert_eq!(summarize(&spans, 10, 1..3).bold, MarkPresence::All);
}

#[test]
fn styled_typing_takes_the_style_before_the_caret_but_does_not_extend_a_link() {
  let text = "bold link\nnext";
  let spans = vec![span(0..4, bold()), span(5..9, link("t"))];
  assert_eq!(insertion_style(text, &spans, 4, 4), bold());
  assert_eq!(insertion_style(text, &spans, 7, 7), link("t"));
  assert_eq!(insertion_style(text, &spans, 9, 9), SpanStyle::default());
  // At the start the next character lends its marks, never its link.
  assert_eq!(insertion_style(text, &spans, 0, 0), bold());
  assert_eq!(insertion_style("link", &[span(0..4, bold().link("t"))], 0, 0), bold());
  // After a line break nothing on the line precedes the caret.
  assert_eq!(insertion_style(text, &spans, 10, 10), SpanStyle::default());
  // Typing over a selection takes the first selected character's style.
  assert_eq!(insertion_style(text, &spans, 5, 9), link("t"));
}

#[test]
fn styled_slice_rebases_spans_to_the_range_and_runs_cover_every_byte() {
  let spans = vec![span(2..6, bold())];
  assert_eq!(slice(&spans, 4..8), vec![span(0..2, bold())]);
  let text = StyledText::new("ab bold c", vec![span(3..7, bold())]);
  let runs: Vec<_> = text
    .runs()
    .into_iter()
    .map(|(text, style)| (text, style.bold))
    .collect();
  assert_eq!(runs, vec![("ab ", false), ("bold", true), (" c", false)]);
}
