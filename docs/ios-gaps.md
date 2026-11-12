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
- **The bar** is over a screen's list, which shows through it blurred; the platform's material also
  brightens and saturates what is behind it, and the newest system's bar is glass that bends it. The
  toolbar at the bottom and the list a long press on the way back opens are not blurred.
- **How things move** in the stack (the spring's length, how far a large title grows and the way it takes to
  a way back) was set by eye and not measured against the platform; `StackMotion` changes the numbers.
- **A search field** is the field alone: no scopes under it, no suggestions, and it does not come and go
  with a move of its own when it takes the bar's place.
- **Sheets** are there (`Sheet`, and `SheetHost` for the app that draws back behind one), with these
  differences from the platform's: a sheet is never one the window behind can still be used under; there is
  no stack of screens inside a sheet with a bar of its own; the corners of the app drawn back are rounded at
  once and not little by little; and the numbers (the radii, the dim, the spring, how small the app gets)
  were set by eye.
- **Tabs** are there (`Tabs`, `TabOutlet`, `TabBar`), with these differences from the platform's: the bar is
  the plain one across the bottom, not the floating glass one of the newest system with a lens a finger
  drags between tabs; it does not get smaller as the content scrolls; the picture of a tab that is chosen
  does not spring; there is no tab for searching, no "More" tab for those that do not fit, no accessory over
  the bar, and no sidebar in a wide window. Its sizes were set by eye in a simulator.

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

What a person sets for the whole system reaches a lurq app (`ctx.window().system`), and the theme, the stack,
the bars and the sheets follow it. Tried in a simulator: the appearance and the text size, changed while the
app runs. Not checked on a device, and bold text, reduced motion and reduced transparency not checked at all.

- **Dynamic Type** is the theme's typography alone, as on the platform: a size an app gives itself does not
  change unless it asks (`theme.scaled`). The navigation bar's and the bar of tabs' own text does not grow,
  and neither does a large title, which the platform's does. The styles of a form and of Markdown carry sizes
  of their own, which do not follow.
- **Light and dark:** a theme follows with a palette for each. The demo app is dark in both.
- **Bold Text** is the typography's weights; text with a weight of the app's own keeps it, where the
  platform makes every text heavier.
- **Reduce Motion:** the stack fades. A sheet still comes up from the bottom, and a text input's caret, a
  scroll's momentum and an app's own transitions are as always.
- **The Liquid Glass setting** (clear or tinted) is not followed. No API tells it. UIKit keeps it in its own
  defaults (`UIViewGlassTintAmount` in `com.apple.UIKit`), which an app can read and which was seen to change
  with the setting on an iPhone 17, but the name is in no documentation and lurq does not lean on it.
- **Increase Contrast, Differentiate Without Color, Button Shapes, On/Off Labels** are told and nothing in
  lurq acts on them.
- **A change while the app is in front and nothing happens** (the appearance at sunset) is taken at the next
  touch or frame, not at once.
- **VoiceOver, Switch Control, Voice Control:** the screen is one opaque view to them. Nothing in it has a
  label, a role or a value, and nothing can be reached or activated.
- **Larger accessibility sizes, Zoom, Smart Invert:** not checked.

## Performance

- **Late frames.** While a finger drags content or an animation runs, 2 to 5 frames in a hundred come a
  refresh late on an iPhone 17; it was a third of them before the display link was left running between
  frames. Whether a desktop has late frames of its own was not looked into.
