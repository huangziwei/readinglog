//! What the window says that no Kindle screen does, in the Kindle's five
//! languages.

use readinglog_native::lang::Lang;

pub struct Words {
    /// The heading over each week's reading, beside the Kindle's `by_month`.
    pub by_week: &'static str,
    pub import: &'static str,
    pub choose_backup: &'static str,
    pub drop_hint: &'static str,
    pub where_backups: &'static str,
    pub connected: &'static str,
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
}

const ENGLISH: Words = Words {
    by_week: "BY WEEK",
    import: "Import",
    choose_backup: "Choose a backup…",
    drop_hint: "Drop a backup on this window, or choose one.",
    where_backups: "On the Kindle, backups are kept in extensions/readinglog/backups.",
    connected: "Connected",
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
};

const GERMAN: Words = Words {
    by_week: "NACH WOCHE",
    import: "Importieren",
    choose_backup: "Sicherung wählen…",
    drop_hint: "Ziehen Sie eine Sicherung auf dieses Fenster, oder wählen Sie eine.",
    where_backups: "Auf dem Kindle liegen Sicherungen unter extensions/readinglog/backups.",
    connected: "Verbunden",
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
};

const JAPANESE: Words = Words {
    by_week: "週ごと",
    import: "取り込む",
    choose_backup: "バックアップを選ぶ…",
    drop_hint: "バックアップをこのウィンドウにドロップするか、選んでください。",
    where_backups: "Kindle では extensions/readinglog/backups にバックアップがあります。",
    connected: "接続中",
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
};

const SIMPLIFIED: Words = Words {
    by_week: "按周",
    import: "导入",
    choose_backup: "选择备份…",
    drop_hint: "把备份拖到此窗口，或选择一个。",
    where_backups: "在 Kindle 上，备份位于 extensions/readinglog/backups。",
    connected: "已连接",
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
};

const TRADITIONAL: Words = Words {
    by_week: "按週",
    import: "匯入",
    choose_backup: "選擇備份…",
    drop_hint: "把備份拖到此視窗，或選擇一個。",
    where_backups: "在 Kindle 上，備份位於 extensions/readinglog/backups。",
    connected: "已連接",
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
