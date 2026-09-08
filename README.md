# Reading Log

Keep track of your reading, for jailbroken kindles. 

## Build

```sh
git clone https://github.com/huangziwei/readinglog && cd readinglog/
./build.sh
```

## Install

Download and unzip the latest `readinglog-v<x.y.z>-kindle.zip` from the [release page](https://github.com/huangziwei/readinglog/releases), then copy two things onto the device:

| from | to | notes |
|:--|:--|:-- |
| `extensions/readinglog/` | `/mnt/us/extensions/readinglog/` | it has to be here |
| `documents/ReadingLog.sh` | `/mnt/us/documents/ReadingLog.sh` | or any subfolders within `documents`  |

## How It Works

On your Kindle, live log is kept in `/var/log/messages`. Every 15 mins, the live log will be rotated out and be kept in `/var/local/log`, and then a daily backup will be generated the first time you turn on the device in a given day, and be saved to `/mnt/us/system/logbackup`. All your reading statistics are buried in those logs. 

Book identity is redacted in most of the logs, but can be recovered by looking up some shared stats in `/var/local/metadata/cc.db`, given the books are still on the device. Books already removed from the device before the first use of this app can only be rescued to a certain extend via correlating the logs with some indirect sources, such as `/mnt/us/system/vocabulary/vocab.db` (if you ever look up a word in that book), `/mnt/us/documents/My Clippings.txt` (if you ever highlighted a sentence in that book) and the `.sdr` of each book (if you didn't remove them from the device after removing the book).

Backlogs can only go back 30 days of use, and they might not even be complete due to file size limit, and removed sideloaded books will not have covers. Don't expect too much from the backlogs, you might be happier if you just reset the history and start tracking from today. 

## Screenshots

<p align="center">
    <img src=".github/assets/today.png" width="250" alt="Today" />
    <img src=".github/assets/book.png" width="250" alt="Book" />
    <img src=".github/assets/config.png" width="250" alt="Config" />
</p>

<p align="center">    
    <img src=".github/assets/books.png" width="250" alt="books" />
    <img src=".github/assets/rhythm-all-stats.png" width="250" alt="stats" />
    <img src=".github/assets/rhythm-all-trends.png" width="250" alt="trends" />
</p>

<p align="center">
    <img src=".github/assets/rhythm-year.png" width="250" alt="year" />
    <img src=".github/assets/rhythm-month.png" width="250" alt="month" />
    <img src=".github/assets/rhythm-week.png" width="250" alt="week" />
</p>
