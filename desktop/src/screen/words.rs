//! What the window says that no Kindle screen does, in the Kindle's five
//! languages.

use readinglog_native::lang::Lang;

pub struct Words {
    /// The heading over each week's reading, beside the Kindle's `by_month`.
    pub by_week: &'static str,
    /// `mode_timeline`, `mode_book`: the chips naming the two [`super::Mode`]s.
    pub mode_timeline: &'static str,
    pub mode_book: &'static str,
    pub choose_backup: &'static str,
    pub drop_hint: &'static str,
    pub where_backups: &'static str,
    /// `syncing_in`, `syncing_out`: the line `kindle_card` states as a sync takes the
    /// record and gives the notes; `synced` as of `{t}`, `not_synced` for `{why}`
    /// (`in_use`, `unplugged`); `try_again`, its button.
    pub syncing_in: &'static str,
    pub syncing_out: &'static str,
    pub synced: &'static str,
    pub not_synced: &'static str,
    pub in_use: &'static str,
    pub unplugged: &'static str,
    pub try_again: &'static str,
    pub show_folder: &'static str,
    pub change_folder: &'static str,
    pub added: &'static str,
    pub nothing_new: &'static str,
    /// What a book from a backup naming no Kindle is said to be from.
    pub from_backup: &'static str,
    /// The button on the Kindle's `reset_row` that asks [`Words::reset_ask`].
    pub remove_everything: &'static str,
    /// What Reset asks, `reset_note` under it and `reset_do` its answer.
    /// `{what}` is the sittings and books, `{folder}` the record's folder,
    /// `{size}` what it all takes.
    pub reset_ask: &'static str,
    pub reset_note: &'static str,
    pub reset_do: &'static str,
    pub reset_done: &'static str,
    /// The button on each backup, asking the Kindle's `delete_ask` over
    /// `delete_note`: `{file}` the backup's name, `{size}` its weight.
    pub delete: &'static str,
    pub delete_note: &'static str,
    /// The button under the backups, asking `delete_all_ask` over
    /// `delete_all_note`: `{n}` the backups, `{size}` their weight.
    pub delete_all: &'static str,
    pub delete_all_ask: &'static str,
    pub delete_all_note: &'static str,
    pub delete_all_do: &'static str,
    /// The button on each highlight opening its words, and the editor's `save`
    /// and `revert` beside the Kindle's `cancel`; `revert_tip` is `revert`'s tooltip.
    pub edit: &'static str,
    pub save: &'static str,
    pub revert: &'static str,
    pub revert_tip: &'static str,
    /// The field a note is written in, and the button on a highlight adding one.
    pub write_note: &'static str,
    pub add_note: &'static str,
    /// What deleting a note asks, `delete_note_said` under it.
    pub delete_note_ask: &'static str,
    pub delete_note_said: &'static str,
}

const ENGLISH: Words = Words {
    by_week: "BY WEEK",
    mode_timeline: "Timeline",
    mode_book: "Books",
    choose_backup: "Choose a backup…",
    drop_hint: "Drop a backup on this window, or choose one.",
    where_backups: "On the Kindle, backups are kept in extensions/readinglog/backups.",
    syncing_in: "Bringing in its record…",
    syncing_out: "Putting your notes on it…",
    synced: "Up to date · {t}",
    not_synced: "Not synced: {why}",
    in_use: "another app is holding it",
    unplugged: "it was unplugged",
    try_again: "Try again",
    show_folder: "Show Folder",
    change_folder: "Change…",
    added: "Added",
    nothing_new: "Nothing new in this backup.",
    from_backup: "Backup",
    remove_everything: "Remove Everything…",
    reset_ask: "Remove everything?",
    reset_note: "{what}, every backup and every cover held in {folder} are deleted with \
                 the settings, and no copy is kept. It frees {size}. Your Kindles keep \
                 their own records, and importing them again brings those back; a \
                 backup kept only here cannot come back.",
    reset_do: "Remove Everything",
    reset_done: "Everything is removed.",
    delete: "Delete…",
    delete_note: "{file} goes off this computer for good, freeing {size}. The record \
                  itself is not touched.",
    delete_all: "Delete All Backups…",
    delete_all_ask: "Delete every backup?",
    delete_all_note: "Every backup, {n} files, goes off this computer for good, freeing \
                      {size}. The record itself is not touched.",
    delete_all_do: "Delete All",
    edit: "Edit",
    save: "Save",
    revert: "Revert",
    revert_tip: "Use the words the Kindle states.",
    write_note: "Write a note…",
    add_note: "Add note",
    delete_note_ask: "Delete this note?",
    delete_note_said: "It goes for good, here and on its Kindle the next time that Kindle is plugged in.",
};

const GERMAN: Words = Words {
    by_week: "NACH WOCHE",
    mode_timeline: "Chronik",
    mode_book: "Bücher",
    choose_backup: "Sicherung wählen…",
    drop_hint: "Ziehen Sie eine Sicherung auf dieses Fenster, oder wählen Sie eine.",
    where_backups: "Auf dem Kindle liegen Sicherungen unter extensions/readinglog/backups.",
    syncing_in: "Die Aufzeichnung kommt herein…",
    syncing_out: "Ihre Notizen gehen hinüber…",
    synced: "Aktuell · {t}",
    not_synced: "Nicht abgeglichen: {why}",
    in_use: "eine andere App hält ihn",
    unplugged: "er wurde getrennt",
    try_again: "Erneut versuchen",
    show_folder: "Ordner zeigen",
    change_folder: "Ändern…",
    added: "Hinzugefügt",
    nothing_new: "Nichts Neues in dieser Sicherung.",
    from_backup: "Sicherung",
    remove_everything: "Alles entfernen…",
    reset_ask: "Alles entfernen?",
    reset_note: "{what}, alle Sicherungen und alle Cover in {folder} werden samt den \
                 Einstellungen ohne Kopie gelöscht, das gibt {size} frei. Ihre Kindles \
                 behalten ihre eigenen Aufzeichnungen, ein erneuter Import holt sie \
                 zurück; eine Sicherung, die nur hier liegt, nicht.",
    reset_do: "Alles entfernen",
    reset_done: "Alles entfernt.",
    delete: "Löschen…",
    delete_note: "{file} verschwindet endgültig von diesem Computer und gibt {size} \
                  frei. Die Aufzeichnung selbst bleibt unberührt.",
    delete_all: "Alle Sicherungen löschen…",
    delete_all_ask: "Alle Sicherungen löschen?",
    delete_all_note: "Alle Sicherungen, {n} Dateien, verschwinden endgültig von diesem \
                      Computer und geben {size} frei. Die Aufzeichnung selbst bleibt \
                      unberührt.",
    delete_all_do: "Alle löschen",
    edit: "Bearbeiten",
    save: "Speichern",
    revert: "Rückgängig",
    revert_tip: "Den Wortlaut des Kindle übernehmen.",
    write_note: "Notiz schreiben…",
    add_note: "Notiz hinzufügen",
    delete_note_ask: "Diese Notiz löschen?",
    delete_note_said: "Sie verschwindet endgültig, hier und auf ihrem Kindle, sobald er das nächste Mal angeschlossen ist.",
};

const JAPANESE: Words = Words {
    by_week: "週ごと",
    mode_timeline: "時系列",
    mode_book: "本ごと",
    choose_backup: "バックアップを選ぶ…",
    drop_hint: "バックアップをこのウィンドウにドロップするか、選んでください。",
    where_backups: "Kindle では extensions/readinglog/backups にバックアップがあります。",
    syncing_in: "記録を取り込んでいます…",
    syncing_out: "メモを送っています…",
    synced: "最新 · {t}",
    not_synced: "同期できません：{why}",
    in_use: "ほかのアプリが使っています",
    unplugged: "接続が外れました",
    try_again: "もう一度",
    show_folder: "フォルダを表示",
    change_folder: "変更…",
    added: "追加",
    nothing_new: "このバックアップに新しい記録はありません。",
    from_backup: "バックアップ",
    remove_everything: "すべて削除…",
    reset_ask: "すべて削除しますか？",
    reset_note: "{folder} の{what}、バックアップと表紙をすべて、設定とともに削除します。\
                 控えは残りません。{size}の空きができます。Kindle 自身の記録はそのままなので、\
                 取り込み直せば戻ります。ここにしかないバックアップは戻りません。",
    reset_do: "すべて削除",
    reset_done: "すべて削除しました。",
    delete: "削除…",
    delete_note: "{file} をこのコンピュータから完全に削除し、{size} を解放します。記録\
                  そのものはそのまま残ります。",
    delete_all: "バックアップをすべて削除…",
    delete_all_ask: "バックアップをすべて削除しますか？",
    delete_all_note: "バックアップ {n} 件をこのコンピュータから完全に削除し、{size} を\
                      解放します。記録そのものはそのまま残ります。",
    delete_all_do: "すべて削除",
    edit: "編集",
    save: "保存",
    revert: "元に戻す",
    revert_tip: "Kindle の記録にある文面に戻します。",
    write_note: "メモを書く…",
    add_note: "メモを追加",
    delete_note_ask: "このメモを削除しますか？",
    delete_note_said: "完全に消えます。Kindle からも、次につないだときに消えます。",
};

const SIMPLIFIED: Words = Words {
    by_week: "按周",
    mode_timeline: "时间线",
    mode_book: "按书",
    choose_backup: "选择备份…",
    drop_hint: "把备份拖到此窗口，或选择一个。",
    where_backups: "在 Kindle 上，备份位于 extensions/readinglog/backups。",
    syncing_in: "正在取回记录…",
    syncing_out: "正在放入你的笔记…",
    synced: "已同步 · {t}",
    not_synced: "未能同步：{why}",
    in_use: "另一个应用正在使用它",
    unplugged: "它已断开连接",
    try_again: "重试",
    show_folder: "显示文件夹",
    change_folder: "更改…",
    added: "已添加",
    nothing_new: "此备份中没有新内容。",
    from_backup: "备份",
    remove_everything: "全部删除…",
    reset_ask: "全部删除？",
    reset_note: "{folder} 中的{what}、所有备份和封面会连同设置一起删除，不留副本，\
                 可腾出 {size}。Kindle 自己的记录不受影响，重新导入即可恢复；\
                 只存在这里的备份无法恢复。",
    reset_do: "全部删除",
    reset_done: "已全部删除。",
    delete: "删除…",
    delete_note: "{file} 将从这台电脑上彻底删除，释放 {size}。记录本身不受影响。",
    delete_all: "删除所有备份…",
    delete_all_ask: "删除所有备份？",
    delete_all_note: "全部 {n} 个备份将从这台电脑上彻底删除，释放 {size}。记录本身不受影响。",
    delete_all_do: "全部删除",
    edit: "编辑",
    save: "保存",
    revert: "还原",
    revert_tip: "改回 Kindle 记录的文字。",
    write_note: "写笔记…",
    add_note: "添加笔记",
    delete_note_ask: "删除这条笔记？",
    delete_note_said: "它会被永久删除，下次连接时也会从它的 Kindle 上删除。",
};

const TRADITIONAL: Words = Words {
    by_week: "按週",
    mode_timeline: "時間軸",
    mode_book: "按書",
    choose_backup: "選擇備份…",
    drop_hint: "把備份拖到此視窗，或選擇一個。",
    where_backups: "在 Kindle 上，備份位於 extensions/readinglog/backups。",
    syncing_in: "正在取回記錄…",
    syncing_out: "正在放入你的筆記…",
    synced: "已同步 · {t}",
    not_synced: "未能同步：{why}",
    in_use: "另一個應用程式正在使用它",
    unplugged: "它已中斷連線",
    try_again: "重試",
    show_folder: "顯示資料夾",
    change_folder: "更改…",
    added: "已新增",
    nothing_new: "此備份中沒有新內容。",
    from_backup: "備份",
    remove_everything: "全部刪除…",
    reset_ask: "全部刪除？",
    reset_note: "{folder} 中的{what}、所有備份和封面會連同設定一起刪除，不留副本，\
                 可騰出 {size}。Kindle 自己的記錄不受影響，重新匯入即可復原；\
                 只存在這裡的備份無法復原。",
    reset_do: "全部刪除",
    reset_done: "已全部刪除。",
    delete: "刪除…",
    delete_note: "{file} 將從這台電腦上徹底刪除，釋放 {size}。記錄本身不受影響。",
    delete_all: "刪除所有備份…",
    delete_all_ask: "刪除所有備份？",
    delete_all_note: "全部 {n} 個備份將從這台電腦上徹底刪除，釋放 {size}。記錄本身不受影響。",
    delete_all_do: "全部刪除",
    edit: "編輯",
    save: "儲存",
    revert: "復原",
    revert_tip: "改回 Kindle 記錄的文字。",
    write_note: "寫筆記…",
    add_note: "新增筆記",
    delete_note_ask: "刪除這條筆記？",
    delete_note_said: "它會被永久刪除，下次連接時也會從它的 Kindle 上刪除。",
};

impl Words {
    /// [`Words`] in `lang`.
    pub fn of(lang: Lang) -> &'static Words {
        match lang {
            Lang::English => &ENGLISH,
            Lang::German => &GERMAN,
            Lang::Japanese => &JAPANESE,
            Lang::SimplifiedChinese => &SIMPLIFIED,
            Lang::TraditionalChinese => &TRADITIONAL,
        }
    }
}
