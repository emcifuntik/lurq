# iOS: what is not done

What a lurq app on iOS still does differently from an app of the platform, or does not do at all. Kept next
to the work on `feat/ios-touch`; an entry goes when its gap is closed.

"Not checked" means nobody has run it on a device, not that it is known to be broken.

## Scenes

An app starts in a scene of UIKit's (`UIWindowScene`), which an app built with the iOS 27 SDK must: the shell
makes the scene's delegate and puts winit's window into the scene. The app has to name that delegate in its
`Info.plist`, as `crates/mobile_demo/ios/Info.plist` does (`UIApplicationSceneManifest`, with
`UISceneDelegateClassName` set to `LurqSceneDelegate`); winit starts the app with no delegate of its own that
could say so in code. Going to the background and back, and the lock screen, were tried on a device.

- **One scene.** A second window of the app, on an iPad or an outer display, is not there.
- **What the scene tells its delegate** (active, background, what it was opened with: a link, a shortcut) is
  not passed on; winit hears the app's own notifications, which UIKit still sends.

## Text input

Text inputs type through a view of the shell's own that speaks `UITextInput`
(`crates/lurq/src/app/winit_shell/ios_text.rs`). Typing, suggestions, autocorrection, dictation, a delete key
that speeds up, the caret under the space bar, a secure keyboard for a masked input and the person's email
addresses over an email input work on a device. So do the platform's caret, the selection's handles, the loupe
and the menu to copy and paste (`UITextInteraction`); the highlight is painted by the input, under its text,
where UIKit says the selection is.

- **Password AutoFill.** Nothing is offered over a masked input. The system looks for a login and a password
  as two inputs and is shown one view, which every text input shares; and it offers saved passwords only to
  an app with an associated domain. Which of the two stops it here was not checked. Flutter keeps a hidden
  view for each input of an "autofill group"; lurq would need the same, and a way for an app to say which
  inputs go together.
- **Saving a password** after a login: not done, for the same reasons.
- **A code from a message** (`ContentKind::OneTimeCode`): set, not checked. It needs a real message.
- **A triple tap** for a paragraph: not checked.
- **A hardware keyboard:** Cmd shortcuts expect Ctrl; arrows and Tab between inputs are not checked.
- **Putting the keyboard away.** A tap outside an input leaves it up, and a list set to
  (`ScrollState::set_dismisses_keyboard`) puts it away when a finger starts to scroll it. The platform can also
  have the keyboard follow the finger down as the list is dragged; that is not done.
- **Several lines:** scrolling inside the input with a finger, and the caret in a long text, are not checked.
- **The screen turning** with the keyboard up: not checked.
- **Emoji in transformed text** (rotated, scaled) draw nothing: only the plain path draws them through Core
  Text.

## Screens

`StackOutlet` is a stack of screens with a navigation bar (`docs/src/content/docs/routing.md`).

- **The move** is a curve of 350 ms, not the platform's spring, and a finger that lets go carries no speed
  into it.
- **The bar** is opaque and its content does not scroll under it; there is no large title, no search field
  and no material. The way back's chevron is a character, not the platform's symbol.
- **A screen cannot hide the bar** or put its own view in the title's place.
- **Sheets and tabs:** not there.

## Touch

- **A long press of several fingers** has no event.
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
