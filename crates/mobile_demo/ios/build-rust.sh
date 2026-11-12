#!/bin/sh
# Xcode's build phase: builds the Rust binary for the platform Xcode is building for and puts it in the app
# bundle, which Xcode then signs. Xcode's own environment is dropped first: its SDKROOT points at the iOS SDK,
# which the build scripts and proc macros cargo builds for the Mac must not link against.
set -eu

case "$PLATFORM_NAME" in
  iphonesimulator) triple=aarch64-apple-ios-sim ;;
  *) triple=aarch64-apple-ios ;;
esac

# A simulator reads an app's entitlements out of its binary, where Xcode's linker puts the ones it makes up
# for a simulator; without them the keychain, for one, takes nothing from the app. The binary is cargo's, so
# they are put into it here. On a device they are in the signature, which is Xcode's.
set --
simulated="${TARGET_TEMP_DIR:-}/${FULL_PRODUCT_NAME:-}-Simulated.xcent"
if [ "$PLATFORM_NAME" = iphonesimulator ] && [ -f "$simulated" ]; then
  set -- -- -C "link-arg=-Wl,-sectcreate,__TEXT,__entitlements,$simulated"
  if [ -f "$simulated.der" ]; then
    set -- "$@" -C "link-arg=-Wl,-sectcreate,__TEXT,__ents_der,$simulated.der"
  fi
fi

workspace="$SRCROOT/../../.."
cd "$workspace"
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
  DEVELOPER_DIR="$DEVELOPER_DIR" IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET" \
  cargo rustc -p mobile_demo --release --target "$triple" ${CARGO_FEATURES:+--features "$CARGO_FEATURES"} "$@"

cp "target/$triple/release/mobile_demo" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
