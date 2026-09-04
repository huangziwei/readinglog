#!/bin/sh
# ./preview.sh — draws screens to artifacts/preview/*.png, with no display and
# no X server. Anything after the script name goes to the preview binary:
#
#   ./preview.sh --list
#   ./preview.sh rhythm:week rhythm:month rhythm:year --sheet spans
#   ./preview.sh --all --panel pw --panel scribe
#
# $READINGLOG_FONTS names the device's font directory. preview.env sets it
# where the environment does not.
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
[ -f "$ROOT/preview.env" ] && . "$ROOT/preview.env"

if [ -z "${READINGLOG_FONTS:-}" ]; then
    echo "error: READINGLOG_FONTS names no directory" >&2
    echo "       fix: write READINGLOG_FONTS=<the device's fonts> into preview.env" >&2
    exit 1
fi
export READINGLOG_FONTS

cargo build --quiet --manifest-path "$ROOT/native/Cargo.toml" --bin preview

# The PNGs land under the repo, whichever directory this was called from.
for arg in "$@"; do
    [ "$arg" = "--out" ] && exec "$ROOT/target/debug/preview" "$@"
done
exec "$ROOT/target/debug/preview" "$@" --out "$ROOT/artifacts/preview"
