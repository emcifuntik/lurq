#!/bin/sh
# Xcode's build phase: builds the Rust binary for the platform Xcode is building for and puts it in the app
# bundle, which Xcode then signs. Xcode's own environment is dropped first: its SDKROOT points at the iOS SDK,
# which the build scripts and proc macros cargo builds for the Mac must not link against.
set -eu

# The second value is the platform's number in a Mach-O build version: 2 is iOS, 7 the iOS simulator.
case "$PLATFORM_NAME" in
  iphonesimulator) triple=aarch64-apple-ios-sim platform=7 ;;
  *) triple=aarch64-apple-ios platform=2 ;;
esac

workspace="$SRCROOT/../../.."
cd "$workspace"
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
  DEVELOPER_DIR="$DEVELOPER_DIR" IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET" \
  cargo build -p mobile_demo --release --target "$triple" ${CARGO_FEATURES:+--features "$CARGO_FEATURES"}

# winit 0.30 starts the app without a scene delegate. UIKit traps at launch in an app that does so and was
# linked against the iOS 27 SDK or a later one ("no scene lifecycle adoption"); linked against an earlier SDK
# the app still starts. Until winit adopts scenes, the binary says it was built with the iOS 26 SDK.
xcrun vtool -set-build-version "$platform" "$IPHONEOS_DEPLOYMENT_TARGET" 26.0 -replace \
  -output "$TARGET_BUILD_DIR/$EXECUTABLE_PATH" "target/$triple/release/mobile_demo"
