# iOS: what is not done

What a lurq app on iOS still does differently from an app of the platform, or does not do at all. Kept next
to the work on `feat/ios-touch`; an entry goes when its gap is closed.

"Not checked" means nobody has run it on a device, not that it is known to be broken.

## Blocks a real app

- **UIScene.** winit starts the app without a scene delegate, which UIKit traps on in an app linked against the
  iOS 27 SDK. `crates/mobile_demo/ios/build-rust.sh` marks the binary as built with the iOS 26 SDK so that it
  starts. That is a way round, not a fix: the system may drop the old path, and an app marked so is unlikely
  to pass App Store review. winit has no scenes in 0.30.13 nor in 0.31.0-beta.4.

## Text input

Text inputs type through a view of the shell's own that speaks `UITextInput`
(`crates/lurq/src/app/winit_shell/ios_text.rs`). Typing, suggestions, autocorrection, dictation, a delete key
that speeds up, the caret under the space bar, a secure keyboard for a masked input and the person's email
addresses over an email input work on a device.

- **Password AutoFill.** Nothing is offered over a masked input. The system looks for a login and a password
  as two inputs and is shown one view, which every text input shares; and it offers saved passwords only to
  an app with an associated domain. Which of the two stops it here was not checked. Flutter keeps a hidden
  view for each input of an "autofill group"; lurq would need the same, and a way for an app to say which
  inputs go together.
- **Saving a password** after a login: not done, for the same reasons.
- **A code from a message** (`ContentKind::OneTimeCode`): set, not checked. It needs a real message.
- **Geometry.** Asked where a character is on the screen, the view answers with the caret's rectangle whatever
  the position. Enough for suggestions; not for what follows.
- **Selection handles, the loupe, the Copy / Paste menu.** None. A double tap selects a word and there is no
  way to change the selection, and no way to paste. A long press clears the selection.
- **A triple tap** for a paragraph: not checked.
- **The clipboard** on iOS: not checked.
- **A hardware keyboard:** Cmd shortcuts expect Ctrl; arrows and Tab between inputs are not checked.
- **The keyboard is dismissed by a tap outside an input**, because the tap takes the focus. A list of the
  platform keeps it up and dismisses it on a scroll; neither is done.
- **Several lines:** scrolling inside the input with a finger, and the caret in a long text, are not checked.
- **The screen turning** with the keyboard up: not checked.
- **Emoji in transformed text** (rotated, scaled) draw nothing: only the plain path draws them through Core
  Text.

## Touch

- **A long press of several fingers** has no event.
- **Text by finger** is the handles, loupe and menu above.
- Values taken from memory or by eye, not from Apple's documentation: the press delay of 150 ms, the tap's
  highlight of 80 ms, a control's reach of 70 pt, the slops of a pinch (8 pt) and a rotation (about 6°), the
  double tap's 350 ms and 40 pt.

## Scrolling

- **How fast a flick is** is measured over its last 100 ms, not as UIKit measures it, and capped at 8000 pt/s.
  `Tree::set_touch_momentum` changes both.
- **The scroll indicator** is as wide to a finger as it is drawn, 3 pt; the platform's is easier to catch.
- **The status bar's tap** scrolls the outermost list under the middle of the window. An app can turn a list
  off for it (`ScrollState::set_scrolls_to_top`) but not name the one it is for.
- Not there at all: pull to refresh, paging, pinch to zoom inside a scroll, swipe actions on a row.

## The platform

- **Going to the background and coming back:** not checked. What happens to the window, the surface and the
  app's state is not known.
- **The screen turning:** not checked.
- **Light and dark appearance, Dynamic Type, VoiceOver:** not done.
- **Features of lurq other than touch and text** have not been run on iOS: `resources` (the network; `ring`
  did not build without Xcode), `persistent_storage`, `image`, `svg`, `markdown`, the developer tools.
- **Haptics** (`ctx.window().haptic(..)`) were written without a device to feel them; no crash was reported,
  and nobody confirmed the tap itself.

## Performance

- **Single frames of up to 20 ms** against 8.3 ms at 120 Hz, a few a second at worst, measured while
  scrolling on an iPhone 17. Not looked into; it is expected on the desktop too.
- `frame_pacing_tests` measure the wall clock and fail now and then, on `master` as well.
