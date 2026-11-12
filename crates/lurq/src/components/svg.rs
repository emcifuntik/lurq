use crate::{impl_into_node, svg::SvgData};

impl_into_node!(Svg);

impl Svg {
  pub fn new(data: SvgData) -> Self {
    Self::from_node(crate::node::Node::svg(data))
  }

  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self::new(SvgData::from_bytes(bytes))
  }

  // Public constructor that cannot fail; `FromStr` would need an error type and a trait in scope.
  #[allow(clippy::should_implement_trait)]
  pub fn from_str(svg: &str) -> Self {
    Self::new(SvgData::from_str(svg))
  }

  #[cfg(feature = "resources")]
  pub fn from_resource(path: &str) -> Self {
    Self::from_node(crate::node::Node::resource_svg(path))
  }
}
