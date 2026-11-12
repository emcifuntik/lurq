#[cfg(all(feature = "image", feature = "resources"))]
use std::sync::Arc;

#[cfg(feature = "raster")]
use crate::node::BackgroundSize;
use crate::node::{
  BackgroundColor,
  border::{Border, BorderRadius, Borders, ThemedBorderRadius},
  border_size_value::BorderSizeValue,
  color::Color,
  radius_value::RadiusValue,
};

#[derive(Clone, Default)]
pub struct CheckboxStyle {
  pub(crate) width: Option<f32>,
  pub(crate) height: Option<f32>,
  pub(crate) color: Option<Color>,
  pub(crate) border_radius: Option<ThemedBorderRadius>,
  pub(crate) border: Option<Borders>,
  pub(crate) indicator_width: Option<f32>,
  pub(crate) indicator_height: Option<f32>,
  #[cfg(feature = "raster")]
  pub(crate) indicator_image: Option<crate::images::ImageData>,
  #[cfg(all(feature = "image", feature = "resources"))]
  pub(crate) indicator_resource_image: Option<Arc<str>>,
  #[cfg(feature = "raster")]
  pub(crate) indicator_size: BackgroundSize,
}

impl CheckboxStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn width(mut self, width: f32) -> Self {
    self.width = Some(width);
    self
  }

  pub fn height(mut self, height: f32) -> Self {
    self.height = Some(height);
    self
  }

  pub fn size(mut self, width: f32, height: f32) -> Self {
    self.width = Some(width);
    self.height = Some(height);
    self
  }

  pub fn background(mut self, color: impl Into<Color>) -> Self {
    self.color = Some(color.into());
    self
  }

  pub fn rounded(mut self, radius: impl Into<RadiusValue>) -> Self {
    self.border_radius = Some(ThemedBorderRadius::all(radius));
    self
  }

  pub fn corner_radius(mut self, radius: impl Into<RadiusValue>) -> Self {
    self.border_radius = Some(ThemedBorderRadius::all(radius));
    self
  }

  pub fn corner_radius_custom(mut self, radius: BorderRadius) -> Self {
    self.border_radius = Some(radius.into());
    self
  }

  pub fn border_inside(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self.border = Some(Borders::all(Border::inside(width, color)));
    self
  }

  pub fn border_outside(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self.border = Some(Borders::all(Border::outside(width, color)));
    self
  }

  pub fn border_center(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self.border = Some(Borders::all(Border::center(width, color)));
    self
  }

  pub fn border(mut self, border: Border) -> Self {
    self.border = Some(Borders::all(border));
    self
  }

  pub fn border_custom(mut self, border: Borders) -> Self {
    self.border = Some(border);
    self
  }

  pub fn indicator_width(mut self, width: f32) -> Self {
    self.indicator_width = Some(width);
    self
  }

  pub fn indicator_height(mut self, height: f32) -> Self {
    self.indicator_height = Some(height);
    self
  }

  pub fn indicator_size(mut self, width: f32, height: f32) -> Self {
    self.indicator_width = Some(width);
    self.indicator_height = Some(height);
    self
  }

  #[cfg(feature = "raster")]
  pub fn indicator_image(mut self, data: impl Into<crate::images::ImageKind>) -> Self {
    match data.into() {
      crate::images::ImageKind::Bytes(data) => {
        self.indicator_image = Some(data);
      }
      crate::images::ImageKind::Native(data) => {
        self.indicator_image = Some(data.image_data());
      }
      #[cfg(all(feature = "image", feature = "resources"))]
      crate::images::ImageKind::Resource(path) => {
        self.indicator_resource_image = Some(path);
      }
    }
    self
  }

  #[cfg(feature = "raster")]
  pub fn indicator_background_size(mut self, size: BackgroundSize) -> Self {
    self.indicator_size = size;
    self
  }

  #[cfg(feature = "raster")]
  pub fn indicator_cover(self) -> Self {
    self.indicator_background_size(BackgroundSize::Cover)
  }

  #[cfg(feature = "raster")]
  pub fn indicator_contain(self) -> Self {
    self.indicator_background_size(BackgroundSize::Contain)
  }

  pub(crate) fn merge_from(&mut self, other: &Self) {
    if other.width.is_some() {
      self.width = other.width;
    }
    if other.height.is_some() {
      self.height = other.height;
    }
    if other.color.is_some() {
      self.color = other.color;
    }
    if other.border_radius.is_some() {
      self.border_radius = other.border_radius;
    }
    if other.border.is_some() {
      self.border = other.border.clone();
    }
    if other.indicator_width.is_some() {
      self.indicator_width = other.indicator_width;
    }
    if other.indicator_height.is_some() {
      self.indicator_height = other.indicator_height;
    }
    #[cfg(feature = "raster")]
    {
      if other.indicator_image.is_some() {
        self.indicator_image = other.indicator_image.clone();
      }
      #[cfg(all(feature = "image", feature = "resources"))]
      if other.indicator_resource_image.is_some() {
        self.indicator_resource_image = other.indicator_resource_image.clone();
      }
      self.indicator_size = other.indicator_size;
    }
  }
}
