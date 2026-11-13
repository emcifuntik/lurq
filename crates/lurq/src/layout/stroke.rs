//! The shape of a rect's stroke: the ring between its outer edge and its
//! hole, and the dashes along it.
//!
//! `quad.wgsl` and `quad.hlsl` paint with the same formulas; keep the three
//! in step. The software screenshot renderer paints with these, and the
//! backend parity checks compare against them.
//!
//! **The ring.** A stroke has a width per side. Its hole is the rect inset by
//! each side's width, and a corner of the hole is the ellipse CSS gives a
//! padding box: the corner's radius less the widths of the two sides that
//! meet there. So a stroke on one side only follows the corners at its ends
//! and thins out to nothing where it meets a side that has none. On a side
//! with no stroke the hole reaches [`HOLE_MARGIN`] past the edge, so that the
//! ring has no coverage there, not the half-covered hairline two coincident
//! edges would leave.
//!
//! **The radii.** [`StrokeStyle::radii_at`] says which edge of the stroke the
//! rect's radii describe: its outer edge for a border inside an element's
//! box, its hole for a border outside the box, where the rect is the box
//! grown by the widths and the hole has to match the element's own corners.
//! A square corner stays square on both edges.
//!
//! **The dashes.** A dashed stroke is cut along the rect's outline, clockwise
//! from the start of its top edge. The period is fitted to the outline's
//! length, so the last dash meets the first with a whole gap between them.

use crate::layout::render_list::StrokeStyle;

/// How far the hole reaches past an edge whose side has no stroke, in pixels.
pub const HOLE_MARGIN: f32 = 1.0;

/// A stroke as the renderers get it: centre-relative, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeShape {
  /// Half the rect's width and height.
  pub half: [f32; 2],
  /// Corner radii in CSS order: top-left, top-right, bottom-right, bottom-left.
  pub radii: [f32; 4],
  /// Widths in the order top, right, bottom, left.
  pub stroke: [f32; 4],
  pub style: StrokeStyle,
}

/// The index, in CSS corner order, of the quadrant `(x, y)` lies in.
fn corner(x: f32, y: f32) -> usize {
  match (y < 0.0, x < 0.0) {
    (true, true) => 0,
    (true, false) => 1,
    (false, false) => 2,
    (false, true) => 3,
  }
}

/// Signed distance from `(x, y)` to a box of half extent `half` whose corners
/// are ellipses of radii `radius`: the quad shaders' `sd_rounded_box`. A
/// corner with no radius on either axis is square.
fn elliptical_box_distance(x: f32, y: f32, half: [f32; 2], radius: [f32; 2]) -> f32 {
  let (rx, ry) = (radius[0].max(0.0), radius[1].max(0.0));
  let qx = x.abs() - half[0] + rx;
  let qy = y.abs() - half[1] + ry;
  if qx > 0.0 && qy > 0.0 {
    if rx <= 0.0 || ry <= 0.0 {
      return qx.max(qy);
    }
    let (nx, ny) = (qx / rx, qy / ry);
    let length = nx.hypot(ny);
    if length <= 1e-6 {
      return -rx.min(ry);
    }
    let gradient = (nx / rx).hypot(ny / ry).max(1e-6);
    return (length - 1.0) * length / gradient;
  }
  (qx - rx).max(qy - ry)
}

/// How far a side of `width` moves the hole's edge in from the rect's: its
/// width, or out by the margin for a side with no stroke.
fn inset(width: f32) -> f32 {
  if width > 0.0 { width } else { -HOLE_MARGIN }
}

/// Signed distance from `(x, y)` to the ring: negative inside it.
pub fn stroke_distance(x: f32, y: f32, shape: &StrokeShape) -> f32 {
  let [top, right, bottom, left] = shape.stroke.map(|width| width.max(0.0));
  let radius = shape.radii[corner(x, y)].max(0.0);
  // The sides that meet at this quadrant's corner.
  let across = if x < 0.0 { left } else { right };
  let along = if y < 0.0 { top } else { bottom };
  let outer_radius = if radius > 0.0 {
    [
      radius + shape.style.radii_at * across,
      radius + shape.style.radii_at * along,
    ]
  } else {
    [0.0; 2]
  };
  let outer = elliptical_box_distance(x, y, shape.half, outer_radius);

  let hole_half = [
    (shape.half[0] - 0.5 * (inset(left) + inset(right))).max(0.0),
    (shape.half[1] - 0.5 * (inset(top) + inset(bottom))).max(0.0),
  ];
  let hole_centre = [0.5 * (inset(left) - inset(right)), 0.5 * (inset(top) - inset(bottom))];
  let hole_radius = if radius > 0.0 {
    [
      (outer_radius[0] - inset(across)).max(0.0),
      (outer_radius[1] - inset(along)).max(0.0),
    ]
  } else {
    [0.0; 2]
  };
  let hole = elliptical_box_distance(x - hole_centre[0], y - hole_centre[1], hole_half, hole_radius);
  outer.max(-hole)
}

/// How much nearer the second of two edges that meet at a corner has to be
/// for a point to be taken as that edge's. A point as near to both is the
/// first one's wherever it is computed.
const CORNER_BIAS: f32 = 0.01;

/// Whether a point `first` away from one edge of a corner and `second` from
/// the other is taken as the first edge's: the nearer one, and always the one
/// that has a stroke where the other has none, so that the dashes of a side
/// run to its ends.
fn first_edge(first: f32, second: f32, first_width: f32, second_width: f32) -> bool {
  if second_width <= 0.0 {
    true
  } else if first_width <= 0.0 {
    false
  } else {
    first <= second + CORNER_BIAS
  }
}

/// Where along the rect's outline the part of it nearest `(x, y)` is, in
/// pixels clockwise from the start of the top edge, and the outline's whole
/// length. The corners are taken as circles of the rect's radii; `stroke` is
/// the widths of the sides (top, right, bottom, left).
pub fn outline_position(x: f32, y: f32, half: [f32; 2], radii: [f32; 4], stroke: [f32; 4]) -> (f32, f32) {
  let [hx, hy] = half;
  let limit = hx.min(hy).max(0.0);
  let [r0, r1, r2, r3] = radii.map(|radius| radius.clamp(0.0, limit));
  let quarter = std::f32::consts::FRAC_PI_2;
  let top = (2.0 * hx - r0 - r1).max(0.0);
  let right = (2.0 * hy - r1 - r2).max(0.0);
  let bottom = (2.0 * hx - r2 - r3).max(0.0);
  let left = (2.0 * hy - r3 - r0).max(0.0);
  // Where each part of the outline starts.
  let right_arc = top;
  let right_edge = right_arc + quarter * r1;
  let bottom_arc = right_edge + right;
  let bottom_edge = bottom_arc + quarter * r2;
  let left_arc = bottom_edge + bottom;
  let left_edge = left_arc + quarter * r3;
  let top_arc = left_edge + left;
  let total = top_arc + quarter * r0;

  let on_top = (x + hx - r0).clamp(0.0, top);
  let on_right = right_edge + (y + hy - r1).clamp(0.0, right);
  let on_bottom = bottom_edge + (hx - r2 - x).clamp(0.0, bottom);
  let on_left = left_edge + (hy - r3 - y).clamp(0.0, left);
  let position = match corner(x, y) {
    0 => {
      let (cx, cy) = (-hx + r0, -hy + r0);
      if x < cx && y < cy {
        top_arc + (cy - y).atan2(cx - x) * r0
      } else if first_edge(x + hx, y + hy, stroke[3], stroke[0]) {
        on_left
      } else {
        on_top
      }
    }
    1 => {
      let (cx, cy) = (hx - r1, -hy + r1);
      if x > cx && y < cy {
        right_arc + (x - cx).atan2(cy - y) * r1
      } else if first_edge(y + hy, hx - x, stroke[0], stroke[1]) {
        on_top
      } else {
        on_right
      }
    }
    2 => {
      let (cx, cy) = (hx - r2, hy - r2);
      if x > cx && y > cy {
        bottom_arc + (y - cy).atan2(x - cx) * r2
      } else if first_edge(hx - x, hy - y, stroke[1], stroke[2]) {
        on_right
      } else {
        on_bottom
      }
    }
    _ => {
      let (cx, cy) = (-hx + r3, hy - r3);
      if x < cx && y > cy {
        left_arc + (cx - x).atan2(y - cy) * r3
      } else if first_edge(hy - y, x + hx, stroke[2], stroke[3]) {
        on_bottom
      } else {
        on_left
      }
    }
  };
  (position, total)
}

/// How much of a dashed stroke shows at `position` along an outline of
/// length `total`: 1 in a dash, 0 in a gap, with a ramp of a pixel between.
/// 1 everywhere for a stroke that is not dashed.
pub fn dash_coverage(position: f32, total: f32, dash: [f32; 2]) -> f32 {
  let [on, off] = dash;
  if on <= 0.0 || off <= 0.0 || total <= 0.0 {
    return 1.0;
  }
  let period = on + off;
  let fitted = total / (total / period + 0.5).floor().max(1.0);
  let on = on * fitted / period;
  let phase = position - (position / fitted).floor() * fitted;
  let distance = if phase <= on {
    -phase.min(on - phase)
  } else {
    (phase - on).min(fitted - phase)
  };
  (0.5 - distance).clamp(0.0, 1.0)
}

/// The ring's coverage at one of a pixel's four subsamples. The ramp is half
/// as wide as the distance changes over a pixel, and no narrower than half a
/// pixel: what the shaders take from `fwidth`, which makes the ramp of an
/// edge that runs diagonally, as in a corner, wider than one that runs
/// along an axis.
fn ring_sample(x: f32, y: f32, shape: &StrokeShape) -> f32 {
  let distance = stroke_distance(x, y, shape);
  let change =
    (stroke_distance(x + 1.0, y, shape) - distance).abs() + (stroke_distance(x, y + 1.0, shape) - distance).abs();
  (0.5 - distance / (0.5 * change.max(1.0))).clamp(0.0, 1.0)
}

/// The stroke's coverage (0 to 1) at the pixel centre `(x, y)`, relative to
/// the centre of an untransformed rect: the ring over four half-pixel
/// subsamples with half-pixel ramps, as the quad shaders take it, times the
/// dashes at the centre.
pub fn stroke_coverage(x: f32, y: f32, shape: &StrokeShape) -> f32 {
  if shape.half[0] <= 0.0 || shape.half[1] <= 0.0 {
    return 0.0;
  }
  let centre = stroke_distance(x, y, shape);
  // Far from both edges of the ring a pixel is all in or all out.
  let ring = if centre > 2.0 {
    0.0
  } else if centre < -2.0 {
    1.0
  } else {
    [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)]
      .into_iter()
      .map(|(dx, dy)| ring_sample(x + dx, y + dy, shape))
      .sum::<f32>()
      * 0.25
  };
  if ring <= 0.0 || shape.style.dash[0] <= 0.0 {
    return ring;
  }
  let (position, total) = outline_position(x, y, shape.half, shape.radii, shape.stroke);
  ring * dash_coverage(position, total, shape.style.dash)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn shape(stroke: [f32; 4], radius: f32, radii_at: f32) -> StrokeShape {
    StrokeShape {
      half: [50.0, 20.0],
      radii: [radius; 4],
      stroke,
      style: StrokeStyle {
        radii_at,
        ..StrokeStyle::default()
      },
    }
  }

  #[test]
  fn a_ring_of_one_width_is_what_it_always_was() {
    let ring = shape([2.0; 4], 8.0, 0.0);
    // Mid-edges: the stroke is two pixels deep on every side.
    assert_eq!(stroke_coverage(0.0, -19.0, &ring), 1.0);
    assert_eq!(stroke_coverage(0.0, -17.0, &ring), 0.0);
    assert_eq!(stroke_coverage(-49.0, 0.0, &ring), 1.0);
    assert_eq!(stroke_coverage(-47.0, 0.0, &ring), 0.0);
    // The corner: inside the outer arc of radius 8 and outside the inner one of radius 6.
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let on_ring = (-42.0 - 7.0 * diagonal, -12.0 - 7.0 * diagonal);
    assert_eq!(stroke_coverage(on_ring.0, on_ring.1, &ring), 1.0);
    let in_hole = (-42.0 - 4.0 * diagonal, -12.0 - 4.0 * diagonal);
    assert_eq!(stroke_coverage(in_hole.0, in_hole.1, &ring), 0.0);
    let outside = (-42.0 - 9.5 * diagonal, -12.0 - 9.5 * diagonal);
    assert_eq!(stroke_coverage(outside.0, outside.1, &ring), 0.0);
  }

  #[test]
  fn a_stroke_on_one_side_follows_the_corners_at_its_ends_and_thins_out() {
    let left = shape([0.0, 0.0, 0.0, 4.0], 10.0, 0.0);
    // Full width along the straight part of the side.
    assert_eq!(stroke_coverage(-48.0, 0.0, &left), 1.0);
    assert_eq!(stroke_coverage(-45.0, 0.0, &left), 0.0);
    // Outside the rounded corner there is nothing, where a square band would paint.
    assert_eq!(stroke_coverage(-49.5, -19.5, &left), 0.0);
    // Along the corner the stroke gets thinner towards the top edge, and is gone where the top edge starts.
    let depth_at = |angle_deg: f32| {
      let angle = angle_deg.to_radians();
      (0..40)
        .map(|step| 10.0 - step as f32 * 0.25)
        .filter(|radius| {
          let (x, y) = (-40.0 - radius * angle.cos(), -10.0 - radius * angle.sin());
          stroke_distance(x, y, &left) < 0.0
        })
        .count() as f32
        * 0.25
    };
    let (low, middle, high) = (depth_at(10.0), depth_at(45.0), depth_at(80.0));
    assert!(low > middle && middle > high, "{low} {middle} {high}");
    assert!(
      (3.5..=4.0).contains(&low),
      "nearly the whole width near the side: {low}"
    );
    assert!(high < 1.0, "nearly nothing near the top edge: {high}");
  }

  #[test]
  fn a_side_with_no_stroke_has_no_coverage_at_its_edge() {
    let left = shape([0.0, 0.0, 0.0, 4.0], 10.0, 0.0);
    for offset in [-0.4_f32, -0.1, 0.0, 0.1, 0.4] {
      // The top, right and bottom edges, at fractions of a pixel.
      assert_eq!(stroke_coverage(10.0, -20.0 + offset, &left), 0.0, "top at {offset}");
      assert_eq!(stroke_coverage(50.0 + offset, 3.0, &left), 0.0, "right at {offset}");
      assert_eq!(stroke_coverage(10.0, 20.0 + offset, &left), 0.0, "bottom at {offset}");
      // The corners on the far side.
      let diagonal = std::f32::consts::FRAC_1_SQRT_2 * (10.0 + offset);
      assert_eq!(
        stroke_coverage(40.0 + diagonal, -10.0 - diagonal, &left),
        0.0,
        "corner at {offset}"
      );
    }
  }

  #[test]
  fn radii_of_the_hole_grow_the_outer_corner_by_the_width() {
    // An element 96 by 36 with corners of 8 and a border of 2 outside it: the rect is the box grown by 2.
    let outside = shape([2.0; 4], 8.0, 1.0);
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    // The corner's centre is the element's: 8 in from the element's edges, 10 from the rect's.
    let (cx, cy) = (-40.0, -10.0);
    let at = |radius: f32| stroke_coverage(cx - radius * diagonal, cy - radius * diagonal, &outside);
    assert_eq!(at(7.0), 0.0, "inside the element's corner");
    assert_eq!(at(9.0), 1.0, "in the border");
    assert_eq!(at(11.0), 0.0, "outside the border");
  }

  #[test]
  fn a_square_corner_stays_square_wherever_the_radii_are() {
    let outside = shape([2.0; 4], 0.0, 1.0);
    assert_eq!(stroke_coverage(-49.5, -19.5, &outside), 1.0);
    assert_eq!(stroke_coverage(-47.5, -17.5, &outside), 0.0);
  }

  #[test]
  fn the_outline_is_walked_once_clockwise_from_the_start_of_the_top_edge() {
    let (half, radii) = ([50.0, 20.0], [8.0; 4]);
    let at = |x: f32, y: f32| outline_position(x, y, half, radii, [2.0; 4]);
    let quarter = std::f32::consts::FRAC_PI_2 * 8.0;
    let (start, total) = at(-42.0, -20.0);
    assert_eq!(start, 0.0);
    assert!((total - (2.0 * 84.0 + 2.0 * 24.0 + 4.0 * quarter)).abs() < 1e-3);
    let top_end = at(42.0, -20.0).0;
    assert!((top_end - 84.0).abs() < 1e-3);
    let right_start = at(50.0, -12.0).0;
    assert!((right_start - (84.0 + quarter)).abs() < 1e-3);
    let bottom_start = at(42.0, 20.0).0;
    assert!((bottom_start - (84.0 + quarter + 24.0 + quarter)).abs() < 1e-3);
    let left_end = at(-50.0, -12.0).0;
    assert!((left_end - (total - quarter)).abs() < 1e-3);
    // A point in the stroke takes the place of the outline next to it.
    let inside = at(0.0, -18.5).0;
    assert!((inside - 42.0).abs() < 1e-3);
  }

  #[test]
  fn positions_along_the_outline_never_jump() {
    let (half, radii) = ([50.0, 20.0], [8.0, 0.0, 12.0, 4.0]);
    let total = outline_position(0.0, 0.0, half, radii, [2.0; 4]).1;
    // Walk a path one pixel inside the outline.
    let mut previous = None;
    let mut walk = |x: f32, y: f32| {
      let position = outline_position(x, y, half, radii, [2.0; 4]).0;
      if let Some(last) = previous {
        let step: f32 = position - last;
        let step = if step < -total * 0.5 { step + total } else { step };
        // A pixel inside a square corner the walk passes from one edge's end to the next one's start.
        assert!((-0.01..=2.5).contains(&step), "from {last} to {position} at ({x}, {y})");
      }
      previous = Some(position);
    };
    let steps = 400;
    for index in 0..=steps {
      walk(-49.0 + 98.0 * index as f32 / steps as f32, -19.0);
    }
    for index in 0..=steps {
      walk(49.0, -19.0 + 38.0 * index as f32 / steps as f32);
    }
    for index in 0..=steps {
      walk(49.0 - 98.0 * index as f32 / steps as f32, 19.0);
    }
    for index in 0..=steps {
      walk(-49.0, 19.0 - 38.0 * index as f32 / steps as f32);
    }
  }

  #[test]
  fn dashes_fit_the_outline_a_whole_number_of_times() {
    // 100 pixels of outline, dashes of 6 with gaps of 4: ten periods as given.
    assert_eq!(dash_coverage(3.0, 100.0, [6.0, 4.0]), 1.0);
    assert_eq!(dash_coverage(8.0, 100.0, [6.0, 4.0]), 0.0);
    assert_eq!(dash_coverage(13.0, 100.0, [6.0, 4.0]), 1.0);
    // 104 pixels: still ten periods, each a little longer, so the end meets the start with a whole gap.
    let dash = 6.0 * 10.4 / 10.0;
    assert_eq!(dash_coverage(dash - 1.0, 104.0, [6.0, 4.0]), 1.0);
    assert_eq!(dash_coverage(dash + 1.0, 104.0, [6.0, 4.0]), 0.0);
    assert_eq!(dash_coverage(103.0, 104.0, [6.0, 4.0]), 0.0);
    assert_eq!(
      dash_coverage(0.0, 104.0, [6.0, 4.0]),
      0.5,
      "the edge of a dash is half covered"
    );
    // Not dashed.
    assert_eq!(dash_coverage(8.0, 100.0, [0.0, 0.0]), 1.0);
  }

  #[test]
  fn a_dashed_stroke_is_its_ring_cut_along_the_outline() {
    let dashed = StrokeShape {
      style: StrokeStyle {
        dash: [6.0, 4.0],
        radii_at: 0.0,
      },
      ..shape([2.0; 4], 0.0, 0.0)
    };
    // The outline is 280 long: 28 periods of 10. The top edge starts with a dash.
    assert_eq!(stroke_coverage(-47.0, -19.0, &dashed), 1.0);
    assert_eq!(stroke_coverage(-42.0, -19.0, &dashed), 0.0);
    assert_eq!(stroke_coverage(-37.0, -19.0, &dashed), 1.0);
    // The hole stays empty under a dash.
    assert_eq!(stroke_coverage(-47.0, -10.0, &dashed), 0.0);
  }

  #[test]
  fn the_dashes_of_one_side_run_to_its_ends() {
    // A bottom side on a square box: every point of it is the bottom edge's, the ones in the corners too.
    let (half, radii, stroke) = ([75.0, 20.0], [0.0; 4], [0.0, 0.0, 2.0, 0.0]);
    let bottom_edge = 150.0 + 40.0;
    for (x, along) in [(74.5, 0.5), (73.5, 1.5), (0.0, 75.0), (-73.5, 148.5), (-74.5, 149.5)] {
      for y in [18.5, 19.5] {
        let position = outline_position(x, y, half, radii, stroke).0;
        assert!(
          (position - (bottom_edge + along)).abs() < 1e-3,
          "({x}, {y}): {position}"
        );
      }
    }
  }
}
