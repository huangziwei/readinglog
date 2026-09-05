#!/bin/sh
# bin/readinglog.sh — runs $EXT/bin/readinglog.

EXT=/mnt/us/extensions/readinglog
LOG=/mnt/us/logs/readinglog.log
# $OPEN holds one URI.
OPEN=$EXT/open

READER_APP=com.lab126.booklet.reader
# WAIT_SECS bounds wait_reader.
WAIT_SECS=10
# WATCH_SECS bounds follow_open. 0 skips it.
WATCH_SECS=8
# REFILE_SECS bounds follow_open's call to file_view.
REFILE_SECS=3
# TRACE_LINES of /var/log/messages reach $LOG.
TRACE_LINES=120

if pidof readinglog >/dev/null 2>&1; then
    exit 0
fi

# appmgr_state writes each $prop to $LOG under $1.
appmgr_state() {
    echo "[$(date)] $1" >> "$LOG"
    for prop in activeApp activeView activeAppPid peekHistoryView \
                peekAppHistory peekAppHistoryURI backButtonState; do
        echo "    $prop=$(lipc-get-prop com.lab126.appmgrd $prop 2>&1)" >> "$LOG"
    done
}

# file_view files $1 as appmgrd's startView READER:1.
file_view() {
    lipc-set-prop com.lab126.appmgrd startView "READER:1:$1" 2>> "$LOG"
    status=$?
    echo "[$(date)] startView status=$status" >> "$LOG"
    return "$status"
}

# wait_reader returns once activeApp is $READER_APP, within $WAIT_SECS seconds.
wait_reader() {
    waited=0
    while :; do
        active=$(lipc-get-prop com.lab126.appmgrd activeApp 2>/dev/null)
        [ "$active" = "$READER_APP" ] && break
        [ "$waited" -ge "$WAIT_SECS" ] && break
        sleep 1
        waited=$((waited + 1))
    done
    echo "[$(date)] waited ${waited}s, activeApp=$active" >> "$LOG"
    [ "$active" = "$READER_APP" ]
}

# follow_open watches activeApp for $WATCH_SECS seconds, files $1 again inside
# $REFILE_SECS, and appends /var/log/messages to $LOG.
follow_open() {
    [ "$WATCH_SECS" -gt 0 ] || return 0
    (
        appmgr_state "state at exit"
        refiled=no
        elapsed=0
        while [ "$elapsed" -lt "$WATCH_SECS" ]; do
            sleep 1
            elapsed=$((elapsed + 1))
            active=$(lipc-get-prop com.lab126.appmgrd activeApp 2>&1)
            echo "    +${elapsed}s activeApp=$active" >> "$LOG"
            [ "$active" = "$READER_APP" ] && continue
            [ "$refiled" = yes ] && continue
            [ "$elapsed" -gt "$REFILE_SECS" ] && continue
            refiled=yes
            echo "[$(date)] refiling $1" >> "$LOG"
            file_view "$1"
        done
        appmgr_state "state after watch"
        echo "[$(date)] syslog" >> "$LOG"
        grep -aE 'appmgr|shell_integration|booklet\.reader' /var/log/messages \
            2>/dev/null | tail -n "$TRACE_LINES" >> "$LOG"
    ) &
}

# restore_view acts on a KPP_ or LEGACY_ READINGLOG_ORIGIN_VIEW.
restore_view() {
    case "${READINGLOG_ORIGIN_VIEW:-}" in
        KPP_*|LEGACY_*)
            lipc-set-prop com.lab126.appmgrd startView \
                "$READINGLOG_ORIGIN_VIEW:0:app://com.lab126.KPPMainApp?view=$READINGLOG_ORIGIN_VIEW" \
                2>/dev/null
            ;;
    esac
}

# open_book files $OPEN's URI, runs wait_reader, then follow_open.
open_book() {
    uri=$(head -n 1 "$OPEN")
    rm -f "$OPEN"
    [ -n "$uri" ] || return 1
    echo "[$(date)] open $uri" >> "$LOG"
    file_view "$uri"
    status=$?
    wait_reader
    follow_open "$uri"
    return "$status"
}

# on_exit runs open_book, then restore_view where open_book fails.
on_exit() {
    if [ -s "$OPEN" ] && open_book; then
        return
    fi
    restore_view
}
trap on_exit EXIT

rm -f "$OPEN" "$OPEN.partial"

# $LOG's directory.
mkdir -p "$(dirname "$LOG")"

echo "[$(date)] launch $(uname -m)" >> "$LOG"
appmgr_state "state at launch"
"$EXT/bin/readinglog" 2>> "$LOG"
# $(date) overwrites $?.
STATUS=$?
echo "[$(date)] exit=$STATUS" >> "$LOG"
