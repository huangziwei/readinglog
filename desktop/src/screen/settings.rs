//! Config: the Kindle's settings the record is stated under, in the sections,
//! order and words of the Kindle's own config page, the record's folder and the
//! backups imported into it; and the welcome a record holding no sitting opens on.

use std::fmt::Write as _;
use std::path::Path;

use readinglog_native::lang::Strings;
use readinglog_native::view::config::{self, Summary};

use super::{Shown, Words, esc};
use crate::workspace::{self, Workspace};

pub(super) fn page(ws: &Workspace, shown: Option<&Shown>, s: &Strings, w: &Words) -> String {
    let offered = workspace::offered(ws.site.lang());
    let set = |key: &str| -> &str {
        match key {
            "language" => &ws.site.language,
            "week_start" => &ws.site.week_start,
            "color_scheme" => &ws.site.color_scheme,
            "figures" => &ws.site.figures,
            "sitting_floor" => &ws.site.sitting_floor,
            "show_unnamed" => &ws.site.show_unnamed,
            "show_uncovered" => &ws.site.show_uncovered,
            _ => "",
        }
    };
    let summary = shown.map(|shown| Summary::of(&shown.record, &shown.stats, &ws.dir, false));
    let recorded = summary
        .as_ref()
        .and_then(|summary| config::recorded(summary, s));
    let mut out = format!("<header class=\"head\"><h1>{}</h1></header>", esc(s.config));
    let sections: [(&str, &[&str]); 3] = [
        (s.interface, &["language", "color_scheme"]),
        (s.the_calendar, &["week_start"]),
        (
            s.the_record,
            &["show_unnamed", "show_uncovered", "figures", "sitting_floor"],
        ),
    ];
    for (heading, keys) in sections {
        let said = match heading == s.the_record {
            true => recorded.clone().unwrap_or_default(),
            false => String::new(),
        };
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card rows\">",
            esc(heading),
            esc(&said)
        );
        for key in keys.iter() {
            let Some(setting) = offered.iter().find(|o| o.key == *key) else {
                continue;
            };
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
    if let Some(shown) = shown {
        out.push_str(&backups(ws, shown, s));
    }
    out
}

/// The backups imported into the record's folder, as the Kindle's own list
/// states each: when it was written, what it weighs and what it holds.
fn backups(ws: &Workspace, shown: &Shown, s: &Strings) -> String {
    let listed = Summary::listed(&ws.dir, &shown.record, shown.lang);
    if listed.rows.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card rows\">",
        esc(s.backups_row),
        esc(&listed.said)
    );
    for row in &listed.rows {
        let _ = write!(
            out,
            "<div class=\"row\"><span>{}<br><small>{}</small></span><span>{}</span></div>",
            esc(&row.when),
            esc(&row.holds),
            esc(&row.size)
        );
    }
    out.push_str("</div></section>");
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
