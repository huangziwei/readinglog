#!/bin/sh
# device/make-tile.sh — writes the `# Icon:` line of documents/ReadingLog.sh:
# assets/cover.svg -> render -> shrink -> assets/cover.png -> base64.
set -eu

cd "$(dirname "$0")"

SVG="assets/cover.svg"
PNG="assets/cover.png"
TILE="documents/ReadingLog.sh"

# $WIDTH x $HEIGHT is the Kindle library tile, the size of $PNG.
WIDTH=1440
HEIGHT=2200

[ -f "$TILE" ] || { echo "error: $TILE not found (run me from the repo)" >&2; exit 1; }
grep -q '^# Icon: ' "$TILE" || {
    echo "error: $TILE has no '# Icon:' line to replace" >&2
    exit 1
}

RENDERER=""
command -v rsvg-convert >/dev/null 2>&1 && RENDERER="rsvg-convert"
[ -n "$RENDERER" ] || { command -v resvg >/dev/null 2>&1 && RENDERER="resvg"; }

if [ -f "$SVG" ] && [ -n "$RENDERER" ]; then
    echo "==> Rendering $SVG -> $PNG (${WIDTH}x${HEIGHT}) via $RENDERER"
    # rsvg-convert names $PNG with -o; resvg takes it as a positional.
    if [ "$RENDERER" = "rsvg-convert" ]; then
        rsvg-convert -w "$WIDTH" -h "$HEIGHT" -o "$PNG" "$SVG"
    else
        resvg -w "$WIDTH" -h "$HEIGHT" "$SVG" "$PNG"
    fi
    # pngquant shrinks $PNG to a lossy 256-colour palette, oxipng losslessly.
    if command -v pngquant >/dev/null 2>&1; then
        pngquant --force --skip-if-larger --output "$PNG" -- "$PNG"
    elif command -v oxipng >/dev/null 2>&1; then
        oxipng -q -o max --strip safe "$PNG"
    else
        echo "    note: no pngquant or oxipng — cover stays truecolor and the"
        echo "          tile will be several times its usual size"
        echo "          (brew install pngquant, or cargo install oxipng)"
    fi
else
    echo "==> Skipping the SVG render; reusing the committed $PNG"
    [ -n "$RENDERER" ] ||
        echo "    (no renderer: brew install librsvg, or cargo install resvg)"
fi

[ -f "$PNG" ] || { echo "error: no $PNG to embed" >&2; exit 1; }

# `base64` wraps at 76 columns on GNU, emits one line on BSD. $ICON is one line.
ICON="$(base64 < "$PNG" | tr -d '\n')"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
# awk holds the ~21KB $ICON line; sed's line buffer chokes on it.
awk -v icon="$ICON" '
    /^# Icon: / { print "# Icon: data:image/png;base64," icon; next }
    { print }
' "$TILE" > "$TMP"
cat "$TMP" > "$TILE"

echo "==> Embedded $(wc -c < "$PNG" | tr -d ' ')-byte cover; $TILE is now $(wc -c < "$TILE" | tr -d ' ') bytes"
echo "    Copy it to the Kindle's documents/ folder to see it."
