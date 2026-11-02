//! Monotone source coverage, bounded cluster visits, and public paragraph layout.
use std::{
  collections::{BTreeMap, BTreeSet},
  ops::Range,
  sync::Arc,
};

use cosmic_text::{Hinting, LayoutGlyph, ShapeLine, ShapeSpan, ShapeWord, Wrap};

use super::{DecoratedInputLayout, InputConfig};

mod clusters;
pub(super) mod padding;
use clusters::{Cluster, Paragraph};

pub(super) struct RowContext {
  styles: Vec<usize>,
  metrics: Vec<(f32, f32)>,
  blank_words: Vec<Option<usize>>,
}

pub(super) struct PreparedRow {
  line: Option<cosmic_text::LayoutLine>,
  context: Arc<RowContext>,
  padding: padding::Padding,
  offset: usize,
  coverage: Range<usize>,
  ending: Option<Range<usize>>,
  rtl: bool,
  final_row: bool,
  blank_count: usize,
  public_justified: bool,
}

pub(super) fn paragraph(
  rows: &mut Vec<PreparedRow>,
  original: &ShapeLine,
  offset: usize,
  bytes: usize,
  config: &InputConfig,
) {
  let padded = config.styles.iter().any(|style| style.padding != 0.0);
  let paragraph = Paragraph::new(original, config, padded);
  let context = Arc::new(RowContext {
    styles: paragraph.styles.clone(),
    metrics: paragraph.metrics.clone(),
    blank_words: paragraph.blank_words.clone(),
  });
  if !padded {
    public_rows(rows, &paragraph, context, offset, bytes, config);
    return;
  }
  let mut scratch = paragraph.shape.clone();
  scratch.spans.clear();
  if paragraph.clusters.is_empty() {
    rows.push(PreparedRow {
      line: None,
      context,
      padding: padding::Padding { edges: BTreeMap::new() },
      offset,
      coverage: 0..bytes,
      ending: None,
      rtl: original.rtl,
      final_row: true,
      blank_count: 0,
      public_justified: false,
    });
    return;
  }
  let mut first = 0;
  let mut coverage = 0;
  while first < paragraph.clusters.len() {
    let end = row_end(&paragraph, first, config);
    fill_row(&mut scratch, &paragraph, first, end);
    trim_soft_blanks(&mut scratch, first != 0, end != paragraph.clusters.len());
    let blank_count = scratch
      .spans
      .iter()
      .flat_map(|span| &span.words)
      .filter(|word| word.blank)
      .count();
    let unpadded = layout(&scratch, config);
    let padded = padding::apply(&mut scratch, &unpadded, &paragraph, config);
    let mut shaped = layout(&scratch, config);
    padding::restore_ink(&mut shaped, &unpadded, &padded);
    let last_byte = if end == paragraph.clusters.len() {
      bytes
    } else {
      paragraph.clusters[end].range.start
    };
    rows.push(PreparedRow {
      line: Some(shaped),
      context: context.clone(),
      padding: padded,
      offset,
      coverage: coverage..last_byte,
      ending: None,
      rtl: original.rtl,
      final_row: end == paragraph.clusters.len(),
      blank_count,
      public_justified: false,
    });
    coverage = last_byte;
    first = end;
  }
}

fn public_rows(
  rows: &mut Vec<PreparedRow>,
  paragraph: &Paragraph,
  context: Arc<RowContext>,
  offset: usize,
  bytes: usize,
  config: &InputConfig,
) {
  let wrap = if config.wrap { Wrap::WordOrGlyph } else { Wrap::None };
  let width = (config.width.is_finite() && config.width < f32::MAX).then_some(config.width.max(0.0));
  let lines = paragraph.shape.layout(
    config.base.font_size,
    width,
    wrap,
    Some(config.base.text_align.to_cosmic()),
    None,
    Hinting::Disabled,
  );
  let starts: Vec<_> = lines
    .iter()
    .map(|row| row.glyphs.iter().map(|glyph| glyph.start).min().unwrap_or(bytes))
    .collect();
  let count = lines.len();
  let mut coverage = 0;
  for (index, line) in lines.into_iter().enumerate() {
    let end = starts.get(index + 1).copied().unwrap_or(bytes);
    let blank_count = line
      .glyphs
      .iter()
      .filter_map(|glyph| context.blank_words[glyph.metadata])
      .collect::<BTreeSet<_>>()
      .len();
    rows.push(PreparedRow {
      line: Some(line),
      context: context.clone(),
      padding: padding::Padding { edges: BTreeMap::new() },
      offset,
      coverage: coverage..end,
      ending: None,
      rtl: paragraph.shape.rtl,
      final_row: index + 1 == count,
      blank_count,
      public_justified: config.base.text_align == crate::layout::text_style::TextAlign::Justified,
    });
    coverage = end;
  }
  if count == 0 {
    rows.push(PreparedRow {
      line: None,
      context,
      padding: padding::Padding { edges: BTreeMap::new() },
      offset,
      coverage: 0..bytes,
      ending: None,
      rtl: paragraph.shape.rtl,
      final_row: true,
      blank_count: 0,
      public_justified: false,
    });
  }
}

fn trim_soft_blanks(shape: &mut ShapeLine, leading: bool, trailing: bool) {
  // Selection is source-ordered; traversal inside every retained span/word stays untouched.
  let visible = shape
    .spans
    .iter()
    .flat_map(|span| &span.words)
    .filter(|word| !word.blank)
    .flat_map(|word| &word.glyphs)
    .map(|glyph| (glyph.start, glyph.end));
  let (low, high) = visible.fold((usize::MAX, 0), |bounds, glyph| {
    (bounds.0.min(glyph.0), bounds.1.max(glyph.1))
  });
  for span in &mut shape.spans {
    span.words.retain_mut(|word| {
      // Preserve an already empty original word in the engine denominator.
      if !word.blank || word.glyphs.is_empty() {
        return true;
      }
      word
        .glyphs
        .retain(|glyph| (!leading || glyph.end > low) && (!trailing || glyph.start < high));
      !word.glyphs.is_empty()
    });
  }
}

pub(super) fn finish(output: &mut DecoratedInputLayout, mut rows: Vec<PreparedRow>) {
  let natural = rows
    .iter()
    .filter_map(|row| row.line.as_ref())
    .map(|row| {
      let (left, right) = row_extent(&row.glyphs);
      right - left
    })
    .fold(0.0_f32, f32::max);
  let viewport = natural.max(output.config.min_width).min(output.config.width);
  for row in &mut rows {
    if let Some(line) = &mut row.line
      && !row.public_justified
    {
      super::justification::apply(
        line,
        &row.context.blank_words,
        row.rtl,
        row.final_row,
        row.blank_count,
        viewport,
        &output.config,
      );
    }
  }
  for row in rows {
    if let Some(line) = row.line {
      output.append_row(
        line,
        &row.context.styles,
        &row.context.metrics,
        &row.padding,
        row.offset,
        row.coverage,
        row.rtl,
      );
    } else {
      output.append_empty_row(row.offset + row.coverage.start, row.offset + row.coverage.end);
    }
    if let Some(ending) = row.ending {
      output.append_ending(ending);
    }
  }
}

pub(super) fn set_ending(rows: &mut [PreparedRow], ending: Range<usize>) {
  if !ending.is_empty() {
    rows
      .last_mut()
      .expect("each canonical paragraph produces at least one visual row")
      .ending = Some(ending);
  }
}

fn layout(shape: &ShapeLine, config: &InputConfig) -> cosmic_text::LayoutLine {
  // One already-shaped row; private reorder/source-range APIs are never used.
  shape
    .layout(
      config.base.font_size,
      if config.width.is_finite() && config.width < f32::MAX {
        Some(config.width)
      } else {
        None
      },
      Wrap::None,
      Some(cosmic_text::Align::Left),
      None,
      Hinting::Disabled,
    )
    .into_iter()
    .next()
    .unwrap_or_else(|| cosmic_text::LayoutLine {
      w: 0.0,
      max_ascent: 0.0,
      max_descent: 0.0,
      line_height_opt: None,
      glyphs: Vec::new(),
      decorations: Vec::new(),
    })
}

fn fill_row(scratch: &mut ShapeLine, paragraph: &Paragraph, first: usize, end: usize) {
  let locations: BTreeSet<_> = paragraph.clusters[first..end]
    .iter()
    .flat_map(|cluster| cluster.locations.iter().copied())
    .collect();
  let low = locations.first().expect("a row contains at least one shaped cluster").0;
  let high = locations.last().expect("a row contains at least one shaped cluster").0;
  scratch.spans.clear();
  // The initial directional span is required even when this row starts in an
  // incongruent span. Omitting it makes public layout skip that first word.
  if low > 0 {
    scratch.spans.push(empty_span(&paragraph.shape.spans[0]));
  }
  for span_index in low..=high {
    let original = &paragraph.shape.spans[span_index];
    let mut span = empty_span(original);
    let words: BTreeSet<_> = locations
      .range((span_index, 0, 0)..=(span_index, usize::MAX, usize::MAX))
      .map(|location| location.1)
      .collect();
    let (Some(&first_word), Some(&last_word)) = (words.first(), words.last()) else {
      // An intermediate directional span can contain no selected glyphs.
      scratch.spans.push(span);
      continue;
    };
    for word_index in first_word..=last_word {
      let word = &original.words[word_index];
      let glyphs = locations
        .range((span_index, word_index, 0)..=(span_index, word_index, usize::MAX))
        .map(|location| word.glyphs[location.2].clone())
        .collect();
      span.words.push(ShapeWord {
        blank: word.blank,
        glyphs,
      });
    }
    // Empty intermediate spans retain isolate/neutral ordering context.
    scratch.spans.push(span);
  }
}

fn empty_span(original: &ShapeSpan) -> ShapeSpan {
  ShapeSpan {
    level: original.level,
    words: Vec::new(),
    decoration_spans: original.decoration_spans.clone(),
  }
}

fn row_end(paragraph: &Paragraph, first: usize, config: &InputConfig) -> usize {
  if !config.wrap || !config.width.is_finite() || config.width >= f32::MAX {
    return paragraph.clusters.len();
  }
  let width = config.width.max(0.0);
  let mut order = BTreeMap::new();
  let mut advances = 0.0;
  let mut padding = 0.0;
  let mut word_end = first;
  for (index, cluster) in paragraph.clusters.iter().enumerate().skip(first) {
    let added = fragment_delta(&order, cluster, config);
    // Absolute advance bounds also terminate for negative tracking. We do not
    // binary-search a nonmonotone prefix width or repeatedly clone a paragraph.
    if index > first && advances + cluster.advance_bound + padding + added > width {
      return if word_end > first { word_end } else { index };
    }
    order.insert(cluster.rank, cluster.token);
    advances += cluster.advance_bound;
    padding += added;
    if cluster.word_end {
      word_end = index + 1;
    }
  }
  paragraph.clusters.len()
}

fn fragment_delta(order: &BTreeMap<usize, Option<usize>>, cluster: &Cluster, config: &InputConfig) -> f32 {
  let previous = order.range(..cluster.rank).next_back().map(|(_, token)| *token);
  let next = order.range(cluster.rank..).next().map(|(_, token)| *token);
  let cost = |token: Option<usize>| token.map_or(0.0, |index| config.styles[index].padding * 2.0);
  let same = |a: Option<Option<usize>>, b: Option<Option<usize>>| a.is_some() && a == b && a.flatten().is_some();
  let mut delta = cost(cluster.token);
  if same(previous, next) {
    delta += cost(previous.flatten());
  }
  if same(previous, Some(cluster.token)) {
    delta -= cost(cluster.token);
  }
  if same(Some(cluster.token), next) {
    delta -= cost(cluster.token);
  }
  delta
}

/// The left and right of a row's glyphs, as the bounds of its clusters fold
/// to them: the right is never left of 0, the left never right of the right.
pub(super) fn row_extent(glyphs: &[LayoutGlyph]) -> (f32, f32) {
  let right = glyphs
    .iter()
    .map(|glyph| glyph.x.max(glyph.x + glyph.w))
    .fold(0.0_f32, f32::max);
  let left = glyphs
    .iter()
    .map(|glyph| glyph.x.min(glyph.x + glyph.w))
    .fold(right, f32::min);
  (left, right)
}

pub(super) fn visual_bounds(glyphs: &[LayoutGlyph]) -> BTreeMap<(usize, usize), (f32, f32)> {
  let mut bounds = BTreeMap::new();
  for glyph in glyphs {
    let left = glyph.x.min(glyph.x + glyph.w);
    let right = glyph.x.max(glyph.x + glyph.w);
    let entry = bounds.entry((glyph.start, glyph.end)).or_insert((left, right));
    entry.0 = entry.0.min(left);
    entry.1 = entry.1.max(right);
  }
  bounds
}
