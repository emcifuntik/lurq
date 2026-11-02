//! Span arithmetic over canonical span lists (sorted, disjoint, styled only,
//! equal neighbours merged). Each edit is linear in the number of spans, not
//! in the length of the text.
use std::ops::Range;

use super::{MarkPresence, SelectionStyle, SpanStyle, StyledSpan};
use crate::node::text_selection::{clamp_to_char_boundary, previous_char_boundary};

/// `spans` in canonical form over `text`; where spans overlap, the later
/// one wins.
pub(crate) fn normalize(text: &str, spans: Vec<StyledSpan>) -> Vec<StyledSpan> {
  let mut out: Vec<StyledSpan> = Vec::new();
  let in_order = spans.windows(2).all(|pair| pair[0].range.end <= pair[1].range.start);
  for span in spans {
    let start = clamp_to_char_boundary(text, span.range.start);
    let end = clamp_to_char_boundary(text, span.range.end);
    if start >= end || span.style.is_plain() {
      continue;
    }
    if in_order {
      out.push(StyledSpan::new(start..end, span.style));
    } else {
      out = overwrite(out, start..end, span.style);
    }
  }
  merge(out)
}

/// The spans after replacing `range` with `inserted` bytes of `style`.
pub(crate) fn splice(spans: &[StyledSpan], range: Range<usize>, inserted: usize, style: &SpanStyle) -> Vec<StyledSpan> {
  let removed = range.end - range.start;
  let shift = |offset: usize| offset - removed + inserted;
  let mut out = Vec::with_capacity(spans.len() + 1);
  for span in spans {
    if span.range.start < range.start {
      out.push(StyledSpan::new(
        span.range.start..span.range.end.min(range.start),
        span.style.clone(),
      ));
    }
    if span.range.end > range.end {
      let start = span.range.start.max(range.end);
      out.push(StyledSpan::new(shift(start)..shift(span.range.end), span.style.clone()));
    }
  }
  if inserted > 0 && !style.is_plain() {
    out.push(StyledSpan::new(range.start..range.start + inserted, style.clone()));
  }
  out.sort_by_key(|span| span.range.start);
  merge(out)
}

/// The spans with `change` applied to the style of every byte in `range` of
/// a text `len` bytes long.
pub(crate) fn restyle(
  spans: &[StyledSpan],
  len: usize,
  range: Range<usize>,
  change: impl Fn(&mut SpanStyle),
) -> Vec<StyledSpan> {
  let mut out = Vec::with_capacity(spans.len() + 2);
  for (run, style) in runs(spans, len) {
    let inside = run.start.max(range.start)..run.end.min(range.end);
    if inside.is_empty() {
      out.push(StyledSpan::new(run, style));
      continue;
    }
    out.push(StyledSpan::new(run.start..inside.start, style.clone()));
    let mut changed = style.clone();
    change(&mut changed);
    out.push(StyledSpan::new(inside.clone(), changed));
    out.push(StyledSpan::new(inside.end..run.end, style));
  }
  merge(out)
}

/// The style of the byte at `offset`; plain where no span covers it.
pub(crate) fn style_at(spans: &[StyledSpan], offset: usize) -> SpanStyle {
  let index = spans.partition_point(|span| span.range.end <= offset);
  spans
    .get(index)
    .filter(|span| span.range.start <= offset)
    .map(|span| span.style.clone())
    .unwrap_or_default()
}

/// The spans inside `range`, with offsets from its start.
pub(crate) fn slice(spans: &[StyledSpan], range: Range<usize>) -> Vec<StyledSpan> {
  spans
    .iter()
    .filter_map(|span| {
      let start = span.range.start.max(range.start);
      let end = span.range.end.min(range.end);
      (start < end).then(|| StyledSpan::new(start - range.start..end - range.start, span.style.clone()))
    })
    .collect()
}

/// Which marks cover the non-empty `range` of a text `len` bytes long.
pub(crate) fn summarize(spans: &[StyledSpan], len: usize, range: Range<usize>) -> SelectionStyle {
  let mut counts = [0usize; 4];
  let mut total = 0;
  let mut links = Vec::new();
  for (run, style) in runs(spans, len) {
    if run.start.max(range.start) >= run.end.min(range.end) {
      continue;
    }
    total += 1;
    for (count, mark) in counts.iter_mut().zip(super::TextMark::ALL) {
      *count += usize::from(style.has(mark));
    }
    links.push(style.link);
  }
  let presence = |count: usize| match count {
    0 => MarkPresence::None,
    count if count == total => MarkPresence::All,
    _ => MarkPresence::Some,
  };
  let common = links
    .first()
    .cloned()
    .flatten()
    .filter(|first| links.iter().all(|link| link.as_deref() == Some(first.as_ref())));
  SelectionStyle {
    bold: presence(counts[0]),
    italic: presence(counts[1]),
    code: presence(counts[2]),
    strikethrough: presence(counts[3]),
    has_link: links.iter().any(Option::is_some),
    link: common,
  }
}

/// The style text typed over `start..end` takes: the first selected
/// character's when replacing a selection; else the character's before the
/// caret, except that a link does not grow past its end. At the start of a
/// line nothing precedes the caret, so the character after it lends its
/// marks but not its link.
pub(crate) fn insertion_style(text: &str, spans: &[StyledSpan], start: usize, end: usize) -> SpanStyle {
  if start < end {
    return style_at(spans, start);
  }
  let before = previous_char_boundary(text, start);
  let line_start = start == 0 || text[before..start].contains(['\n', '\r']);
  if line_start {
    let after = text[start..].chars().next().filter(|ch| !matches!(ch, '\n' | '\r'));
    return after.map_or_else(SpanStyle::default, |_| SpanStyle {
      link: None,
      ..style_at(spans, start)
    });
  }
  let mut style = style_at(spans, before);
  if style.link.is_some() && style_at(spans, start).link != style.link {
    style.link = None;
  }
  style
}

/// Every byte of a text `len` bytes long in runs of one style, plain runs
/// included, in order.
pub(super) fn runs(spans: &[StyledSpan], len: usize) -> Vec<(Range<usize>, SpanStyle)> {
  let mut out = Vec::with_capacity(spans.len() * 2 + 1);
  let mut at = 0;
  for span in spans {
    if span.range.start > at {
      out.push((at..span.range.start, SpanStyle::default()));
    }
    out.push((span.range.clone(), span.style.clone()));
    at = span.range.end;
  }
  if at < len {
    out.push((at..len, SpanStyle::default()));
  }
  out
}

/// `spans` with `range` taken over by `style`.
fn overwrite(spans: Vec<StyledSpan>, range: Range<usize>, style: SpanStyle) -> Vec<StyledSpan> {
  let mut out = Vec::with_capacity(spans.len() + 2);
  for span in spans {
    if span.range.start < range.start {
      out.push(StyledSpan::new(
        span.range.start..span.range.end.min(range.start),
        span.style.clone(),
      ));
    }
    if span.range.end > range.end {
      out.push(StyledSpan::new(
        span.range.start.max(range.end)..span.range.end,
        span.style,
      ));
    }
  }
  out.push(StyledSpan::new(range, style));
  out.sort_by_key(|span| span.range.start);
  out
}

/// Sorted spans without empty or plain ones, equal neighbours merged.
fn merge(spans: Vec<StyledSpan>) -> Vec<StyledSpan> {
  let mut out: Vec<StyledSpan> = Vec::with_capacity(spans.len());
  for span in spans {
    if span.range.is_empty() || span.style.is_plain() {
      continue;
    }
    if let Some(last) = out.last_mut()
      && last.range.end == span.range.start
      && last.style == span.style
    {
      last.range.end = span.range.end;
      continue;
    }
    out.push(span);
  }
  out
}

#[cfg(test)]
#[path = "../../../tests/input/text_input/styled_ops.rs"]
mod tests;
