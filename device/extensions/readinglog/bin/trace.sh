#!/bin/sh
# bin/trace.sh — turns bin/readinglog.sh's open-path tracing on and off.
#
# Tracing writes appmgr_state's seven properties, a per-second activeApp watch
# for WATCH_SECS, and TRACE_LINES of /var/log/messages every time a book is
# opened. That is the loudest thing the extension writes, and it is worth its
# bytes only while an open that failed is being reproduced.
#
# The switch is the presence of $TRACE. This script flips it and says which way.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log
TRACE=$EXT/trace

mkdir -p "$(dirname "$LOG")"

if [ -e "$TRACE" ]; then
    rm -f "$TRACE"
    echo "[$(date)] trace off" >> "$LOG"
    echo "Open-path tracing is off."
else
    : > "$TRACE"
    echo "[$(date)] trace on" >> "$LOG"
    echo "Open-path tracing is on. Reproduce the open, then run this again."
fi
