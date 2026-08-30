#!/bin/sh
# ./build.sh — cross-compiles readinglog-native for $TARGET and stages
# device/extensions/readinglog/ and device/documents/ReadingLog.sh for a copy
# to /mnt/us.
set -eu

TARGET="armv7-unknown-linux-musleabihf"
ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="$ROOT/device/extensions/readinglog/bin/readinglog"

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "error: rustup target '$TARGET' is not installed" >&2
    echo "       fix: rustup target add $TARGET" >&2
    exit 1
fi

# $VERSION is [workspace.package].version in Cargo.toml.
VERSION="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0}
                f && /^version *=/{gsub(/[" ]/,""); sub(/^version=/,""); print; exit}' \
    "$ROOT/Cargo.toml")"
[ -n "$VERSION" ] || { echo "error: no version in [workspace.package]" >&2; exit 1; }

# $CONFIG carries $VERSION in its <version> element.
CONFIG="$ROOT/device/extensions/readinglog/config.xml"
sed "s|<version>[^<]*</version>|<version>$VERSION</version>|" "$CONFIG" > "$CONFIG.new"
mv "$CONFIG.new" "$CONFIG"

echo "==> building readinglog $VERSION for $TARGET"
cargo build --release --target "$TARGET" -p readinglog-native --bin readinglog-native

# $OUT drops the `-native` suffix `cargo build` gives the binary.
mkdir -p "$(dirname "$OUT")"
cp "$ROOT/target/$TARGET/release/readinglog-native" "$OUT"
chmod +x "$OUT" 2>/dev/null || true

echo "==> staged $(ls -lh "$OUT" | awk '{print $5}') -> device/extensions/readinglog/bin/readinglog"
file "$OUT" 2>/dev/null || true

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

==> install — copy these two onto the device

    device/extensions/readinglog/    ->  /mnt/us/extensions/readinglog/
    device/documents/ReadingLog.sh   ->  /mnt/us/documents/ReadingLog.sh

Launch from the library tile or from KUAL; both run bin/readinglog.sh.
The session store lives in /mnt/us/extensions/readinglog/sessions.tsv.
Logs, if anything goes wrong, in /mnt/us/logs/readinglog.log.
EOF
