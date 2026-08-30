#!/bin/sh
# bin/collect.sh — folds the log into the store and draws nothing.
#
# The syslog is a stream: `tinyrot` prunes its rotated chunks and the firmware
# keeps about a month of daily snapshots. Running this from cron keeps the store
# current for a reader who opens the app less often than that.
#
#     */30 * * * * /mnt/us/extensions/readinglog/bin/collect.sh
#
# Launching the app collects too, so this is only needed to cover a long gap.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log

if pidof readinglog >/dev/null 2>&1; then
    exit 0
fi

mkdir -p "$(dirname "$LOG")"
"$EXT/bin/readinglog" --collect 2>> "$LOG"
