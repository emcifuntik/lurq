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
  /// What the last thing done with the secret came to.
  kept: Signal<String>,
}

/// The name the demo's secret is kept under.
const SECRET: &str = "demo-token";

impl Component for AuthScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      said: ctx.signal("Not asked yet".to_owned()),
      open: ctx.signal(false),
      kept: ctx.signal(String::new()),
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
    // A secret in the keychain: the system asks for the face or the finger before it gives it back.
    let keep = {
      let kept = self.kept.clone();
      move || {
        kept.set(
          match biometry::keep_secret(SECRET, b"a token to log in with", AuthPolicy::BiometryOrPasscode) {
            Ok(()) => "Kept. Reading it asks who you are.".to_owned(),
            Err(error) => format!("Not kept: {error:?}"),
          },
        );
      }
    };
    let read = {
      let kept = self.kept.clone();
      move || {
        let kept = kept.clone();
        kept.set("Asking…".to_owned());
        biometry::read_secret(SECRET, "Read the secret that was kept", move |answer| {
          kept.set(match answer {
            Ok(secret) => format!("Read: {}", String::from_utf8_lossy(&secret)),
            Err(error) => format!("Not read: {error:?}"),
          });
        });
      }
    };
    let forget = {
      let kept = self.kept.clone();
      move || {
        kept.set(match biometry::forget_secret(SECRET) {
          Ok(()) => "Forgotten.".to_owned(),
          Err(error) => format!("Not forgotten: {error:?}"),
        });
      }
    };
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
      .child(caption("A secret kept in the keychain, behind the same check").width(FILL))
      .child(button("Keep a secret", keep).width(FILL))
      .child(button("Read it", read).width(FILL))
      .child(button("Forget it", forget).width(FILL))
      .child(card(label(&format!(
        "{} {}",
        self.kept.get(),
        if biometry::has_secret(SECRET) {
          "One is kept."
        } else {
          "None is kept."
        }
      ))))
  }
}
