#!/bin/sh
# bin/collect.sh — folds the log into the store and draws nothing. The syslog
# is pruned as it rotates, so a cron entry keeps the store current for a reader
# who opens the app less often than that. Launching the app collects too.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log

if pidof readinglog >/dev/null 2>&1; then
    exit 0
fi

mkdir -p "$(dirname "$LOG")"
"$EXT/bin/readinglog" --collect 2>> "$LOG"
