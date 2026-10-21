//! Lookups in OpenSky's aircraft database (`aircraftDatabase.csv`), which maps
//! transponder addresses to registration, type and operator.

use std::io::Read;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AircraftInfo {
    pub registration: Option<String>,
    /// ICAO type designator, e.g. `A20N`.
    pub typecode: Option<String>,
    pub operator: Option<String>,
}

#[derive(Deserialize)]
struct Row {
    icao24: String,
    registration: String,
    typecode: String,
    operator: String,
}

fn non_empty(s: String) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Scans the database for one airframe. The file is ~95 MB, but a single
/// streaming pass takes about a second, which is fine for a CLI lookup.
pub fn lookup(path: impl AsRef<Path>, icao24: &str) -> Result<Option<AircraftInfo>, csv::Error> {
    lookup_in(std::fs::File::open(path)?, icao24)
}

pub fn lookup_in(reader: impl Read, icao24: &str) -> Result<Option<AircraftInfo>, csv::Error> {
    let wanted = icao24.trim().to_ascii_lowercase();
    for row in csv::Reader::from_reader(reader).deserialize::<Row>() {
        let row = row?;
        if row.icao24.eq_ignore_ascii_case(&wanted) {
            return Ok(Some(AircraftInfo {
                registration: non_empty(row.registration),
                typecode: non_empty(row.typecode),
                operator: non_empty(row.operator),
            }));
        }
    }
    Ok(None)
}

/// Letters used in US registrations: I and O are left out (they look like 1 and 0).
const N_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
/// Addresses for an optional one- or two-letter suffix: none, A, AA..AZ, B, ...
const N_SUFFIXES: u32 = 1 + 24 * 25;
/// Each digit position covers the suffixes after it plus ten deeper buckets;
/// the fifth position takes one digit or letter, or nothing.
const N_BUCKET4: u32 = 1 + 24 + 10;
const N_BUCKET3: u32 = 10 * N_BUCKET4 + N_SUFFIXES;
const N_BUCKET2: u32 = 10 * N_BUCKET3 + N_SUFFIXES;
const N_BUCKET1: u32 = 10 * N_BUCKET2 + N_SUFFIXES;

/// The US registration (N-number) for a transponder address. The FAA hands
/// out addresses A00001 (N1) to ADF7C7 (N99999) in N-number order, so it can
/// be computed for airframes too new for the aircraft database.
pub fn us_registration(icao24: &str) -> Option<String> {
    let addr = u32::from_str_radix(icao24.trim(), 16).ok()?;
    let mut rest = addr.checked_sub(0xA00001).filter(|&o| o < 9 * N_BUCKET1)?;
    let suffix = |o: u32| match o {
        0 => String::new(),
        _ => {
            let first = N_LETTERS[((o - 1) / 25) as usize] as char;
            match (o - 1) % 25 {
                0 => first.to_string(),
                r => format!("{first}{}", N_LETTERS[r as usize - 1] as char),
            }
        }
    };

    let mut reg = format!("N{}", rest / N_BUCKET1 + 1);
    rest %= N_BUCKET1;
    for bucket in [N_BUCKET2, N_BUCKET3, N_BUCKET4] {
        if rest < N_SUFFIXES {
            return Some(reg + &suffix(rest));
        }
        rest -= N_SUFFIXES;
        reg.push_str(&(rest / bucket).to_string());
        rest %= bucket;
    }
    match rest {
        0 => {}
        1..=24 => reg.push(N_LETTERS[rest as usize - 1] as char),
        _ => reg.push_str(&(rest - 25).to_string()),
    }
    Some(reg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#""icao24","registration","manufacturericao","typecode","operator","notes"
"aa3487","N757F","RAYTHEON","BE36","",""
"406B2F","G-TTNA","AIRBUS","A20N","British Airways",""
"#;

    #[test]
    fn finds_airframe_case_insensitively() {
        let info = lookup_in(SAMPLE.as_bytes(), "406b2f").unwrap().unwrap();
        assert_eq!(info.registration.as_deref(), Some("G-TTNA"));
        assert_eq!(info.typecode.as_deref(), Some("A20N"));
        assert_eq!(info.operator.as_deref(), Some("British Airways"));
    }

    #[test]
    fn empty_fields_become_none() {
        let info = lookup_in(SAMPLE.as_bytes(), "AA3487").unwrap().unwrap();
        assert_eq!(info.operator, None);
        assert_eq!(lookup_in(SAMPLE.as_bytes(), "000000").unwrap(), None);
    }

    #[test]
    fn computes_us_registrations() {
        assert_eq!(us_registration("a00001").as_deref(), Some("N1"));
        assert_eq!(us_registration("ADF7C7").as_deref(), Some("N99999"));
        // Southwest's 737 MAX 8, checked against the FAA register.
        assert_eq!(us_registration("ac3b78").as_deref(), Some("N8879Q"));
        assert_eq!(us_registration("406b2f"), None);
        assert_eq!(us_registration("adf7c8"), None);
    }
}
