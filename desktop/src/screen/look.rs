//! The colours a screen is drawn in, off the Kindle's palette for the colour
//! scheme set, and a tone for each book off its cover.

use std::path::Path;

use readinglog_native::ui::paint::Palette;

/// Every colour a page and its charts take, as RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub paper: [u8; 3],
    pub side: [u8; 3],
    pub card: [u8; 3],
    pub ink: [u8; 3],
    pub soft: [u8; 3],
    pub faint: [u8; 3],
    pub rule: [u8; 3],
    pub edge: [u8; 3],
    /// A day with no reading; [`Look::level`] answers it at zero.
    pub l0: [u8; 3],
    /// The four steps `charts::level` names, lightest first.
    pub steps: [[u8; 3]; 4],
    pub bar: [u8; 3],
    pub mark: [u8; 3],
    pub accent: [u8; 3],
    pub accent_soft: [u8; 3],
}

impl Look {
    /// Paper, ink and rules in warm greys; `palette` for every mark.
    pub fn light(palette: &Palette) -> Self {
        Self {
            paper: [0xFB, 0xFA, 0xF7],
            side: [0xF2, 0xF0, 0xEB],
            card: [0xFF, 0xFF, 0xFF],
            ink: [0x1D, 0x1D, 0x1F],
            soft: [0x6B, 0x6B, 0x70],
            faint: [0xA4, 0xA3, 0xA0],
            rule: [0xE6, 0xE3, 0xDC],
            edge: [0xEE, 0xEB, 0xE5],
            l0: [0xEC, 0xE9, 0xE2],
            steps: [
                palette.steps[0],
                palette.steps[1],
                palette.steps[2],
                palette.steps[3],
            ],
            bar: palette.steps[2],
            mark: palette.mark,
            accent: palette.steps[3],
            accent_soft: palette.steps[1],
        }
    }

    /// The colour of a day on `level`, zero being no reading.
    pub fn level(&self, level: usize) -> [u8; 3] {
        match level {
            0 => self.l0,
            n => self.steps[(n - 1).min(3)],
        }
    }

    /// The stylesheet's colour variables, as one `:root` rule.
    pub fn css(&self) -> String {
        let pairs = [
            ("paper", self.paper),
            ("side", self.side),
            ("card", self.card),
            ("ink", self.ink),
            ("soft", self.soft),
            ("faint", self.faint),
            ("rule", self.rule),
            ("edge", self.edge),
            ("l0", self.l0),
            ("accent", self.accent),
            ("accent-soft", self.accent_soft),
            ("mark", self.mark),
        ];
        let mut out = String::from(":root {\n");
        for (name, rgb) in pairs {
            out.push_str(&format!("  --{name}: {};\n", hex(rgb)));
        }
        out.push_str("}\n");
        out
    }
}

pub fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A colour standing for the cover at `path`: its most colourful quarter,
/// averaged, then held to a lightness a bar on paper reads at.
pub fn cover_tone(path: &Path) -> Option<[u8; 3]> {
    let picture = image::open(path).ok()?.thumbnail(24, 36).to_rgb8();
    let mut pixels: Vec<(f32, [f32; 3])> = picture
        .pixels()
        .map(|p| {
            let rgb = [p[0], p[1], p[2]].map(|c| c as f32 / 255.0);
            let (_, s, l) = hsl(rgb);
            // `s` weighed down near white and near black.
            (s * (1.0 - (2.0 * l - 1.0).abs()), rgb)
        })
        .collect();
    pixels.sort_by(|a, b| b.0.total_cmp(&a.0));
    let take = (pixels.len() / 4).max(1);
    let mut sum = [0.0f32; 3];
    for (_, rgb) in &pixels[..take] {
        for c in 0..3 {
            sum[c] += rgb[c];
        }
    }
    let (h, s, l) = hsl(sum.map(|c| c / take as f32));
    Some(from_hsl(h, s.clamp(0.18, 0.62), l.clamp(0.36, 0.56)))
}

/// A tone for a book with no cover, off its title.
pub fn title_tone(title: &str) -> [u8; 3] {
    let mut hash: u32 = 2166136261;
    for b in title.bytes() {
        hash = (hash ^ b as u32).wrapping_mul(16777619);
    }
    from_hsl((hash % 360) as f32, 0.32, 0.48)
}

/// `rgb`, each channel 0 to 1, as hue in degrees, saturation and lightness.
fn hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = match max {
        m if m == r => 60.0 * (((g - b) / d) % 6.0),
        m if m == g => 60.0 * ((b - r) / d + 2.0),
        _ => 60.0 * ((r - g) / d + 4.0),
    };
    ((h + 360.0) % 360.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_turns_round_through_its_own_hue() {
        for rgb in [
            [200u8, 40, 40],
            [40, 160, 90],
            [30, 60, 200],
            [128, 128, 128],
        ] {
            let (h, s, l) = hsl(rgb.map(|c| c as f32 / 255.0));
            let back = from_hsl(h, s, l);
            for c in 0..3 {
                assert!(
                    (back[c] as i32 - rgb[c] as i32).abs() <= 1,
                    "{rgb:?} → {back:?}"
                );
            }
        }
    }

    #[test]
    fn a_title_tone_stands_at_one_lightness() {
        let (_, s, l) = hsl(title_tone("Any Title").map(|c| c as f32 / 255.0));
        assert!(
            (l - 0.48).abs() < 0.01 && (s - 0.32).abs() < 0.02,
            "{s} {l}"
        );
    }
}
