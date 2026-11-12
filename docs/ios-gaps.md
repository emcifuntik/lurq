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

- **Password AutoFill.** The view that types says it is a text field while its input hides what is typed,
  which is what UIKit offers saved passwords to, and a text field that is not seen stands for each other
  input of the login, for the platform to fill the name beside the password. Checked on an iPhone 17, with
  the system's passwords and with a password manager's. Saving a password after a login needs the app's
  associated domain and was not tried.
- **A code from a message** (`ContentKind::OneTimeCode`): set, not checked. It needs a real message.
- **A hardware keyboard:** its keys are passed to the tree, and shortcuts take Command. Written with no
  keyboard to try it on: nothing of it was checked.
- **Putting the keyboard away.** A tap outside an input leaves it up, and a list set to
  (`ScrollState::set_keyboard_dismissal`) puts it away when a finger starts to scroll it, or has it follow the
  finger down. The following is UIKit's own, borrowed: a scroll view that is not seen, whose pan gesture is
  put on the window's view. Written without a device to try it on: not checked.
- **Emoji in transformed text** (rotated, scaled) draw nothing: only the plain path draws them through Core
  Text.

## Screens

`StackOutlet` is a stack of screens with a navigation bar (`docs/src/content/docs/routing.md`).

- **The way back's arrow** is the platform's symbol with the `raster` feature; without it, a drawing after
  it.
- **The bar** is over a screen's list, which shows through it blurred, as it does through the toolbar and
  the list a long press on the way back opens; the platform's material also brightens and saturates what is
  behind it, and the newest system's bar is glass that bends it.
- **How things move** in the stack (the spring's length, how far a large title grows and the way it takes to
  a way back) was set by eye and not measured against the platform; `StackMotion` changes the numbers.
- **A search field** is the field alone: no scopes under it, no suggestions, and it does not come and go
  with a move of its own when it takes the bar's place.
- **Sheets** are there (`Sheet`, and `SheetHost` for the app that draws back behind one), with these
  differences from the platform's: a sheet is never one the window behind can still be used under; the
  corners of the app drawn back are rounded at once and not little by little; a list of a stack inside a
  sheet does not give the finger to the sheet at its top; and the numbers (the radii, the dim, the spring,
  how small the app gets) were set by eye.
- **Tabs** are there (`Tabs`, `TabOutlet`, `TabBar`), flat or floating, with these differences from the
  platform's: a floating bar is a blurred rounded bar, not glass, and has no lens a finger drags between
  tabs; as a list is scrolled the bar goes out of view, where the platform's shrinks to a small one; the
  picture of a tab that is chosen does not spring; the tab to search in is a place apart and does not open
  into a field; the list of the tabs that do not fit cannot be put in another order; there is no accessory
  over the bar, and no sidebar in a wide window. Its sizes were set by eye in a simulator.

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
- **Pull to refresh** (`PullToRefresh`) and **swipe actions** (`SwipeRow`) are there, written without a device
  to pull and swipe on: only their tests have run. The spinner is drawn by lurq, not the platform's; an
  action does not stretch as the row is pulled past it; and opening one row does not close another unless
  the app ties them with `SwipeRowProps::open`.
- **Paging** (`ScrollState::set_paging`) and **pinch to zoom** (`ZoomScroll`) are there, written without a
  device to try them on: only their tests have run, and a double tap in a simulator. How fast a lift throws
  a list at its next page (200 pt/s) and the spring that takes it there were set by eye. A pinch stops at the
  ends of its range where the platform's gives and springs back; a double tap changes the scale with no
  move; and what is zoomed is drawn scaled, its text not laid out anew.

## The platform

- **The screen turning.** The window, the safe area and the keyboard follow, and a phone on its side shows
  no status bar. Now and then, turned back upright, the window shows a frame drawn for half its height until
  something asks for another; the shell asks for one when the window's size is not the last frame's, which
  made it rarer and did not end it. Not looked into further.
- **Face ID and Touch ID** (`app::biometry`): an app asks the person to prove who they are, and keeps a
  secret in the keychain behind the same check. Written without a device to try it on: the secret was kept,
  read and forgotten in a simulator, which asks for no face. A Mac's Touch ID is not asked, and its keychain
  not used, though it has the same frameworks.
- **Features of lurq other than touch and text** have not been run on iOS: `resources` (the network; `ring`
  did not build without Xcode), `persistent_storage`, `image`, `svg`, `markdown`, the developer tools.
- **Haptics** (`ctx.window().haptic(..)`) were written without a device to feel them; no crash was reported,
  and nobody confirmed the tap itself.

## Appearance and accessibility

What a person sets for the whole system reaches a lurq app (`ctx.window().system`), and the theme, the stack,
the bars and the sheets follow it. Tried in a simulator: the appearance and the text size, changed while the
app runs. Not checked on a device, and bold text, reduced motion and reduced transparency not checked at all.

- **Dynamic Type** is the theme's typography, what is made from it (a form's labels, Markdown's sizes, which
  are shares of it) and a stack's large title: a size an app gives itself does not change unless it asks
  (`theme.scaled`). The navigation bar's and the bar of tabs' own text keeps its size, as the platform's
  does, but the platform shows a tab large when it is held, which lurq does not.
- **Light and dark:** a theme follows with a palette for each. The demo app is dark in both.
- **Bold Text** makes every text two weights heavier, where the font has the weights.
- **Reduce Motion:** the stack and a sheet fade. A text input's caret, a scroll's momentum and an app's own
  transitions are as always.
- **Increase Contrast** makes the bars solid and their lines and unchosen tabs stand out; **Button Shapes**
  underlines the way back. Nothing else in lurq acts on them, nor on **Differentiate Without Color** and
  **On/Off Labels**, which are only told.
- **The Liquid Glass setting** (clear or tinted) is not followed. No API tells it. UIKit keeps it in its own
  defaults (`UIViewGlassTintAmount` in `com.apple.UIKit`), which an app can read and which was seen to change
  with the setting on an iPhone 17, but the name is in no documentation and lurq does not lean on it.
- **A change while the app is in front and nothing happens** (the appearance at sunset) is taken at the next
  touch or frame, not at once.
- **VoiceOver, Switch Control, Voice Control:** the screen is one opaque view to them. Nothing in it has a
  label, a role or a value, and nothing can be reached or activated.
- **Larger accessibility sizes, Zoom, Smart Invert:** not checked.

## Performance

- **Late frames.** While a finger drags content or an animation runs, 2 to 5 frames in a hundred come a
  refresh late on an iPhone 17; it was a third of them before the display link was left running between
  frames. Whether a desktop has late frames of its own was not looked into.
