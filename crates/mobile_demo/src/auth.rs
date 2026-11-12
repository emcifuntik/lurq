//! Asking the person to prove who they are: with their face or their finger, and their passcode when that
//! fails. An app asks before it opens, or before it shows what is behind a lock.

use lurq::{
  app::{
    biometry::{self, AuthPolicy, Biometry},
    component::Component,
    ctx::Ctx,
  },
  components::Column,
  core::Signal,
  node::Element,
};

use crate::ui::{FILL, button, caption, card, label};

pub(crate) struct AuthScreen {
  /// What the last ask came to, and whether what is behind the lock is shown.
  said: Signal<String>,
  open: Signal<bool>,
}

impl Component for AuthScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      said: ctx.signal("Not asked yet".to_owned()),
      open: ctx.signal(false),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let has = match biometry::available() {
      Biometry::FaceId => "Face ID",
      Biometry::TouchId => "Touch ID",
      Biometry::OpticId => "Optic ID",
      Biometry::None => "nothing to check by",
    };
    let ask = |policy: AuthPolicy| {
      let (said, open) = (self.said.clone(), self.open.clone());
      move || {
        let (said, open) = (said.clone(), open.clone());
        said.set("Asking…".to_owned());
        // The answer comes on a thread of the system's: a signal takes it from there.
        biometry::authenticate("Show what is behind the lock", policy, move |answer| {
          open.set(answer.is_ok());
          said.set(match answer {
            Ok(()) => "Let through".to_owned(),
            Err(error) => format!("Not let through: {error:?}"),
          });
        });
      }
    };
    let lock = self.open.clone();
    Column::new()
      .width(FILL)
      .padding(16.0)
      .spacing(12.0)
      .child(caption(&format!("This device has {has}.")).width(FILL))
      .child(
        button(
          "Ask, with the passcode to fall back on",
          ask(AuthPolicy::BiometryOrPasscode),
        )
        .width(FILL),
      )
      .child(button("Ask for the face or the finger alone", ask(AuthPolicy::BiometryOnly)).width(FILL))
      .child(card(label(&self.said.get())))
      .child(card(label(if self.open.get() {
        "Behind the lock: what only the person may see."
      } else {
        "Locked."
      })))
      .child(button("Lock again", move || lock.set(false)).width(FILL))
  }
}
