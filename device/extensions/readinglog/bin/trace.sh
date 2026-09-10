#!/bin/sh
# Turns bin/readinglog.sh's open-path tracing on and off; the switch is the
# presence of $TRACE. Tracing writes appmgr_state's properties, an activeApp
# watch and TRACE_LINES of /var/log/messages on every open.

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
