#!/bin/sh
# Name: ReadingLog Diagnostics
# Author: _hzw

# scripts/dumplogs.sh — writes $OUT: the $WORK/markers lines of every log in
# $SOURCES, the $COLUMNS rows of $CATALOG, the tables of $FREETIME_DB, $STORE,
# $APP_LOG and the zone file $TZ_PATHS names.

# LC_ALL=C orders markers.log by byte and takes lines that are not UTF-8.
LC_ALL=C
export LC_ALL

OUT=/mnt/us/dumplogs.zip
# $WORK holds the entries and the deflate streams.
WORK=/mnt/us/dumplogs.part
# $CAP bounds $OUT. halve gives up bytes, at most $TRIMS times, oldest day
# first, and report.txt says what went. Nothing bounds what $SOURCES reads.
CAP=5242880
TRIMS=8
# $HOLD_SECS bounds hold.
HOLD_SECS=6
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

# $TZ_PATHS lists the zone file: the symlink every firmware keeps, then the
# two layouts it resolves through.
TZ_PATHS="/etc/localtime /var/local/system/tz /var/base-local/metadata/system/tz"
TZ_VARS="/var/local/system/tzVar /var/base-local/metadata/system/tzVar"

# $POWERD_DIR holds one kdb record a file, each reading `RG002`, a length,
# `<DATA>`, then the value. $POWERD_KEYS are the seconds of idle before the
# screensaver takes the screen, then the seconds before the device suspends.
POWERD_DIR=/etc/kdb.src/platform/system/daemon/powerd
POWERD_KEYS="t1_timeout t2_timeout"

# $FREETIME_DB holds per-day and per-book `timeread` in seconds, keyed by
# `accessdate` and by `asin`.
FREETIME_DB=/mnt/us/system/freetime/freetime.db

# $CATALOG_PATHS lists the catalog paths, newest firmware first.
CATALOG_PATHS="/var/base-local/metadata/cc.db /var/local/metadata/cc.db /var/local/cc.db"

# $SKIP_TYPES lists the p_cdeType values that name something other than
# reading. It holds whatever `catalog::SKIP_TYPES` holds.
SKIP_TYPES="'AUDI'"

# $COLUMNS and $FROM select the Entries rows sqlite3 writes to catalog.tsv.
COLUMNS="coalesce(p_contentSize, 0), p_cdeKey, p_cdeType,
    coalesce(p_titles_0_nominal, ''), coalesce(p_credits_0_name_collation, ''),
    coalesce(p_percentFinished, -1), coalesce(p_thumbnail, ''),
    coalesce(p_lastAccess, 0), coalesce(p_languages_0, ''),
    replace(coalesce(j_credits, ''), char(10), ' '),
    coalesce(p_location, ''), coalesce(p_readState, -1)"
FROM="from Entries
    where p_cdeKey is not null and p_cdeKey <> ''
      and (p_cdeType is null or p_cdeType not in ($SKIP_TYPES))"

# $CENSUS counts every Entries row by p_cdeType, whatever $FROM does with it,
# and with it the rows carrying no key and the rows naming a file.
CENSUS="select coalesce(p_cdeType, '(null)'), count(*),
    sum(p_cdeKey is null or p_cdeKey = ''),
    sum(p_location is not null and p_location <> '')
  from Entries group by 1 order by 2 desc"

# lines counts the lines of $1 and bytes its size, each 0 where $1 is missing.
lines() {
    [ -f "$1" ] || { echo 0; return; }
    echo $(($(wc -l < "$1")))
}

bytes() {
    [ -f "$1" ] || { echo 0; return; }
    echo $(($(wc -c < "$1")))
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

# versions answers the version span $1's `=== ` header blocks cover, and when
# the first was stamped. Nothing where $1 carries no block.
versions() {
    [ -f "$1" ] || return
    first=$(grep -a '^=== ' "$1" 2>/dev/null | head -n 1 | awk '{print $2}')
    last=$(grep -a '^=== ' "$1" 2>/dev/null | tail -n 1 | awk '{print $2}')
    at=$(grep -a '^=== ' "$1" 2>/dev/null | head -n 1 | awk '{print $3}')
    blocks=$(grep -ac '^=== ' "$1" 2>/dev/null)
    [ -n "$first" ] || return
    if [ "$first" = "$last" ]; then
        echo "version $first over $blocks blocks, first stamped $at"
    else
        echo "versions $first -> $last over $blocks blocks, first stamped $at"
    fi
}

# failures answers how many lines of $1 open with `!!` or `??`, and the most
# recent of them.
failures() {
    [ -f "$1" ] || return
    n=$(grep -ac '^\(!!\|??\)' "$1" 2>/dev/null)
    [ "${n:-0}" -gt 0 ] || { echo "no error lines"; return; }
    last=$(grep -a '^\(!!\|??\)' "$1" 2>/dev/null | tail -n 1 | cut -c1-72)
    echo "$n error lines, last: $last"
}

# spans answers the first and last bracketed date in $1.
spans() {
    [ -f "$1" ] || return
    first=$(grep -a '^\[' "$1" 2>/dev/null | head -n 1 | sed 's/^\[\([^]]*\)\].*/\1/')
    last=$(grep -a '^\[' "$1" 2>/dev/null | tail -n 1 | sed 's/^\[\([^]]*\)\].*/\1/')
    [ -n "$first" ] || return
    echo "spans $first -> $last"
}

# target answers what the symlink $1 points at, and nothing where $1 is not
# one.
target() {
    ls -l "$1" 2>/dev/null | sed -n 's/.* -> //p'
}

# footer answers whether the zone file $1 ends in an empty POSIX-TZ footer.
footer() {
    [ -f "$1" ] || { echo "no zone file"; return; }
    left=$(tail -c 2 "$1" 2>/dev/null | tr -d '\n' | wc -c)
    case $((left)) in
    0) echo "empty" ;;
    *) echo "present" ;;
    esac
}

# kdb answers the value of the kdb record $1, which is whatever follows its
# `<DATA>` line, and nothing where the file does not open.
kdb() {
    [ -r "$1" ] || return
    sed -n '/<DATA>/,$p' "$1" 2>/dev/null | tail -n +2 | tr -d '\n'
}

# timeouts answers the $POWERD_KEYS records under $POWERD_DIR, one a line, and
# says so where the directory is not there.
timeouts() {
    [ -d "$POWERD_DIR" ] || { echo "no $POWERD_DIR"; return; }
    for key in $POWERD_KEYS; do
        printf '%s %s\n' "$key" "$(kdb "$POWERD_DIR/$key")"
    done
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

# keep_markers writes the $WORK/markers lines of $1 to standard output, through
# `gzip -dc` where gzipped answers yes.
keep_markers() {
    if gzipped "$1"; then
        gzip -dc "$1" 2>/dev/null | grep -aF -f "$WORK/markers"
    else
        grep -aF -f "$WORK/markers" -- "$1" 2>/dev/null
    fi
}

# held answers how many lines of $1 carry $2, and 0 where none do.
held() {
    n=$(grep -acF -- "$2" "$1" 2>/dev/null)
    echo $((n))
}

# named lists the files in $1 whose name starts with $2, oldest first.
named() {
    find "$1" -maxdepth 1 -type f -name "$2*" 2>/dev/null | sort
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
for chunk in $(named "$LOG_DIR" "messages_"); do
    SOURCES="$SOURCES $chunk"
    CHUNKS=$((CHUNKS + 1))
done
for dump in $(named "$DUMP_DIR" "log_backup_"); do
    SOURCES="$SOURCES $dump"
    DUMPS=$((DUMPS + 1))
done
TOTAL=$((LIVE + CHUNKS + DUMPS))

say "ReadingLog diagnostics"
say "$TOTAL logs to read. Leave this screen up until it says done."
say ""

# $WORK/sizes holds one `bytes<tab>lines<tab>timer<tab>path` row per source;
# $BYTES_READ sums the bytes. markers.log is sorted and de-duplicated across
# every source, and names none of them; $WORK/sizes counts each one apart.
: > "$WORK/raw"
: > "$WORK/sizes"
BYTES_READ=0
read_so_far=0
for source in $SOURCES; do
    read_so_far=$((read_so_far + 1))
    say "log $read_so_far of $TOTAL"
    size=$(bytes "$source")
    BYTES_READ=$((BYTES_READ + size))
    keep_markers "$source" > "$WORK/one"
    printf '%s\t%s\t%s\t%s\n' \
        "$size" "$(lines "$WORK/one")" \
        "$(held "$WORK/one" ReadingTimerController)" "$source" >> "$WORK/sizes"
    cat "$WORK/one" >> "$WORK/raw"
done
rm -f "$WORK/one"

# $WORK/raw lines open with `YYMMDD:HHMMSS`. `sort -u` keys on the whole line,
# and lines sharing a second come out alphabetically.
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
    sqlite3 -separator "	" "$CATALOG" "$CENSUS" \
        > "$WORK/e/catalog-types.tsv" 2>/dev/null
fi

say "reading the scorecard"

# freetime-schema.sql is every `create table` $FREETIME_DB holds and
# freetime-<table>.tsv the rows of each. sqlite_master names both the tables
# and the column order the TSVs carry.
FREETIME_TABLES=
if [ -r "$FREETIME_DB" ] && command -v sqlite3 >/dev/null 2>&1; then
    sqlite3 "$FREETIME_DB" \
        "select sql || ';' from sqlite_master
         where type = 'table' and sql is not null order by name" \
        > "$WORK/e/freetime-schema.sql" 2>/dev/null
    FREETIME_TABLES=$(sqlite3 "$FREETIME_DB" \
        "select name from sqlite_master
         where type = 'table' and name not like 'sqlite_%' order by name" \
        2>/dev/null)
    for table in $FREETIME_TABLES; do
        sqlite3 -separator "	" "$FREETIME_DB" "select * from '$table'" \
            > "$WORK/e/freetime-$table.tsv" 2>/dev/null
    done
fi

[ -f "$STORE" ] && cp "$STORE" "$WORK/e/sessions.tsv"
[ -f "$APP_LOG" ] && cp "$APP_LOG" "$WORK/e/readinglog.log"

say "reading the clock"

# $TZ_FILE is the first of $TZ_PATHS that opens, $TZ_VAR the KINDLE_TZ line
# beside it. $TZ_FILE is copied into $WORK/e as localtime.tzif.
TZ_FILE=
for path in $TZ_PATHS; do
    [ -f "$path" ] || continue
    TZ_FILE=$path
    break
done
TZ_VAR=
for path in $TZ_VARS; do
    [ -r "$path" ] || continue
    TZ_VAR=$(cat "$path" 2>/dev/null)
    break
done
[ -n "$TZ_FILE" ] && cp "$TZ_FILE" "$WORK/e/localtime.tzif"

# $LOG_NOW is the `YYMMDD:HHMMSS` of the newest line in $LIVE_LOG, and
# $LOG_FROM the file it came from. report.txt prints it beside `date`.
LOG_FROM=$LIVE_LOG
LOG_NOW=$(tail -n 1 "$LIVE_LOG" 2>/dev/null | cut -c 1-13)
case "$LOG_NOW" in
[0-9][0-9][0-9][0-9][0-9][0-9]:[0-9][0-9][0-9][0-9][0-9][0-9]) ;;
*)
    LOG_FROM=$WORK/e/markers.log
    LOG_NOW=$(tail -n 1 "$LOG_FROM" 2>/dev/null | cut -c 1-13)
    ;;
esac

# report.txt names /etc/version.txt, meminfo, $CONFIG's version, the clock the
# device and its log stand on, the $SOURCES read with their sizes, and the
# lines each entry beside it holds.
{
    echo "readinglog diagnostics"
    echo "written        $(date)"
    echo "device         $(cat /etc/prettyversion.txt 2>/dev/null)"
    echo "               $(sed -n 1p /etc/version.txt 2>/dev/null)"
    echo "               $(cat /proc/device-tree/model 2>/dev/null) $(uname -m)"
    echo "memory         $(meminfo)"
    echo "app            $(sed -n 's|.*<version>\(.*\)</version>.*|\1|p' "$CONFIG" 2>/dev/null)"
    echo
    echo "clock          local    $(date)"
    echo "               utc      $(date -u)"
    echo "               offset   $(date '+%z') at epoch $(date '+%s'), per libc"
    echo "               syslog   ${LOG_NOW:-no stamp}, off ${LOG_FROM##*/}"
    echo "               date     $(date '+%y%m%d:%H%M%S'), the same shape as the line above"
    echo "                        — minutes apart is an idle log, an hour apart is the fault"
    echo "               zone     ${TZ_FILE:-none of $TZ_PATHS}, $(bytes "$TZ_FILE") bytes"
    localtime=$(target /etc/localtime)
    echo "               /etc/localtime -> ${localtime:-not a symlink}"
    echo "               ${TZ_VAR:-no tzVar}, POSIX footer $(footer "$TZ_FILE")"
    echo
    echo "markers.log    $MARKER_LINES lines, off $LIVE live, $CHUNKS chunks, $DUMPS dumps"
    echo "catalog.tsv    $(lines "$WORK/e/catalog.tsv") rows from ${CATALOG:-nowhere}"
    echo "catalog-types  type, rows, rows with no key, rows naming a file:"
    sed 's/^/    /' "$WORK/e/catalog-types.tsv" 2>/dev/null
    echo "sessions.tsv   $(lines "$WORK/e/sessions.tsv") lines"
    echo "readinglog.log $(lines "$WORK/e/readinglog.log") lines, $(exits "$WORK/e/readinglog.log")"
    for said in "$(versions "$WORK/e/readinglog.log")" \
                "$(failures "$WORK/e/readinglog.log")" \
                "$(spans "$WORK/e/readinglog.log")"; do
        [ -n "$said" ] && echo "               $said"
    done
    echo
    echo "powerd         the idle timeouts every sleep in the log is measured"
    echo "               against, in seconds, out of $POWERD_DIR;"
    echo "               a page interval past t1_timeout is idle, not reading:"
    timeouts | sed 's/^/    /'
    echo
    echo "freetime.db    the reading-data aggregator's own store, a pipeline"
    echo "               separate from the reading timer: its band is 0-500 and"
    echo "               it credits a clipped real elapsed time where the timer"
    echo "               credits an out-of-band page nothing. $FREETIME_DB"
    if [ -n "$FREETIME_TABLES" ]; then
        echo "               table, rows:"
        for table in $FREETIME_TABLES; do
            printf '    %s\t%s\n' "$table" "$(lines "$WORK/e/freetime-$table.tsv")"
        done
        echo "               freetime-schema.sql names the columns, in order."
    elif [ -r "$FREETIME_DB" ]; then
        echo "               present, but sqlite3 did not read it"
    else
        echo "               not on this device"
    fi
    echo
    echo "sources        $TOTAL read, $BYTES_READ bytes, every log the device holds"
    echo "               the $LARGEST largest, in bytes:"
    sort -rn "$WORK/sizes" | head -n "$LARGEST" | cut -f1,4 | sed 's/^/    /'
    echo
    echo "               every source, live first then chunks and dumps oldest"
    echo "               first — bytes, marker lines, timer lines, path. This is"
    echo "               the only record of which file a line came from:"
    echo "               markers.log is sorted and de-duplicated across all $TOTAL."
    sed 's/^/    /' "$WORK/sizes"
    echo
    echo "localtime.tzif is the zone file above, byte for byte."
    echo
    echo "markers.log lines sharing a second are alphabetical, not as written:"
    echo "    CloseBook lands ahead of PreviousPage. Anything reading two lines"
    echo "    in order must group by second first."
    echo
    echo "markers.log holds the log lines carrying one of these and no others,"
    echo "with the lines of markers.log each one accounts for. A marker reading 0"
    echo "is one this firmware never writes, not one this reader never triggered:"
    while IFS= read -r marker; do
        [ -n "$marker" ] || continue
        printf '    %7s  %s\n' "$(held "$WORK/e/markers.log" "$marker")" "$marker"
    done < "$WORK/markers"
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

# trim_markers gives up markers.log's oldest `YYMMDD` day. A file holding one
# day gives up its oldest half.
trim_markers() {
    f=$WORK/e/markers.log
    [ -s "$f" ] || return 1
    rm -f "$WORK/trimmed"
    oldest=$(head -n 1 "$f" | cut -c1-6)
    if [ -n "$oldest" ]; then
        grep -av "^$oldest" "$f" > "$WORK/trimmed" 2>/dev/null
    fi
    if [ ! -s "$WORK/trimmed" ]; then
        keep=$(($(lines "$f") / 2))
        [ "$keep" -ge 1 ] || return 1
        tail -n "$keep" "$f" > "$WORK/trimmed"
    fi
    mv "$WORK/trimmed" "$f"
    MARKER_TRIMS=$((MARKER_TRIMS + 1))
}

# trim_app drops readinglog.log's oldest `=== ` block whole, keeping every
# failure line above the cut and never cutting below the newest two. A log
# carrying no block gives up its oldest half.
trim_app() {
    f=$WORK/e/readinglog.log
    [ -s "$f" ] || return 1
    rm -f "$WORK/trimmed"
    blocks=$(grep -ac '^=== ' "$f" 2>/dev/null)
    if [ "${blocks:-0}" -ge 3 ]; then
        awk '/^=== /{seen++} seen >= 2 || /^!!/ || /^\?\?/' "$f" > "$WORK/trimmed"
        BLOCK_TRIMS=$((BLOCK_TRIMS + 1))
    else
        keep=$(($(lines "$f") / 2))
        [ "$keep" -ge 1 ] || return 1
        tail -n "$keep" "$f" > "$WORK/trimmed"
        APP_TRIMS=$((APP_TRIMS + 1))
    fi
    [ -s "$WORK/trimmed" ] || return 1
    mv "$WORK/trimmed" "$f"
}

# halve takes bytes off whichever of the two is larger.
halve() {
    if [ "$(bytes "$WORK/e/markers.log")" -gt "$(bytes "$WORK/e/readinglog.log")" ]; then
        trim_markers
    else
        trim_app
    fi
}

MARKER_TRIMS=0
BLOCK_TRIMS=0
APP_TRIMS=0
left=$TRIMS
while [ "$(bytes "$WORK/zip")" -gt "$CAP" ] && [ "$left" -gt 0 ]; do
    halve || break
    say "trimming: markers $MARKER_TRIMS, log $((BLOCK_TRIMS + APP_TRIMS))"
    pack || break
    left=$((left - 1))
done

# $WORK/e/report.txt is written above this point; halve appends what it gave up.
if [ $((MARKER_TRIMS + BLOCK_TRIMS + APP_TRIMS)) -gt 0 ]; then
    {
        echo
        echo "trimmed        to hold $OUT under $CAP bytes:"
        [ "$MARKER_TRIMS" -gt 0 ] && echo "    markers.log gave up $MARKER_TRIMS oldest days"
        [ "$BLOCK_TRIMS" -gt 0 ] && echo "    readinglog.log gave up $BLOCK_TRIMS header blocks, failures kept"
        [ "$APP_TRIMS" -gt 0 ] && echo "    readinglog.log gave up its oldest half $APP_TRIMS times (no header blocks in it)"
    } >> "$WORK/e/report.txt"
    pack || true
fi

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
