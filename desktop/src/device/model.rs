//! A Kindle's model, read off the code in its serial's first six characters.

/// Each model of a serial led by `B` or `9`, and its codes, the serial's third
/// and fourth characters.
const FIRST: &[(&str, &[&str])] = &[
    ("Kindle 2", &["02", "03"]),
    ("Kindle DX", &["04", "05"]),
    ("Kindle DX Graphite", &["09"]),
    ("Kindle 3", &["08", "06", "0A"]),
    ("Kindle 4", &["0E", "23"]),
    ("Kindle Touch", &["0F", "11", "10", "12"]),
    ("Kindle Paperwhite", &["24", "1B", "1D", "1F", "1C", "20"]),
    (
        "Kindle Paperwhite 2",
        &[
            "D4", "5A", "D5", "D6", "D7", "D8", "F2", "17", "60", "F4", "F9", "62", "61", "5F",
        ],
    ),
    ("Kindle Basic", &["C6", "DD"]),
    ("Kindle Voyage", &["13", "54", "2A", "4F", "52", "53"]),
];

/// Each model of any other serial, and its codes, the serial's fourth through
/// sixth characters.
const SECOND: &[(&str, &[&str])] = &[
    (
        "Kindle Paperwhite 3",
        &[
            "0G1", "0G2", "0G4", "0G5", "0G6", "0G7", "0KB", "0KC", "0KD", "0KE", "0KF", "0KG",
            "0LK", "0LL",
        ],
    ),
    ("Kindle Oasis", &["0GC", "0GD", "0GR", "0GS", "0GT", "0GU"]),
    (
        "Kindle Oasis 2",
        &[
            "0LM", "0LN", "0LP", "0LQ", "0P1", "0P2", "0P6", "0P7", "0P8", "0S1", "0S2", "0S3",
            "0S4", "0S7", "0SA",
        ],
    ),
    ("Kindle Basic 2", &["0DU", "0K9", "0KA"]),
    (
        "Kindle Paperwhite 4",
        &[
            "0PP", "0T1", "0T2", "0T3", "0T4", "0T5", "0T6", "0T7", "0TJ", "0TK", "0TL", "0TM",
            "0TN", "102", "103", "16Q", "16R", "16S", "16T", "16U", "16V",
        ],
    ),
    (
        "Kindle Basic 3",
        &["10L", "0WF", "0WG", "0WH", "0WJ", "0VB"],
    ),
    (
        "Kindle Oasis 3",
        &["11L", "0WQ", "0WP", "0WN", "0WM", "0WL"],
    ),
    (
        "Kindle Paperwhite 5",
        &["1Q0", "1PX", "1VD", "21A", "2BJ", "2DK"],
    ),
    ("Kindle Paperwhite 5 SE", &["1LG", "219", "2BH"]),
    (
        "Kindle Basic 4",
        &["22D", "25T", "23A", "2AQ", "2AP", "1XH", "22C"],
    ),
    (
        "Kindle Scribe",
        &["27J", "2BL", "263", "227", "2BM", "23L", "23M", "270"],
    ),
    (
        "Kindle Colorsoft",
        &[
            "3H2", "3H4", "3H6", "3H7", "3H9", "3JT", "3J6", "455", "456", "4EP", "34X", "3HB",
        ],
    ),
    (
        "Kindle Basic 5",
        &["A89", "3L2", "3L3", "3L4", "3L5", "3L6", "3KM"],
    ),
    (
        "Kindle Paperwhite 6",
        &[
            "33W", "33X", "346", "349", "3H3", "3H5", "3H8", "3HA", "3J5", "3JS",
        ],
    ),
    (
        "Kindle Scribe",
        &["3V0", "3V1", "3X5", "3UV", "3X4", "3X3", "41E", "410"],
    ),
    (
        "Kindle Scribe 3",
        &["4PG", "4PE", "4PL", "4F8", "4FA", "454"],
    ),
    (
        "Kindle Scribe Colorsoft",
        &["4VX", "4PF", "4PH", "4F9", "4FB", "46P"],
    ),
];

/// The model `serial` names, whole or cut; `None` for a code neither table
/// holds.
pub fn of(serial: &str) -> Option<&'static str> {
    let chars: Vec<char> = serial.trim().chars().collect();
    let code = |from: usize, to: usize| -> Option<String> {
        chars.get(from..to).map(|c| c.iter().collect())
    };
    let (table, code) = match chars.first() {
        Some('B' | '9') => (FIRST, code(2, 4)?),
        Some(_) => (SECOND, code(3, 6)?),
        None => return None,
    };
    table
        .iter()
        .find(|(_, codes)| codes.contains(&code.as_str()))
        .map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_names_its_model_whole_or_cut() {
        assert_eq!(of("G0003H2123456789"), None, "code at the fourth character");
        assert_eq!(of("G003H21234567890"), Some("Kindle Colorsoft"));
        assert_eq!(of("G003H2-7890"), Some("Kindle Colorsoft"));
        assert_eq!(of("G000LM-ABCD"), Some("Kindle Oasis 2"));
        assert_eq!(of("G0027J-ABCD"), Some("Kindle Scribe"));
        assert_eq!(of("G003V0-ABCD"), Some("Kindle Scribe"));
        assert_eq!(of("B024ABCDEF"), Some("Kindle Paperwhite"));
        assert_eq!(of("90D4ABCDEF"), Some("Kindle Paperwhite 2"));
        assert_eq!(of("G00ZZZ-ABCD"), None);
        assert_eq!(of("G0"), None);
        assert_eq!(of(""), None);
    }
}
