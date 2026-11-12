//! Backdrop blur: a node blurs what was painted behind its box, like CSS
//! `backdrop-filter: blur()`, and then paints itself over the result.
//!
//! Layout gives such a node a [`QuadContent::Backdrop`](crate::layout::quad::QuadContent::Backdrop)
//! quad before its own quads, and the runtime turns it into a
//! [`BackdropCmd`]. What is "behind" is what the node's target holds when the
//! command's order comes up: the window, or the layer the node is inside (a
//! faded or blurred ancestor), so a layer's backdrop blurs see only the
//! layer's own content.
//!
//! A render engine stops painting the target there, copies the pixels around
//! the node's box out of it, blurs the copy with the passes of a layer blur
//! (see [`crate::layout::opacity_layer`]) and puts the result back inside
//! the box, in place of what was there ([`BackdropRegion`]).

#[cfg(any(feature = "render", feature = "devtools"))]
use crate::layout::opacity_layer::{LayerBlur, LayerBounds, LayerCmd};
use crate::layout::quad::ClipRect;

/// Blurs what the target holds inside a box before the draws of `order` and
/// after it are painted.
#[derive(Clone, Copy)]
pub struct BackdropCmd {
  /// The render order the blur happens at: after every draw of a lower
  /// order, before every draw of a higher one. No draw has this order.
  pub order: usize,
  /// The node's box in physical pixels of the window.
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  /// Corner radii of the box: top left, top right, bottom right, bottom left.
  pub radii: [f32; 4],
  /// The 2x2 transform of the box around `transform_origin` (relative to the
  /// box), as for a [`RectCmd`](crate::layout::render_list::RectCmd). The
  /// blur covers the transformed box's bounding box; see [`BackdropRegion`].
  pub transform: [f32; 4],
  pub transform_origin: [f32; 2],
  /// The clip the node is under. The blur is cut to its rectangle.
  pub clip: ClipRect,
  /// Standard deviation of the Gaussian, in physical pixels.
  pub sigma: f32,
  /// How much of the blurred backdrop replaces the sharp one, `0.0..=1.0`:
  /// the node's own opacity.
  pub opacity: f32,
}

impl std::fmt::Debug for BackdropCmd {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let clip = self
      .clip
      .active
      .then_some([self.clip.x, self.clip.y, self.clip.width, self.clip.height]);
    f.debug_struct("BackdropCmd")
      .field("order", &self.order)
      .field("x", &self.x)
      .field("y", &self.y)
      .field("width", &self.width)
      .field("height", &self.height)
      .field("radii", &self.radii)
      .field("transform", &self.transform)
      .field("transform_origin", &self.transform_origin)
      .field("clip", &clip)
      .field("sigma", &self.sigma)
      .field("opacity", &self.opacity)
      .finish()
  }
}

/// A backdrop blur below this many physical pixels of deviation changes no
/// pixel.
#[cfg(any(feature = "render", feature = "devtools"))]
const MIN_BACKDROP_SIGMA: f32 = 0.05;

/// What a [`BackdropCmd`] comes to in one target.
#[cfg(any(feature = "render", feature = "devtools"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropRegion {
  /// The blur as a layer: `bounds` are the window pixels it changes, the
  /// [`LayerBlur`] rectangle the pixels copied out of the target (the
  /// bounds grown by the blur's reach, within the target), and `opacity` the
  /// command's. Its orders are unused.
  pub layer: LayerCmd,
  /// The box the blurred pixels are put back in, in physical pixels of the
  /// window (x, y, width, height), and its corner radii.
  pub mask: [f32; 4],
  pub radii: [f32; 4],
}

#[cfg(any(feature = "render", feature = "devtools"))]
impl BackdropRegion {
  /// The region of `cmd` in a target covering `target_size` window pixels
  /// from `target_origin`. `None` when the blur changes nothing there.
  pub(crate) fn new(cmd: &BackdropCmd, target_origin: [i32; 2], target_size: [u32; 2]) -> Option<Self> {
    if !(cmd.sigma >= MIN_BACKDROP_SIGMA && cmd.opacity > 0.0 && cmd.width > 0.0 && cmd.height > 0.0) {
      return None;
    }
    let (mask, radii) = mask_of(cmd);
    let mut shown = [mask[0], mask[1], mask[0] + mask[2], mask[1] + mask[3]];
    if cmd.clip.active {
      shown = [
        shown[0].max(cmd.clip.x),
        shown[1].max(cmd.clip.y),
        shown[2].min(cmd.clip.x + cmd.clip.width),
        shown[3].min(cmd.clip.y + cmd.clip.height),
      ];
    }
    // The target's pixels inside the window: a layer may be painted for
    // pixels outside it, which are never shown.
    let target = [
      target_origin[0].max(0),
      target_origin[1].max(0),
      target_origin[0] + target_size[0] as i32,
      target_origin[1] + target_size[1] as i32,
    ];
    if !shown.iter().all(|value| value.is_finite()) {
      return None;
    }
    let x0 = (shown[0].floor() as i32).max(target[0]);
    let y0 = (shown[1].floor() as i32).max(target[1]);
    let x1 = (shown[2].ceil() as i32).min(target[2]);
    let y1 = (shown[3].ceil() as i32).min(target[3]);
    if x1 <= x0 || y1 <= y0 {
      return None;
    }
    // As for a blurred layer: the reach of the blur, and what the passes'
    // sampling adds. Past the target's edge its edge pixels repeat.
    let pad = crate::layout::opacity_layer::blur_pad(cmd.sigma);
    let copied_x0 = (x0 - pad).max(target_origin[0]);
    let copied_y0 = (y0 - pad).max(target_origin[1]);
    let copied_x1 = (x1 + pad).min(target[2]);
    let copied_y1 = (y1 + pad).min(target[3]);
    Some(Self {
      layer: LayerCmd {
        start_order: cmd.order,
        end_order: cmd.order,
        opacity: cmd.opacity.clamp(0.0, 1.0),
        bounds: LayerBounds {
          x: x0 as u32,
          y: y0 as u32,
          width: (x1 - x0) as u32,
          height: (y1 - y0) as u32,
        },
        blur: Some(LayerBlur {
          sigma: cmd.sigma,
          x: copied_x0,
          y: copied_y0,
          width: (copied_x1 - copied_x0) as u32,
          height: (copied_y1 - copied_y0) as u32,
        }),
      },
      mask,
      radii,
    })
  }
}

/// The box a backdrop blur fills, and its corner radii. An untransformed box
/// is the node's own. Under a scale or a flip it is the scaled box, with the
/// radii scaled by the smaller factor; under any other transform it is the
/// bounding box of the transformed one, without radii.
#[cfg(any(feature = "render", feature = "devtools"))]
fn mask_of(cmd: &BackdropCmd) -> ([f32; 4], [f32; 4]) {
  let [a, b, c, d] = cmd.transform;
  if a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 {
    return ([cmd.x, cmd.y, cmd.width, cmd.height], cmd.radii);
  }
  let [origin_x, origin_y] = cmd.transform_origin;
  let corners = [(0.0, 0.0), (cmd.width, 0.0), (0.0, cmd.height), (cmd.width, cmd.height)].map(|(x, y)| {
    let (dx, dy) = (x - origin_x, y - origin_y);
    (cmd.x + origin_x + a * dx + c * dy, cmd.y + origin_y + b * dx + d * dy)
  });
  let x0 = corners.iter().map(|corner| corner.0).fold(f32::INFINITY, f32::min);
  let y0 = corners.iter().map(|corner| corner.1).fold(f32::INFINITY, f32::min);
  let x1 = corners.iter().map(|corner| corner.0).fold(f32::NEG_INFINITY, f32::max);
  let y1 = corners.iter().map(|corner| corner.1).fold(f32::NEG_INFINITY, f32::max);
  let radii = if b == 0.0 && c == 0.0 {
    let scale = a.abs().min(d.abs());
    let [top_left, top_right, bottom_right, bottom_left] = cmd.radii.map(|radius| radius * scale);
    // A flip swaps the corners across its axis.
    let (top_left, top_right, bottom_right, bottom_left) = if a < 0.0 {
      (top_right, top_left, bottom_left, bottom_right)
    } else {
      (top_left, top_right, bottom_right, bottom_left)
    };
    if d < 0.0 {
      [bottom_left, bottom_right, top_right, top_left]
    } else {
      [top_left, top_right, bottom_right, bottom_left]
    }
  } else {
    [0.0; 4]
  };
  ([x0, y0, x1 - x0, y1 - y0], radii)
}

#[cfg(all(test, any(feature = "render", feature = "devtools")))]
mod tests {
  use super::*;

  fn cmd(x: f32, y: f32, width: f32, height: f32, sigma: f32) -> BackdropCmd {
    BackdropCmd {
      order: 3,
      x,
      y,
      width,
      height,
      radii: [1.0, 2.0, 3.0, 4.0],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [width * 0.5, height * 0.5],
      clip: ClipRect::default(),
      sigma,
      opacity: 1.0,
    }
  }

  #[test]
  fn the_copied_pixels_reach_past_the_box_and_stop_at_the_target() {
    // Sigma 4 reaches 12 pixels; one pass adds 3 for its sampling.
    let region = BackdropRegion::new(&cmd(40.5, 30.0, 50.0, 20.0, 4.0), [0, 0], [200, 60]).unwrap();
    assert_eq!(
      region.layer.bounds,
      LayerBounds {
        x: 40,
        y: 30,
        width: 51,
        height: 20
      }
    );
    let blur = region.layer.blur.unwrap();
    assert_eq!((blur.x, blur.y), (40 - 15, 30 - 15));
    // Below, the target ends 10 pixels under the box.
    assert_eq!((blur.x + blur.width as i32, blur.y + blur.height as i32), (91 + 15, 60));
    assert_eq!(region.mask, [40.5, 30.0, 50.0, 20.0]);
    assert_eq!(region.radii, [1.0, 2.0, 3.0, 4.0]);
  }

  #[test]
  fn a_clip_and_the_target_cut_what_is_blurred() {
    let mut clipped = cmd(40.0, 30.0, 50.0, 20.0, 4.0);
    clipped.clip = ClipRect {
      x: 60.0,
      y: 0.0,
      width: 100.0,
      height: 100.0,
      active: true,
      border_radius: None,
    };
    let region = BackdropRegion::new(&clipped, [0, 0], [200, 60]).unwrap();
    assert_eq!((region.layer.bounds.x, region.layer.bounds.width), (60, 30));
    // The mask is still the whole box: its corners stay where they are.
    assert_eq!(region.mask[0], 40.0);

    // A layer target that starts inside the box and outside the window.
    let region = BackdropRegion::new(&cmd(-20.0, 10.0, 100.0, 20.0, 4.0), [-30, 0], [70, 60]).unwrap();
    assert_eq!((region.layer.bounds.x, region.layer.bounds.width), (0, 40));
    assert_eq!(region.layer.blur.unwrap().x, -15);

    assert!(BackdropRegion::new(&cmd(300.0, 10.0, 50.0, 20.0, 4.0), [0, 0], [200, 60]).is_none());
    assert!(BackdropRegion::new(&cmd(40.0, 10.0, 50.0, 20.0, 0.0), [0, 0], [200, 60]).is_none());
    let mut faded = cmd(40.0, 10.0, 50.0, 20.0, 4.0);
    faded.opacity = 0.0;
    assert!(BackdropRegion::new(&faded, [0, 0], [200, 60]).is_none());
  }

  #[test]
  fn a_transformed_box_is_its_scaled_or_bounding_box() {
    let mut scaled = cmd(40.0, 30.0, 50.0, 20.0, 4.0);
    scaled.transform = [2.0, 0.0, 0.0, 0.5];
    let (mask, radii) = mask_of(&scaled);
    assert_eq!(mask, [15.0, 35.0, 100.0, 10.0]);
    assert_eq!(radii, [0.5, 1.0, 1.5, 2.0]);

    let mut flipped = cmd(40.0, 30.0, 50.0, 20.0, 4.0);
    flipped.transform = [-1.0, 0.0, 0.0, 1.0];
    let (mask, radii) = mask_of(&flipped);
    assert_eq!(mask, [40.0, 30.0, 50.0, 20.0]);
    assert_eq!(radii, [2.0, 1.0, 4.0, 3.0]);

    let mut turned = cmd(40.0, 30.0, 50.0, 20.0, 4.0);
    turned.transform = [0.0, 1.0, -1.0, 0.0];
    let (mask, radii) = mask_of(&turned);
    assert_eq!(mask, [55.0, 15.0, 20.0, 50.0]);
    assert_eq!(radii, [0.0; 4]);
  }
}
