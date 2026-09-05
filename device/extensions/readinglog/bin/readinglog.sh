#!/bin/sh
# bin/readinglog.sh — runs $EXT/bin/readinglog for documents/ReadingLog.sh and
# for the menu.json entry.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log
# Where the app leaves a book to open, as one file:// URI.
OPEN=$EXT/open

# Relaunching over a running instance can leave the framework stopped and the
# screen frozen.
if pidof readinglog >/dev/null 2>&1; then
    exit 0
fi

# READINGLOG_ORIGIN_VIEW is set by documents/ReadingLog.sh, unset from
# menu.json: a KUAL menu is not a view to hand back to.
restore_view() {
    case "${READINGLOG_ORIGIN_VIEW:-}" in
        KPP_*|LEGACY_*)
            lipc-set-prop com.lab126.appmgrd startView \
                "$READINGLOG_ORIGIN_VIEW:0:app://com.lab126.KPPMainApp?view=$READINGLOG_ORIGIN_VIEW" \
                2>/dev/null
            ;;
    esac
}

# Hand the book to the reader. `appmgrd` takes the mimetype from the URI's
# extension and starts the booklet registered for it.
open_book() {
    uri=$(head -n 1 "$OPEN")
    rm -f "$OPEN"
    [ -n "$uri" ] || return 1
    echo "[$(date)] open $uri" >> "$LOG"
    lipc-set-prop com.lab126.appmgrd start "$uri" 2>> "$LOG"
}

# A book asked for takes the screen. With none, or with one that would not
# open, the view this launch came from is handed back.
on_exit() {
    if [ -s "$OPEN" ] && open_book; then
        return
    fi
    restore_view
}
trap on_exit EXIT

# Only this run's own request is acted on.
rm -f "$OPEN" "$OPEN.partial"

# `dirname $LOG` for the `>>` redirects below.
mkdir -p "$(dirname "$LOG")"

echo "[$(date)] launch $(uname -m)" >> "$LOG"
"$EXT/bin/readinglog" 2>> "$LOG"
# `$(date)` below overwrites `$?` in some shells.
STATUS=$?
echo "[$(date)] exit=$STATUS" >> "$LOG"
