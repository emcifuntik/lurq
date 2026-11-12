use lurq::{
  app::theme::ShadowStyle,
  components::{Image, Svg},
  layout::{Alignment, StackAlignment, text_style::FontWeight},
  node::{BoxShadow, Element, Style, color::Color, dimension::Dimension},
  svg::SvgData,
};

use crate::style::{BG, BORDER, PRIMARY, SECONDARY, SURFACE, TEXT, TEXT_MUTED, text};

const FILL_WIDTH: Dimension = Dimension::Pct(100.0);
const CONTENT_PAD: f32 = 32.0;
const CARD_RADIUS: f32 = 8.0;

const LINE_CHART_SVG: &str = include_str!("../assets/line-chart-parallel-svgrepo-com.svg");
const LINE_CHART_SVG_BYTES: &[u8] = include_bytes!("../assets/line-chart-parallel-svgrepo-com.svg");

const IMAGE_ASSETS: &[(&str, &str)] = &[
  ("JPG", "skebob.jpg"),
  ("PNG alpha", "transparent.png"),
  ("GIF", "six-seven.gif"),
  ("WebP", "animated-webp-supported.webp"),
];

pub(crate) fn visual_content() -> Element {
  let content = lurq::components::Column::new()
    .spacing(24.0)
    .child(text("Visual Styling", 28.0, FontWeight::Bold, TEXT).width(FILL_WIDTH))
    .child(section_title("Color Palette"))
    .child(color_palette())
    .child(section_title("Border Radius"))
    .child(radius_showcase())
    .child(section_title("Box Shadows"))
    .child(shadow_showcase())
    .child(section_title("Layer Blur"))
    .child(blur_showcase())
    .child(section_title("Backdrop Blur"))
    .child(backdrop_blur_showcase())
    .child(section_title("Clipping (Overflow)"))
    .child(clip_showcase());

  let content = content.child(section_title("SVG")).child(svg_showcase());

  let content = content
    .child(section_title("Plain Images"))
    .child(plain_images_showcase())
    .child(section_title("Sized Images"))
    .child(sized_images_showcase())
    .child(section_title("Intrinsic Images"))
    .child(intrinsic_images_showcase())
    .child(section_title("Background Images"))
    .child(background_images_showcase());

  content.padding(CONTENT_PAD).width(FILL_WIDTH).background(BG).into()
}

fn section_title(label: &str) -> Element {
  text(label, 18.0, FontWeight::Bold, TEXT).width(FILL_WIDTH).into()
}

fn color_palette() -> Element {
  let colors: &[(&str, &str)] = &[
    ("red", "#EF4444"),
    ("org", "#F97316"),
    ("yel", "#EAB308"),
    ("grn", "#22C55E"),
    ("blu", "#3B82F6"),
    ("pur", "#8B5CF6"),
  ];

  lurq::components::Column::new()
    .spacing(16.0)
    .child(
      lurq::components::Row::new()
        .spacing(12.0)
        .with_children(colors.iter().map(|(name, hex)| color_swatch(name, hex)))
        .width(FILL_WIDTH),
    )
    .child(alpha_row())
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn color_swatch(name: &str, hex: &str) -> Element {
  lurq::components::Column::new()
    .spacing(4.0)
    .align_items(Alignment::Center)
    .justify(lurq::layout::layout_kind::Justify::Center)
    .child(text(hex, 10.0, FontWeight::Normal, "#ffffff"))
    .child(text(name, 11.0, FontWeight::Bold, "#ffffff"))
    .size(Dimension::Pct(100.0), 70.0)
    .background(hex)
    .rounded(8.0)
    .flex(1.0)
    .into()
}

fn alpha_row() -> Element {
  let alphas: &[(&str, f32)] = &[("100%", 1.0), ("80%", 0.8), ("60%", 0.6), ("40%", 0.4), ("20%", 0.2)];

  lurq::components::Row::new()
    .spacing(12.0)
    .align_items(Alignment::Center)
    .child(text("Alpha:", 12.0, FontWeight::Normal, TEXT_MUTED))
    .with_children(alphas.iter().map(|(label, alpha)| {
      lurq::components::Column::new()
        .align_items(Alignment::Center)
        .justify(lurq::layout::layout_kind::Justify::Center)
        .child(text(label, 11.0, FontWeight::Normal, "#ffffff"))
        .size(60.0, 32.0)
        .background(PRIMARY)
        .rounded(4.0)
        .opacity(*alpha)
    }))
    .width(FILL_WIDTH)
    .into()
}

fn radius_showcase() -> Element {
  let radii: &[(&str, f32)] = &[
    ("rounded(0) — sharp", 0.0),
    ("rounded(8) — subtle", 8.0),
    ("rounded(16) — rounded", 16.0),
    ("rounded(40) — pill", 40.0),
  ];

  lurq::components::Row::new()
    .spacing(24.0)
    .align_items(Alignment::Center)
    .with_children(radii.iter().map(|(label, radius)| {
      lurq::components::Column::new()
        .spacing(8.0)
        .align_items(Alignment::Center)
        .child(
          lurq::components::Rect::new(140.0, 50.0)
            .background(PRIMARY)
            .rounded(*radius),
        )
        .child(text(label, 11.0, FontWeight::Normal, TEXT_MUTED))
        .height(80.0)
        .flex(1.0)
    }))
    .padding_horizontal(24.0)
    .padding_vertical(16.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

/// Theme roles, an explicit layered shadow, a hard offset, a glow, an inset
/// well, and a card that rises on hover. Shadows paint outside their element,
/// so the rows let them overflow (containers clip by default).
fn shadow_showcase() -> Element {
  let shadow_card = |label: &str, card: lurq::components::Rect| {
    lurq::components::Column::new()
      .spacing(10.0)
      .align_items(Alignment::Center)
      .overflow_visible()
      .child(card.size(120.0, 64.0).background("#ffffff").rounded(10.0))
      .child(text(label, 11.0, FontWeight::Normal, "#475569"))
      .flex(1.0)
  };
  let row = |cards: Vec<lurq::components::Column>| {
    lurq::components::Row::new()
      .spacing(24.0)
      .overflow_visible()
      .with_children(cards)
      .width(FILL_WIDTH)
  };

  lurq::components::Column::new()
    .spacing(32.0)
    .child(row(vec![
      shadow_card(
        "ShadowStyle::Sm",
        lurq::components::Rect::default().box_shadow(ShadowStyle::Sm),
      ),
      shadow_card(
        "ShadowStyle::Md",
        lurq::components::Rect::default().box_shadow(ShadowStyle::Md),
      ),
      shadow_card(
        "ShadowStyle::Lg",
        lurq::components::Rect::default().box_shadow(ShadowStyle::Lg),
      ),
      shadow_card(
        "hover me",
        lurq::components::Rect::default()
          .box_shadow(ShadowStyle::Sm)
          .hovered(|style: Style| style.box_shadow(ShadowStyle::Lg)),
      ),
    ]))
    .child(row(vec![
      shadow_card(
        "layered",
        lurq::components::Rect::default().box_shadow([
          BoxShadow::new(0.0, 1.0, 1.0, "#0f172a1f"),
          BoxShadow::new(0.0, 8.0, 24.0, "#0f172a33").spread(-4.0),
        ]),
      ),
      shadow_card(
        "hard offset",
        lurq::components::Rect::default().box_shadow(BoxShadow::new(6.0, 6.0, 0.0, "#1e293b")),
      ),
      shadow_card(
        "glow",
        lurq::components::Rect::default().box_shadow(BoxShadow::new(0.0, 0.0, 16.0, PRIMARY).spread(2.0)),
      ),
      shadow_card(
        "inset",
        lurq::components::Rect::default().box_shadow(BoxShadow::new(0.0, 2.0, 8.0, "#0f172a59").inset()),
      ),
    ]))
    .padding(32.0)
    .width(FILL_WIDTH)
    .background("#e2e8f0")
    .rounded(CARD_RADIUS)
    .into()
}

/// The same card blurred as one layer by growing radii, and once faded too.
fn blur_showcase() -> Element {
  let blur_card = |label: &str, radius: f32, opacity: f32| {
    lurq::components::Column::new()
      .spacing(10.0)
      .align_items(Alignment::Center)
      .overflow_visible()
      .child(
        lurq::components::Column::new()
          .size(120.0, 64.0)
          .padding(10.0)
          .spacing(6.0)
          .background("#ffffff")
          .border_inside(1.0, BORDER)
          .rounded(10.0)
          .child(text("Blurred", 14.0, FontWeight::Bold, TEXT))
          .child(lurq::components::Rect::new(64.0, 10.0).background(PRIMARY).rounded(5.0))
          .opacity(opacity)
          .blur(radius),
      )
      .child(text(label, 11.0, FontWeight::Normal, "#475569"))
      .flex(1.0)
  };

  lurq::components::Row::new()
    .spacing(24.0)
    .overflow_visible()
    .child(blur_card("blur(0)", 0.0, 1.0))
    .child(blur_card("blur(2)", 2.0, 1.0))
    .child(blur_card("blur(6)", 6.0, 1.0))
    .child(blur_card("blur(16)", 16.0, 1.0))
    .child(blur_card("blur(6) + opacity(0.5)", 6.0, 0.5))
    .width(FILL_WIDTH)
    .into()
}

/// Frosted glass: panels that blur the stripes and the label behind them by
/// growing radii.
fn backdrop_blur_showcase() -> Element {
  let stripes = lurq::components::Row::new().with_children(
    (0..12)
      .map(|index| lurq::components::Rect::new(40.0, 96.0).background(if index % 2 == 0 { PRIMARY } else { SECONDARY }))
      .collect::<Vec<_>>(),
  );
  let glass = |label: &str, radius: f32| {
    lurq::components::Column::new()
      .size(96.0, 64.0)
      .padding(10.0)
      .background("#ffffff40")
      .border_inside(1.0, "#ffffff99")
      .rounded(12.0)
      .backdrop_blur(radius)
      .child(text(label, 11.0, FontWeight::Bold, TEXT))
  };

  lurq::components::Stack::new()
    .size(480.0, 96.0)
    .rounded(CARD_RADIUS)
    .clip()
    .child(stripes)
    .child(lurq::components::Column::new().padding(16.0).child(text(
      "Behind the glass",
      22.0,
      FontWeight::Bold,
      "#ffffff",
    )))
    .child(
      lurq::components::Row::new()
        .padding(16.0)
        .spacing(16.0)
        .child(glass("blur 0", 0.0))
        .child(glass("blur 4", 4.0))
        .child(glass("blur 12", 12.0))
        .child(glass("blur 32", 32.0)),
    )
    .into()
}

fn clip_showcase() -> Element {
  lurq::components::Row::new()
    .spacing(80.0)
    .align_items(Alignment::Center)
    .child(clip_example("Overflow::Visible", false))
    .child(clip_example("Overflow::Hidden", true))
    .padding_horizontal(60.0)
    .padding_vertical(20.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .overflow_visible()
    .into()
}

fn clip_example(label: &str, clip: bool) -> Element {
  let mut parent = lurq::components::Stack::new()
    .child(
      lurq::components::Rect::new(80.0, 50.0)
        .background("#F59E0B")
        .opacity(0.7)
        .rounded(4.0)
        .absolute_position(60.0, 20.0),
    )
    .size(120.0, 80.0)
    .background("#0F172A")
    .rounded(4.0)
    .border_inside(1.0, Color::from_hex(BORDER));
  if clip {
    parent = parent.clip();
  } else {
    parent = parent.overflow_visible();
  }

  let mut col = lurq::components::Column::new()
    .spacing(8.0)
    .align_items(Alignment::Center)
    .child(text(label, 12.0, FontWeight::Normal, TEXT_MUTED))
    .child(parent);
  if !clip {
    col = col.overflow_visible();
  }
  col.into()
}

fn svg_showcase() -> Element {
  let content = lurq::components::Column::new().spacing(16.0).child(svg_case_box(
    "SVG from string / bytes",
    "Svg::from_str and Svg::from_bytes",
    vec![
      svg_preview(
        "from_str",
        Svg::from_str(LINE_CHART_SVG)
          .size(Dimension::Pct(100.0), 120.0)
          .rounded(6.0)
          .clip(),
        false,
      ),
      svg_preview(
        "from_bytes",
        Svg::from_bytes(LINE_CHART_SVG_BYTES)
          .size(Dimension::Pct(100.0), 120.0)
          .rounded(6.0)
          .clip(),
        false,
      ),
    ],
  ));

  let content = content.child(svg_case_box(
    "SVG from resource",
    "line-chart-parallel-svgrepo-com.svg",
    vec![svg_preview(
      "resource",
      Svg::from_resource("line-chart-parallel-svgrepo-com.svg")
        .size(Dimension::Pct(100.0), 120.0)
        .rounded(6.0)
        .clip(),
      false,
    )],
  ));

  content
    .child(svg_case_box(
      "SVG intrinsic",
      "uses viewBox size",
      vec![svg_preview(
        "intrinsic",
        Svg::from_str(LINE_CHART_SVG)
          .rounded(6.0)
          .clip()
          .border_inside(1.0, Color::from_hex(BORDER)),
        true,
      )],
    ))
    .child(svg_case_box(
      "SVG sized",
      "120 x 120 frame",
      vec![svg_preview(
        "size(120, 120)",
        Svg::from_str(LINE_CHART_SVG).size(120.0, 120.0).rounded(6.0).clip(),
        false,
      )],
    ))
    .child(svg_case_box(
      "SVG stroke paths",
      "line chart",
      vec![svg_preview(
        "inline strokes",
        Svg::from_str(LINE_CHART_SVG)
          .size(Dimension::Pct(100.0), 120.0)
          .rounded(6.0)
          .clip(),
        false,
      )],
    ))
    .child(svg_case_box(
      "SVG color overrides",
      "fill and stroke overrides",
      vec![
        svg_preview(
          "stroke override",
          Svg::new(SvgData::from_str(LINE_CHART_SVG).with_stroke(Color::from_hex(PRIMARY)))
            .size(Dimension::Pct(100.0), 120.0)
            .rounded(6.0)
            .clip(),
          false,
        ),
        svg_preview(
          "fill + stroke",
          Svg::new(
            SvgData::from_str(LINE_CHART_SVG)
              .with_fill(Color::from_hex(SECONDARY))
              .with_stroke(Color::from_hex(PRIMARY)),
          )
          .size(Dimension::Pct(100.0), 120.0)
          .rounded(6.0)
          .clip(),
          false,
        ),
      ],
    ))
    .width(FILL_WIDTH)
    .into()
}

fn svg_case_box(label: &str, detail: &str, previews: Vec<Element>) -> Element {
  lurq::components::Column::new()
    .spacing(12.0)
    .child(text(label, 13.0, FontWeight::Bold, TEXT).width(FILL_WIDTH))
    .child(text(detail, 10.0, FontWeight::Normal, TEXT_MUTED).width(FILL_WIDTH))
    .child(
      lurq::components::Row::new()
        .spacing(16.0)
        .align_items(Alignment::Stretch)
        .with_children(previews)
        .width(FILL_WIDTH),
    )
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn svg_preview(label: &str, svg: Svg, intrinsic: bool) -> Element {
  let column = lurq::components::Column::new().spacing(8.0);

  let column = if intrinsic {
    column.child(svg)
  } else {
    column.child(
      lurq::components::Stack::new()
        .stack_align(StackAlignment::Center)
        .child(svg)
        .size(360.0, 132.0)
        .background("#0B1220")
        .rounded(6.0)
        .clip()
        .border_inside(1.0, Color::from_hex(BORDER)),
    )
  };

  column.child(text(label, 10.0, FontWeight::Normal, TEXT_MUTED)).into()
}

fn plain_images_showcase() -> Element {
  lurq::components::Row::new()
    .spacing(16.0)
    .align_items(Alignment::Stretch)
    .with_children(
      IMAGE_ASSETS
        .iter()
        .map(|(label, path)| image_resource_card(label, path)),
    )
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn sized_images_showcase() -> Element {
  lurq::components::Row::new()
    .spacing(24.0)
    .align_items(Alignment::Start)
    .with_children(IMAGE_ASSETS.iter().map(|(label, path)| sized_image_card(label, path)))
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn intrinsic_images_showcase() -> Element {
  lurq::components::Column::new()
    .spacing(24.0)
    .child(
      lurq::components::Row::new()
        .spacing(24.0)
        .align_items(Alignment::Start)
        .child(intrinsic_image_card(IMAGE_ASSETS[0].0, IMAGE_ASSETS[0].1))
        .child(intrinsic_image_card(IMAGE_ASSETS[2].0, IMAGE_ASSETS[2].1))
        .width(FILL_WIDTH),
    )
    .child(
      lurq::components::Row::new()
        .spacing(24.0)
        .align_items(Alignment::Start)
        .child(intrinsic_image_card(IMAGE_ASSETS[1].0, IMAGE_ASSETS[1].1))
        .child(intrinsic_image_card(IMAGE_ASSETS[3].0, IMAGE_ASSETS[3].1))
        .width(FILL_WIDTH),
    )
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn background_images_showcase() -> Element {
  lurq::components::Column::new()
    .spacing(16.0)
    .child(text("background-size: cover", 13.0, FontWeight::Bold, TEXT).width(FILL_WIDTH))
    .child(
      lurq::components::Row::new()
        .spacing(16.0)
        .align_items(Alignment::Stretch)
        .with_children(
          IMAGE_ASSETS
            .iter()
            .map(|(label, path)| background_resource_card(label, path, true)),
        )
        .width(FILL_WIDTH),
    )
    .child(text("background-size: contain", 13.0, FontWeight::Bold, TEXT).width(FILL_WIDTH))
    .child(
      lurq::components::Row::new()
        .spacing(16.0)
        .align_items(Alignment::Stretch)
        .with_children(
          IMAGE_ASSETS
            .iter()
            .map(|(label, path)| background_resource_card(label, path, false)),
        )
        .width(FILL_WIDTH),
    )
    .padding(24.0)
    .width(FILL_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CARD_RADIUS)
    .into()
}

fn sized_image_card(label: &str, path: &str) -> Element {
  lurq::components::Column::new()
    .spacing(8.0)
    .align_items(Alignment::Center)
    .child(
      Image::from_resource(path)
        .width(120.0)
        .rounded(6.0)
        .clip()
        .border_inside(1.0, Color::from_hex(BORDER)),
    )
    .child(text(&format!("width=120 {label}"), 12.0, FontWeight::Bold, TEXT))
    .child(text(path, 10.0, FontWeight::Normal, TEXT_MUTED))
    .flex(1.0)
    .into()
}

fn intrinsic_image_card(label: &str, path: &str) -> Element {
  lurq::components::Column::new()
    .spacing(8.0)
    .child(
      Image::from_resource(path)
        .rounded(6.0)
        .clip()
        .border_inside(1.0, Color::from_hex(BORDER)),
    )
    .child(text(&format!("Intrinsic {label}"), 12.0, FontWeight::Bold, TEXT))
    .child(text(path, 10.0, FontWeight::Normal, TEXT_MUTED))
    .into()
}

fn image_resource_card(label: &str, path: &str) -> Element {
  lurq::components::Column::new()
    .spacing(8.0)
    .child(
      Image::from_resource(path)
        .size(Dimension::Pct(100.0), 132.0)
        .rounded(6.0)
        .clip()
        .border_inside(1.0, Color::from_hex(BORDER)),
    )
    .child(text(&format!("Image {label}"), 12.0, FontWeight::Bold, TEXT))
    .child(text(path, 10.0, FontWeight::Normal, TEXT_MUTED))
    .flex(1.0)
    .into()
}

fn background_resource_card(label: &str, path: &str, cover: bool) -> Element {
  let mut background = lurq::components::Stack::new()
    .size(Dimension::Pct(100.0), 132.0)
    .background("#0B1220")
    .background_image(path)
    .child(
      lurq::components::Column::new()
        .spacing(2.0)
        .child(text(label, 11.0, FontWeight::Bold, TEXT))
        .child(text("child", 9.0, FontWeight::Normal, TEXT_MUTED))
        .padding_horizontal(8.0)
        .padding_vertical(6.0)
        .background("#111827")
        .rounded(4.0)
        .absolute_position(10.0, 10.0),
    )
    .rounded(6.0)
    .clip()
    .border_inside(1.0, Color::from_hex(BORDER));
  if cover {
    background = background.background_cover();
  } else {
    background = background.background_contain();
  }

  lurq::components::Column::new()
    .spacing(8.0)
    .child(background)
    .child(text(&format!("Background {label}"), 12.0, FontWeight::Bold, TEXT))
    .child(text(path, 10.0, FontWeight::Normal, TEXT_MUTED))
    .flex(1.0)
    .into()
}
