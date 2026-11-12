//! What the person set for the whole system, and what follows it: the theme's palette for the appearance,
//! its typography at the person's text size and weight, and the stack and bars with motion or transparency
//! reduced. Change them in Settings, or in the control centre, and come back.

use lurq::{
  app::{
    ctx::Ctx,
    theme::{PaletteColor, TypographyStyle},
  },
  components::{Column, StackOutlet, Text},
  node::Element,
};

use crate::ui::{BG, FILL, caption, card, label};

pub(crate) fn system(ctx: &mut Ctx) -> Element {
  let system = ctx.window().system;
  let scale = ctx.theme().text_scale();
  let says = |name: &str, on: bool| label(&format!("{name}: {}", if on { "on" } else { "off" }));

  // The theme's own: a palette for each appearance, and every style of its typography.
  let mut themed = Column::new()
    .width(FILL)
    .padding(14.0)
    .spacing(6.0)
    .rounded(10.0)
    .background(PaletteColor::SurfaceBase)
    .border_inside(1.0, PaletteColor::Border);
  for (name, style) in [
    ("Heading", TypographyStyle::Heading),
    ("Title", TypographyStyle::Title),
    (
      "Body: the theme's typography is as large and as heavy as the person asked",
      TypographyStyle::Body,
    ),
    ("Description", TypographyStyle::Description),
    ("Caption", TypographyStyle::Caption),
  ] {
    themed = themed.child(
      Text::new(name)
        .variant(style)
        .color(PaletteColor::TextPrimary)
        .width(FILL),
    );
  }

  let content = Column::new()
    .width(FILL)
    .padding(16.0)
    .spacing(12.0)
    .child(caption("Change these in Settings or the control centre and come back.").width(FILL))
    .child(card(
      Column::new()
        .spacing(4.0)
        .child(label(&format!("Appearance: {:?}", system.appearance)))
        .child(label(&format!(
          "Text size: {:.0}% (theme at {:.0}%)",
          system.text_scale * 100.0,
          scale * 100.0
        )))
        .child(says("Bold text", system.bold_text))
        .child(says("Reduce motion", system.reduce_motion))
        .child(says("Reduce transparency", system.reduce_transparency))
        .child(says("Increase contrast", system.increase_contrast))
        .child(says("Differentiate without color", system.differentiate_without_color))
        .child(says("Button shapes", system.button_shapes))
        .child(says("On/off labels", system.on_off_labels)),
    ))
    .child(caption("In the theme's palette for the appearance, and its typography:").width(FILL))
    .child(themed)
    .child(
      caption("With motion reduced a screen fades in where it stands. With transparency reduced the bars are solid.")
        .width(FILL),
    );
  StackOutlet::large_title_scroll(ctx, content)
    .size(FILL, FILL)
    .background(BG)
    .into()
}
