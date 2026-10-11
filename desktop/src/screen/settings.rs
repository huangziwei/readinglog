//! Config: the Kindle's settings the record is stated under, in the sections
//! and words of the Kindle's own config page, and the record's folder; and the
//! welcome a record holding no sitting opens on.

use std::fmt::Write as _;
use std::path::Path;

use readinglog_native::lang::Strings;

use super::{Words, esc};
use crate::workspace::{self, Workspace};

pub(super) fn page(ws: &Workspace, s: &Strings, w: &Words) -> String {
    let offered = workspace::offered(ws.site.lang());
    let set = |key: &str| -> &str {
        match key {
            "language" => &ws.site.language,
            "week_start" => &ws.site.week_start,
            "color_scheme" => &ws.site.color_scheme,
            "figures" => &ws.site.figures,
            "sitting_floor" => &ws.site.sitting_floor,
            _ => "",
        }
    };
    let mut out = format!("<header class=\"head\"><h1>{}</h1></header>", esc(s.config));
    let sections: [(&str, &[&str]); 3] = [
        (s.interface, &["language", "color_scheme"]),
        (s.the_calendar, &["week_start"]),
        (s.the_record, &["figures", "sitting_floor"]),
    ];
    for (heading, keys) in sections {
        let _ = write!(
            out,
            "<section><h2>{}</h2><div class=\"card rows\">",
            esc(heading)
        );
        for setting in offered.iter().filter(|o| keys.contains(&o.key)) {
            let _ = write!(
                out,
                "<div class=\"row\"><span>{}</span><span class=\"chips\">",
                esc(&setting.label)
            );
            for pick in &setting.picks {
                let _ = write!(
                    out,
                    "<button type=\"button\" class=\"chip{}\" data-set=\"{}\" data-value=\"{}\">{}</button>",
                    if pick.token == set(setting.key).trim() {
                        " on"
                    } else {
                        ""
                    },
                    setting.key,
                    esc(&pick.token),
                    esc(&pick.label)
                );
            }
            out.push_str("</span></div>");
        }
        if heading == s.the_record {
            let dir = ws.dir.to_string_lossy();
            let _ = write!(
                out,
                "<div class=\"row\"><span class=\"path\">{}</span><span class=\"acts\">\
                 <button type=\"button\" data-act=\"reveal\" data-path=\"{}\">{}</button>\
                 <button type=\"button\" data-act=\"folder\">{}</button></span></div>",
                esc(&dir),
                esc(&dir),
                esc(w.show_folder),
                esc(w.change_folder)
            );
        }
        out.push_str("</div></section>");
    }
    out
}

/// The page a record holding no sitting opens on: a backup to drop or pick, a
/// Kindle mounted, and where a Kindle keeps its backups.
pub(super) fn welcome(w: &Words, kindle: Option<&Path>) -> String {
    let mut out = format!(
        "<div class=\"welcome\"><h1>Reading Log</h1><p class=\"lede\">{}</p>\
         <button type=\"button\" class=\"drop\" data-act=\"pick\">{}</button>",
        esc(w.drop_hint),
        esc(w.choose_backup)
    );
    if let Some(root) = kindle {
        let _ = write!(
            out,
            "<div class=\"kindle wide\"><b>Kindle</b> {} \
             <button type=\"button\" class=\"button\" data-act=\"kindle\" data-root=\"{}\">{}</button></div>",
            esc(w.connected),
            esc(&root.to_string_lossy()),
            esc(w.import)
        );
    }
    let _ = write!(out, "<p class=\"hint\">{}</p></div>", esc(w.where_backups));
    out
}
