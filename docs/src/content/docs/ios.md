---
title: iOS
description: Run a lurq app on an iPhone, and what the shell does there for touch, scrolling, the keyboard and the safe area.
---

Added after **lurq 0.59.1**, for the iPhone. The `winit` shell with the `wgpu` renderer runs the same component
tree on a phone as on a desktop; this page says what an app has to set up and what it gets. What is not done yet
is listed in `docs/ios-gaps.md` in the repository. The iPad and several windows are not supported.

`crates/mobile_demo` is a complete example: a screen for each thing below, and the Xcode project that wraps it.

## Setting up

An iOS app is a bundle that Xcode builds and signs, so the Rust binary needs a small Xcode project around it.

1. Add the targets: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
2. Depend on lurq with the `winit` and `wgpu` features. The app's `main` is the one it has on a desktop.
3. Make an Xcode project whose only build phase is a script that runs `cargo build --target ...` and copies the
   binary to `$TARGET_BUILD_DIR/$EXECUTABLE_PATH`. Name that path as the phase's output, or Xcode will not sign
   what the script put there. `crates/mobile_demo/ios/build-rust.sh` is such a script; it clears Xcode's
   environment first, because cargo's build scripts must not link against the iOS SDK.
4. Add the two keys below to the app's `Info.plist`.

### Info.plist

```xml
<!-- The app's one scene. lurq makes the delegate's class. -->
<key>UIApplicationSceneManifest</key>
<dict>
  <key>UIApplicationSupportsMultipleScenes</key>
  <false/>
  <key>UISceneConfigurations</key>
  <dict>
    <key>UIWindowSceneSessionRoleApplication</key>
    <array>
      <dict>
        <key>UISceneConfigurationName</key>
        <string>Default</string>
        <key>UISceneDelegateClassName</key>
        <string>LurqSceneDelegate</string>
      </dict>
    </array>
  </dict>
</dict>
<!-- Lets an iPhone with a 120 Hz display draw above 60 frames a second. -->
<key>CADisableMinimumFrameDurationOnPhone</key>
<true/>
```

- **The scene manifest is required.** UIKit refuses to start an app built with the iOS 27 SDK that declares no
  scene: it traps at launch. The shell registers the class `LurqSceneDelegate` and puts its window into the
  scene; the app only names the class.
- Without `CADisableMinimumFrameDurationOnPhone` an iPhone holds the app to 60 frames a second.

### Running

```sh
# A simulator
xcodebuild -project ios/MyApp.xcodeproj -scheme MyApp -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=<simulator id>" build
xcrun simctl install <simulator id> <path to MyApp.app>
xcrun simctl launch --console-pty <simulator id> <bundle id>

# A phone
xcodebuild -project ios/MyApp.xcodeproj -scheme MyApp -sdk iphoneos \
  -destination "id=<device id>" DEVELOPMENT_TEAM=<team> -allowProvisioningUpdates build
xcrun devicectl device install app --device <device id> <path to MyApp.app>
xcrun devicectl device process launch --console --device <device id> <bundle id>
```

`--console` shows what the app writes to standard error. A phone has to be unlocked for the launch.

## The safe area

The window covers the whole screen, under the status bar and the home indicator. `WindowInfo::safe_area` says
how far in from each edge content stays clear of them, in logical pixels; it is zero on a desktop.

```rust
let safe_area = ctx.window().safe_area;
Column::new()
    .background(background) // reaches the screen's edges
    .padding_top(safe_area.top)
    .padding_bottom(safe_area.bottom)
    .padding_left(safe_area.left)
    .padding_right(safe_area.right)
```

The on-screen keyboard counts: while it is up `safe_area.bottom` is its height, frame by frame as it rises, so
content padded by the safe area ends above it. The focused text input is scrolled into view as it does.

## Touch

One finger is the pointer. `on_click`, `on_mouse_down` and the rest fire as for a mouse, with these differences,
which are the platform's:

- **A press waits** 150 ms over content that scrolls, to see whether the finger scrolls instead. A finger that
  moves more than 10 points scrolls, and there is no click. A quick tap still clicks at once.
- **Nothing is hovered.** Hover styles are not applied under a finger.
- **A tap shows as pressed** for a moment even when the finger was down for less than a frame.
- `MouseEvent::pointer` is `PointerKind::Touch` for these events, for an element that must tell.

A finger's own events are separate, and bubble like the mouse's:

| Handler | Event | When |
| --- | --- | --- |
| `on_touch_start`, `on_touch_move`, `on_touch_end`, `on_touch_cancel` | `TouchEvent` | each finger, by its `id` |
| `on_tap` | `TapEvent` | a tap, with how many `fingers` |
| `on_long_press` | `LongPressEvent` | a finger held for half a second |
| `on_swipe` | `SwipeEvent` | a quick move, with its `direction` and `velocity` |
| `on_pan` | `PanEvent` | a drag, from start to end |
| `on_pinch` | `PinchEvent` | two fingers moving apart or together: `scale` |
| `on_rotate` | `RotateEvent` | two fingers turning: `rotation`, in radians |
| `on_edge_pan` | `EdgePanEvent` | a finger in from a screen edge |
| `on_history_swipe` | `HistorySwipeEvent` | the same from the left edge (back) or the right (forward) |

```rust
let navigator = ctx.navigator();
Column::new()
    .on_tap(move |event: TapEvent| fingers.set(event.fingers))
    .on_history_swipe(move |event: HistorySwipeEvent| {
        if let Some(navigator) = &navigator {
            match event.direction {
                HistoryDirection::Back => navigator.back(),
                HistoryDirection::Forward => navigator.forward(),
            };
        }
        event.stop_propagation();
    })
```

A handler of `on_touch_start` that calls `prevent_default` takes the finger: it is not the pointer, so it
neither clicks nor scrolls. Two fingers that pinch or rotate are not the pointer either.

### Haptics

```rust
ctx.window().haptic(Haptic::Selection);
```

`Haptic` is `Light`, `Medium`, `Heavy`, `Selection`, `Success`, `Warning` or `Error`. It does nothing where there
is no haptic engine.

## Screens

`StackOutlet` moves between the router's screens as the platform's apps do: a push comes in from the right, a
finger from the left edge drags the top screen back, the screens under stay alive, and a navigation bar shows
the title and the way back. See [A Stack Of Screens](../routing/#a-stack-of-screens).

## Scrolling

Scroll containers scroll under a finger as `UIScrollView` does: the content follows the finger, goes on with
momentum when it lifts, stretches past its ends and springs back, and stops under a finger that lands on it.
One gesture scrolls one container, the innermost that has room in the finger's direction.

- **The scroll indicator.** On iOS a bar is `ScrollBarStyle::indicator()` unless styled otherwise: thin, over
  the content, shown while it scrolls.
- **A tap on the status bar** scrolls the outermost vertical list to its top.
  `ScrollState::set_scrolls_to_top(false)` leaves a list out of it.
- **The keyboard** stays up when a list is scrolled, unless the list says otherwise:
  `ScrollState::set_dismisses_keyboard(true)` puts it away when a finger starts to scroll.
- **Momentum** can be changed for the whole window with `Tree::set_touch_momentum(TouchMomentum { .. })`:
  the deceleration rate, and the slowest and fastest speeds. The defaults are the platform's.

`VirtualizedList` scrolls the same way.

## Text inputs

A focused `TextInput` brings the on-screen keyboard up, and loses it with the focus. The platform does the
editing by finger: its own caret, the handles at a selection's ends, the loupe, and the menu to copy, cut, paste
and select all. Suggestions, autocorrection, dictation and the caret under the space bar work as in any text
field.

What the keyboard is, an input says:

```rust
TextInput::new(email)
    .single_line()
    .keyboard(KeyboardKind::Email)
    .content(ContentKind::Email)
    .return_key(ReturnKey::Next)
```

- `keyboard`: `Text`, `Email`, `Url`, `Number`, `Decimal`, `Phone` or `Search`.
- `return_key`: what the Return key says: `Default`, `Done`, `Go`, `Next`, `Search` or `Send`. It is a label:
  the key is an Enter key press to the input, as on a desktop.
- `content`: what the text is, for the platform to offer the person's own: `Name`, `Email`, `Username`,
  `Password`, `NewPassword`, `OneTimeCode`, `Phone`; `None` to offer nothing.
- `mask()` makes the keyboard secure: no suggestions, and the keyboard is left out of a screen recording.

**A tap outside an input leaves the keyboard up**, as in the platform's apps: only another text input takes the
focus from the one being typed into. A screen of inputs gives a way to put it away, such as a list that
dismisses the keyboard when scrolled (above).

Saved passwords are not offered yet.

## Frames

The shell presents from a `CADisplayLink` at the display's full rate, and only while something changes: an idle
app draws nothing. Going to the background and coming back needs nothing from the app.
