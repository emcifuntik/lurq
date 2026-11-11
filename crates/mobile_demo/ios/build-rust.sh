#!/bin/sh
# Xcode's build phase: builds the Rust binary for the platform Xcode is building for and puts it in the app
# bundle, which Xcode then signs. Xcode's own environment is dropped first: its SDKROOT points at the iOS SDK,
# which the build scripts and proc macros cargo builds for the Mac must not link against.
set -eu

case "$PLATFORM_NAME" in
  iphonesimulator) triple=aarch64-apple-ios-sim ;;
  *) triple=aarch64-apple-ios ;;
esac

workspace="$SRCROOT/../../.."
cd "$workspace"
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
  DEVELOPER_DIR="$DEVELOPER_DIR" IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET" \
  cargo build -p mobile_demo --release --target "$triple" ${CARGO_FEATURES:+--features "$CARGO_FEATURES"}

cp "target/$triple/release/mobile_demo" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
