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
