//! `posts/<name>.md`: TOML front matter between two `+++` lines, then
//! Markdown. A post names any number of books, or none.

use std::path::Path;

use serde::Deserialize;

use readinglog_native::date;

/// The line opening and closing the front matter.
const FENCE: &str = "+++";

/// One post.
#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    /// Its file's name under `posts/`, without `.md`.
    pub name: String,
    /// Its address under `posts/`, off the file's name.
    pub slug: String,
    pub title: String,
    /// The day it is dated, as a day count.
    pub date: i64,
    /// The books it is about, each by key.
    pub books: Vec<String>,
    /// Whether its front matter marks it a draft.
    pub draft: bool,
    /// The Markdown under the front matter.
    pub body: String,
}

#[derive(Deserialize)]
struct Front {
    title: String,
    date: toml::Value,
    #[serde(default)]
    books: Vec<String>,
    #[serde(default)]
    draft: bool,
}

/// Every post under `dir`, newest first, and a line naming each file that
/// could not be read as one.
pub fn load(dir: &Path) -> (Vec<Post>, Vec<String>) {
    let mut posts: Vec<Post> = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (posts, problems);
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();
    for path in paths {
        let named = path.file_name().unwrap_or_default().to_string_lossy();
        let read = std::fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|text| parse(&path, &text));
        match read {
            Ok(post) if posts.iter().any(|p| p.slug == post.slug) => {
                problems.push(format!(
                    "posts/{named}: another post already has the address {}",
                    post.slug
                ));
            }
            Ok(post) => posts.push(post),
            Err(why) => problems.push(format!("posts/{named}: {why}")),
        }
    }
    posts.sort_by(|a, b| (b.date, &a.slug).cmp(&(a.date, &b.slug)));
    (posts, problems)
}

/// The post `text` holds, saved as `posts/<name>.md`, or why it is none.
pub fn read(name: &str, text: &str) -> Result<Post, String> {
    parse(Path::new(&format!("{name}.md")), text)
}

/// The post `text` holds, its slug off `path`'s name.
fn parse(path: &Path, text: &str) -> Result<Post, String> {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let slug = slug(&stem);
    if slug.is_empty() {
        return Err("its name gives it no address".into());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|r| r.strip_prefix('\n').or_else(|| r.strip_prefix("\r\n")))
        .ok_or("it does not open on a +++ line")?;
    let (front, body) = split_fence(rest).ok_or("its front matter has no closing +++ line")?;
    let front: Front = toml::from_str(front).map_err(|err| err.message().to_string())?;
    let date = day_of(&front.date).ok_or("its date is not a day, such as 2026-10-11")?;
    Ok(Post {
        name: stem.into_owned(),
        slug,
        title: front.title,
        date,
        books: front.books,
        draft: front.draft,
        body: body.to_string(),
    })
}

/// The front matter in `rest` and the body under the `+++` line closing it.
fn split_fence(rest: &str) -> Option<(&str, &str)> {
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

/// A TOML date, or a string spelling one, as a day count.
fn day_of(value: &toml::Value) -> Option<i64> {
    match value {
        toml::Value::Datetime(at) => {
            let day = at.date?;
            Some(date::days_from_civil(
                i64::from(day.year),
                i64::from(day.month),
                i64::from(day.day),
            ))
        }
        toml::Value::String(text) => date::parse_day(text.trim()),
        _ => None,
    }
}

/// `text` as an address: lower-case Latin letters and digits, a hyphen between
/// runs, at most [`SLUG_MAX`] characters, an apostrophe joining the letters
/// either side. Empty where `text` holds no Latin letter or digit.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\'' | '’' => {}
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }
    let out = out.trim_end_matches('-');
    match out.len() <= SLUG_MAX {
        true => out.to_string(),
        // Cut at the last hyphen inside the limit, where there is one.
        false => {
            let cut = &out[..SLUG_MAX];
            cut.rsplit_once('-')
                .map_or(cut, |(head, _)| head)
                .to_string()
        }
    }
}

/// The longest address [`slug`] makes.
const SLUG_MAX: usize = 60;

/// `markdown` as HTML.
pub fn html(markdown: &str) -> String {
    use pulldown_cmark::{Options, Parser, html::push_html};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    push_html(&mut out, Parser::new_ext(markdown, options));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;

    #[test]
    fn a_post_reads_its_front_matter_and_keeps_its_body() {
        let text = "+++\ntitle = \"On a book\"\ndate = 2026-10-11\nbooks = [\"B00KEY\"]\n+++\n\nIt was *good*.\n";
        let post = parse(Path::new("posts/On a Book!.md"), text).unwrap();
        assert_eq!(post.slug, "on-a-book");
        assert_eq!(post.title, "On a book");
        assert_eq!(post.date, date::days_from_civil(2026, 10, 11));
        assert_eq!(post.books, ["B00KEY"]);
        assert!(!post.draft);
        assert_eq!(html(&post.body).trim(), "<p>It was <em>good</em>.</p>");

        let quoted = "+++\r\ntitle = \"x\"\r\ndate = \"2026-01-02\"\r\ndraft = true\r\n+++\r\nBody";
        let post = parse(Path::new("x.md"), quoted).unwrap();
        assert!(post.draft);
        assert_eq!(post.date, date::days_from_civil(2026, 1, 2));
        assert_eq!(post.body, "Body");
    }

    #[test]
    fn a_file_that_is_not_a_post_is_named_and_left_out() {
        let dir = scratch("posts");
        let good = "+++\ntitle = \"Kept\"\ndate = 2026-10-01\n+++\nOne\n";
        std::fs::write(dir.join("kept.md"), good).unwrap();
        std::fs::write(dir.join("newer.md"), good.replace("10-01", "10-05")).unwrap();
        std::fs::write(dir.join("kept!.md"), good).unwrap();
        std::fs::write(dir.join("open.md"), "+++\ntitle = \"x\"\n").unwrap();
        std::fs::write(
            dir.join("undated.md"),
            "+++\ntitle = \"x\"\ndate = 3\n+++\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a post").unwrap();
        let (posts, problems) = load(&dir);
        let slugs: Vec<&str> = posts.iter().map(|p| p.slug.as_str()).collect();
        assert_eq!(slugs, ["newer", "kept"]);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn an_address_is_latin_letters_and_digits_with_hyphens_between() {
        assert_eq!(slug("The Name of the Rose"), "the-name-of-the-rose");
        assert_eq!(slug("  1984 — Orwell  "), "1984-orwell");
        assert_eq!(slug("川辺の記録"), "");
        assert_eq!(slug("Won’t Look Back"), "wont-look-back");
        let long = slug(&"word ".repeat(30));
        assert!(long.len() <= SLUG_MAX && long.ends_with("word"), "{long}");
    }
}
