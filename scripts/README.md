# Diagnostics

A diagnostics script is bundled in `/mnt/us/extensions/readinglog/bin/`. Run `sh /mnt/us/extensions/readinglog/bin/dump.sh` in Kterm, it will create a `dumplogs.zip` file in `/mnt/us/`.

Or, move the `dumplogs.sh` file to `/mnt/us/documents/` (if your jailbreak support Scriptlet), you can tap it in the library or home (it will appear in the library as a coverless "ReadingLog Diagnostics").

After it finished, send me the `dumplogs.zip` either via issue or email.

The zip holds `report.txt` — a summary, including the clock the device and its
log stand on — plus the reading-timer lines of the system log, the catalog rows
the app reads, the session store, the app's own log, and `localtime.tzif`, the
small zone file the device keeps at `/etc/localtime`.
