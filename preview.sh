#!/bin/sh
# ./preview.sh [shots...] — draws screens into artifacts/preview/*.png.
# ./preview.sh --list names every shot, panel and sketch.
# $READINGLOG_FONTS names the device's fonts; preview.env sets it.
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
[ -f "$ROOT/preview.env" ] && . "$ROOT/preview.env"

if [ -z "${READINGLOG_FONTS:-}" ]; then
    echo "error: READINGLOG_FONTS names no directory" >&2
    echo "       fix: write READINGLOG_FONTS=<the device's fonts> into preview.env" >&2
    exit 1
fi
export READINGLOG_FONTS

# An empty artifacts/sketch/mod.rs where the tree holds none.
if [ ! -f "$ROOT/artifacts/sketch/mod.rs" ]; then
    mkdir -p "$ROOT/artifacts/sketch"
    printf 'use super::Sketch;\n\npub const DRAFTS: &[Sketch] = &[];\n' \
        > "$ROOT/artifacts/sketch/mod.rs"
fi

cargo build --quiet --manifest-path "$ROOT/native/Cargo.toml" --bin preview

# --out "$ROOT/artifacts/preview" unless "$@" names one.
for arg in "$@"; do
    [ "$arg" = "--out" ] && exec "$ROOT/target/debug/preview" "$@"
done
exec "$ROOT/target/debug/preview" "$@" --out "$ROOT/artifacts/preview"
