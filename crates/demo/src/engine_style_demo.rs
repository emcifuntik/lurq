//! Style by hand: the theme's foreground and the foreground of an element, disabled elements, focus rings,
//! text decorations and line limits, transitions of what is only painted, and reduced motion.

use lurq::{
  animation::Transition,
  app::{component::Component, ctx::Ctx, theme::PaletteColor},
  components::{Button, Column, Markdown, MarkdownProps, Row, ScrollVertical, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    text_style::{TextStrikethrough, TextStyle, TextUnderline},
  },
  node::{BoxShadow, CursorIcon, Element, Outline, color::Color, transform::Transform2D},
};

use crate::{
  engine_kit::{FILL, card, chip, choice, chosen, controls, counted, label, note, page, slider, toggle},
  style::PRIMARY,
};

/// The fill the samples of text are on, light: the foreground is dark until it is set.
const PAPER: &str = "#f8fafc";
const LONG: &str = "The quick brown fox jumps over the lazy dog while seven wizards quietly box, and then the fox, \
                    having jumped, sits down to think about what the dog made of it all and whether it will ever \
                    jump again.";

#[derive(Clone, Copy)]
enum Ink {
  Default,
  Slate,
  Blue,
  Red,
}

impl Ink {
  fn color(self) -> Color {
    Color::from_hex(match self {
      Self::Default => "#000000",
      Self::Slate => "#334155",
      Self::Blue => "#1d4ed8",
      Self::Red => "#b91c1c",
    })
  }
}

const INKS: [(&str, Ink); 4] = [
  ("black, the default", Ink::Default),
  ("slate", Ink::Slate),
  ("blue", Ink::Blue),
  ("red", Ink::Red),
];

pub(crate) struct StyleDemo {
  ink: Signal<usize>,
  typed: Signal<String>,
  disabled: Signal<bool>,
  disabled_clicks: Signal<u32>,
  disabled_text: Signal<String>,
  lines: Signal<i32>,
  moved: Signal<bool>,
  reduced: Signal<bool>,
}

fn paper() -> Column {
  Column::new()
    .spacing(8.0)
    .padding(16.0)
    .width(FILL)
    .background(PAPER)
    .rounded(8.0)
}

/// A placeholder is muted whatever the foreground is: what is typed is what takes it.
fn placeholder() -> TextStyle {
  TextStyle {
    color: Color::from_hex("#94a3b8"),
    ..TextStyle::default()
  }
}

fn focusable(caption: &str) -> Button {
  Button::new(caption)
    .padding_horizontal(14.0)
    .height(32.0)
    .background("#e2e8f0")
    .rounded(6.0)
    .tab_index(0)
    .transition(Transition::outline().duration_ms(140))
    .focus_visible(|style| style.outline(Outline::new(2.0, PRIMARY).offset(2.0)))
}

impl StyleDemo {
  fn foreground(&self, ctx: &mut Ctx) -> Element {
    let named = TextStyle {
      font_size: 15.0,
      ..TextStyle::default()
    };
    card("The theme's foreground")
      .child(controls([
        Element::from(label("PaletteColor::Foreground")),
        choice(&INKS, &self.ink).into(),
      ]))
      .child(
        paper()
          .child(Text::new("A text with no color: it takes the theme's foreground."))
          .child(Text::styled(
            "A text given a style that keeps the default color: it takes it too.",
            named,
          ))
          .child(Text::new("A text that names its color keeps it.").color("#15803d"))
          .child(
            TextInput::new(self.typed.clone())
              .placeholder("Type here: the text and the caret take it")
              .placeholder_style(placeholder())
              .padding_horizontal(10.0)
              .size(360.0, 32.0)
              .background("#ffffff")
              .rounded(6.0)
              .border_inside(1.0, Color::from_hex("#94a3b8"))
              .tab_index(0),
          )
          .child(Markdown::mount(
            ctx,
            MarkdownProps::new("A **Markdown** document whose style names no color takes it, `code` and all."),
          )),
      )
      .child(label("An element's own foreground comes first, for everything in it"))
      .child(
        Row::new()
          .spacing(12.0)
          .child(
            Row::new()
              .spacing(10.0)
              .padding(14.0)
              .background("#0f172a")
              .rounded(8.0)
              .foreground("#f8fafc")
              .child(Text::new("Inbox"))
              .child(Text::new("Unread"))
              .child(Text::new("12").color(PaletteColor::Accent)),
          )
          .child(
            Row::new()
              .padding(14.0)
              .rounded(8.0)
              .background(PAPER)
              .foreground("#475569")
              .cursor(CursorIcon::Pointer)
              .transition(Transition::foreground().duration_ms(200))
              .transition(Transition::background_color().duration_ms(200))
              .hovered(|style| style.background(PRIMARY).foreground("#ffffff"))
              .child(Text::new("Hover me: one style recolors the fill and the label")),
          ),
      )
      .into()
  }

  fn disabled(&self) -> Element {
    let off = self.disabled.get();
    let clicks = self.disabled_clicks.clone();
    let mut lines = Column::new().width(FILL).padding(8.0);
    for index in 1..=12 {
      lines = lines.child(Text::new(&format!("Line {index} of a list that still scrolls")).height(22.0));
    }
    let group = paper()
      .disabled(off)
      .when_disabled(|style| style.opacity(0.45))
      .child(
        Row::new()
          .spacing(10.0)
          .align_items(Alignment::Center)
          .child(focusable("A button").on_click(move |_| clicks.update(|count| *count += 1)))
          .child(focusable("Another"))
          .child(
            TextInput::new(self.disabled_text.clone())
              .placeholder("An input")
              .placeholder_style(placeholder())
              .padding_horizontal(10.0)
              .size(180.0, 32.0)
              .background("#ffffff")
              .rounded(6.0)
              .border_inside(1.0, Color::from_hex("#94a3b8"))
              .tab_index(0),
          ),
      )
      .child(
        ScrollVertical::new(lines)
          .size(320.0, 88.0)
          .background("#ffffff")
          .rounded(6.0),
      );
    card("Disabled")
      .child(controls([
        Element::from(toggle("the group is disabled", &self.disabled)),
        counted("clicks on the button", self.disabled_clicks.get()).into(),
      ]))
      .child(group)
      .child(note(
        "Disabled, nothing in the group takes a click, a hover, focus or Tab, and focus that was in it is let go. The \
         list in it still scrolls, by wheel and by its bar.",
      ))
      .into()
  }

  fn focus(&self) -> Element {
    card("Focus rings")
      .child(
        paper().child(
          Row::new()
            .spacing(12.0)
            .child(focusable("First"))
            .child(focusable("Second"))
            .child(focusable("Third")),
        ),
      )
      .child(
        // No padding: the buttons fill their container to its edges, which clip what is in it.
        Row::new()
          .background(PAPER)
          .rounded(6.0)
          .child(focusable("At the edge"))
          .child(focusable("of its container")),
      )
      .child(note(
        "Press Tab: a ring grows in around the focused button, outside its box, and moves nothing. Click a button: \
         no ring. The ring of a button at its container's edge is not cut there.",
      ))
      .into()
  }

  fn text(&self) -> Element {
    card("Text")
      .child(
        paper()
          .child(Text::new("An underlined text").underline(TextUnderline::new()))
          .child(
            Text::new("An underline of its own color and thickness")
              .underline(TextUnderline::new().color(Color::from_hex("#b91c1c")).thickness(2.0)),
          )
          .child(Text::new("A struck text").strikethrough(TextStrikethrough::new()))
          .child(
            Text::new(LONG)
              .underline(TextUnderline::new())
              .width(300.0)
              .max_lines(2),
          ),
      )
      .child(slider("Lines at most", &self.lines, 1, 6))
      .child(paper().child(Text::new(LONG).width(300.0).max_lines(self.lines.get().max(1) as u32)))
      .child(note(
        "The text is cut to the lines it may take and ends in an ellipsis. Try a fractional display scale: it never \
         paints a line more than its box holds.",
      ))
      .into()
  }

  fn motion(&self, ctx: &mut Ctx) -> Element {
    let theme = ctx.theme().clone();
    let reduced = self.reduced.clone();
    let now = ctx.reduced_motion();
    let moved = self.moved.clone();
    let lift = Column::new()
      .size(260.0, 64.0)
      .padding(12.0)
      .background(PAPER)
      .rounded(10.0)
      .cursor(CursorIcon::Pointer)
      .transition(Transition::box_shadow().duration_ms(250))
      .transition(Transition::outline().duration_ms(250))
      .hovered(|style| {
        style
          .box_shadow(BoxShadow::new(0.0, 12.0, 28.0, Color::new(0, 0, 0, 140)))
          .outline(Outline::new(2.0, PRIMARY).offset(3.0))
      })
      .child(Text::new("Hover: a shadow and an outline come in"));
    let slide = Column::new()
      .size(260.0, 64.0)
      .padding(12.0)
      .background(PRIMARY)
      .rounded(10.0)
      .cursor(CursorIcon::Pointer)
      .foreground("#ffffff")
      .opacity(if self.moved.get() { 0.55 } else { 1.0 })
      .transform(Transform2D::translate(if self.moved.get() { 220.0 } else { 0.0 }, 0.0))
      .transition(Transition::transform().duration_ms(600))
      .transition(Transition::opacity().duration_ms(600))
      .on_click(move |_| moved.update(|moved| *moved = !*moved))
      .child(Text::new("Click: it moves and fades"));
    card("Transitions of paint, and reduced motion")
      .child(controls([
        Element::from(chip("the app reduces motion", self.reduced.get(), move || {
          let on = !reduced.get();
          reduced.set(on);
          // `None` would follow the system's setting again.
          theme.set_reduced_motion(Some(on));
        })),
        label(if now {
          "in effect: motion is reduced"
        } else {
          "in effect: motion is not reduced"
        })
        .into(),
      ]))
      .child(
        Row::new()
          .spacing(40.0)
          .padding(16.0)
          .width(FILL)
          .height(120.0)
          .child(lift),
      )
      .child(Row::new().padding(16.0).width(FILL).height(104.0).child(slide))
      .child(note(
        "With motion reduced the box is at its new place at once and still fades. The shadow and the outline are no \
         motion and come in either way.",
      ))
      .into()
  }
}

impl Component for StyleDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      ink: ctx.signal(0),
      typed: ctx.signal(String::new()),
      disabled: ctx.signal(false),
      disabled_clicks: ctx.signal(0),
      disabled_text: ctx.signal(String::new()),
      lines: ctx.signal(3),
      moved: ctx.signal(false),
      reduced: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    // The role is the theme's: every page's text that names no color follows it.
    ctx
      .theme()
      .set_palette_color(PaletteColor::Foreground, chosen(&INKS, &self.ink).color());
    page(
      "Style",
      "The samples are on light paper: text that names no color is black until the theme's foreground is set.",
    )
    .child(self.foreground(ctx))
    .child(self.disabled())
    .child(self.focus())
    .child(self.text())
    .child(self.motion(ctx))
  }
}
