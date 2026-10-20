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
}
