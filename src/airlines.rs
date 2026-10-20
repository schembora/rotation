//! Airline names by ICAO code, from OpenFlights' `airlines.dat`. Used to name
//! the operator from a callsign when the aircraft database doesn't say.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Columns: id, name, alias, iata, icao, callsign, country, active ("Y"/"N").
/// Missing values are written as `\N`.
type Row = (String, String, String, String, String, String, String, String);

#[derive(Debug, Default)]
pub struct AirlineDb {
    by_icao: HashMap<String, String>,
}

impl AirlineDb {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, csv::Error> {
        Self::from_reader(std::fs::File::open(path)?)
    }

    /// Several airlines can share a code over time; an active one wins over a
    /// defunct one, otherwise the first listed is kept.
    pub fn from_reader(reader: impl Read) -> Result<Self, csv::Error> {
        let mut by_icao: HashMap<String, (String, bool)> = HashMap::new();
        let rows = csv::ReaderBuilder::new().has_headers(false).from_reader(reader);
        for row in rows.into_deserialize::<Row>() {
            let (_, name, _, _, icao, _, _, active) = row?;
            if icao.len() != 3 || !icao.chars().all(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let active = active == "Y";
            let entry = by_icao.entry(icao).or_insert_with(|| (name.clone(), active));
            if active && !entry.1 {
                *entry = (name, true);
            }
        }
        let by_icao = by_icao.into_iter().map(|(icao, (name, _))| (icao, name)).collect();
        Ok(Self { by_icao })
    }

    /// Airline for an airline-style callsign such as `BAW304` or `EZY49EV`.
    pub fn for_callsign(&self, callsign: &str) -> Option<&str> {
        let prefix = callsign.trim().get(..3)?.to_ascii_uppercase();
        self.by_icao.get(&prefix).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"1355,"British Airways",\N,"BA","BAW","SPEEDBIRD","United Kingdom","Y"
9000,"Old Speedbird Co",\N,"","OLD","OLDIE","United Kingdom","N"
9001,"New Speedbird Co",\N,"","OLD","NEWBIE","United Kingdom","Y"
2,"135 Airways",\N,"","GNL","GENERAL","United States","N"
1,"Private flight",\N,"-","N/A","","","Y"
"#;

    #[test]
    fn names_airline_from_callsign() {
        let db = AirlineDb::from_reader(SAMPLE.as_bytes()).unwrap();
        assert_eq!(db.for_callsign("BAW304 "), Some("British Airways"));
        assert_eq!(db.for_callsign("gnl12"), Some("135 Airways"));
        assert_eq!(db.for_callsign("XX"), None);
    }

    #[test]
    fn active_airline_wins_shared_code() {
        let db = AirlineDb::from_reader(SAMPLE.as_bytes()).unwrap();
        assert_eq!(db.for_callsign("OLD1"), Some("New Speedbird Co"));
    }
}
