#!/bin/sh
# bin/readinglog.sh — runs $EXT/bin/readinglog for documents/ReadingLog.sh and
# for the menu.json entry.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log

# Relaunching over a running instance can leave the framework stopped and the
# screen frozen.
if pidof readinglog >/dev/null 2>&1; then
    exit 0
fi

# READINGLOG_ORIGIN_VIEW is set by documents/ReadingLog.sh, unset from
# menu.json: a KUAL menu is not a view to hand back to.
restore_view_on_exit() {
    case "${READINGLOG_ORIGIN_VIEW:-}" in
        KPP_*|LEGACY_*)
            lipc-set-prop com.lab126.appmgrd startView \
                "$READINGLOG_ORIGIN_VIEW:0:app://com.lab126.KPPMainApp?view=$READINGLOG_ORIGIN_VIEW" \
                2>/dev/null
            ;;
    esac
}
trap restore_view_on_exit EXIT

# `dirname $LOG` for the `>>` redirects below.
mkdir -p "$(dirname "$LOG")"

echo "[$(date)] launch $(uname -m)" >> "$LOG"
"$EXT/bin/readinglog" 2>> "$LOG"
# `$(date)` below overwrites `$?` in some shells.
STATUS=$?
echo "[$(date)] exit=$STATUS" >> "$LOG"
