#!/bin/sh
# ./preview.sh [shots...] — draws screens into artifacts/preview/*.png;
# --list names every shot, --record DIR draws a real sessions.tsv.
# $READINGLOG_FONTS names the Kindle's fonts; preview.env sets it.
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
[ -f "$ROOT/preview.env" ] && . "$ROOT/preview.env"

if [ -z "${READINGLOG_FONTS:-}" ]; then
    echo "error: READINGLOG_FONTS names no directory" >&2
    echo "       fix: write READINGLOG_FONTS=<the Kindle's fonts> into preview.env" >&2
    exit 1
fi
export READINGLOG_FONTS

cargo build --quiet --manifest-path "$ROOT/native/Cargo.toml" --bin preview

# --out and --covers default under $ROOT unless "$@" names them. The covers are
# one set shared by every run, never a copy under each --out.
out= covers=
for arg in "$@"; do
    [ "$arg" = "--out" ] && out=named
    [ "$arg" = "--covers" ] && covers=named
done
[ -z "$out" ] && set -- "$@" --out "$ROOT/artifacts/preview"
[ -z "$covers" ] && set -- "$@" --covers "$ROOT/artifacts/preview/art"
exec "$ROOT/target/debug/preview" "$@"
