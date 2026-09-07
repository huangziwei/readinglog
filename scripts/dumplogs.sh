#!/bin/sh
# Name: ReadingLog Diagnostics
# Author: _hzw

# scripts/dumplogs.sh — writes $OUT: the $WORK/markers lines of every log in
# $SOURCES, the $COLUMNS rows of $CATALOG, $STORE and $APP_LOG.

# LC_ALL=C orders markers.log by byte and takes lines that are not UTF-8.
LC_ALL=C
export LC_ALL

OUT=/mnt/us/dumplogs.zip
# $WORK holds the entries and the deflate streams.
WORK=/mnt/us/dumplogs.part
# $CAP bounds $OUT. $TRIMMED gives up its oldest half, $TRIMS times.
CAP=5242880
TRIMS=8
# $HOLD_SECS bounds hold.
HOLD_SECS=6
# $DAYS bounds recent by mtime.
DAYS=14
# report.txt names the $LARGEST largest sources by size.
LARGEST=5

# $LIVE_LOG, $LOG_DIR and $DUMP_DIR hold the paths $SOURCES draws from.
LIVE_LOG=/var/log/messages
LOG_DIR=/var/local/log
DUMP_DIR=/mnt/us/system/logbackup

EXT=/mnt/us/extensions/readinglog
STORE=$EXT/sessions.tsv
CONFIG=$EXT/config.xml
APP_LOG=/mnt/us/logs/readinglog.log

# $CATALOG_PATHS lists the catalog paths, newest firmware first.
CATALOG_PATHS="/var/base-local/metadata/cc.db /var/local/metadata/cc.db /var/local/cc.db"

# $COLUMNS and $FROM select the Entries rows sqlite3 writes to catalog.tsv.
COLUMNS="coalesce(p_contentSize, 0), p_cdeKey, p_cdeType,
    coalesce(p_titles_0_nominal, ''), coalesce(p_credits_0_name_collation, ''),
    coalesce(p_percentFinished, -1), coalesce(p_thumbnail, ''),
    coalesce(p_lastAccess, 0), coalesce(p_languages_0, ''),
    replace(coalesce(j_credits, ''), char(10), ' '),
    coalesce(p_location, ''), coalesce(p_readState, -1)"
FROM="from Entries
    where p_cdeKey is not null and p_cdeKey <> ''
      and p_cdeType in ('EBOK', 'PDOC', 'MAGZ')"

# lines counts the lines of $1 and bytes its size, each 0 where $1 is missing.
lines() {
    [ -f "$1" ] || { echo 0; return; }
    echo $(($(wc -l < "$1")))
}

bytes() {
    [ -f "$1" ] || { echo 0; return; }
    echo $(($(wc -c < "$1")))
}

# count answers how many paths $1 names, 0 where it names none.
count() {
    [ -n "$1" ] || { echo 0; return; }
    echo "$1" | wc -l
}

# meminfo answers /proc/meminfo's MemTotal and MemFree on one line.
meminfo() {
    grep -E '^(MemTotal|MemFree):' /proc/meminfo 2>/dev/null | tr -s ' \n' ' '
}

# exits answers how many `exit=` lines $1 holds, and the codes among them that
# are not 0.
exits() {
    [ -f "$1" ] || { echo "not on the device"; return; }
    all=$(grep -c 'exit=' "$1" 2>/dev/null)
    bad=$(sed -n 's/.*exit=\([0-9][0-9]*\).*/\1/p' "$1" | grep -v '^0$' | sort -u |
        tr '\n' ' ')
    echo "$all exits${bad:+, non-zero ${bad% }}"
}

# say writes "$*" to standard error, a line at a time.
say() {
    echo "$*" >&2
}

# hold sleeps $HOLD_SECS where standard output is not a terminal.
hold() {
    [ -t 1 ] || sleep "$HOLD_SECS"
}

# leave deletes $WORK.
leave() {
    rm -rf "$WORK"
}
trap leave EXIT

rm -rf "$WORK"
mkdir -p "$WORK/e" || exit 1

# -------------------------------------------------------------------- $WORK/e

# $WORK/markers holds one marker a line, matched whole by `grep -F`.
cat > "$WORK/markers" <<'EOF'
ReadingTimerController
SchemaName[ereader_open_book]
SchemaName[ereader_close_book]
SchemaName[ereader_book_consume_content]
SchemaName[ereader_book_page_turn]
SchemaName[ereader_book_linear_page_actions]
SchemaName[ereader_content_point]
SchemaName[ereader_reader_latency_ops]
SchemaName[ereader_reader_page_turn_latency_ops]
ereader_powerd_state_change
lipc:evts:name=outOfScreenSaver, origin=com.lab126.powerd
lipc:evts:name=goingToScreenSaver, origin=com.lab126.powerd
lipc:evts:name=suspending, origin=com.lab126.powerd
EOF

# $WORK/magic holds the two bytes a gzip file opens with.
printf '\037\213' > "$WORK/magic"

# gzipped answers whether $1 opens with the $WORK/magic bytes.
gzipped() {
    head -c 2 "$1" 2>/dev/null | grep -qaF -f "$WORK/magic"
}

# keep_markers appends the $WORK/markers lines of $1 to $WORK/raw, through
# `gzip -dc` where gzipped answers yes.
keep_markers() {
    if gzipped "$1"; then
        gzip -dc "$1" 2>/dev/null | grep -aF -f "$WORK/markers"
    else
        grep -aF -f "$WORK/markers" -- "$1" 2>/dev/null
    fi >> "$WORK/raw"
}

# named lists the files in $1 whose name starts with $2, oldest first.
named() {
    find "$1" -maxdepth 1 -type f -name "$2*" 2>/dev/null | sort
}

# recent narrows named to the files $DAYS covers by mtime, and answers named
# where $DAYS covers none of them.
recent() {
    found=$(find "$1" -maxdepth 1 -type f -name "$2*" -mtime "-$DAYS" 2>/dev/null |
        sort)
    [ -n "$found" ] || found=$(named "$1" "$2")
    echo "$found"
}

# $SOURCES are the log files to read, live first. $LIVE, $CHUNKS and $DUMPS
# count them by source.
SOURCES=
LIVE=0
CHUNKS=0
DUMPS=0
if [ -r "$LIVE_LOG" ]; then
    SOURCES=$LIVE_LOG
    LIVE=1
elif [ -e "$LIVE_LOG" ]; then
    say "$LIVE_LOG will not open. Run this as root."
fi
for chunk in $(recent "$LOG_DIR" "messages_"); do
    SOURCES="$SOURCES $chunk"
    CHUNKS=$((CHUNKS + 1))
done
for dump in $(recent "$DUMP_DIR" "log_backup_"); do
    SOURCES="$SOURCES $dump"
    DUMPS=$((DUMPS + 1))
done
TOTAL=$((LIVE + CHUNKS + DUMPS))

# $OUTSIDE counts the named files $DAYS leaves out of $SOURCES.
ALL_CHUNKS=$(count "$(named "$LOG_DIR" "messages_")")
ALL_DUMPS=$(count "$(named "$DUMP_DIR" "log_backup_")")
OUTSIDE=$((ALL_CHUNKS + ALL_DUMPS - CHUNKS - DUMPS))

say "ReadingLog diagnostics"
say "$TOTAL logs to read. Leave this screen up until it says done."
say ""

# $WORK/sizes holds one `bytes<tab>path` row per source; $BYTES_READ sums them.
: > "$WORK/raw"
: > "$WORK/sizes"
BYTES_READ=0
read_so_far=0
for source in $SOURCES; do
    read_so_far=$((read_so_far + 1))
    say "log $read_so_far of $TOTAL"
    size=$(bytes "$source")
    BYTES_READ=$((BYTES_READ + size))
    printf '%s\t%s\n' "$size" "$source" >> "$WORK/sizes"
    keep_markers "$source"
done

# $WORK/raw lines open with `YYMMDD:HHMMSS`; `sort -u` orders on it.
say "ordering $(lines "$WORK/raw") lines"
sort -u "$WORK/raw" > "$WORK/e/markers.log"
rm -f "$WORK/raw"
MARKER_LINES=$(lines "$WORK/e/markers.log")

say "reading the catalog"

CATALOG=
for db in $CATALOG_PATHS; do
    [ -r "$db" ] || continue
    CATALOG=$db
    break
done
if [ -n "$CATALOG" ] && command -v sqlite3 >/dev/null 2>&1; then
    # sqlite3 runs the query twice, `sleep 2` apart.
    sqlite3 -separator "	" "$CATALOG" "select $COLUMNS $FROM" \
        > "$WORK/e/catalog.tsv" 2>/dev/null ||
        {
            sleep 2
            sqlite3 -separator "	" "$CATALOG" "select $COLUMNS $FROM" \
                > "$WORK/e/catalog.tsv" 2>/dev/null
        }
fi

[ -f "$STORE" ] && cp "$STORE" "$WORK/e/sessions.tsv"
[ -f "$APP_LOG" ] && cp "$APP_LOG" "$WORK/e/readinglog.log"

# report.txt names /etc/version.txt, meminfo, $CONFIG's version, the $SOURCES
# read with their sizes, and the lines each entry beside it holds.
{
    echo "readinglog diagnostics"
    echo "written        $(date)"
    echo "device         $(cat /etc/prettyversion.txt 2>/dev/null)"
    echo "               $(sed -n 1p /etc/version.txt 2>/dev/null)"
    echo "               $(cat /proc/device-tree/model 2>/dev/null) $(uname -m)"
    echo "memory         $(meminfo)"
    echo "app            $(sed -n 's|.*<version>\(.*\)</version>.*|\1|p' "$CONFIG" 2>/dev/null)"
    echo
    echo "markers.log    $MARKER_LINES lines, off $LIVE live, $CHUNKS chunks, $DUMPS dumps, $DAYS days"
    echo "catalog.tsv    $(lines "$WORK/e/catalog.tsv") rows from ${CATALOG:-nowhere}"
    echo "sessions.tsv   $(lines "$WORK/e/sessions.tsv") lines"
    echo "readinglog.log $(lines "$WORK/e/readinglog.log") lines, $(exits "$WORK/e/readinglog.log")"
    echo
    echo "sources        $TOTAL read, $BYTES_READ bytes, $OUTSIDE more outside $DAYS days"
    echo "               the $LARGEST largest, in bytes:"
    sort -rn "$WORK/sizes" | head -n "$LARGEST" | sed 's/^/    /'
    echo
    echo "markers.log holds the log lines carrying one of these and no others:"
    sed 's/^/    /' "$WORK/markers"
} > "$WORK/e/report.txt"

# ------------------------------------------------------------------ $WORK/zip

# le writes $1 as $2 bytes, least significant first.
le() {
    value=$1
    count=$2
    octal=
    while [ "$count" -gt 0 ]; do
        octal="$octal\\0$(printf '%03o' $((value % 256)))"
        value=$((value / 256))
        count=$((count - 1))
    done
    printf '%b' "$octal"
}

# stamp_zip sets $DOS_TIME and $DOS_DATE from `date`, stripping each field's
# leading `0`.
stamp_zip() {
    set -- $(date '+%Y %m %d %H %M %S')
    year=$1
    [ "$year" -ge 1980 ] || year=1980
    DOS_DATE=$(((year - 1980) * 512 + ${2#0} * 32 + ${3#0}))
    DOS_TIME=$((${4#0} * 2048 + ${5#0} * 32 + ${6#0} / 2))
}

# deflate writes the deflate stream of $1 to $WORK/data and its CRC-32 to
# $WORK/crc, and sets $CSIZE and $USIZE. `gzip -c` frames that stream in a
# ten-byte header and an eight-byte CRC-32 and length.
deflate() {
    gzip -c < "$1" > "$WORK/gz" || return 1
    # `tail -c 1` takes FLG, and `$( )` empties it where FLG is NUL.
    [ -z "$(head -c 4 "$WORK/gz" | tail -c 1)" ] || return 1
    USIZE=$(bytes "$1")
    CSIZE=$(($(bytes "$WORK/gz") - 18))
    [ "$CSIZE" -gt 0 ] || return 1
    head -c $((10 + CSIZE)) "$WORK/gz" | tail -c "$CSIZE" > "$WORK/data"
    tail -c 8 "$WORK/gz" | head -c 4 > "$WORK/crc"
    rm -f "$WORK/gz"
}

# add appends $1 under `dumplogs/` to $WORK/zip and its central directory row
# to $WORK/dir, advancing $OFFSET and $COUNT.
add() {
    name=dumplogs/$(basename "$1")
    deflate "$1" || return 1
    {
        printf 'PK\003\004'
        le 20 2
        le 0 2
        le 8 2
        le "$DOS_TIME" 2
        le "$DOS_DATE" 2
        cat "$WORK/crc"
        le "$CSIZE" 4
        le "$USIZE" 4
        le ${#name} 2
        le 0 2
        printf '%s' "$name"
        cat "$WORK/data"
    } >> "$WORK/zip"
    {
        printf 'PK\001\002'
        le 20 2
        le 20 2
        le 0 2
        le 8 2
        le "$DOS_TIME" 2
        le "$DOS_DATE" 2
        cat "$WORK/crc"
        le "$CSIZE" 4
        le "$USIZE" 4
        le ${#name} 2
        le 0 2
        le 0 2
        le 0 2
        le 0 2
        le 0 4
        le "$OFFSET" 4
        printf '%s' "$name"
    } >> "$WORK/dir"
    OFFSET=$((OFFSET + 30 + ${#name} + CSIZE))
    COUNT=$((COUNT + 1))
    rm -f "$WORK/data" "$WORK/crc"
}

# pack writes every entry under $WORK/e into $WORK/zip.
pack() {
    : > "$WORK/zip"
    : > "$WORK/dir"
    OFFSET=0
    COUNT=0
    for entry in "$WORK"/e/*; do
        [ -s "$entry" ] || continue
        add "$entry" || return 1
    done
    [ "$COUNT" -gt 0 ] || return 1
    cat "$WORK/dir" >> "$WORK/zip"
    {
        printf 'PK\005\006'
        le 0 2
        le 0 2
        le "$COUNT" 2
        le "$COUNT" 2
        le "$(bytes "$WORK/dir")" 4
        le "$OFFSET" 4
        le 0 2
    } >> "$WORK/zip"
}

say "packing $MARKER_LINES marker lines"
stamp_zip
pack || { say "nothing to write"; hold; exit 1; }

# $TRIMMED names the entries halve trims.
TRIMMED="markers.log readinglog.log"

# halve keeps the newest half of each of $TRIMMED, answering how many lines
# stand across them.
halve() {
    standing=0
    for entry in $TRIMMED; do
        [ -s "$WORK/e/$entry" ] || continue
        keep=$(($(lines "$WORK/e/$entry") / 2))
        if [ "$keep" -lt 1 ]; then
            rm -f "$WORK/e/$entry"
            continue
        fi
        tail -n "$keep" "$WORK/e/$entry" > "$WORK/trimmed"
        mv "$WORK/trimmed" "$WORK/e/$entry"
        standing=$((standing + keep))
    done
    echo "$standing"
}

left=$TRIMS
while [ "$(bytes "$WORK/zip")" -gt "$CAP" ] && [ "$left" -gt 0 ]; do
    standing=$(halve)
    [ "$standing" -gt 0 ] || break
    say "trimming to $standing lines"
    pack || break
    left=$((left - 1))
done

# mv writes $OUT.new and renames it over $OUT.
mv "$WORK/zip" "$OUT.new" && mv "$OUT.new" "$OUT" || {
    say "$OUT would not be written"
    hold
    exit 1
}

say ""
say "done. $OUT holds $(bytes "$OUT") bytes."
say "Plug in over USB and send dumplogs.zip from the Kindle's top folder."
hold
