//! Text input with the on-screen keyboard: whether it comes up and goes away, what reaches the field (typed
//! text, autocorrection, dictation, emoji), and whether a field at the bottom of the screen stays in view
//! above the keyboard.

use lurq::{
  app::{component::Component, ctx::Ctx, events::KeyboardEvent},
  components::{Column, ContentKind, KeyboardKind, ReturnKey, Spacer, StackOutlet, TextInput},
  core::Signal,
  layout::text_style::{FontWeight, TextStyle},
  node::{Element, color::Color},
};

use crate::ui::{
  BG, BORDER, FILL, MUTED, PRIMARY, SURFACE, TAP_TARGET, TEXT, button, caption, card, event_log, push_log, text,
};

pub(crate) struct InputsScreen {
  single: Signal<String>,
  secret: Signal<String>,
  email: Signal<String>,
  code: Signal<String>,
  notes: Signal<String>,
  bottom: Signal<String>,
  log: Signal<Vec<String>>,
}

impl Component for InputsScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      single: ctx.signal(String::new()),
      secret: ctx.signal(String::new()),
      email: ctx.signal(String::new()),
      code: ctx.signal(String::new()),
      notes: ctx.signal("Two lines to start with.\nSelect a word, move the caret, paste.".to_owned()),
      bottom: ctx.signal(String::new()),
      log: ctx.signal(Vec::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let window = ctx.window();
    let safe_area = window.safe_area;
    let keys = self.log.clone();
    let cleared = (self.single.clone(), self.secret.clone(), self.bottom.clone());
    let content = Column::new()
      .spacing(14.0)
      .padding(16.0)
      .child(field(
        "One line",
        TextInput::new(self.single.clone())
          .placeholder("Type here")
          .single_line()
          .return_key(ReturnKey::Done),
      ))
      .child(caption(&format!(
        "{} characters: “{}”",
        self.single.get().chars().count(),
        self.single.get()
      )))
      .child(field(
        "Password",
        TextInput::new(self.secret.clone()).placeholder("Hidden").single_line().mask(),
      ))
      .child(field(
        "Email: its own keyboard, and Next on the Return key",
        TextInput::new(self.email.clone())
          .placeholder("name@example.com")
          .single_line()
          .keyboard(KeyboardKind::Email)
          .return_key(ReturnKey::Next),
      ))
      .child(field(
        "Code: digits only, and the code from a message",
        TextInput::new(self.code.clone())
          .placeholder("000000")
          .single_line()
          .keyboard(KeyboardKind::Number)
          .content(ContentKind::OneTimeCode),
      ))
      .child(field("Several lines", TextInput::new(self.notes.clone()).textarea().rows(3, 6)))
      .child(card(
        Column::new()
          .spacing(8.0)
          .child(caption(&format!(
            "The window is {:.0} by {:.0}; the safe area leaves {:.0} at the bottom.",
            window.logical_width(),
            window.logical_height(),
            safe_area.bottom
          )))
          .child(button("Clear the one-line fields", move || {
            cleared.0.set(String::new());
            cleared.1.set(String::new());
            cleared.2.set(String::new());
          })),
      ))
      .child(event_log(&self.log))
      // Enough above it that the last field starts under where the keyboard comes up.
      .child(Spacer::new().height(260.0))
      .child(field(
        "At the bottom: is it above the keyboard?",
        TextInput::new(self.bottom.clone())
          .placeholder("Type here")
          .single_line()
          .return_key(ReturnKey::Send),
      ))
      .width(FILL)
      .on_key_down(move |event: KeyboardEvent| {
        push_log(&keys, format!("key “{}” ({})", event.key, event.code));
      });

    // A tap outside an input leaves the keyboard up; scrolling the screen puts it away.
    if let Some(scroll) = StackOutlet::screen_scroll(ctx) {
      scroll.set_dismisses_keyboard(true);
    }
    StackOutlet::large_title_scroll(ctx, content)
      .size(FILL, FILL)
      .background(BG)
  }
}

/// A labelled input the size of a finger.
fn field(name: &str, input: TextInput) -> Element {
  Column::new()
    .spacing(6.0)
    .child(text(name, 13.0, FontWeight::Bold, MUTED))
    .child(
      input
        .text_style(TextStyle {
          font_size: 17.0,
          color: Color::from_hex(TEXT),
          ..TextStyle::default()
        })
        .caret_color(Color::from_hex(PRIMARY))
        .padding_horizontal(12.0)
        .padding_vertical(10.0)
        .min_height(TAP_TARGET)
        .width(FILL)
        .background(SURFACE)
        .border_inside(1.0, Color::from_hex(BORDER))
        .rounded(10.0)
        .focused(|style| style.border_inside(2.0, Color::from_hex(PRIMARY))),
    )
    .width(FILL)
    .into()
}
