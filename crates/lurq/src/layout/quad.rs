use crate::{
  layout::{
    render_list::RenderGradient,
    text_style::{TextStyle, VerticalAlign},
  },
  node::{
    TextTransformMode,
    border::{BorderRadius, ResolvedBorders},
    color::Color,
    transform::Transform2D,
  },
};

#[derive(Clone, Copy, Default)]
pub struct ClipRect {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub active: bool,
  pub border_radius: Option<BorderRadius>,
}

impl ClipRect {
  /// Maps a clip laid out in a subtree's layout coordinates into the screen
  /// space its transformed quads are painted in (clips are tested against
  /// fragment positions).
  ///
  /// Axis-aligned transforms (scale, flip, translation) map exactly; corner
  /// radii follow flips and scale by the smaller axis scale. Other transforms
  /// clip to the bounding box of the transformed rect without radii, which never
  /// cuts content inside the transformed box.
  pub(crate) fn transformed(self, transform: Transform2D) -> Self {
    if !self.active || transform.is_identity() {
      return self;
    }
    let corners = [
      (self.x, self.y),
      (self.x + self.width, self.y),
      (self.x, self.y + self.height),
      (self.x + self.width, self.y + self.height),
    ]
    .map(|(x, y)| transform.transform_point(x, y));
    let min_x = corners.iter().map(|corner| corner.0).fold(f32::INFINITY, f32::min);
    let min_y = corners.iter().map(|corner| corner.1).fold(f32::INFINITY, f32::min);
    let max_x = corners.iter().map(|corner| corner.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = corners.iter().map(|corner| corner.1).fold(f32::NEG_INFINITY, f32::max);
    let border_radius = if transform.is_axis_aligned() {
      self.border_radius.map(|radius| axis_aligned_radius(radius, transform))
    } else {
      None
    };
    Self {
      x: min_x,
      y: min_y,
      width: max_x - min_x,
      height: max_y - min_y,
      active: true,
      border_radius,
    }
  }
}

fn axis_aligned_radius(radius: BorderRadius, transform: Transform2D) -> BorderRadius {
  let scale = transform.a.abs().min(transform.d.abs());
  let (flip_x, flip_y) = (transform.a < 0.0, transform.d < 0.0);
  let corner = |top: bool, left: bool| {
    let value = match (top != flip_y, left != flip_x) {
      (true, true) => radius.top_left,
      (true, false) => radius.top_right,
      (false, false) => radius.bottom_right,
      (false, true) => radius.bottom_left,
    };
    value * scale
  };
  BorderRadius {
    top_left: corner(true, true),
    top_right: corner(true, false),
    bottom_right: corner(false, false),
    bottom_left: corner(false, true),
  }
}

pub struct Quad {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  /// Opacity within the quad's opacity layer. A node with `opacity < 1`
  /// fades its subtree as one layer (`RenderList::layers`), so its quads
  /// keep `1.0` here, unless the node paints a single primitive, which takes
  /// the node's opacity directly (see [`crate::layout::opacity_layer`]).
  pub opacity: f32,
  pub transform: Transform2D,
  pub transform_origin: Option<[f32; 2]>,
  pub content: QuadContent,
  pub border_radius: Option<BorderRadius>,
  pub border: Option<ResolvedBorders>,
  pub clip: ClipRect,
}

#[doc(hidden)]
pub use crate::app::glyph_engine::input_tokens::DecoratedInputLayout;

pub enum QuadContent {
  #[doc(hidden)]
  DecoratedInput {
    layout: std::sync::Arc<DecoratedInputLayout>,
  },
  Rect {
    color: Color,
    gradient: Option<RenderGradient>,
  },
  /// Blurs what is painted behind the quad's box by this radius, in logical
  /// pixels, before the quads after it are painted. Paints nothing itself;
  /// the quad's opacity is how much of the blurred backdrop shows. See
  /// [`crate::layout::backdrop`].
  Backdrop {
    blur: f32,
  },
  Text {
    text: String,
    style: TextStyle,
    wrap: bool,
    vertical_align: VerticalAlign,
    transform_mode: TextTransformMode,
  },
  RichText {
    spans: Vec<RichTextSpan>,
    wrap: bool,
    vertical_align: VerticalAlign,
    transform_mode: TextTransformMode,
  },
  #[cfg(feature = "raster")]
  Image {
    data: crate::images::ImageData,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
  },
  #[cfg(feature = "raster")]
  Video {
    data: crate::images::ImageData,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
  },
  #[cfg(feature = "svg")]
  Svg {
    data: crate::svg::SvgData,
  },
  /// A box shadow cast by the quad's box (its `border_radius` included). For
  /// an inset shadow the box is the element's padding box.
  BoxShadow(crate::node::box_shadow::ResolvedBoxShadow),
  None,
}

#[derive(Clone, PartialEq)]
pub struct RichTextSpan {
  pub text: String,
  pub style: TextStyle,
}
