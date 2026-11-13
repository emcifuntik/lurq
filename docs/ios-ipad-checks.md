# iPad: what is still to be checked

Further windows of an app on iOS (`WindowOpener`, `AppEvent::WindowAsked`) were written with no tablet to
try them on. In a simulated iPad one was opened by the app, pressed and scrolled by synthetic input, and
closed by the app. What follows needs a finger, a real system and, for the last part, a display. An entry
goes when it was tried, and what it found goes to `docs/ios-gaps.md`.

## Running the demo on an iPad

The demo's project builds for both device families. With the iPad plugged in and trusted:

```sh
xcrun devicectl list devices                      # the iPad's identifier
cd crates/mobile_demo/ios
xcodebuild -project MobileDemo.xcodeproj -scheme MobileDemo -sdk iphoneos \
  -destination "id=<the iPad's identifier>" -derivedDataPath ../../../target/xcode \
  BUNDLE_ID_PREFIX=<yours> DEVELOPMENT_TEAM=<yours> -allowProvisioningUpdates build
xcrun devicectl device install app --device <the iPad's identifier> \
  ../../../target/xcode/Build/Products/Release-iphoneos/MobileDemo.app
```

The first screen's last line lists the app's events, the latest first: `WindowAsked` shows there.

## A window the app opens

More tab, **Open another window**.

- [ ] A second window appears, titled *Another window*, with the first still there to go back to.
- [ ] Its first line says the window's size and safe area. They are the window's own: not the first
      window's, and not the screen's when the window is smaller than the screen.
- [ ] **Pressed N times** counts each tap, and shows as pressed under the finger.
- [ ] The list scrolls with a finger: it follows it, goes on with momentum, stretches past its ends and
      springs back, and stops under a finger that lands on it.
- [ ] Pressed again, the button in the first window opens a third window, and each is its own.

## Windows beside each other

- [ ] With the two windows side by side, each takes touches of its own, and a touch in one does nothing in
      the other.
- [ ] A window made larger and smaller by its corner lays its content out anew, and its first line follows.
- [ ] Turned, the device turns both windows. Known gap: the second window's safe area may be the one it had
      before the turn.

## Closing

- [ ] **Close this window** closes the window, and the first comes back in front.
- [ ] A window closed by the person (its close control, or swiped away among the app's windows) is gone for
      the app too: **Open another window** opens a new one, and nothing is left waiting or drawn stale.
- [ ] With the first window closed by the person and the second left: the app goes on, and can be opened
      again from the home screen. This was not thought through; write down what happens.

## A window the person asks for

- [ ] Asked of the system for a new window of the app (the app's windows, then the plus; or the app's icon
      dragged out of the Dock), a window opens with the demo's *Another window* in it, and the first screen
      lists `WindowAsked { external: false }`.

## A display plugged in

With a display on a cable, or one the screen is sent to that takes an app's own content.

- [ ] The first screen lists `WindowAsked { external: true }`, and the display shows *On the display*, not a
      copy of the device's screen.
- [ ] The content fills the display, at the display's size, and its first line says that size.
- [ ] Unplugged, the app goes on; plugged in again, the window is there again.
- [ ] The same on an iPhone, where the More tab's button otherwise opens nothing.

## What the demo was never run on

The demo's other screens were laid out for a phone and have not been seen on a tablet at all. A pass over
them is worth it: the navigation bar and the bar of tabs at a tablet's width, a sheet (the platform's is a
card in the middle there, lurq's comes up from the bottom), and the keyboard, which on a tablet may float or
be split.

## Known not to be there

Nothing to check, so that it is not taken for a fault: a further window has no keyboard (a text input in it
is not typed into), no tap on the status bar to scroll to the top, and draws its frames as winit asks for
them, not at the display's own pace. `Active`, `Background` and the rest are told of the first window alone.
