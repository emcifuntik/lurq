---
title: Theme
description: Semantic palette, typography, radius, spacing, border size, shadow, scrollbar, and form theme roles, with named extras.
---

# Theme

The runtime theme is a set of semantic roles. Palette colors, typography styles, radius sizes, spacing sizes, border sizes, and shadow styles are enums with a matching field on the theme structs for each built-in role, plus an `Extra` variant for application-defined roles (see [Extra Roles](#extra-roles)). The scrollbar style and breakpoints have no extras.

Use concrete colors and dimensions for one-off visuals. Use theme roles when the value should follow the active runtime theme.

```rust
use lurq::{
  app::theme::{BorderSize, PaletteColor, RadiusSize, SpacingSize, TypographyStyle},
  components::{Column, Rect, Text},
};

Column::new()
  .spacing(SpacingSize::Md)
  .padding(SpacingSize::Lg)
  .child(Text::new("Settings").variant(TypographyStyle::Title))
  .child(
    Rect::new(120.0, 36.0)
      .background(PaletteColor::Accent)
      .border_inside(BorderSize::Sm, PaletteColor::Border)
      .rounded(RadiusSize::Md),
  )
```

## Runtime Theme

`App::theme()` returns the app theme. `Tree::mount_root` passes that theme into the root context, and descendants read it with `ctx.theme()`.

```rust
let theme = app.theme();
theme.set_palette_color(PaletteColor::Accent, lurq::node::color::Color::from_hex("#2563eb"));
tree.mount_root::<Root>(&mut app, ());
```

Inside components:

```rust
let accent = ctx.theme().palette_color(PaletteColor::Accent);
let gap = ctx.theme().spacing_value(SpacingSize::Sm);
```

Calling `ctx.theme()` during render subscribes that component to theme version changes. Mutating the theme rerenders subscribers on the next pass.

The main theme accessors are:

| Method | Purpose |
| --- | --- |
| `theme.palette()` / `theme.set_palette(...)` | Read or replace `ThemePalette`. |
| `theme.palette_color(key)` / `theme.set_palette_color(key, color)` | Read or set one palette role. |
| `theme.typography()` / `theme.set_typography(...)` | Read or replace `ThemeTypography`. |
| `theme.typography_style(key)` / `theme.set_typography_style(key, style)` | Read or set one typography role. |
| `theme.default_text_style()` / `theme.set_default_text_style(style)` | Compatibility alias for the `body` typography style. |
| `theme.radii()` / `theme.set_radii(...)` | Read or replace `ThemeRadii`. |
| `theme.radius_value(key)` / `theme.set_radius_value(key, value)` | Read or set one radius role. |
| `theme.spacing()` / `theme.set_spacing(...)` | Read or replace `ThemeSpacing`. |
| `theme.spacing_value(key)` / `theme.set_spacing_value(key, value)` | Read or set one spacing role. |
| `theme.border_sizes()` / `theme.set_border_sizes(...)` | Read or replace `ThemeBorderSizes`. |
| `theme.border_size_value(key)` / `theme.set_border_size_value(key, value)` | Read or set one border-size role. |
| `theme.shadows()` / `theme.set_shadows(...)` | Read or replace `ThemeShadows`. |
| `theme.shadow_style(key)` / `theme.set_shadow_style(key, shadows)` | Read or set one shadow role (a list of `BoxShadow`s). |
| `theme.scrollbar()` / `theme.set_scrollbar(...)` | Read or replace the default `ScrollBarStyle`. |
| `theme.breakpoints()` / `theme.set_breakpoints(...)` | Read or replace `ThemeBreakpoints`. |
| `theme.breakpoint_value(key)` / `theme.set_breakpoint_value(key, value)` | Read or set one breakpoint threshold. |
| `theme.form()` / `theme.set_form(...)` | Read or replace `FormTheme`; requires the `form` feature. |

Use `theme.lens(getter, setter)` when UI code needs a focused mutable handle for one theme value:

```rust
use lurq::{app::theme::PaletteColor, node::color::Color};

let brand = ctx.theme().lens(
  |theme| theme.palette_color(PaletteColor::Accent),
  |theme, color| theme.set_palette_color(PaletteColor::Accent, color),
);

brand.set(Color::from_hex("#2563eb"));
```

## Palette

Palette roles are named by `PaletteColor` and stored as public fields on `ThemePalette`.

```rust
use lurq::{app::theme::PaletteColor, node::color::Color};

app.theme().set_palette_color(PaletteColor::Accent, Color::from_hex("#2563eb"));

let mut palette = lurq::app::theme::ThemePalette::default();
palette.surface_base = Color::from_hex("#ffffff");
palette.border_focus = Color::from_hex("#2563eb");
app.theme().set_palette(palette);
```

Available roles:

| `PaletteColor` | `ThemePalette` field | Default |
| --- | --- | --- |
| `Accent` | `accent` | `#2563eb` |
| `AccentHover` | `accent_hover` | `#1d4ed8` |
| `AccentMuted` | `accent_muted` | `#dbeafe` |
| `SurfaceBase` | `surface_base` | `#ffffff` |
| `SurfacePanel` | `surface_panel` | `#f8fafc` |
| `SurfaceRaised` | `surface_raised` | `#ffffff` |
| `SurfaceInput` | `surface_input` | `#ffffff` |
| `Border` | `border` | `#e2e8f0` |
| `BorderStrong` | `border_strong` | `#94a3b8` |
| `BorderFocus` | `border_focus` | `#2563eb` |
| `TextPrimary` | `text_primary` | `#0f172a` |
| `TextSecondary` | `text_secondary` | `#334155` |
| `TextMuted` | `text_muted` | `#64748b` |
| `TextInverse` | `text_inverse` | `#ffffff` |
| `Success` | `success` | `#16a34a` |
| `SuccessMuted` | `success_muted` | `#dcfce7` |
| `Warning` | `warning` | `#d97706` |
| `WarningMuted` | `warning_muted` | `#fef3c7` |
| `Danger` | `danger` | `#dc2626` |
| `DangerMuted` | `danger_muted` | `#fee2e2` |
| `Info` | `info` | `#0284c7` |
| `InfoMuted` | `info_muted` | `#e0f2fe` |

`PaletteColor::Extra(name)` names an application-defined color stored in `ThemePalette::extra`, a `HashMap<Arc<str>, Color>`. Build one with `PaletteColor::extra("brand")`; a `&str` or `Arc<str>` converts into it, so the theme setters accept names directly:

```rust
use lurq::{app::theme::PaletteColor, node::color::Color};

app.theme().set_palette_color("brand", Color::from_hex("#7c3aed"));
let brand = app.theme().palette_color(PaletteColor::extra("brand"));
```

Palette roles can be passed anywhere a background or text color accepts a theme color:

```rust
use lurq::{app::theme::PaletteColor, components::{Rect, Text}};

Rect::new(80.0, 32.0).background(PaletteColor::Accent);
Text::new("Muted").color(PaletteColor::TextMuted);
```

## Typography

Typography roles are named by `TypographyStyle` and stored as public fields on `ThemeTypography`.

```rust
use lurq::{
  app::theme::TypographyStyle,
  layout::text_style::{FontWeight, TextAlign, TextStyle},
};

app.theme().set_typography_style(TypographyStyle::Heading, TextStyle {
  font_size: 28.0,
  weight: FontWeight::Bold,
  text_align: TextAlign::Left,
  ..TextStyle::default()
});
```

Available roles:

| `TypographyStyle` | `ThemeTypography` field | Default |
| --- | --- | --- |
| `Heading` | `heading` | `24px`, bold |
| `Title` | `title` | `20px`, bold |
| `Body` | `body` | `TextStyle::default()` |
| `Description` | `description` | `14px` |
| `Caption` | `caption` | `12px` |
| `Label` | `label` | `13px`, medium |
| `FieldLabel` | `field_label` | `13px`, medium |
| `Button` | `button` | `13px`, medium |
| `Link` | `link` | body defaults |
| `Mono` | `mono` | body defaults with the generic `monospace` family |

`TypographyStyle::Extra(name)` names an application-defined style stored in `ThemeTypography::extra`, a `HashMap<Arc<str>, TextStyle>`; see [Extra Roles](#extra-roles).

`Text::new` uses `TypographyStyle::Body`. Use `.variant(...)` for themed text, and `Text::styled(...)` for a one-off style that should not follow a typography role.

`TextStyle::text_align` supports `TextAlign::Left`, `Center`, `Right`, `Justified`, and `End`. `Text::text_align(...)` aligns text inside the text node's box, and `TextInput::text_align(...)` applies the same alignment to value and placeholder text inside the input content box. Both builders also accept layout `Alignment`.

`Text::text_overflow(...)` accepts `TextOverflow::Clip` or `TextOverflow::Elipsis`. `Clip` is the default. `Elipsis` renders a single-line text quad with `…` when the text is wider than its available width.

```rust
use lurq::{
  app::theme::TypographyStyle,
  components::{Text, TextOverflow},
};

Text::new("Headline").variant(TypographyStyle::Heading);
Text::new("Caption").variant(TypographyStyle::Caption);
Text::new("Long endpoint name").text_overflow(TextOverflow::Elipsis);
```

### Generic Families

`TextStyle::font_family` (and `CanvasFont`'s family) may name a CSS generic family: `"serif"`, `"sans-serif"`, `"monospace"`, `"cursive"` or `"fantasy"`, in any ASCII case. It draws with the platform's font for that family rather than a font of that name, and `sans-serif` draws as the empty family does. [`GenericFamily`](https://docs.rs/lurq/latest/lurq/layout/text_style/enum.GenericFamily.html) lists the fonts of every platform and the fallbacks. The default `Mono` role, Markdown's inline code, code blocks and tables, and the devtools use `monospace`.

The fonts are chosen when the `App` is created. To draw a generic family in a font the app bundles, load the font and register an alias of the keyword; an alias is resolved before the keyword, in any ASCII case:

```rust
app.install_fonts(
  [include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").to_vec()],
  [("monospace", "JetBrains Mono")],
);
```

### Font Weight

`FontWeight` has the CSS named weights and a numeric escape hatch:

| `FontWeight` | Weight |
| --- | --- |
| `Thin` | `100` |
| `ExtraLight` | `200` |
| `Light` | `300` |
| `Normal` | `400` (default) |
| `Medium` | `500` |
| `SemiBold` | `600` |
| `Bold` | `700` |
| `ExtraBold` | `800` |
| `Black` | `900` |
| `Numeric(n)` | `n`, clamped to `1..=1000` |

`FontWeight::value()` returns the number. Weights compare and hash by value, so `FontWeight::Numeric(600) == FontWeight::SemiBold`, and `FontWeight::from(600)` builds a numeric weight.

Text uses the loaded face of the requested family chosen by CSS font matching, as implemented by fontdb. An exact weight wins. Otherwise a request of 400–449 tries 500 next and one of 450–500 tries 400 next; then requests up to 500 take the nearest lighter face and requests above 500 the nearest heavier face, falling back to the nearest face on the other side. Faces are never synthesized: with only Regular and Bold loaded, `Medium` renders Regular and `SemiBold` renders Bold. Load every weight the design uses, for example:

```rust
app.install_fonts(
  [
    include_bytes!("../assets/fonts/Inter-Regular.ttf").to_vec(),
    include_bytes!("../assets/fonts/Inter-Medium.ttf").to_vec(),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf").to_vec(),
    include_bytes!("../assets/fonts/Inter-Bold.ttf").to_vec(),
  ],
  [("ui", "Inter")],
);
```

The resolved weight is cached per family and cleared whenever fonts are loaded. A family with no loaded faces is matched against the generic sans-serif family, where its text falls back.

### Letter Spacing

`TextStyle::letter_spacing` adds space after every glyph, in logical pixels; negative values tighten text. The default is `0.0`. As with CSS `letter-spacing`, spaces are spaced too and the last glyph of a line keeps its trailing spacing, so `"abcd"` with `-1.0` measures 4px narrower. Spacing scales with the display scale factor like `font_size`, and measurement, wrapping, painting, carets, hit testing, and selection all use the spaced advances.

```rust
use lurq::{
  app::theme::TypographyStyle,
  components::{Text, TextInput},
  layout::text_style::{FontWeight, TextStyle},
};

app.theme().set_typography_style(TypographyStyle::Heading, TextStyle {
  font_size: 28.0,
  weight: FontWeight::Bold,
  letter_spacing: -0.5,
  ..TextStyle::default()
});

Text::new("Overview").variant(TypographyStyle::Heading).letter_spacing(-1.0);
TextInput::new(query.clone()).letter_spacing(0.5);
```

`Text::letter_spacing(...)` overrides the spacing of whatever style the text resolves to, including typography roles. `TextInput::letter_spacing(...)` sets it on the value and placeholder styles; a later `text_style(...)` or `placeholder_style(...)` replaces it, as with `text_align(...)`. Markdown styles take `MarkdownTextStyle::letter_spacing`, which `font_size_scale` does not scale, and canvas text takes `CanvasFont::letter_spacing`. Within rich text every span's spacing is in pixels, whatever its font size. Non-finite values are treated as `0.0`.

### Font Features

`TextStyle::font_features` holds OpenType feature settings, like CSS `font-feature-settings`. Each `FontFeature` is a four-byte tag and a value: `0` turns the feature off, `1` turns it on, and features that pick an alternate (`salt`, `cvNN`) take its index. The default is empty, which keeps the shaper's default features, including standard ligatures (`liga`), contextual alternates (`calt`), and kerning.

Programming ligatures can misrender commands and code. Geist Mono, for example, shapes `--` into one cell-wide glyph drawn over the preceding cell, so `gh auth login --hostname` shows as `login--hostname` and the line is one cell shorter. Define a mono role with them off:

```rust
use lurq::{
  app::theme::TypographyStyle,
  components::Text,
  layout::text_style::{FontFeature, FontFeatures, TextStyle},
};

let no_ligatures = FontFeatures::new([FontFeature::disable(*b"liga"), FontFeature::disable(*b"calt")]);
app.theme().set_typography_style(TypographyStyle::Mono, TextStyle {
  font_family: "Geist Mono".into(),
  font_size: 13.0,
  font_features: no_ligatures.clone(),
  ..TextStyle::default()
});

Text::new("gh auth login --hostname github.com").variant(TypographyStyle::Mono);
Text::new("a -> b").variant(TypographyStyle::Mono).font_features(FontFeatures::default());
```

Settings are stored sorted by tag with one setting per tag; when a tag appears more than once the last setting wins, so equal settings compare and cache alike in any order. Font features are part of every text cache key, and measurement, wrapping, painting, carets, hit testing, and selection all use the shaped glyphs. Tags a font does not have are ignored.

`Text::font_features(...)` overrides the settings of whatever style the text resolves to, including typography roles. `TextInput::font_features(...)` sets them on the value and placeholder styles; a later `text_style(...)` or `placeholder_style(...)` replaces them. Markdown styles take `MarkdownTextStyle::font_features` (for example on `ThemeMarkdown::inline_code` and `code_block`), which replaces the base style's settings, and canvas text takes `CanvasFont::font_features`. In rich text, cosmic-text shapes a word whose spans share a font as one run with the first span's features, so give spans that meet inside a word the same settings.

`ThemeFonts` remains as a compatibility shape with `body`, `heading`, and `mono`. Converting it into `ThemeTypography` only fills those three roles and leaves the rest at defaults.

## Radius

Radius roles are named by `RadiusSize` and stored as public fields on `ThemeRadii`.

| `RadiusSize` | `ThemeRadii` field | Default |
| --- | --- | --- |
| `Sm` | `sm` | `3.0` |
| `Md` | `md` | `5.0` |
| `Lg` | `lg` | `6.0` |

Use radius roles anywhere a corner radius accepts a `RadiusValue`:

```rust
use lurq::{app::theme::RadiusSize, components::Rect};

app.theme().set_radius_value(RadiusSize::Md, 5.0);

Rect::new(100.0, 40.0)
  .rounded(RadiusSize::Md)
  .corner_radius_top_left(RadiusSize::Sm);
```

## Spacing

Spacing roles are named by `SpacingSize` and stored as public fields on `ThemeSpacing`.

| `SpacingSize` | `ThemeSpacing` field | Default |
| --- | --- | --- |
| `Xs` | `xs` | `4px` |
| `Sm` | `sm` | `8px` |
| `Md` | `md` | `12px` |
| `Lg` | `lg` | `16px` |
| `Xl` | `xl` | `24px` |
| `Section` | `section` | `32px` |

Use spacing roles for container gaps and padding:

```rust
use lurq::{app::theme::SpacingSize, components::Column};

app.theme().set_spacing_value(SpacingSize::Section, 40.0);

Column::new()
  .spacing(SpacingSize::Sm)
  .padding(SpacingSize::Lg);
```

Spacing values are `Dimension`s, so a role can be set to pixel, percentage, or auto dimensions where that makes sense:

```rust
use lurq::{app::theme::SpacingSize, node::dimension::Dimension};

app.theme().set_spacing_value(SpacingSize::Md, Dimension::Px(14.0));
```

## Border Size

Border-size roles are named by `BorderSize` and stored as public fields on `ThemeBorderSizes`.

| `BorderSize` | `ThemeBorderSizes` field | Default |
| --- | --- | --- |
| `Sm` | `sm` | `1px` |
| `Md` | `md` | `2px` |
| `Lg` | `lg` | `3px` |

Use border-size roles anywhere a border width accepts a `BorderSizeValue`:

```rust
use lurq::{app::theme::{BorderSize, PaletteColor}, components::Rect};

app.theme().set_border_size_value(BorderSize::Md, 2.0);

Rect::new(100.0, 40.0)
  .border_inside(BorderSize::Sm, PaletteColor::Border)
  .focused(|style| style.border_inside(BorderSize::Md, PaletteColor::BorderFocus));
```

## Shadow

Shadow (elevation) roles are named by `ShadowStyle` and stored as public fields on `ThemeShadows`. Each role is a list of [box shadows](../styling-events/#box-shadows), painted first on top, so one role can stack a tight contact shadow over a soft ambient one. Lists are shared `Arc<[BoxShadow]>` values: reading a role or cloning the table does not copy them.

| `ShadowStyle` | `ThemeShadows` field | Default (`offset_x offset_y blur spread color`) |
| --- | --- | --- |
| `Sm` | `sm` | `0 1 2 0 #0000000d` |
| `Md` | `md` | `0 4 6 -1 #0000001a`, `0 2 4 -2 #0000001a` |
| `Lg` | `lg` | `0 10 15 -3 #0000001a`, `0 4 6 -4 #0000001a` |

Use shadow roles anywhere a box shadow is accepted, including hover, active, and focus styles:

```rust
use lurq::{
  app::theme::{PaletteColor, RadiusSize, ShadowStyle},
  components::Rect,
  node::{BoxShadow, color::Color},
};

app.theme().set_palette_color(PaletteColor::extra("shadow"), Color::from_hex("#0f172a33"));
app.theme().set_shadow_style(ShadowStyle::Md, vec![
  BoxShadow::new(0.0, 4.0, 12.0, PaletteColor::extra("shadow")).spread(-2.0),
]);

Rect::new(240.0, 120.0)
  .background(PaletteColor::SurfaceRaised)
  .rounded(RadiusSize::Lg)
  .box_shadow(ShadowStyle::Md)
  .hovered(|style| style.box_shadow(ShadowStyle::Lg));
```

A shadow's color is a `BackgroundColor`, so a palette role (as above) keeps shadows in step with light and dark palettes. Setting a role to an empty list removes that elevation everywhere it is used. Changing a shadow role repaints; it never relayouts, because shadows do not take part in layout.

## Extra Roles

Every role enum except `Breakpoint` has an `Extra` variant for roles the built-in set does not cover, such as a design system's overline or card radius. Each theme struct stores extras in a public `extra` map keyed by `Arc<str>`:

| Role | Extra variant | Storage |
| --- | --- | --- |
| `PaletteColor` | `Extra(Arc<str>)` | `ThemePalette::extra: HashMap<Arc<str>, Color>` |
| `TypographyStyle` | `Extra(RoleName)` | `ThemeTypography::extra: HashMap<Arc<str>, TextStyle>` |
| `RadiusSize` | `Extra(RoleName)` | `ThemeRadii::extra: HashMap<Arc<str>, f32>` |
| `SpacingSize` | `Extra(RoleName)` | `ThemeSpacing::extra: HashMap<Arc<str>, Dimension>` |
| `BorderSize` | `Extra(RoleName)` | `ThemeBorderSizes::extra: HashMap<Arc<str>, f32>` |
| `ShadowStyle` | `Extra(RoleName)` | `ThemeShadows::extra: HashMap<Arc<str>, Arc<[BoxShadow]>>` |

Build a role with `extra(name)`, or convert a `&str` or `Arc<str>`. The theme setters therefore take names directly:

```rust
use lurq::{
  app::theme::{RadiusSize, ShadowStyle, SpacingSize, TypographyStyle},
  components::{Column, Rect, Text},
  layout::text_style::{FontWeight, TextStyle},
  node::BoxShadow,
};

let theme = app.theme();
theme.set_typography_style("overline", TextStyle {
  font_size: 9.0,
  weight: FontWeight::SemiBold,
  ..TextStyle::default()
});
theme.set_radius_value("card", 10.0);
theme.set_spacing_value("gutter", 20.0);
theme.set_shadow_style("popover", vec![BoxShadow::new(0.0, 12.0, 32.0, "#0f172a40").spread(-4.0)]);

Column::new()
  .padding(SpacingSize::extra("gutter"))
  .child(Text::new("RECENT").variant("overline"))
  .child(
    Rect::new(200.0, 120.0)
      .rounded(RadiusSize::extra("card"))
      .box_shadow(ShadowStyle::extra("popover")),
  );
```

Radius, spacing, border-size, typography, and shadow roles are `Copy`, nest in `Copy` values such as `Padding`, and are stored many times in every element, so their names are interned: `RoleName` is a 4-byte handle to a name stored once for the life of the process, and each of these roles is 8 bytes. `RoleName` dereferences to `str` and compares equal to a `&str`; `as_str()` returns the name. Use a fixed vocabulary of role names, not per-item data.

A missing extra name follows the palette:

- The theme tables' `get` and `resolve` panic, for example `radius size not found: card`. `try_get` and `try_resolve` return `None`. The `Theme` accessors (`palette_color`, `typography_style`, `radius_value`, `spacing_value`, `border_size_value`, `shadow_style`) call `get`.
- Nodes never panic. As an unresolved palette color paints nothing, an unresolved radius, spacing, or border size resolves to `0`, an unresolved shadow style paints no shadow, and an unresolved typography variant uses the default text style.

`Breakpoint` has no extras: `Responsive` orders overrides by the `Breakpoint` enum, not by threshold, so a named threshold could not be placed in that order.

## Scrollbar

`theme.scrollbar()` is the default style for scroll components. Set it once to make scrollbars consistent across the app:

```rust
use lurq::{
  layout::scrollbar::{ScrollBarStyle, ScrollBarVisibility},
  node::color::Color,
};

app.theme().set_scrollbar(ScrollBarStyle {
  visible: ScrollBarVisibility::Auto,
  width: 7.0,
  thumb_color: Color::from_hex("#64748b"),
  thumb_radius: 4.0,
  ..ScrollBarStyle::default()
});
```

Scroll components can still override the theme default with `.scrollbar(...)`. `.scrollbar_hovered(...)` receives the effective style, whether it came from the theme or the component.

## Breakpoints

Breakpoints are named viewport-width thresholds, stored as public fields on `ThemeBreakpoints` and keyed by `Breakpoint`. Thresholds are logical pixels and expected to be non-decreasing.

| `Breakpoint` | `ThemeBreakpoints` field | Default |
| --- | --- | --- |
| `Sm` | `sm` | `640.0` |
| `Md` | `md` | `768.0` |
| `Lg` | `lg` | `1024.0` |
| `Xl` | `xl` | `1280.0` |

Configure thresholds through the theme, one role or all at once:

```rust
use lurq::app::theme::{Breakpoint, ThemeBreakpoints};

app.theme().set_breakpoint_value(Breakpoint::Md, 820.0);

app.theme().set_breakpoints(ThemeBreakpoints {
  sm: 600.0,
  md: 900.0,
  lg: 1200.0,
  xl: 1600.0,
});
```

Inside components, read the current breakpoint with `ctx.breakpoint()`. It resolves the window's logical width against the theme thresholds and returns `Option<Breakpoint>`, where `None` is the base tier (narrower than `Sm`). Reading it subscribes the component to breakpoint changes only — it rerenders when the resolved breakpoint crosses a threshold, not on every resize.

```rust
use lurq::{app::theme::Breakpoint, components::{Column, Row}};

fn render(&self, ctx: &mut Ctx) -> Element {
  if ctx.breakpoint() >= Some(Breakpoint::Lg) {
    Row::new().child(nav).child(content).into()
  } else {
    Column::new().child(nav).child(content).into()
  }
}
```

### Responsive Values

`Responsive<T>` holds a base value plus per-breakpoint overrides. Resolution is mobile-first: for the current breakpoint it uses the value set there or, if unset, the nearest smaller breakpoint that is set, falling back to `base`. Resolve it with `ctx.responsive(...)`.

```rust
use lurq::responsive::Responsive;

let columns = Responsive::new(1).md(2).lg(3).xl(4);
let count = ctx.responsive(&columns); // 1 below md, 2 at md, 3 at lg, 4 at xl
```

Any `T` works, so the same pattern drives padding, font sizes, widths, or whole layout values. Like `ctx.breakpoint()`, `ctx.responsive(...)` only rerenders the component when the resolved breakpoint changes.

## Form Theme

Form theme roles require the `form` feature:

```toml
lurq = { version = "0.53.0", features = ["form"] }
```

`FormTheme` groups compound form styling into semantic roles:

| Field | Purpose |
| --- | --- |
| `form.field` | Label, hint, error text, and field spacing. |
| `form.input` | Text input frame, text, placeholder, caret, focus, and error colors. |
| `form.checkbox` | Compound checkbox colors and radius. |
| `form.slider` | Compound slider track and thumb colors. |
| `form.button` | Primary and secondary compound button roles. |

Form text roles use typography and palette together:

```rust
use lurq::app::theme::{FormTextRole, PaletteColor, TypographyStyle};

FormTextRole {
  typography: TypographyStyle::Caption,
  color: PaletteColor::TextMuted,
};
```

### Field Roles

`theme.form().field` is a `FormFieldTheme`.

| Field | Default |
| --- | --- |
| `spacing` | `SpacingSize::Xs` |
| `label` | `TypographyStyle::FieldLabel` + `PaletteColor::TextPrimary` |
| `hint` | `TypographyStyle::Caption` + `PaletteColor::TextMuted` |
| `error` | `TypographyStyle::Caption` + `PaletteColor::Danger` |

### Input Roles

`theme.form().input` is a `FormInputTheme`.

| Field | Default |
| --- | --- |
| `height` | `36px` |
| `padding` | horizontal `10px`, vertical `8px` |
| `radius` | `RadiusSize::Md` |
| `background` | `PaletteColor::SurfaceInput` |
| `border` | `PaletteColor::Border` |
| `border_focus` | `PaletteColor::BorderFocus` |
| `background_error` | `PaletteColor::DangerMuted` |
| `border_error` | `PaletteColor::Danger` |
| `text` | `TypographyStyle::Body` + `PaletteColor::TextPrimary` |
| `placeholder` | `TypographyStyle::Body` + `PaletteColor::TextMuted` |
| `caret` | `PaletteColor::BorderFocus` |

### Button Roles

`theme.form().button` is a `FormButtonTheme` with `primary` and `secondary` `FormButtonRole`s.

Both button roles own layout values (`width`, `height`, `padding`) and semantic theme references (`radius`, background roles, border roles, text role). Compound form controls draw their default borders with `BorderSize::Sm`. Defaults:

| Role | Background | Border | Text |
| --- | --- | --- | --- |
| `primary` | `Accent` | `Accent` | `Button` + `TextInverse` |
| `secondary` | `SurfaceInput` | `BorderStrong` | `Button` + `TextPrimary` |

Primary hover and active states use `AccentHover`. Secondary hover uses `SurfacePanel` and active uses `Border`. Both roles draw a `border_focus` border (`BorderFocus`) while the button is [focus-visible](../focus-navigation/#focus-visible): after Tab or a key press, not after a click.

### Checkbox And Slider Roles

Compound checkbox defaults:

| Field | Default |
| --- | --- |
| `background` | `SurfaceInput` |
| `border` | `Border` |
| `border_hover` | `BorderFocus` |
| `checked_background` | `Accent` |
| `checked_border` | `Accent` |
| `checked_background_hover` | `AccentHover` |
| `border_focus` | `BorderFocus` |
| `radius` | `RadiusSize::Sm` |

Compound slider defaults:

| Field | Default |
| --- | --- |
| `track` | `Border` |
| `track_hover` | `BorderStrong` |
| `thumb` | `Accent` |
| `thumb_hover` | `AccentHover` |
| `thumb_border_focus` | `BorderFocus` |

### Updating Form Roles

Read the current form theme, modify the semantic roles, then replace it:

```rust
use lurq::app::theme::{
  FormTextRole, PaletteColor, RadiusSize, TypographyStyle,
};

let mut form = app.theme().form().clone();

form.field.label = FormTextRole {
  typography: TypographyStyle::Label,
  color: PaletteColor::TextSecondary,
};

form.input.radius = RadiusSize::Lg;
form.input.border_focus = PaletteColor::Info;
form.button.primary.background = PaletteColor::Success;
form.button.primary.background_hover = PaletteColor::AccentHover;

app.theme().set_form(form);
```

## Concrete Values

Theme roles are for shared semantics. Component APIs still accept concrete values where a local override is clearer:

```rust
use lurq::{components::Rect, node::color::Color};

Rect::new(80.0, 32.0)
  .background("#0f172a")
  .rounded(4.0)
  .border_inside(1.0, Color::from_hex("#334155"));
```

Prefer concrete values for isolated drawings, debug visuals, or one-off component details. Prefer theme roles for app surfaces, text, controls, repeated spacing, repeated border widths, elevations, and reusable component defaults.
