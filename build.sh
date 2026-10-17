#!/bin/sh
# ./build.sh [desktop|all]: with no argument, `native` stages device/ for a copy
# to /mnt/us; `desktop` builds the desktop app and installs it as $INSTALLED;
# `all` runs `native`, then `desktop`.
set -eu

TARGET="armv7-unknown-linux-musleabihf"
ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="$ROOT/device/extensions/readinglog/bin/readinglog"
# $BUNDLE is the app `cargo tauri build` writes, `productName` in desktop/tauri.conf.json.
BUNDLE="$ROOT/target/release/bundle/macos/Reading Log.app"
INSTALLED="/Applications/Reading Log.app"

# $VERSION is [workspace.package].version in Cargo.toml.
VERSION="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0}
                f && /^version *=/{gsub(/[" ]/,""); sub(/^version=/,""); print; exit}' \
    "$ROOT/Cargo.toml")"
[ -n "$VERSION" ] || { echo "error: no version in [workspace.package]" >&2; exit 1; }

# `native` cross-compiles readinglog-native for $TARGET and stages
# device/extensions/readinglog/ and device/documents/ReadingLog.sh.
native() {
    if ! rustup target list --installed | grep -qx "$TARGET"; then
        echo "error: rustup target '$TARGET' is not installed" >&2
        echo "       fix: rustup target add $TARGET" >&2
        exit 1
    fi

    # $CONFIG carries $VERSION in its <version> element.
    CONFIG="$ROOT/device/extensions/readinglog/config.xml"
    sed "s|<version>[^<]*</version>|<version>$VERSION</version>|" "$CONFIG" > "$CONFIG.new"
    mv "$CONFIG.new" "$CONFIG"

    echo "==> building readinglog $VERSION for $TARGET"
    cargo build --manifest-path "$ROOT/Cargo.toml" --release --target "$TARGET" \
        -p readinglog-native --bin readinglog-native

    # $OUT drops the `-native` suffix `cargo build` gives the binary.
    mkdir -p "$(dirname "$OUT")"
    cp "$ROOT/target/$TARGET/release/readinglog-native" "$OUT"
    chmod +x "$OUT" 2>/dev/null || true

    echo "==> staged $(ls -lh "$OUT" | awk '{print $5}') -> device/extensions/readinglog/bin/readinglog"
    file "$OUT" 2>/dev/null || true

    # $DUMP is a copy of $ROOT/scripts/dumplogs.sh.
    DUMP="$ROOT/device/extensions/readinglog/bin/dump.sh"
    cp "$ROOT/scripts/dumplogs.sh" "$DUMP"
    chmod +x "$DUMP" 2>/dev/null || true
    echo "==> staged scripts/dumplogs.sh -> device/extensions/readinglog/bin/dump.sh"

    # $TILE carries $COVER on its `# Icon:` line, embedded by device/make-tile.sh.
    TILE="$ROOT/device/documents/ReadingLog.sh"
    COVER="$ROOT/device/assets/cover.png"
    if ! grep -q '^# Icon: data:image/png;base64,' "$TILE" 2>/dev/null; then
        echo "warning: $TILE has no embedded cover — run device/make-tile.sh" >&2
    elif [ -f "$COVER" ] && [ "$COVER" -nt "$TILE" ]; then
        echo "warning: assets/cover.png is newer than the tile's embedded icon" >&2
        echo "         the old cover would ship — run device/make-tile.sh" >&2
    fi

    cat <<'EOF'

==> install — copy these two onto the Kindle

    device/extensions/readinglog/    ->  /mnt/us/extensions/readinglog/
    device/documents/ReadingLog.sh   ->  /mnt/us/documents/ReadingLog.sh

Launch from the library tile or from KUAL; both run bin/readinglog.sh.
The record lives in /mnt/us/extensions/readinglog/sessions.tsv.
Logs, if anything goes wrong, in /mnt/us/logs/readinglog.log.

Diagnostics, for a bug report, write /mnt/us/dumplogs.zip:

    sh /mnt/us/extensions/readinglog/bin/dump.sh

scripts/dumplogs.sh is the same file, for /mnt/us/documents/ where the
jailbreak runs scriptlets.

An open that goes wrong needs the open-path trace, which is off by default
because it is the loudest thing the extension writes. Turn it on, reproduce
the open, turn it off again:

    sh /mnt/us/extensions/readinglog/bin/trace.sh

or the "Reading Log: trace an open" entry in KUAL.
EOF
}

# `desktop` builds $BUNDLE from desktop/ and puts it at $INSTALLED, any app
# standing there removed first.
desktop() {
    if ! cargo tauri --version >/dev/null 2>&1; then
        echo "error: the Tauri CLI is not installed" >&2
        echo "       fix: cargo install tauri-cli --version '^2' --locked" >&2
        exit 1
    fi

    echo "==> building Reading Log $VERSION for this Mac"
    (cd "$ROOT/desktop" && cargo tauri build)
    if [ ! -d "$BUNDLE" ]; then
        echo "error: no app at $BUNDLE" >&2
        exit 1
    fi

    rm -rf "$INSTALLED"
    ditto "$BUNDLE" "$INSTALLED"
    echo "==> installed Reading Log $VERSION -> $INSTALLED"
}

case "${1:-}" in
    "") native ;;
    desktop) desktop ;;
    all) native; desktop ;;
    *)
        echo "usage: ./build.sh [desktop|all]" >&2
        exit 2
        ;;
esac
