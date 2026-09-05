//! [`PROPERTY`] on [`SOURCE`], set through `lipc-hash-prop`.
//! `MarkAsReadHandler` answers on it and files a `ContentReadStateRecord`.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// The lipc source `openBook` also sits on.
const SOURCE: &str = "com.lab126.readnow";

/// The hasharray property `MarkAsReadHandler` answers on.
const PROPERTY: &str = "kppMarkAsReadAction";

/// The `kppMAR` values setting `ReadState.READ_MANUAL` and
/// `ReadState.UNREAD_MANUAL`.
const READ: &str = "actionRead";
const UNREAD: &str = "actionUnread";

/// Whether `field` can stand inside a quoted hasharray value.
fn plain(field: &str) -> bool {
    !field.is_empty() && !field.contains(['"', '\\', '{', '}', '\n'])
}

/// The hasharray `lipc-hash-prop` reads on its standard input. `None` where
/// [`plain`] refuses either field.
fn hash(cde_key: &str, cde_type: &str, read: bool) -> Option<String> {
    if !plain(cde_key) || !plain(cde_type) {
        return None;
    }
    let action = match read {
        true => READ,
        false => UNREAD,
    };
    Some(format!(
        "{{kppCdeKey = \"{cde_key}\", kppCdeType = \"{cde_type}\", kppMAR = \"{action}\"}}\n"
    ))
}

/// Set [`PROPERTY`] to mark the book `cde_key` names read or unread,
/// answering whether `lipc-hash-prop` ran and exited clean.
pub fn set(cde_key: &str, cde_type: &str, read: bool) -> bool {
    let Some(said) = hash(cde_key, cde_type, read) else {
        eprintln!("mark: {cde_key} {cde_type} carries a character the property cannot take");
        return false;
    };
    let mut child = match Command::new("lipc-hash-prop")
        .arg(SOURCE)
        .arg(PROPERTY)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("mark: lipc-hash-prop would not run: {err}");
            return false;
        }
    };
    if let Some(stdin) = child.stdin.as_mut()
        && let Err(err) = stdin.write_all(said.as_bytes())
    {
        eprintln!("mark: {PROPERTY} took no value: {err}");
    }
    match child.wait() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("mark: lipc-hash-prop {status}");
            false
        }
        Err(err) => {
            eprintln!("mark: lipc-hash-prop did not finish: {err}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hasharray_carries_the_three_keys_the_handler_reads() {
        let said = hash("B00OKPCRLG", "EBOK", true).expect("a plain key and type");
        assert_eq!(
            said.trim_end(),
            "{kppCdeKey = \"B00OKPCRLG\", kppCdeType = \"EBOK\", kppMAR = \"actionRead\"}"
        );
        assert!(said.ends_with('\n'), "lipc-hash-prop reads a line");
        assert!(hash("B00OKPCRLG", "EBOK", false).unwrap().contains(UNREAD));
    }

    #[test]
    fn a_key_that_could_break_the_quoting_sends_nothing() {
        for (key, cde_type) in [
            ("A\"B", "EBOK"),
            ("AB", "EB\"OK"),
            ("A\\B", "EBOK"),
            ("A}B", "EBOK"),
            ("A\nB", "EBOK"),
            ("", "EBOK"),
            ("AB", ""),
        ] {
            assert!(
                hash(key, cde_type, true).is_none(),
                "{key:?} {cde_type:?} reached the property"
            );
        }
        // A `*`-prefixed key and a `CR!` sideload key both stand.
        assert!(hash("*aa11bb22", "PDOC", true).is_some());
        assert!(hash("CR!ABC123", "PDOC", true).is_some());
    }
}
