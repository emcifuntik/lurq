use std::{
  collections::{BTreeMap, BTreeSet},
  ops::Range,
};

use cosmic_text::{Align, Hinting, ShapeLine, Wrap};

use super::super::InputConfig;

pub(super) struct Cluster {
  pub(super) range: Range<usize>,
  pub(super) locations: Vec<(usize, usize, usize)>,
  pub(super) advance_bound: f32,
  pub(super) token: Option<usize>,
  pub(super) rank: usize,
  pub(super) word_end: bool,
}

pub(super) struct Paragraph {
  pub(super) shape: ShapeLine,
  pub(super) styles: Vec<usize>,
  pub(super) metrics: Vec<(f32, f32)>,
  pub(super) blank_words: Vec<Option<usize>>,
  pub(super) clusters: Vec<Cluster>,
}

impl Paragraph {
  /// The paragraph's glyph styles and metrics; `with_clusters`, also the
  /// clusters that padded rows are broken and filled by. Rows without
  /// padding are laid out by cosmic-text and need none.
  pub(super) fn new(original: &ShapeLine, config: &InputConfig, with_clusters: bool) -> Self {
    let mut shape = original.clone();
    let mut styles = Vec::new();
    let mut metrics = Vec::new();
    let mut blank_words = Vec::new();
    let mut word_id = 0;
    let mut ends = BTreeSet::new();
    let mut clusters: BTreeMap<(usize, usize), Cluster> = BTreeMap::new();
    for (span_index, span) in shape.spans.iter_mut().enumerate() {
      for (word_index, word) in span.words.iter_mut().enumerate() {
        word_id += 1;
        if let Some(end) = word.glyphs.iter().map(|glyph| glyph.end).max() {
          ends.insert(end);
        }
        for (glyph_index, glyph) in word.glyphs.iter_mut().enumerate() {
          let style = glyph.metadata;
          glyph.metadata = styles.len();
          styles.push(style);
          blank_words.push(word.blank.then_some(word_id));
          let em = glyph
            .metrics_opt
            .map_or(config.base.font_size, |metrics| metrics.font_size);
          metrics.push((glyph.ascent * em, glyph.descent * em));
          if !with_clusters {
            continue;
          }
          let token = style.checked_sub(1);
          let entry = clusters.entry((glyph.start, glyph.end)).or_insert_with(|| Cluster {
            range: glyph.start..glyph.end,
            locations: Vec::new(),
            advance_bound: 0.0,
            token,
            rank: 0,
            word_end: false,
          });
          entry.locations.push((span_index, word_index, glyph_index));
          entry.advance_bound += glyph.width(config.base.font_size).abs();
        }
      }
    }
    if !with_clusters {
      return Self {
        shape,
        styles,
        metrics,
        blank_words,
        clusters: Vec::new(),
      };
    }
    let visual = shape.layout(
      config.base.font_size,
      None,
      Wrap::None,
      Some(Align::Left),
      None,
      Hinting::Disabled,
    );
    let glyphs: Vec<_> = visual.into_iter().flat_map(|row| row.glyphs).collect();
    let mut order: Vec<_> = super::visual_bounds(&glyphs).into_iter().collect();
    order.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.0.cmp(&b.0)));
    for (rank, (range, _)) in order.into_iter().enumerate() {
      if let Some(cluster) = clusters.get_mut(&range) {
        cluster.rank = rank;
      }
    }
    let mut clusters: Vec<_> = clusters.into_values().collect();
    for cluster in &mut clusters {
      cluster.word_end = ends.contains(&cluster.range.end);
    }
    Self {
      shape,
      styles,
      metrics,
      blank_words,
      clusters,
    }
  }
}
