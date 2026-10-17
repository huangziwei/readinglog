//! Config: the Kindle's settings in the sections, order and words of its own
//! config page, the record's folder, Reset and the backups; and the welcome a
//! record holding no sitting opens on.

use std::fmt::Write as _;
use std::path::Path;

use readinglog_native::backup;
use readinglog_native::lang::Strings;
use readinglog_native::record::Record;
use readinglog_native::view::config::{self, Summary};

use super::{Shown, Words, esc};
use crate::workspace::{self, RECORD, Workspace};

pub(super) fn page(ws: &Workspace, shown: Option<&Shown>, s: &Strings, w: &Words) -> String {
    let offered = workspace::offered(ws.config.lang());
    let set = |key: &str| -> &str {
        match key {
            "language" => &ws.config.language,
            "week_start" => &ws.config.week_start,
            "color_scheme" => &ws.config.color_scheme,
            "figures" => &ws.config.figures,
            "sitting_floor" => &ws.config.sitting_floor,
            "show_unnamed" => &ws.config.show_unnamed,
            "show_uncovered" => &ws.config.show_uncovered,
            _ => "",
        }
    };
    let summary = shown.map(|shown| {
        let mut summary = Summary::of(&shown.record, &shown.stats, &ws.dir, false);
        for kindle in ws.kindles() {
            summary.backups += backup::list(&kindle).len();
            summary.backup_bytes += backup::sizes(&kindle).1;
        }
        summary
    });
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
            // `reset`: one button, standing where `ws.kept` names an entry.
            if !ws.kept().is_empty() {
                let _ = write!(
                    out,
                    "<div class=\"row\"><span>{}</span><span class=\"acts\">\
                     <button type=\"button\" data-act=\"reset\">{}</button></span></div>",
                    esc(s.reset_row),
                    esc(w.remove_everything)
                );
            }
        }
        out.push_str("</div></section>");
    }
    out.push_str(&backups(ws, s, w));
    out
}

/// Every backup in the record's folder, newest first, as the Kindle's own list
/// states it against its own folder's record, each with its delete, and under
/// them the delete for them all.
fn backups(ws: &Workspace, s: &Strings, w: &Words) -> String {
    let every = ws.every_backup();
    let mut rows = Vec::new();
    for run in every.chunk_by(|a, b| a.0 == b.0) {
        let own = Record::load(&run[0].0.join(RECORD));
        for (_, held) in run {
            rows.push((held, config::row(held, &stated(&held.path), &own, s)));
        }
    }
    if rows.is_empty() {
        return String::new();
    }
    rows.sort_by(|a, b| b.0.stamp.cmp(&a.0.stamp));
    let said = s.n_files.replace("{n}", &rows.len().to_string()).replace(
        "{size}",
        &config::bytes(rows.iter().map(|(b, _)| b.bytes).sum()),
    );
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card rows\">",
        esc(s.backups_row),
        esc(&said)
    );
    for (held, row) in &rows {
        let _ = write!(
            out,
            "<div class=\"row\"><span>{}<br><small>{}</small></span><span class=\"acts\">{}\
             <button type=\"button\" data-act=\"delete\" data-path=\"{}\">{}</button></span></div>",
            esc(&row.when),
            esc(&row.holds),
            esc(&row.size),
            esc(&held.path.to_string_lossy()),
            esc(w.delete)
        );
    }
    let _ = write!(
        out,
        "</div><p class=\"acts below\"><button type=\"button\" data-act=\"delete-all\">{}</button></p></section>",
        esc(w.delete_all)
    );
    out
}

/// What the backup at `at` states of itself, the record inside it standing in
/// for the sittings and days an `about.tsv` naming no first day leaves out.
fn stated(at: &Path) -> backup::About {
    let said = backup::about(at).unwrap_or_default();
    if said.is_empty() || !said.first.is_empty() {
        return said;
    }
    match backup::peek(at) {
        Ok(inside) => {
            let read = backup::About::of(&inside, &said.written);
            backup::About {
                sittings: read.sittings,
                first: read.first,
                last: read.last,
                ..said
            }
        }
        Err(_) => said,
    }
}

/// The page a record holding no sitting opens on: a backup to drop or pick, the
/// card of the Kindle plugged in, and where a Kindle keeps its backups.
pub(super) fn welcome(w: &Words, kindle: Option<&str>) -> String {
    let mut out = format!(
        "<div class=\"welcome\"><h1>Reading Log</h1><p class=\"lede\">{}</p>\
         <button type=\"button\" class=\"drop\" data-act=\"pick\">{}</button>",
        esc(w.drop_hint),
        esc(w.choose_backup)
    );
    if let Some(serial) = kindle {
        out.push_str(&super::kindle_card(
            w,
            crate::kindles::model(serial),
            serial,
            " wide",
        ));
    }
    let _ = write!(out, "<p class=\"hint\">{}</p></div>", esc(w.where_backups));
    out
}
