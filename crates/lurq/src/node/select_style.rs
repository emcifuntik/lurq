use crate::{
  app::theme::{BorderSize, PaletteColor, RadiusSize, TypographyStyle},
  layout::{scrollbar::ScrollBarStyle, text_style::TextStyle},
  node::{
    BackgroundColor, BoxShadowValue, Node, SpacingValue, TextColor,
    border::{Border, BorderRadius, Borders, ThemedBorderRadius},
    border_size_value::BorderSizeValue,
    padding::Padding,
    radius_value::RadiusValue,
    select_icon::{SelectCheckmarkPosition, SelectIcon},
  },
};

/// Visual style for one box-like part of a `Select` (the trigger, the menu
/// container, or an option row). Every field is optional so state overrides
/// (hovered/focused/open/selected/highlighted/disabled) can merge on top of a
/// base part.
#[derive(Clone, Default)]
pub struct SelectPartStyle {
  pub(crate) background: Option<BackgroundColor>,
  pub(crate) border: Option<Borders>,
  pub(crate) border_radius: Option<ThemedBorderRadius>,
  pub(crate) box_shadow: Option<BoxShadowValue>,
  pub(crate) padding: Option<Padding>,
  pub(crate) text: Option<TextStyle>,
  pub(crate) typography: Option<TypographyStyle>,
  pub(crate) text_color: Option<TextColor>,
  pub(crate) opacity: Option<f32>,
  pub(crate) min_width: Option<f32>,
  pub(crate) min_height: Option<f32>,
}

impl SelectPartStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn background(mut self, color: impl Into<BackgroundColor>) -> Self {
    self.background = Some(color.into());
    self
  }

  pub fn rounded(mut self, radius: impl Into<RadiusValue>) -> Self {
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

  pub fn border_custom(mut self, border: Borders) -> Self {
    self.border = Some(border);
    self
  }

  pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
    self.padding = Some(padding.into());
    self
  }

  /// An explicit text style. A [`typography`](Self::typography) role, when
  /// set, takes precedence over it.
  pub fn text(mut self, text: TextStyle) -> Self {
    self.text = Some(text);
    self
  }

  /// Text in a theme typography role.
  pub fn typography(mut self, typography: impl Into<TypographyStyle>) -> Self {
    self.typography = Some(typography.into());
    self
  }

  /// Text colour, a `Color` or a `PaletteColor` role; overrides the colour of
  /// the text style or typography role.
  pub fn text_color(mut self, color: impl Into<TextColor>) -> Self {
    self.text_color = Some(color.into());
    self
  }

  /// A box shadow: a `ShadowStyle` role, one `BoxShadow` or a list. Inset
  /// shadows draw rings inside the part without changing its border.
  pub fn box_shadow(mut self, shadow: impl Into<BoxShadowValue>) -> Self {
    self.box_shadow = Some(shadow.into());
    self
  }

  /// Opacity of the part and its content, `0.0..=1.0`. Applies to the menu
  /// and option rows; the trigger ignores it.
  pub fn opacity(mut self, opacity: f32) -> Self {
    self.opacity = Some(opacity.clamp(0.0, 1.0));
    self
  }

  pub fn min_width(mut self, width: f32) -> Self {
    self.min_width = Some(width);
    self
  }

  pub fn min_height(mut self, height: f32) -> Self {
    self.min_height = Some(height);
    self
  }

  pub(crate) fn merge_from(&mut self, other: &Self) {
    if other.background.is_some() {
      self.background = other.background.clone();
    }
    if other.border.is_some() {
      self.border = other.border.clone();
    }
    if other.border_radius.is_some() {
      self.border_radius = other.border_radius;
    }
    if other.padding.is_some() {
      self.padding = other.padding;
    }
    if other.box_shadow.is_some() {
      self.box_shadow = other.box_shadow.clone();
    }
    if other.text.is_some() {
      self.text = other.text.clone();
    }
    if other.typography.is_some() {
      self.typography = other.typography;
    }
    if other.text_color.is_some() {
      self.text_color = other.text_color.clone();
    }
    if other.opacity.is_some() {
      self.opacity = other.opacity;
    }
    if other.min_width.is_some() {
      self.min_width = other.min_width;
    }
    if other.min_height.is_some() {
      self.min_height = other.min_height;
    }
  }

  /// The text style glyphs without a typography role inherit.
  pub(crate) fn text_style(&self) -> Option<&TextStyle> {
    self.text.as_ref()
  }

  /// A text node for `content` in this part's text style.
  pub(crate) fn text_node(&self, content: &str) -> Node {
    let node = match (self.typography, self.text.as_ref()) {
      (Some(typography), _) => Node::text(content).text_variant(typography),
      (None, Some(style)) => Node::text_styled(content, style.clone()),
      (None, None) => Node::text(content),
    };
    match &self.text_color {
      Some(color) => node.text_color(color.clone()),
      None => node,
    }
  }
}

/// Full styling surface for a `Select`. Base parts are always present; the
/// state-specific parts (`*_hovered`, `*_focused`, `trigger_open`,
/// `option_selected`, `option_highlighted`, `option_disabled`, ...) merge onto
/// their base when active.
///
/// Option rows merge, in order: `option`, then `option_selected` when
/// selected, `option_hovered` under the pointer, the keyboard highlight, the
/// selected-and-hovered and selected-and-highlighted parts, and finally
/// `option_disabled`. The keyboard highlight uses `option_highlighted` and
/// `option_selected_highlighted` when `option_highlighted` is set; otherwise
/// it looks like the pointer hover (`option_hovered` /
/// `option_selected_hovered`).
#[derive(Clone)]
pub struct SelectStyle {
  pub(crate) trigger: SelectPartStyle,
  pub(crate) trigger_hovered: Option<SelectPartStyle>,
  pub(crate) trigger_focused: Option<SelectPartStyle>,
  pub(crate) trigger_open: Option<SelectPartStyle>,
  pub(crate) placeholder_text: Option<TextStyle>,
  pub(crate) menu: SelectPartStyle,
  pub(crate) menu_scrollbar: Option<ScrollBarStyle>,
  pub(crate) option: SelectPartStyle,
  pub(crate) option_hovered: Option<SelectPartStyle>,
  pub(crate) option_selected: Option<SelectPartStyle>,
  pub(crate) option_selected_hovered: Option<SelectPartStyle>,
  pub(crate) option_highlighted: Option<SelectPartStyle>,
  pub(crate) option_selected_highlighted: Option<SelectPartStyle>,
  pub(crate) option_disabled: Option<SelectPartStyle>,
  pub(crate) option_detail: SelectPartStyle,
  pub(crate) chevron: SelectIcon,
  pub(crate) chevron_open: Option<SelectIcon>,
  pub(crate) chevron_color: Option<TextColor>,
  pub(crate) chevron_size: f32,
  pub(crate) checkmark: SelectIcon,
  pub(crate) checkmark_color: Option<TextColor>,
  pub(crate) checkmark_size: Option<f32>,
  pub(crate) checkmark_position: SelectCheckmarkPosition,
  pub(crate) checkmark_gap: SpacingValue,
  pub(crate) single_checkmark: bool,
  pub(crate) min_menu_width: f32,
  pub(crate) max_menu_height: f32,
  pub(crate) menu_gap: f32,
}

/// Width of the checkmark slot when no `checkmark_size` is set.
pub(crate) const DEFAULT_CHECKMARK_SLOT: f32 = 16.0;

/// Interaction state of one option row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectOptionState {
  pub(crate) hovered: bool,
  pub(crate) highlighted: bool,
  pub(crate) selected: bool,
  pub(crate) disabled: bool,
}

impl Default for SelectStyle {
  fn default() -> Self {
    Self {
      trigger: SelectPartStyle::new()
        .background(PaletteColor::SurfaceInput)
        .border_inside(BorderSize::Sm, PaletteColor::Border)
        .rounded(RadiusSize::Md)
        .padding(Padding::symmetric(10.0, 8.0))
        .min_height(36.0),
      trigger_hovered: Some(SelectPartStyle::new().border_inside(BorderSize::Sm, PaletteColor::BorderFocus)),
      trigger_focused: Some(SelectPartStyle::new().border_inside(BorderSize::Sm, PaletteColor::BorderFocus)),
      trigger_open: Some(SelectPartStyle::new().border_inside(BorderSize::Sm, PaletteColor::BorderFocus)),
      menu: SelectPartStyle::new()
        .background(PaletteColor::SurfaceRaised)
        .border_inside(BorderSize::Sm, PaletteColor::Border)
        .rounded(RadiusSize::Md),
      option: SelectPartStyle::new().padding(Padding::symmetric(10.0, 0.0)),
      option_hovered: Some(SelectPartStyle::new().background(PaletteColor::SurfacePanel)),
      option_selected: Some(SelectPartStyle::new().background(PaletteColor::Accent)),
      option_selected_hovered: Some(SelectPartStyle::new().background(PaletteColor::AccentHover)),
      option_disabled: Some(SelectPartStyle::new().opacity(0.45)),
      option_detail: SelectPartStyle::new()
        .typography(TypographyStyle::Caption)
        .text_color(PaletteColor::TextMuted),
      ..Self::unstyled()
    }
  }
}

impl SelectStyle {
  pub fn new() -> Self {
    Self::default()
  }

  /// A style without fills, borders, padding or state parts, for an app that
  /// styles the select itself. Its trigger paints nothing until a part sets
  /// a `background` or border.
  pub fn unstyled() -> Self {
    Self {
      trigger: SelectPartStyle::default(),
      trigger_hovered: None,
      trigger_focused: None,
      trigger_open: None,
      placeholder_text: None,
      menu: SelectPartStyle::default(),
      menu_scrollbar: None,
      option: SelectPartStyle::default(),
      option_hovered: None,
      option_selected: None,
      option_selected_hovered: None,
      option_highlighted: None,
      option_selected_highlighted: None,
      option_disabled: None,
      option_detail: SelectPartStyle::default(),
      chevron: SelectIcon::default_chevron(),
      chevron_open: None,
      chevron_color: None,
      chevron_size: 10.0,
      checkmark: SelectIcon::default_checkmark(),
      checkmark_color: None,
      checkmark_size: None,
      checkmark_position: SelectCheckmarkPosition::Leading,
      checkmark_gap: SpacingValue::from(6.0),
      single_checkmark: false,
      min_menu_width: 0.0,
      max_menu_height: 240.0,
      menu_gap: 4.0,
    }
  }

  /// Replaces the trigger part. The default part fills the trigger with the
  /// theme's `SurfaceInput`; a part without a `background` paints no fill,
  /// so the trigger shows what is behind it, like a button or text input
  /// without a background.
  pub fn trigger(mut self, style: SelectPartStyle) -> Self {
    self.trigger = style;
    self
  }

  pub fn trigger_hovered(mut self, style: SelectPartStyle) -> Self {
    self.trigger_hovered = Some(style);
    self
  }

  /// Merged over the trigger while the select has keyboard focus
  /// (focus-visible, like a browser's select ring: a click does not show it),
  /// after the hovered part.
  pub fn trigger_focused(mut self, style: SelectPartStyle) -> Self {
    self.trigger_focused = Some(style);
    self
  }

  /// Merged over the trigger while the menu is open, after the hovered and
  /// focused parts.
  pub fn trigger_open(mut self, style: SelectPartStyle) -> Self {
    self.trigger_open = Some(style);
    self
  }

  pub fn placeholder_text(mut self, style: TextStyle) -> Self {
    self.placeholder_text = Some(style);
    self
  }

  /// The menu container. Its padding insets the options inside the scroll
  /// viewport; its box shadow paints around the menu.
  pub fn menu(mut self, style: SelectPartStyle) -> Self {
    self.menu = style;
    self
  }

  /// The menu's scrollbar; the theme scrollbar when unset.
  pub fn menu_scrollbar(mut self, style: ScrollBarStyle) -> Self {
    self.menu_scrollbar = Some(style);
    self
  }

  pub fn option(mut self, style: SelectPartStyle) -> Self {
    self.option = style;
    self
  }

  /// Merged over an option under the pointer. The pointer hover changes the
  /// row's fill, border and box shadow; text styling follows the row's other
  /// states.
  pub fn option_hovered(mut self, style: SelectPartStyle) -> Self {
    self.option_hovered = Some(style);
    self
  }

  pub fn option_selected(mut self, style: SelectPartStyle) -> Self {
    self.option_selected = Some(style);
    self
  }

  pub fn option_selected_hovered(mut self, style: SelectPartStyle) -> Self {
    self.option_selected_hovered = Some(style);
    self
  }

  /// Merged over the option the keyboard highlight is on. Setting it
  /// separates the highlight from the pointer hover, e.g. an inset ring drawn
  /// with an inset `box_shadow` that composes with the hover fill.
  pub fn option_highlighted(mut self, style: SelectPartStyle) -> Self {
    self.option_highlighted = Some(style);
    self
  }

  /// Merged over a selected option the keyboard highlight is on, after
  /// `option_highlighted`. Only used when `option_highlighted` is set.
  pub fn option_selected_highlighted(mut self, style: SelectPartStyle) -> Self {
    self.option_selected_highlighted = Some(style);
    self
  }

  /// Merged last over a disabled option. Disabled options never take the
  /// pointer hover or the keyboard highlight.
  pub fn option_disabled(mut self, style: SelectPartStyle) -> Self {
    self.option_disabled = Some(style);
    self
  }

  /// Text style of an option's detail line (see `SelectOption::detail`); only
  /// its text fields apply.
  pub fn option_detail(mut self, style: SelectPartStyle) -> Self {
    self.option_detail = style;
    self
  }

  /// Replaces the chevron glyph `▾`.
  pub fn chevron(mut self, icon: SelectIcon) -> Self {
    self.chevron = icon;
    self
  }

  /// A different chevron while the menu is open; the closed chevron is used
  /// when unset.
  pub fn chevron_open(mut self, icon: SelectIcon) -> Self {
    self.chevron_open = Some(icon);
    self
  }

  /// Chevron colour, a `Color` or `PaletteColor` role.
  pub fn chevron_color(mut self, color: impl Into<TextColor>) -> Self {
    self.chevron_color = Some(color.into());
    self
  }

  /// Font size of a chevron glyph without a typography role, which sits in a
  /// slot `size + 4` wide. Typography glyphs and elements keep their natural
  /// size.
  pub fn chevron_size(mut self, size: f32) -> Self {
    self.chevron_size = size;
    self
  }

  /// Replaces the checkmark glyph `✓`.
  pub fn checkmark(mut self, icon: SelectIcon) -> Self {
    self.checkmark = icon;
    self
  }

  /// Checkmark colour, a `Color` or `PaletteColor` role.
  pub fn checkmark_color(mut self, color: impl Into<TextColor>) -> Self {
    self.checkmark_color = Some(color.into());
    self
  }

  /// Width of the checkmark slot, which every option reserves so labels do
  /// not shift, and the font size of a checkmark glyph without a typography
  /// role. Unset, the slot is 16 wide and the glyph uses the option's text
  /// size.
  pub fn checkmark_size(mut self, size: f32) -> Self {
    self.checkmark_size = Some(size.max(0.0));
    self
  }

  pub fn checkmark_position(mut self, position: SelectCheckmarkPosition) -> Self {
    self.checkmark_position = position;
    self
  }

  /// Space between the checkmark slot and the label; a number or a spacing
  /// role. Defaults to 6.
  pub fn checkmark_gap(mut self, gap: impl Into<SpacingValue>) -> Self {
    self.checkmark_gap = gap.into();
    self
  }

  /// Shows the checkmark on the selected option of a single-select too. Off
  /// by default: single-select marks its selection with `option_selected`
  /// only. Multi-select always shows checkmarks.
  pub fn single_checkmark(mut self, show: bool) -> Self {
    self.single_checkmark = show;
    self
  }

  pub fn max_menu_height(mut self, height: f32) -> Self {
    self.max_menu_height = height;
    self
  }

  /// Keeps the open menu at least this wide when its trigger is narrower.
  /// The menu still fits inside the viewport.
  pub fn min_menu_width(mut self, width: f32) -> Self {
    self.min_menu_width = width;
    self
  }

  pub fn menu_gap(mut self, gap: f32) -> Self {
    self.menu_gap = gap;
    self
  }

  /// The trigger part resolved for the current interaction/open state.
  pub(crate) fn resolved_trigger(&self, hovered: bool, focus_visible: bool, open: bool) -> SelectPartStyle {
    let mut style = self.trigger.clone();
    if hovered && let Some(part) = &self.trigger_hovered {
      style.merge_from(part);
    }
    if focus_visible && let Some(part) = &self.trigger_focused {
      style.merge_from(part);
    }
    if open && let Some(part) = &self.trigger_open {
      style.merge_from(part);
    }
    style
  }

  /// An option row resolved for its state; see the order on [`SelectStyle`].
  pub(crate) fn resolved_option(&self, state: SelectOptionState) -> SelectPartStyle {
    let SelectOptionState {
      hovered,
      highlighted,
      selected,
      disabled,
    } = state;
    let (hovered, highlighted) = (hovered && !disabled, highlighted && !disabled);
    let (highlight, selected_highlight) = match &self.option_highlighted {
      Some(part) => (Some(part), self.option_selected_highlighted.as_ref()),
      None => (self.option_hovered.as_ref(), self.option_selected_hovered.as_ref()),
    };
    let layers = [
      (selected, self.option_selected.as_ref()),
      (hovered, self.option_hovered.as_ref()),
      (highlighted, highlight),
      (selected && hovered, self.option_selected_hovered.as_ref()),
      (selected && highlighted, selected_highlight),
      (disabled, self.option_disabled.as_ref()),
    ];
    let mut style = self.option.clone();
    for part in layers.into_iter().filter_map(|(active, part)| part.filter(|_| active)) {
      style.merge_from(part);
    }
    style
  }

  /// Whether option rows draw a checkmark slot.
  pub(crate) fn shows_checkmark(&self, multiple: bool) -> bool {
    multiple || self.single_checkmark
  }

  pub(crate) fn checkmark_slot_width(&self) -> f32 {
    self.checkmark_size.unwrap_or(DEFAULT_CHECKMARK_SLOT)
  }
}
