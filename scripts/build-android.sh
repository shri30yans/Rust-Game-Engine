#!/usr/bin/env bash
# Builds a game as a signed arm64 Android APK at target/android/GAME-debug.apk.
#
# Usage: scripts/build-android.sh [--dry-run] [GAME]
#   GAME defaults to sandbox. It names the folder games/GAME, whose crate (games/GAME/game) is
#   the package built, and whose manifest (games/GAME/android/AndroidManifest.xml) is packaged.
#   The native library is lib<crate name with dashes as underscores>.so, which the manifest's
#   android.app.lib_name must match.
#   --dry-run checks the game and prints the plan without the NDK or any build.
#
# Needs: the Android SDK (platform android-36, build-tools, NDK) under ANDROID_HOME, JDK
# (java and keytool on PATH), and the aarch64-linux-android Rust target. The NDK's clang links
# the library directly; cargo-ndk is not used. Re-running is safe: the staging folder is rebuilt
# each time and the debug keystore is created only once.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_HOME="${ANDROID_HOME:-$HOME/.local/trove-android/sdk}"
PLATFORM="android-36"
MIN_SDK=26
TARGET_SDK=35
TARGET="aarch64-linux-android"

die() {
    echo "build-android: $*" >&2
    exit 1
}

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=1
    shift
fi
GAME="${1:-sandbox}"
if [[ $# -gt 1 ]]; then
    die "too many arguments; usage: build-android.sh [--dry-run] [GAME]"
fi
if [[ ! "$GAME" =~ ^[a-z][a-z0-9-]*$ ]]; then
    die "invalid game name '$GAME': use lower-case letters, digits and dashes"
fi
GAME_DIR="$ROOT/games/$GAME"
if [[ ! -f "$GAME_DIR/game/Cargo.toml" ]]; then
    die "no game crate at games/$GAME/game (is '$GAME' a game?)"
fi
MANIFEST="$GAME_DIR/android/AndroidManifest.xml"
if [[ ! -f "$MANIFEST" ]]; then
    die "no APK manifest at games/$GAME/android/AndroidManifest.xml"
fi
LIB_NAME="lib${GAME//-/_}"
OUT="$ROOT/target/android"
KEYSTORE="$OUT/debug.keystore"
APK="$OUT/$GAME-debug.apk"

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "build-android: dry run for game $GAME"
    echo "build-android: crate games/$GAME/game, manifest $MANIFEST"
    echo "build-android: library $LIB_NAME.so, APK $APK"
    exit 0
fi

# Prints the highest-sorting entry of a directory, or nothing when it does not exist.
highest() {
    if [[ -d "$1" ]]; then
        ls -1 "$1" | sort -V | tail -n 1
    fi
}

# Collect every missing prerequisite so one run reports them all.
missing=()
need() {
    if [[ ! -e "$1" ]]; then
        missing+=("$2: $1")
    fi
}

NDK_VERSION="$(highest "$ANDROID_HOME/ndk")"
BUILD_TOOLS_VERSION="$(highest "$ANDROID_HOME/build-tools")"
NDK="$ANDROID_HOME/ndk/$NDK_VERSION"
BUILD_TOOLS="$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION"
JAR="$ANDROID_HOME/platforms/$PLATFORM/android.jar"

case "$(uname -s)" in
    Darwin) HOST_TAG="darwin-x86_64" ;;
    Linux) HOST_TAG="linux-x86_64" ;;
    *) die "unsupported host: $(uname -s)" ;;
esac
LLVM_BIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
CLANG="$LLVM_BIN/aarch64-linux-android$MIN_SDK-clang"
AR="$LLVM_BIN/llvm-ar"
AAPT2="$BUILD_TOOLS/aapt2"
ZIPALIGN="$BUILD_TOOLS/zipalign"
APKSIGNER="$BUILD_TOOLS/apksigner"

need "$ANDROID_HOME" "Android SDK (set ANDROID_HOME)"
if [[ -z "$NDK_VERSION" ]]; then
    missing+=("Android NDK: none under $ANDROID_HOME/ndk")
fi
if [[ -z "$BUILD_TOOLS_VERSION" ]]; then
    missing+=("Android build-tools: none under $ANDROID_HOME/build-tools")
fi
need "$JAR" "Android platform $PLATFORM"
need "$CLANG" "NDK clang for $TARGET"
need "$AR" "NDK llvm-ar"
need "$AAPT2" "build-tools aapt2"
need "$ZIPALIGN" "build-tools zipalign"
need "$APKSIGNER" "build-tools apksigner"
need "$MANIFEST" "APK manifest"
for tool in cargo rustup java keytool zip; do
    if ! command -v "$tool" >/dev/null; then
        missing+=("command on PATH: $tool")
    fi
done
if [[ ${#missing[@]} -gt 0 ]]; then
    echo "build-android: missing prerequisites:" >&2
    for item in "${missing[@]}"; do
        echo "  - $item" >&2
    done
    exit 1
fi

installed_targets="$(rustup target list --installed)"
case "$installed_targets" in
    *"$TARGET"*) ;;
    *) die "Rust target $TARGET is not installed (run: rustup target add $TARGET)" ;;
esac

mkdir -p "$OUT"
cd "$ROOT"

echo "build-android: NDK $NDK_VERSION, build-tools $BUILD_TOOLS_VERSION, $TARGET"
cargo run -p cook -- "$GAME"
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG" \
CC_aarch64_linux_android="$CLANG" \
AR_aarch64_linux_android="$AR" \
cargo build --release --target "$TARGET" -p "$GAME" --lib

SO="$ROOT/target/$TARGET/release/$LIB_NAME.so"
if [[ ! -f "$SO" ]]; then
    die "cargo did not produce $SO"
fi

# The native library must sit at lib/<abi>/<name> inside the APK.
STAGE="$OUT/stage"
rm -rf "$STAGE"
mkdir -p "$STAGE/lib/arm64-v8a" "$STAGE/assets"
cp "$SO" "$STAGE/lib/arm64-v8a/$LIB_NAME.so"
PACK="$GAME_DIR/content/cooked/$GAME.pak"
if [[ ! -f "$PACK" ]]; then
    die "cooker did not produce $PACK"
fi
cp "$PACK" "$STAGE/assets/$GAME.pak"

rm -f "$OUT/base.apk" "$OUT/unaligned.apk" "$OUT/aligned.apk"
"$AAPT2" link \
    --manifest "$MANIFEST" \
    -I "$JAR" \
    --min-sdk-version "$MIN_SDK" \
    --target-sdk-version "$TARGET_SDK" \
    --version-code 1 \
    --version-name 0.1.0 \
    -o "$OUT/base.apk"

cp "$OUT/base.apk" "$OUT/unaligned.apk"
(cd "$STAGE" && zip -q -X -0 "$OUT/unaligned.apk" "lib/arm64-v8a/$LIB_NAME.so")
(cd "$STAGE" && zip -q -X -0 "$OUT/unaligned.apk" "assets/$GAME.pak")
"$ZIPALIGN" -p -f 4 "$OUT/unaligned.apk" "$OUT/aligned.apk"

# The debug keystore is made once and reused, so every build signs with the same key.
if [[ ! -f "$KEYSTORE" ]]; then
    keytool -genkeypair \
        -keystore "$KEYSTORE" \
        -storepass android \
        -keypass android \
        -alias androiddebugkey \
        -keyalg RSA \
        -keysize 2048 \
        -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US" \
        -noprompt
fi

rm -f "$APK"
"$APKSIGNER" sign \
    --ks "$KEYSTORE" \
    --ks-key-alias androiddebugkey \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$APK" \
    "$OUT/aligned.apk"

echo "build-android: apksigner verify"
"$APKSIGNER" verify --verbose "$APK"

echo "build-android: aapt2 badging"
"$AAPT2" dump badging "$APK" | grep -E "^(package|minSdkVersion|targetSdkVersion|native-code)"

echo "build-android: APK $APK"
echo "build-android: size $(wc -c < "$APK" | tr -d ' ') bytes"
