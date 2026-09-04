#!/bin/sh
# ./preview.sh [shots...] — draws screens into artifacts/preview/*.png.
# ./preview.sh --list names every shot, panel and sketch.
# $READINGLOG_FONTS names the device's fonts; preview.env sets it.
# --store DIR draws a real sessions.tsv in place of the fixture.
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

# --out and --art default under $ROOT unless "$@" names them. The jackets are
# one set shared by every run, never a copy under each --out.
out= art=
for arg in "$@"; do
    [ "$arg" = "--out" ] && out=named
    [ "$arg" = "--art" ] && art=named
done
[ -z "$out" ] && set -- "$@" --out "$ROOT/artifacts/preview"
[ -z "$art" ] && set -- "$@" --art "$ROOT/artifacts/preview/art"
exec "$ROOT/target/debug/preview" "$@"
