use std::sync::Arc;

use crate::{
  app::theme::TypographyStyle,
  layout::text_style::TextStyle,
  node::{Element, Node, TextColor},
};

/// What a `Select` draws for its chevron or an option's checkmark.
///
/// The default chevron is the text glyph `▾` and the default checkmark `✓`,
/// both drawn in the surrounding part's text style. Apps can supply a glyph in
/// a typography role (an icon-font code point with an `Extra` role that selects
/// the icon font and size) or an app-built element such as an SVG icon.
#[derive(Clone)]
pub enum SelectIcon {
  /// A text glyph. With a typography role the glyph uses that role's font,
  /// size and colour; without one it inherits the surrounding part's text
  /// style at the configured size. A configured colour overrides both.
  Glyph {
    glyph: Arc<str>,
    typography: Option<TypographyStyle>,
  },
  /// App-built content. The supplier receives the configured colour
  /// (`chevron_color` / `checkmark_color`), or `None` when none is set. Built
  /// menus are plain nodes: return an icon, SVG, text or layout element, not
  /// a stateful component.
  Element(Arc<dyn Fn(Option<TextColor>) -> Element + Send + Sync>),
}

impl SelectIcon {
  /// A glyph drawn in the surrounding part's text style.
  pub fn text(glyph: impl Into<Arc<str>>) -> Self {
    Self::Glyph {
      glyph: glyph.into(),
      typography: None,
    }
  }

  /// A glyph drawn in a typography role, e.g. an icon-font code point with
  /// `TypographyStyle::extra("icon")`.
  pub fn glyph(glyph: impl Into<Arc<str>>, typography: impl Into<TypographyStyle>) -> Self {
    Self::Glyph {
      glyph: glyph.into(),
      typography: Some(typography.into()),
    }
  }

  /// App-built content; see [`SelectIcon::Element`].
  pub fn element(supplier: impl Fn(Option<TextColor>) -> Element + Send + Sync + 'static) -> Self {
    Self::Element(Arc::new(supplier))
  }

  pub(crate) fn default_chevron() -> Self {
    Self::text("\u{25BE}")
  }

  pub(crate) fn default_checkmark() -> Self {
    Self::text("\u{2713}")
  }

  /// Whether the icon keeps its natural width (typography glyphs and
  /// elements) instead of a slot sized from the configured size.
  pub(crate) fn is_plain_text(&self) -> bool {
    matches!(self, Self::Glyph { typography: None, .. })
  }

  /// Builds the icon node. `base` is the surrounding part's text style, used
  /// by glyphs without a typography role, with `size` as their font size.
  pub(crate) fn build(&self, base: Option<&TextStyle>, size: Option<f32>, color: Option<&TextColor>) -> Node {
    match self {
      Self::Glyph {
        glyph,
        typography: Some(typography),
      } => with_color(Node::text(glyph).text_variant(*typography).text_wrap(false), color),
      Self::Glyph {
        glyph,
        typography: None,
      } => {
        let mut style = base.cloned().unwrap_or_default();
        if let Some(size) = size {
          style.font_size = size;
        }
        with_color(Node::text_styled(glyph, style).text_wrap(false), color)
      }
      Self::Element(supplier) => supplier(color.cloned()).into_node(),
    }
  }
}

fn with_color(node: Node, color: Option<&TextColor>) -> Node {
  match color {
    Some(color) => node.text_color(color.clone()),
    None => node,
  }
}

/// What an option's leading or trailing element is built for: the state of
/// its row in the open menu, or the closed field showing the selected option.
/// `SelectOption::leading_with` and `trailing_with` receive it each time the
/// row or field is built, so the element can follow the label's look.
///
/// The pointer hover is not a state here: it repaints a row's fill, border
/// and shadow without rebuilding the row, as for the label.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct SelectAdornmentState {
  /// The option is selected.
  pub selected: bool,
  /// The keyboard highlight is on the option's row. `false` on the field.
  pub highlighted: bool,
  /// The option is disabled.
  pub disabled: bool,
  /// The element is built for the closed field, not for a row of the menu.
  pub in_field: bool,
  /// The `text_color` of the row's resolved part (`option` with its state
  /// parts merged), or of the trigger part on the field; `None` when the
  /// part sets none and the label uses its text style's colour.
  pub text_color: Option<TextColor>,
}

/// Builds an option's leading or trailing element for a state.
pub(crate) type SelectAdornment = Arc<dyn Fn(&SelectAdornmentState) -> Element + Send + Sync>;

/// Which side of an option row the checkmark slot sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectCheckmarkPosition {
  /// Before the label (the default).
  #[default]
  Leading,
  /// After the label, at the row's trailing edge.
  Trailing,
}
