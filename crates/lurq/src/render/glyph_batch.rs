//! Glyph runs a renderer draws with one instanced draw.
//!
//! A render list's glyphs come as runs: the glyphs of one text, with one
//! paint order and one clip. Texts that follow one another in paint order
//! with nothing else painted between them lie one after another in the list,
//! and one instanced draw over several of them paints the same instances in
//! the same order as a draw per text. Two kinds of runs may share a draw:
//!
//! - runs under the same clip, drawn with that clip as before;
//! - in the window, runs whose clips cut nothing off them (`GlyphRun::spared`), whatever the clips are: drawn without a
//!   clip, they paint the same pixels. Most text is like this: a label inside its row, a message inside the list's
//!   viewport.
//!
//! So a list of labels is a draw or two instead of one per label, with the
//! same pixels.
// Only the GPU renderers batch; `render` alone (Canvas) builds this unused.
#![cfg_attr(not(any(feature = "wgpu", feature = "dx12")), allow(dead_code))]

use crate::{
  app::glyph_engine::GLYPH_ATLAS_PADDING,
  layout::{quad::ClipRect, render_list::GlyphCmd},
};

/// Pixels along each edge of a glyph's quad that paint nothing when the quad
/// is drawn texel for pixel: the atlas keeps `GLYPH_ATLAS_PADDING` transparent
/// texels around every glyph, and the innermost of them is left to the
/// sampler's rounding.
const TRANSPARENT_EDGE: f32 = (GLYPH_ATLAS_PADDING - 1) as f32;

/// The transform of a glyph drawn as its rect.
const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// Whether two clips cut the same pixels the same way.
pub(crate) fn same_clip(a: ClipRect, b: ClipRect) -> bool {
  a.active == b.active
    && a.x == b.x
    && a.y == b.y
    && a.width == b.width
    && a.height == b.height
    && a.border_radius == b.border_radius
}

/// Whether `clip` has a rounded corner, which the shaders cut, not the
/// scissor.
fn rounded(clip: ClipRect) -> bool {
  clip.border_radius.is_some_and(|radius| {
    radius.top_left > 0.0 || radius.top_right > 0.0 || radius.bottom_right > 0.0 || radius.bottom_left > 0.0
  })
}

/// Whether `glyph` is drawn texel for pixel: at whole pixels and at its atlas
/// size, without a shadow's blur. Its outermost pixels then sample the
/// atlas's transparent padding.
fn texel_for_pixel(glyph: &GlyphCmd) -> bool {
  // A whole number of pixels, without a call into the math library.
  let whole = |value: f32| value == (value as i32) as f32;
  // No short circuit: `clip_spares` asks this of every glyph of a frame.
  (glyph.shadow_sigma <= 0.0)
    & whole(glyph.x)
    & whole(glyph.y)
    & (glyph.atlas_max[0] - glyph.atlas_min[0] == glyph.width)
    & (glyph.atlas_max[1] - glyph.atlas_min[1] == glyph.height)
}

/// Whether a scissor rectangle cuts nothing off `glyph`: every pixel it
/// paints lies inside. `scissor` is `[left, top, right, bottom]` in the
/// pixels the glyph is placed in.
///
/// What a glyph paints is its rect:
///
/// - grown as the glyph shaders grow it for a text shadow's blur;
/// - less its outermost pixels (`TRANSPARENT_EDGE`) when it is drawn texel for pixel: they sample the atlas's
///   transparent padding, and a pixel blended with no alpha stays as it was.
///
/// A glyph drawn through a transform counts as cut: its quad is not its rect.
fn scissor_spares(glyph: &GlyphCmd, scissor: [f32; 4]) -> bool {
  // How far the painted pixels reach beyond the rect; negative inside it.
  let reach = if texel_for_pixel(glyph) {
    -TRANSPARENT_EDGE
  } else if glyph.shadow_sigma > 0.0 {
    (glyph.shadow_sigma * 2.0).ceil() + 1.0
  } else {
    0.0
  };
  glyph.transform == IDENTITY
    && glyph.x - reach >= scissor[0]
    && glyph.y - reach >= scissor[1]
    && glyph.x + glyph.width + reach <= scissor[2]
    && glyph.y + glyph.height + reach <= scissor[3]
}

/// Whether a scissor rectangle cuts nothing off the glyphs of a run; see
/// [`scissor_spares`].
fn clip_spares(glyphs: &[GlyphCmd], scissor: [f32; 4]) -> bool {
  // Text is nearly always drawn texel for pixel and untransformed: the bounds
  // of the run, less the transparent edge, then say it for all its glyphs.
  let mut plain = true;
  let [mut left, mut top] = [f32::INFINITY; 2];
  let [mut right, mut bottom] = [f32::NEG_INFINITY; 2];
  for glyph in glyphs {
    plain &= texel_for_pixel(glyph) & (glyph.transform == IDENTITY);
    left = left.min(glyph.x);
    top = top.min(glyph.y);
    right = right.max(glyph.x + glyph.width);
    bottom = bottom.max(glyph.y + glyph.height);
  }
  if plain {
    return left + TRANSPARENT_EDGE >= scissor[0]
      && top + TRANSPARENT_EDGE >= scissor[1]
      && right - TRANSPARENT_EDGE <= scissor[2]
      && bottom - TRANSPARENT_EDGE <= scissor[3];
  }
  glyphs.iter().all(|glyph| scissor_spares(glyph, scissor))
}

/// The glyphs of one text in a render list: `count` glyphs from `start`,
/// with one paint order and one clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphRun {
  pub start: usize,
  pub count: usize,
  /// Drawn in the window, the run's clip cuts nothing off it: it has no
  /// clip, or a rectangular one that every pixel its glyphs paint lies in.
  pub spared: bool,
}

/// Splits `glyphs` into their runs, in the list's order, and hands each to
/// `run` with its paint order.
///
/// `window_scissor` gives the scissor rectangle of a rectangular clip in the
/// window as `[left, top, right, bottom]`, or `None` when nothing is left of
/// it; it is asked once per run with such a clip.
pub(crate) fn glyph_runs(
  glyphs: &[GlyphCmd],
  mut window_scissor: impl FnMut(ClipRect) -> Option<[f32; 4]>,
  mut run: impl FnMut(usize, GlyphRun),
) {
  let mut start = 0;
  while start < glyphs.len() {
    let order = glyphs[start].order;
    let clip = glyphs[start].clip;
    let mut end = start + 1;
    while end < glyphs.len() && glyphs[end].order == order && same_clip(glyphs[end].clip, clip) {
      end += 1;
    }
    // A run without a clip is drawn without one whatever its glyphs; rounded
    // corners and a scissor nothing is left of cut any.
    let spared = !clip.active
      || (!rounded(clip) && window_scissor(clip).is_some_and(|scissor| clip_spares(&glyphs[start..end], scissor)));
    run(
      order,
      GlyphRun {
        start,
        count: end - start,
        spared,
      },
    );
    start = end;
  }
}

/// The glyphs of one draw and how it is clipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphBatch {
  /// Glyphs in the draw, from the first run's start.
  pub count: usize,
  /// Draws after the first run that joined it.
  pub joined: usize,
  /// No clip cuts the draw's glyphs: it is drawn without one. Otherwise it
  /// is drawn with the clip of its first glyph, which all its glyphs share.
  pub unclipped: bool,
}

/// The draw that starts with the run `first`.
///
/// `following` yields the draws after that run in paint order: a glyph run,
/// or `None` for any other draw. `in_window` says the draw goes to the
/// window, where a run its clip spares may be drawn without the clip (a layer
/// limits an unclipped draw to its bounds).
///
/// A run joins the draw while it begins at the glyph the draw ends at and
/// either it and the draw's runs so far are all spared by their clips, or it
/// has the first run's clip and that run is not spared. The first draw that
/// does not join ends the batch.
pub(crate) fn batch_glyph_runs(
  glyphs: &[GlyphCmd],
  first: GlyphRun,
  following: impl IntoIterator<Item = Option<GlyphRun>>,
  in_window: bool,
) -> GlyphBatch {
  let clip = glyphs[first.start].clip;
  let unclipped = in_window && first.spared;
  let mut batch = GlyphBatch {
    count: first.count,
    joined: 0,
    unclipped,
  };
  for run in following {
    let Some(next) = run else {
      break;
    };
    let joins = next.start == first.start + batch.count
      && if unclipped {
        next.spared
      } else {
        same_clip(glyphs[next.start].clip, clip)
      };
    if !joins {
      break;
    }
    batch.count += next.count;
    batch.joined += 1;
  }
  batch
}

#[cfg(test)]
mod tests {
  use super::{GlyphBatch, GlyphRun, batch_glyph_runs, clip_spares, glyph_runs, scissor_spares};
  use crate::{
    layout::{quad::ClipRect, render_list::GlyphCmd},
    node::border::BorderRadius,
  };

  fn glyph(order: usize, x: f32, clip: ClipRect) -> GlyphCmd {
    GlyphCmd {
      order,
      x,
      y: 10.0,
      width: 8.0,
      height: 8.0,
      color: [1.0; 4],
      atlas_min: [0.0; 2],
      atlas_max: [8.0; 2],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0; 2],
      sharpness: 1.0,
      color_glyph: false,
      shadow_sigma: 0.0,
      clip,
    }
  }

  fn clip(width: f32) -> ClipRect {
    ClipRect {
      x: 0.0,
      y: 0.0,
      width,
      height: 100.0,
      active: true,
      border_radius: None,
    }
  }

  /// Three texts of two glyphs each at x 10..18 and 20..28, the third under
  /// a narrower clip than the first two.
  fn glyphs() -> Vec<GlyphCmd> {
    [(1, 100.0), (1, 100.0), (2, 100.0), (2, 100.0), (3, 50.0), (3, 50.0)]
      .into_iter()
      .enumerate()
      .map(|(index, (order, width))| glyph(order, 10.0 + 10.0 * (index % 2) as f32, clip(width)))
      .collect()
  }

  fn run(start: usize, spared: bool) -> GlyphRun {
    GlyphRun {
      start,
      count: 2,
      spared,
    }
  }

  fn batch(count: usize, joined: usize, unclipped: bool) -> GlyphBatch {
    GlyphBatch {
      count,
      joined,
      unclipped,
    }
  }

  /// The scissor of a rectangular clip in a window larger than any clip here.
  fn scissor(clip: ClipRect) -> Option<[f32; 4]> {
    Some([clip.x, clip.y, clip.x + clip.width, clip.y + clip.height])
  }

  fn runs(glyphs: &[GlyphCmd]) -> Vec<(usize, GlyphRun)> {
    let mut runs = Vec::new();
    glyph_runs(glyphs, scissor, |order, run| runs.push((order, run)));
    runs
  }

  #[test]
  fn glyphs_split_into_runs_by_order_and_clip() {
    assert_eq!(
      runs(&glyphs()),
      [(1, run(0, true)), (2, run(2, true)), (3, run(4, true))]
    );
    assert_eq!(runs(&[]), []);

    // Two texts with one order under different clips are two runs.
    let mut glyphs = glyphs();
    for glyph in &mut glyphs[..4] {
      glyph.order = 7;
    }
    glyphs[2].clip = clip(60.0);
    glyphs[3].clip = clip(60.0);
    assert_eq!(runs(&glyphs), [(7, run(0, true)), (7, run(2, true)), (3, run(4, true))]);
  }

  #[test]
  fn a_run_is_spared_when_its_clip_cuts_none_of_its_glyphs() {
    // The third text's second glyph ends at x 28, its clip at 25.
    let mut glyphs = glyphs();
    glyphs[4].clip = clip(25.0);
    glyphs[5].clip = clip(25.0);
    assert_eq!(runs(&glyphs)[2], (3, run(4, false)));

    // A rounded clip is cut by the shaders, wherever the glyphs are.
    let mut rounded = clip(100.0);
    rounded.border_radius = Some(BorderRadius {
      top_left: 4.0,
      top_right: 0.0,
      bottom_right: 0.0,
      bottom_left: 0.0,
    });
    let mut glyphs = self::glyphs();
    glyphs[0].clip = rounded;
    glyphs[1].clip = rounded;
    assert_eq!(runs(&glyphs)[0], (1, run(0, false)));

    // A clip nothing is left of spares nothing.
    let mut none_left = Vec::new();
    glyph_runs(&self::glyphs(), |_| None, |_, run| none_left.push(run.spared));
    assert_eq!(none_left, [false, false, false]);
  }

  #[test]
  fn a_run_without_a_clip_is_spared_whatever_its_glyphs() {
    let mut glyphs = glyphs();
    for glyph in &mut glyphs[..2] {
      glyph.clip = ClipRect::default();
      glyph.transform = [0.0, 1.0, -1.0, 0.0];
    }
    assert_eq!(runs(&glyphs)[0], (1, run(0, true)));
  }

  #[test]
  fn runs_that_follow_under_the_same_clip_join_the_draw() {
    let following = [Some(run(2, false))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, false), following, true),
      batch(4, 1, false)
    );
  }

  #[test]
  fn a_run_under_another_clip_ends_a_clipped_draw() {
    let following = [Some(run(2, false)), Some(run(4, false))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, false), following, true),
      batch(4, 1, false)
    );
    let following = [Some(run(4, false))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(2, false), following, true),
      batch(2, 0, false)
    );
  }

  #[test]
  fn runs_their_clips_spare_join_whatever_the_clips() {
    let following = [Some(run(2, true)), Some(run(4, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, true), following, true),
      batch(6, 2, true)
    );
  }

  #[test]
  fn in_a_layer_spared_runs_keep_their_clips() {
    let following = [Some(run(2, true)), Some(run(4, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, true), following, false),
      batch(4, 1, false)
    );
  }

  #[test]
  fn a_run_its_clip_cuts_ends_an_unclipped_draw() {
    // The second text is cut by its clip, though the clip is the first's.
    let following = [Some(run(2, false)), Some(run(4, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, true), following, true),
      batch(2, 0, true)
    );
    // It then starts a clipped draw, which the third text's clip ends.
    let following = [Some(run(4, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(2, false), following, true),
      batch(2, 0, false)
    );
  }

  #[test]
  fn another_draw_between_two_runs_ends_the_draw() {
    // A rect painted between two texts keeps its place in the order.
    let following = [None, Some(run(2, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, true), following, true),
      batch(2, 0, true)
    );
    let following = [None, Some(run(2, false))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, false), following, true),
      batch(2, 0, false)
    );
  }

  #[test]
  fn a_run_that_does_not_continue_the_instances_ends_the_draw() {
    // Runs sorted into another order than they lie in the list.
    let following = [Some(run(4, true)), Some(run(2, true))];
    assert_eq!(
      batch_glyph_runs(&glyphs(), run(0, true), following, true),
      batch(2, 0, true)
    );
  }

  #[test]
  fn a_run_alone_is_its_own_draw() {
    assert_eq!(batch_glyph_runs(&glyphs(), run(4, false), [], true), batch(2, 0, false));
  }

  #[test]
  fn a_scissor_spares_a_glyph_inside_it_and_cuts_one_across_its_edges() {
    // At x 10.5..18.5, y 10..18, between whole pixels: its whole rect paints.
    let mut glyph = glyphs()[0].clone();
    glyph.x += 0.5;
    assert!(scissor_spares(&glyph, [10.5, 10.0, 18.5, 18.0]));
    assert!(!scissor_spares(&glyph, [11.0, 10.0, 18.5, 18.0]));
    assert!(!scissor_spares(&glyph, [10.5, 11.0, 18.5, 18.0]));
    assert!(!scissor_spares(&glyph, [10.5, 10.0, 18.0, 18.0]));
    assert!(!scissor_spares(&glyph, [10.5, 10.0, 18.5, 17.0]));
  }

  #[test]
  fn the_transparent_edge_of_a_glyph_drawn_texel_for_pixel_may_be_cut() {
    // At whole pixels and at its atlas size, the glyph's outermost pixel on
    // each side samples the atlas's padding: cutting it changes nothing.
    let glyph = glyphs()[0].clone();
    assert!(scissor_spares(&glyph, [11.0, 11.0, 17.0, 17.0]));
    assert!(!scissor_spares(&glyph, [12.0, 11.0, 17.0, 17.0]));
    assert!(!scissor_spares(&glyph, [11.0, 12.0, 17.0, 17.0]));
    assert!(!scissor_spares(&glyph, [11.0, 11.0, 16.0, 17.0]));
    assert!(!scissor_spares(&glyph, [11.0, 11.0, 17.0, 16.0]));

    // Drawn at another size than its atlas rect, every pixel of it paints.
    let mut scaled = glyph;
    scaled.atlas_max = [16.0, 16.0];
    assert!(scissor_spares(&scaled, [10.0, 10.0, 18.0, 18.0]));
    assert!(!scissor_spares(&scaled, [11.0, 11.0, 17.0, 17.0]));
  }

  #[test]
  fn a_run_is_cut_when_any_of_its_glyphs_is() {
    // Glyphs at x 10..18 and 20..28, y 10..18, drawn texel for pixel.
    let plain = &glyphs()[..2];
    assert!(clip_spares(plain, [11.0, 11.0, 27.0, 17.0]));
    assert!(!clip_spares(plain, [12.0, 11.0, 27.0, 17.0]));
    assert!(!clip_spares(plain, [11.0, 11.0, 26.0, 17.0]));

    // With one glyph between whole pixels, each glyph answers for itself:
    // that one paints its whole rect, the other may lose its edge.
    let mut mixed = plain.to_vec();
    mixed[1].x += 0.5;
    assert!(clip_spares(&mixed, [11.0, 10.0, 28.5, 18.0]));
    assert!(!clip_spares(&mixed, [11.0, 10.0, 28.0, 18.0]));
    assert!(!clip_spares(&mixed, [12.0, 10.0, 28.5, 18.0]));
    assert!(!clip_spares(&mixed, [11.0, 11.0, 28.5, 18.0]));
  }

  #[test]
  fn a_shadow_needs_room_for_its_blur_and_a_transformed_glyph_is_never_spared() {
    let mut shadow = glyphs()[0].clone();
    shadow.shadow_sigma = 1.5;
    // The shaders pad a shadow's quad by ceil(2 * sigma) + 1 = 4 pixels.
    assert!(scissor_spares(&shadow, [6.0, 6.0, 22.0, 22.0]));
    assert!(!scissor_spares(&shadow, [7.0, 6.0, 22.0, 22.0]));

    let mut turned = glyphs()[0].clone();
    turned.transform = [0.0, 1.0, -1.0, 0.0];
    assert!(!scissor_spares(&turned, [0.0, 0.0, 1000.0, 1000.0]));

    let mut unplaced = glyphs()[0].clone();
    unplaced.x = f32::NAN;
    assert!(!scissor_spares(&unplaced, [0.0, 0.0, 1000.0, 1000.0]));
  }
}
