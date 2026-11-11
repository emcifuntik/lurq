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
- **What the app is opened with:** a link of the app's own scheme reaches it (`AppEvent::OpenUrl`), tried on a
  device. A universal link and a home screen shortcut: not checked.

## Text input

Text inputs type through a view of the shell's own that speaks `UITextInput`
(`crates/lurq/src/app/winit_shell/ios_text.rs`). Typing, suggestions, autocorrection, dictation, a delete key
that speeds up, the caret under the space bar, a secure keyboard for a masked input and the person's email
addresses over an email input work on a device. So do the platform's caret, the selection's handles, the loupe
and the menu to copy and paste (`UITextInteraction`); the highlight is painted by the input, under its text,
where UIKit says the selection is. A triple tap selects a paragraph, and a long text scrolls
inside its input.

- **Password AutoFill.** Nothing is offered over a masked input. The system looks for a login and a password
  as two inputs and is shown one view, which every text input shares; and it offers saved passwords only to
  an app with an associated domain. Which of the two stops it here was not checked. Flutter keeps a hidden
  view for each input of an "autofill group"; lurq would need the same, and a way for an app to say which
  inputs go together.
- **Saving a password** after a login: not done, for the same reasons.
- **A code from a message** (`ContentKind::OneTimeCode`): set, not checked. It needs a real message.
- **A hardware keyboard:** its keys are passed to the tree, and shortcuts take Command. Written with no
  keyboard to try it on: nothing of it was checked.
- **Putting the keyboard away.** A tap outside an input leaves it up, and a list set to
  (`ScrollState::set_dismisses_keyboard`) puts it away when a finger starts to scroll it. The platform can also
  have the keyboard follow the finger down as the list is dragged; that is not done.
- **Emoji in transformed text** (rotated, scaled) draw nothing: only the plain path draws them through Core
  Text.

## Screens

`StackOutlet` is a stack of screens with a navigation bar (`docs/src/content/docs/routing.md`).

- **The way back's arrow** is the platform's symbol with the `raster` feature; without it, a drawing after
  it.
- **The move** is a spring of 500 ms that starts at the finger's speed; the platform's own spring was not
  measured against it.
- **The bar** is opaque when it has a color: content does not show through it, since the renderer has no
  blur to make the platform's material with. There is no search field in it.
- **How things move** in the stack (the spring's length, how far a large title grows and the way it takes to
  a way back) was set by eye and not measured against the platform; `StackMotion` changes the numbers.
- **A long press on the way back** shows no list of the screens under.
- **No search field, no toolbar at the bottom, no prompt over the title.**
- **Sheets:** a screen that comes up from the bottom over the stack, with its detents and the drag that
  puts it away. Not there.
- **Tabs:** a tab bar with a stack of screens for each tab. Not there.

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

- **The screen turning.** The window, the safe area and the keyboard follow, and a phone on its side shows
  no status bar. Now and then, turned back upright, the window shows a frame drawn for half its height until
  something asks for another; the shell asks for one when the window's size is not the last frame's, which
  made it rarer and did not end it. Not looked into further.
- **Face ID and Touch ID.** An app cannot ask the person to prove who they are, to open the app or a part of
  it (`LocalAuthentication`), nor keep a secret behind it in the keychain. Not done. An app that asks needs
  `NSFaceIDUsageDescription` in its `Info.plist`.
- **Features of lurq other than touch and text** have not been run on iOS: `resources` (the network; `ring`
  did not build without Xcode), `persistent_storage`, `image`, `svg`, `markdown`, the developer tools.
- **Haptics** (`ctx.window().haptic(..)`) were written without a device to feel them; no crash was reported,
  and nobody confirmed the tap itself.

## Appearance and accessibility

None of what a person sets for the whole system reaches a lurq app. Not done:

- **Light and dark appearance:** the app is not told which the system is in, nor that it changed, and the
  theme does not follow it.
- **Dynamic Type:** text keeps its size whatever text size the person chose.
- **Bold Text:** text keeps its weight.
- **Increase Contrast, Reduce Transparency, Differentiate Without Color, Button Shapes, On/Off Labels.**
- **Reduce Motion:** the stack's screens and every animation move as always. The platform cross-fades in
  their place.
- **VoiceOver, Switch Control, Voice Control:** the screen is one opaque view to them. Nothing in it has a
  label, a role or a value, and nothing can be reached or activated.
- **Larger accessibility sizes, Zoom, Smart Invert:** not checked.

## Performance

- **Late frames.** While a finger drags content or an animation runs, 2 to 5 frames in a hundred come a
  refresh late on an iPhone 17; it was a third of them before the display link was left running between
  frames. Whether a desktop has late frames of its own was not looked into.
- Six tests fail when every test target is run together, on `master` as well, and pass run alone
  (`window_chrome` twice, `generic_family_serif`, `give_way_drop_below`, `dpi_scaled_wrapped_text`,
  `building_a_tree_allocates_no_box_or_watch_per_node`). Not looked into.
