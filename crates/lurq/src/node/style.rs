use std::sync::atomic::{AtomicU32, Ordering};

use crate::{
  layout::layout_kind::{FlexParams, FrameConstraints},
  node::{
    background_color::BackgroundColor,
    border::{Border, BorderRadius, Borders, ThemedBorderRadius},
    border_size_value::BorderSizeValue,
    box_shadow::BoxShadowValue,
    cursor::CursorIcon,
    dimension::Dimension,
    interaction_state::InteractionState,
    padding::Padding,
    radius_value::RadiusValue,
    spacing_value::SpacingValue,
  },
};

#[derive(Clone, Default)]
pub struct Style {
  pub(crate) color: Option<BackgroundColor>,
  pub(crate) border_radius: Option<ThemedBorderRadius>,
  pub(crate) border: Option<Borders>,
  pub(crate) box_shadow: Option<Box<BoxShadowValue>>,
  pub(crate) cursor: Option<CursorIcon>,
  pub(crate) frame: Option<FrameConstraints>,
  pub(crate) padding: Option<Padding>,
  pub(crate) flex: Option<FlexParams>,
}

#[derive(Clone, Default)]
pub(crate) struct StateStyles {
  pub(crate) hovered: Option<Style>,
  pub(crate) active: Option<Style>,
  pub(crate) focused: Option<Style>,
  pub(crate) focus_visible: Option<Style>,
  /// While files from the OS are dragged over the node.
  pub(crate) drag_over: Option<Style>,
  layout_sources: LayoutStyleSources,
}

// Style merging replaces each layout field as a whole. Cache its winning
// state rather than copying paint properties or allocating their shadows.
// The low byte holds interaction bits; the other bytes select layout fields.
// The empty StateStyles is shared by LazyBox, so this cache must be Sync.
#[derive(Default)]
struct LayoutStyleSources(AtomicU32);

impl Clone for LayoutStyleSources {
  fn clone(&self) -> Self {
    Self(AtomicU32::new(self.0.load(Ordering::Relaxed)))
  }
}

#[cfg(test)]
thread_local! {
  pub(crate) static STYLE_OPERATIONS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

impl Style {
  pub fn new() -> Self {
    #[cfg(test)]
    STYLE_OPERATIONS.with(|counts| counts.set((counts.get().0 + 1, counts.get().1)));
    Self::default()
  }

  pub fn background(mut self, color: impl Into<BackgroundColor>) -> Self {
    self.color = Some(color.into());
    self
  }

  pub fn width(mut self, width: impl Into<Dimension>) -> Self {
    self.frame_mut().width = Some(width.into());
    self
  }

  pub fn height(mut self, height: impl Into<Dimension>) -> Self {
    self.frame_mut().height = Some(height.into());
    self
  }

  pub fn size(mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    let frame = self.frame_mut();
    frame.width = Some(width.into());
    frame.height = Some(height.into());
    self
  }

  pub fn min_width(mut self, width: impl Into<Dimension>) -> Self {
    self.frame_mut().min_width = Some(width.into());
    self
  }

  pub fn max_width(mut self, width: impl Into<Dimension>) -> Self {
    self.frame_mut().max_width = Some(width.into());
    self
  }

  pub fn min_height(mut self, height: impl Into<Dimension>) -> Self {
    self.frame_mut().min_height = Some(height.into());
    self
  }

  pub fn max_height(mut self, height: impl Into<Dimension>) -> Self {
    self.frame_mut().max_height = Some(height.into());
    self
  }

  pub fn min_size(mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    let frame = self.frame_mut();
    frame.min_width = Some(width.into());
    frame.min_height = Some(height.into());
    self
  }

  pub fn max_size(mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    let frame = self.frame_mut();
    frame.max_width = Some(width.into());
    frame.max_height = Some(height.into());
    self
  }

  pub fn frame(mut self, frame: FrameConstraints) -> Self {
    self.frame = Some(frame);
    self
  }

  pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
    let padding = padding.into();
    if let Some(existing) = &mut self.padding {
      existing.merge_from(&padding);
    } else {
      self.padding = Some(padding);
    }
    self
  }

  pub fn padding_custom(mut self, padding: Padding) -> Self {
    self.padding = Some(padding);
    self
  }

  pub fn padding_horizontal(self, value: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::horizontal(value.into()))
  }

  pub fn padding_vertical(self, value: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::vertical(value.into()))
  }

  pub fn padding_left(mut self, value: impl Into<SpacingValue>) -> Self {
    self.padding = Some(self.padding.unwrap_or_default().left(value.into()));
    self
  }

  pub fn padding_right(mut self, value: impl Into<SpacingValue>) -> Self {
    self.padding = Some(self.padding.unwrap_or_default().right(value.into()));
    self
  }

  pub fn padding_top(mut self, value: impl Into<SpacingValue>) -> Self {
    self.padding = Some(self.padding.unwrap_or_default().top(value.into()));
    self
  }

  pub fn padding_bottom(mut self, value: impl Into<SpacingValue>) -> Self {
    self.padding = Some(self.padding.unwrap_or_default().bottom(value.into()));
    self
  }

  pub fn flex(mut self, factor: f32) -> Self {
    self.flex = Some(FlexParams::grow(factor));
    self
  }

  pub fn flex_shrink(mut self, factor: f32) -> Self {
    self.flex = Some(FlexParams {
      grow: 0.0,
      shrink: factor,
      basis: None,
    });
    self
  }

  pub fn flex_full(mut self, grow: f32, shrink: f32, basis: Option<f32>) -> Self {
    self.flex = Some(FlexParams { grow, shrink, basis });
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

  pub fn corner_radius_top_left(mut self, radius: impl Into<RadiusValue>) -> Self {
    self
      .border_radius
      .get_or_insert_with(ThemedBorderRadius::default)
      .top_left = radius.into();
    self
  }

  pub fn corner_radius_top_right(mut self, radius: impl Into<RadiusValue>) -> Self {
    self
      .border_radius
      .get_or_insert_with(ThemedBorderRadius::default)
      .top_right = radius.into();
    self
  }

  pub fn corner_radius_bottom_right(mut self, radius: impl Into<RadiusValue>) -> Self {
    self
      .border_radius
      .get_or_insert_with(ThemedBorderRadius::default)
      .bottom_right = radius.into();
    self
  }

  pub fn corner_radius_bottom_left(mut self, radius: impl Into<RadiusValue>) -> Self {
    self
      .border_radius
      .get_or_insert_with(ThemedBorderRadius::default)
      .bottom_left = radius.into();
    self
  }

  pub fn rounded(mut self, radius: impl Into<RadiusValue>) -> Self {
    self.border_radius = Some(ThemedBorderRadius::all(radius));
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

  pub fn border_top(mut self, border: Border) -> Self {
    self.border.get_or_insert_with(Borders::default).top = Some(border);
    self
  }

  pub fn border_right(mut self, border: Border) -> Self {
    self.border.get_or_insert_with(Borders::default).right = Some(border);
    self
  }

  pub fn border_bottom(mut self, border: Border) -> Self {
    self.border.get_or_insert_with(Borders::default).bottom = Some(border);
    self
  }

  pub fn border_left(mut self, border: Border) -> Self {
    self.border.get_or_insert_with(Borders::default).left = Some(border);
    self
  }

  /// The box shadow while this style applies, for example a higher
  /// [`ShadowStyle`](crate::app::theme::ShadowStyle) on hover.
  /// [`BoxShadowValue::none`] removes the element's own shadow.
  pub fn box_shadow(mut self, shadow: impl Into<BoxShadowValue>) -> Self {
    self.box_shadow = Some(Box::new(shadow.into()));
    self
  }

  pub fn cursor(mut self, cursor: CursorIcon) -> Self {
    self.cursor = Some(cursor);
    self
  }

  pub(crate) fn merge_from(&mut self, other: &Style) {
    #[cfg(test)]
    STYLE_OPERATIONS.with(|counts| counts.set((counts.get().0, counts.get().1 + 1)));
    if other.color.is_some() {
      self.color = other.color.clone();
    }
    if other.border_radius.is_some() {
      self.border_radius = other.border_radius;
    }
    if other.border.is_some() {
      self.border = other.border.clone();
    }
    if other.box_shadow.is_some() {
      self.box_shadow = other.box_shadow.clone();
    }
    if other.cursor.is_some() {
      self.cursor = other.cursor;
    }
    if other.frame.is_some() {
      self.frame = other.frame;
    }
    if other.padding.is_some() {
      self.padding = other.padding.clone();
    }
    if other.flex.is_some() {
      self.flex = other.flex;
    }
  }

  pub(crate) fn affects_layout(&self) -> bool {
    self.frame.is_some() || self.padding.is_some() || self.flex.is_some()
  }

  fn frame_mut(&mut self) -> &mut FrameConstraints {
    self.frame.get_or_insert_with(FrameConstraints::default)
  }
}

impl StateStyles {
  pub(crate) fn invalidate_layout_style(&mut self) {
    self.layout_sources.0.store(0, Ordering::Relaxed);
  }

  fn style_at(&self, index: u8) -> Option<&Style> {
    match index {
      1 => self.focused.as_ref(),
      2 => self.focus_visible.as_ref(),
      3 => self.hovered.as_ref(),
      4 => self.active.as_ref(),
      5 => self.drag_over.as_ref(),
      _ => None,
    }
  }

  fn layout_sources(&self, state: &InteractionState) -> [u8; 3] {
    if !self.affects_layout() {
      return [0; 3];
    }
    let states = state.layout_states();
    let state_bits = states
      .iter()
      .enumerate()
      .fold(0, |bits, (index, active)| bits | ((*active as u32) << index));
    let cached = self.layout_sources.0.load(Ordering::Relaxed);
    if cached & 0xff == state_bits {
      return [(cached >> 8) as u8, (cached >> 16) as u8, (cached >> 24) as u8];
    }
    let mut fields = [0; 3];
    for (index, active) in states.into_iter().enumerate() {
      if active && let Some(style) = self.style_at(index as u8 + 1) {
        for (field, present) in [style.frame.is_some(), style.padding.is_some(), style.flex.is_some()]
          .into_iter()
          .enumerate()
        {
          if present {
            fields[field] = index as u8 + 1;
          }
        }
      }
    }
    let sources = state_bits | ((fields[0] as u32) << 8) | ((fields[1] as u32) << 16) | ((fields[2] as u32) << 24);
    self.layout_sources.0.store(sources, Ordering::Relaxed);
    fields
  }

  pub(crate) fn layout_frame(&self, state: &InteractionState) -> Option<FrameConstraints> {
    self
      .style_at(self.layout_sources(state)[0])
      .and_then(|style| style.frame)
  }

  pub(crate) fn layout_padding(&self, state: &InteractionState) -> Option<Padding> {
    self
      .style_at(self.layout_sources(state)[1])
      .and_then(|style| style.padding)
  }

  pub(crate) fn layout_flex(&self, state: &InteractionState) -> Option<FlexParams> {
    self
      .style_at(self.layout_sources(state)[2])
      .and_then(|style| style.flex)
  }

  pub(crate) fn affects_layout(&self) -> bool {
    self.hovered.as_ref().is_some_and(Style::affects_layout)
      || self.active.as_ref().is_some_and(Style::affects_layout)
      || self.focused.as_ref().is_some_and(Style::affects_layout)
      || self.focus_visible.as_ref().is_some_and(Style::affects_layout)
      || self.drag_over.as_ref().is_some_and(Style::affects_layout)
  }
}
