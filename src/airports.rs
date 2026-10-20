use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::Path;

use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tzf_rs::DefaultFinder;

use crate::geo::Coord;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Airport {
    pub icao: String,
    pub iata: String,
    pub name: String,
    pub city: String,
    pub country: String,
    pub coord: Coord,
    pub tz: Tz,
}

/// One row of OurAirports' `airports.csv`. Only the columns we use are listed;
/// the rest are ignored by header name, so column order changes are harmless.
#[derive(Debug, Deserialize)]
struct RawAirport {
    ident: String,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    latitude_deg: Option<f64>,
    longitude_deg: Option<f64>,
    iso_country: String,
    municipality: String,
    scheduled_service: String,
    icao_code: String,
    iata_code: String,
}

impl RawAirport {
    /// Keep only airports a passenger airliner could plausibly be scheduled into.
    fn into_airport(self, tz_finder: &DefaultFinder) -> Option<Airport> {
        let excluded = matches!(self.kind.as_str(), "closed" | "heliport" | "balloonport");
        if excluded || self.scheduled_service != "yes" || self.iata_code.is_empty() {
            return None;
        }
        let coord = Coord::new(self.latitude_deg?, self.longitude_deg?);
        let tz = tz_finder.get_tz_name(coord.lon, coord.lat).parse().ok()?;
        let icao = if self.icao_code.is_empty() { self.ident } else { self.icao_code };
        Some(Airport {
            icao,
            iata: self.iata_code,
            name: self.name,
            city: self.municipality,
            country: self.iso_country,
            coord,
            tz,
        })
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Csv(csv::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "reading airports file: {e}"),
            LoadError::Csv(e) => write!(f, "parsing airports csv: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        LoadError::Io(e)
    }
}

impl From<csv::Error> for LoadError {
    fn from(e: csv::Error) -> Self {
        LoadError::Csv(e)
    }
}

/// Airports indexed by both IATA and ICAO code.
#[derive(Debug, Default)]
pub struct AirportDb {
    airports: Vec<Airport>,
    by_code: HashMap<String, usize>,
}

impl AirportDb {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        Self::from_reader(std::fs::File::open(path)?)
    }

    pub fn from_reader(reader: impl Read) -> Result<Self, LoadError> {
        let mut db = Self::default();
        let tz_finder = DefaultFinder::new();
        for row in csv::Reader::from_reader(reader).deserialize::<RawAirport>() {
            if let Some(airport) = row?.into_airport(&tz_finder) {
                db.insert(airport);
            }
        }
        Ok(db)
    }

    /// First airport to claim a code keeps it, should the source data ever
    /// contain duplicates (the filtered OurAirports set currently has none).
    fn insert(&mut self, airport: Airport) {
        let idx = self.airports.len();
        self.by_code.entry(airport.iata.clone()).or_insert(idx);
        self.by_code.entry(airport.icao.clone()).or_insert(idx);
        self.airports.push(airport);
    }

    /// Look up by IATA (`LHR`) or ICAO (`EGLL`) code, case-insensitively.
    pub fn get(&self, code: &str) -> Option<&Airport> {
        let idx = *self.by_code.get(&code.trim().to_ascii_uppercase())?;
        Some(&self.airports[idx])
    }

    pub fn len(&self) -> usize {
        self.airports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.airports.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Airport> {
        self.airports.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#""id","ident","type","name","latitude_deg","longitude_deg","elevation_ft","continent","iso_country","iso_region","municipality","scheduled_service","icao_code","iata_code","gps_code","local_code","home_link","wikipedia_link","keywords"
2434,"EGLL","large_airport","London Heathrow Airport",51.470748,-0.459909,83,"EU","GB","GB-ENG","London","yes","EGLL","LHR","EGLL",,,,
1,"XHEL","heliport","Some Heliport",51.0,0.0,0,"EU","GB","GB-ENG","Nowhere","yes","","XHL","",,,,
2,"XPRV","small_airport","Private Strip",51.0,0.0,0,"EU","GB","GB-ENG","Nowhere","no","","","",,,,
3,"KJFK","large_airport","John F Kennedy International Airport",40.639447,-73.779317,13,"NA","US","US-NY","New York","yes","KJFK","JFK","KJFK",,,,
"#;

    #[test]
    fn loads_and_filters() {
        let db = AirportDb::from_reader(SAMPLE.as_bytes()).unwrap();
        assert_eq!(db.len(), 2);
        assert!(db.get("XHL").is_none());
    }

    #[test]
    fn lookup_by_either_code() {
        let db = AirportDb::from_reader(SAMPLE.as_bytes()).unwrap();
        let lhr = db.get("lhr").unwrap();
        assert_eq!(lhr.icao, "EGLL");
        assert_eq!(lhr.city, "London");
        assert_eq!(lhr.tz, chrono_tz::Europe::London);
        assert_eq!(db.get("KJFK").unwrap().iata, "JFK");
    }
}
