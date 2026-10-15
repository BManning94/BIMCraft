//! A project's units: how lengths and areas are shown, and how typed lengths read. The model keeps
//! millimetres, and so does every command; units change only what drawings, schedules and the
//! desktop show. Feet and inches round to the nearest sixteenth of an inch.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    #[default]
    #[serde(rename = "mm")]
    Millimetres,
    #[serde(rename = "cm")]
    Centimetres,
    #[serde(rename = "m")]
    Metres,
    /// Fractional inches.
    #[serde(rename = "in")]
    Inches,
    /// Decimal feet.
    #[serde(rename = "ft")]
    Feet,
    /// Feet and fractional inches.
    #[serde(rename = "ft-in")]
    FeetInches,
}

const INCH: f64 = 25.4;
const FOOT: f64 = 304.8;

impl Units {
    pub const ALL: [Units; 6] = [Units::Millimetres, Units::Centimetres, Units::Metres, Units::Inches, Units::Feet, Units::FeetInches];

    /// Its name in commands and files: mm, cm, m, in, ft, ft-in.
    pub fn name(self) -> &'static str {
        match self {
            Units::Millimetres => "mm",
            Units::Centimetres => "cm",
            Units::Metres => "m",
            Units::Inches => "in",
            Units::Feet => "ft",
            Units::FeetInches => "ft-in",
        }
    }

    /// As a menu names it.
    pub fn label(self) -> &'static str {
        match self {
            Units::Millimetres => "Millimetres",
            Units::Centimetres => "Centimetres",
            Units::Metres => "Metres",
            Units::Inches => "Inches",
            Units::Feet => "Feet",
            Units::FeetInches => "Feet and inches",
        }
    }

    pub fn from_name(name: &str) -> Option<Units> {
        Units::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn imperial(self) -> bool {
        matches!(self, Units::Inches | Units::Feet | Units::FeetInches)
    }

    /// A length as drawings show it: 3000, 300, 3.00, 118 1/8", 9.84', 9'-10 1/8".
    pub fn length(self, mm: f64) -> String {
        match self {
            Units::Millimetres => format!("{}", mm.round() + 0.0),
            Units::Centimetres => format!("{}", (mm).round() / 10.0 + 0.0),
            // Rounded first, so what rounds to nothing shows no minus.
            Units::Metres => format!("{:.2}", (mm / 10.0).round() / 100.0 + 0.0),
            Units::Feet => format!("{:.2}'", (mm / FOOT * 100.0).round() / 100.0 + 0.0),
            Units::Inches | Units::FeetInches => {
                // In sixteenths of an inch.
                let n = (mm.abs() / INCH * 16.0).round();
                let sign = if mm < 0.0 && n > 0.0 { "-" } else { "" };
                let (feet, rest) = if self == Units::FeetInches { ((n / 192.0).floor(), n % 192.0) } else { (0.0, n) };
                let (whole, part) = ((rest / 16.0).floor(), rest % 16.0);
                // The sixteenths in lowest terms.
                let (mut top, mut bottom) = (part as u32, 16);
                while top > 0 && top % 2 == 0 {
                    (top, bottom) = (top / 2, bottom / 2);
                }
                let inches = match (whole, top) {
                    (w, 0) => format!("{w}\""),
                    (0.0, t) if self == Units::Inches => format!("{t}/{bottom}\""),
                    (w, t) => format!("{w} {t}/{bottom}\""),
                };
                if self == Units::FeetInches { format!("{sign}{feet}'-{inches}") } else { format!("{sign}{inches}") }
            }
        }
    }

    /// A length with its unit, as the desktop labels one: 3000 mm, 3.00 m, 9'-10 1/8".
    pub fn with_unit(self, mm: f64) -> String {
        match self {
            Units::Millimetres | Units::Centimetres | Units::Metres => format!("{} {}", self.length(mm), self.name()),
            _ => self.length(mm),
        }
    }

    /// An area in square metres or, in inches and feet, square feet; and that unit.
    pub fn area(self, mm2: f64) -> (f64, &'static str) {
        if self.imperial() { (mm2 / (FOOT * FOOT), "ft²") } else { (mm2 / 1e6, "m²") }
    }

    /// A length typed, in mm: a number alone is in these units (feet, in feet and inches); one
    /// written with its unit (mm, cm, m, ft or ', in or ") is in that. Feet and inches can come
    /// together, and inches with a fraction: 10' 6 1/2", 10'-6", 6 1/2".
    pub fn parse(self, text: &str) -> Option<f64> {
        let t = text.trim().to_ascii_lowercase();
        let mm = if let Some(n) = t.strip_suffix("mm") {
            number(n)?
        } else if let Some(n) = t.strip_suffix("cm") {
            number(n)? * 10.0
        } else if let Some(n) = t.strip_suffix('m') {
            number(n)? * 1000.0
        } else if t.contains(['\'', '"']) || t.ends_with("ft") || t.ends_with("in") {
            let t = t.replace("ft", "'").replace("in", "\"");
            let (feet, rest) = t.split_once('\'').unwrap_or(("", &t));
            let negative = feet.trim().starts_with('-') || (feet.trim().is_empty() && rest.trim().starts_with('-'));
            let feet = if feet.trim().is_empty() { 0.0 } else { number(feet)?.abs() };
            let rest = rest.trim().trim_start_matches('-').trim().trim_end_matches('"');
            let inches = if rest.trim().is_empty() { 0.0 } else { inches(rest)? };
            (feet * FOOT + inches * INCH) * if negative { -1.0 } else { 1.0 }
        } else {
            match self {
                Units::Millimetres => number(&t)?,
                Units::Centimetres => number(&t)? * 10.0,
                Units::Metres => number(&t)? * 1000.0,
                Units::Inches => t.strip_prefix('-').map_or_else(|| inches(&t), |t| inches(t).map(|x| -x))? * INCH,
                Units::Feet | Units::FeetInches => number(&t)? * FOOT,
            }
        };
        // To the micron: what was typed exactly stays exact.
        let mm = (mm * 1e3).round() / 1e3;
        mm.is_finite().then_some(mm + 0.0)
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// Inches typed: 6, 6.5, 6 1/2, or 1/2.
fn inches(s: &str) -> Option<f64> {
    let fraction = |f: &str| {
        let (a, b) = f.split_once('/')?;
        let (a, b) = (number(a)?, number(b)?);
        (b != 0.0).then(|| a / b)
    };
    match s.split_whitespace().collect::<Vec<_>>().as_slice() {
        [whole, part] => Some(number(whole)? + fraction(part)?),
        [one] if one.contains('/') => fraction(one),
        [one] => number(one),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_show_in_the_project_units() {
        let shown = |u: Units, mm: f64| u.length(mm);
        assert_eq!(shown(Units::Millimetres, 3000.4), "3000");
        assert_eq!(shown(Units::Centimetres, 3005.0), "300.5");
        assert_eq!(shown(Units::Metres, 2950.0), "2.95");
        assert_eq!(shown(Units::Inches, 3000.0), "118 1/8\"");
        assert_eq!(shown(Units::Inches, 12.7), "1/2\"");
        assert_eq!(shown(Units::Feet, 3000.0), "9.84'");
        assert_eq!(shown(Units::FeetInches, 3000.0), "9'-10 1/8\"");
        assert_eq!(shown(Units::FeetInches, 304.8), "1'-0\"");
        assert_eq!(shown(Units::FeetInches, -457.2), "-1'-6\"");
        assert_eq!(shown(Units::FeetInches, 0.1), "0'-0\"", "no minus on what rounds to nothing");
        assert_eq!(Units::Metres.with_unit(3000.0), "3.00 m");
        assert_eq!(Units::FeetInches.with_unit(3000.0), "9'-10 1/8\"");
        let (ft2, unit) = Units::FeetInches.area(18e6);
        assert_eq!((format!("{ft2:.2}"), unit), ("193.75".to_string(), "ft²"));
    }

    #[test]
    fn typed_lengths_read_in_the_project_units_or_their_own() {
        let read = |u: Units, s: &str| u.parse(s);
        assert_eq!(read(Units::Millimetres, "3000"), Some(3000.0));
        assert_eq!(read(Units::Millimetres, " 3 m"), Some(3000.0));
        assert_eq!(read(Units::Metres, "3.2"), Some(3200.0));
        assert_eq!(read(Units::Metres, "250mm"), Some(250.0));
        assert_eq!(read(Units::Centimetres, "12.5"), Some(125.0));
        assert_eq!(read(Units::FeetInches, "10' 6\""), Some(3200.4));
        assert_eq!(read(Units::FeetInches, "10'-6 1/2\""), Some(3213.1));
        assert_eq!(read(Units::FeetInches, "10"), Some(3048.0), "a number alone is feet");
        assert_eq!(read(Units::FeetInches, "-1'-6\""), Some(-457.2));
        assert_eq!(read(Units::FeetInches, "2.5ft"), Some(762.0));
        assert_eq!(read(Units::Millimetres, "6\""), Some(152.4));
        assert_eq!(read(Units::Inches, "6 1/2"), Some(165.1));
        assert_eq!(read(Units::Inches, "30in"), Some(762.0));
        for bad in ["", "x", "1/0\"", "3 m m", "1e999"] {
            assert_eq!(read(Units::FeetInches, bad), None, "{bad}");
        }
        // What's shown reads back as itself.
        for u in Units::ALL {
            let shown = u.length(3213.1);
            assert!((u.parse(&shown).unwrap() - 3213.1).abs() <= 5.0, "{u:?} {shown}");
        }
    }
}
