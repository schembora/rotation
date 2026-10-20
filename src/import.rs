//! Turns raw OpenSky flights into a validated [`Rotation`].

use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};

use crate::aircraft::AircraftInfo;
use crate::model::{Leg, Rotation, RotationError};
use crate::opensky::Flight;

/// Clue times are rounded to this many seconds. OpenSky's first/last-seen
/// times are only approximate, so minute precision would be false precision.
const ROUND_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    NoFlights,
    /// OpenSky couldn't tell which airport flight `n` used.
    UnknownAirport(usize),
    Invalid(RotationError),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NoFlights => write!(f, "no flights that day"),
            ImportError::UnknownAirport(n) => write!(f, "flight {} has no departure or arrival airport", n + 1),
            ImportError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ImportError {}

fn rounded(unix: i64) -> DateTime<Utc> {
    let secs = (unix + ROUND_SECS / 2).div_euclid(ROUND_SECS) * ROUND_SECS;
    DateTime::from_timestamp(secs, 0).expect("OpenSky timestamps are in range")
}

/// Builds one airframe's day from its flights. Ground-only "flights" (same
/// departure and arrival airport) are dropped; anything else odd, such as a
/// missing airport or a gap in the chain, rejects the day.
pub fn rotation_from_flights(
    date: NaiveDate,
    mut flights: Vec<Flight>,
    info: Option<&AircraftInfo>,
) -> Result<Rotation, ImportError> {
    flights.sort_by_key(|f| f.first_seen);
    let mut legs = Vec::new();
    for (i, f) in flights.iter().enumerate() {
        let (Some(from), Some(to)) = (&f.est_departure_airport, &f.est_arrival_airport) else {
            return Err(ImportError::UnknownAirport(i));
        };
        if from == to {
            continue;
        }
        legs.push(Leg {
            from: from.clone(),
            to: to.clone(),
            departure: rounded(f.first_seen),
            arrival: rounded(f.last_seen),
            callsign: f.callsign.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(String::from),
        });
    }
    if legs.is_empty() {
        return Err(ImportError::NoFlights);
    }

    let rotation = Rotation {
        date,
        aircraft_type: info.and_then(|i| i.typecode.clone()).unwrap_or_default(),
        registration: info.and_then(|i| i.registration.clone()),
        icao24: flights.first().map(|f| f.icao24.to_ascii_lowercase()),
        operator: info.and_then(|i| i.operator.clone()),
        legs,
        sample: false,
    };
    rotation.validate().map_err(ImportError::Invalid)?;
    Ok(rotation)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    // 2026-10-20T07:00:00Z
    const T0: i64 = 1_792_479_600;

    fn flight(from: Option<&str>, to: Option<&str>, first: i64, last: i64, callsign: &str) -> Flight {
        Flight {
            icao24: "406B2F".into(),
            first_seen: first,
            last_seen: last,
            est_departure_airport: from.map(String::from),
            est_arrival_airport: to.map(String::from),
            callsign: Some(callsign.into()),
        }
    }

    fn info() -> AircraftInfo {
        AircraftInfo {
            registration: Some("G-TTNA".into()),
            typecode: Some("A20N".into()),
            operator: Some("British Airways".into()),
        }
    }

    #[test]
    fn builds_rounded_rotation_in_time_order() {
        let flights = vec![
            flight(Some("LFPG"), Some("EGLL"), T0 + 7_520, T0 + 12_290, "BAW305  "),
            flight(Some("EGLL"), Some("LFPG"), T0 + 70, T0 + 4_480, "BAW304  "),
        ];
        let r = rotation_from_flights(DATE, flights, Some(&info())).unwrap();
        assert_eq!(r.stops(), ["EGLL", "LFPG", "EGLL"]);
        assert_eq!(r.legs[0].departure.to_rfc3339(), "2026-10-20T07:00:00+00:00");
        assert_eq!(r.legs[0].arrival.to_rfc3339(), "2026-10-20T08:15:00+00:00");
        assert_eq!(r.legs[0].callsign.as_deref(), Some("BAW304"));
        assert_eq!(r.icao24.as_deref(), Some("406b2f"));
        assert_eq!(r.aircraft_type, "A20N");
        assert_eq!(r.registration.as_deref(), Some("G-TTNA"));
    }

    #[test]
    fn drops_ground_only_flights() {
        let flights = vec![
            flight(Some("EGLL"), Some("EGLL"), T0 - 1_800, T0 - 1_200, "BAW304"),
            flight(Some("EGLL"), Some("LFPG"), T0, T0 + 4_500, "BAW304"),
        ];
        let r = rotation_from_flights(DATE, flights, None).unwrap();
        assert_eq!(r.legs.len(), 1);
        assert_eq!(r.aircraft_type, "");
    }

    #[test]
    fn rejects_messy_days() {
        assert_eq!(rotation_from_flights(DATE, vec![], None), Err(ImportError::NoFlights));
        assert_eq!(
            rotation_from_flights(DATE, vec![flight(Some("EGLL"), None, T0, T0 + 4_500, "X")], None),
            Err(ImportError::UnknownAirport(0))
        );
        let gap = vec![
            flight(Some("EGLL"), Some("LFPG"), T0, T0 + 4_500, "X"),
            flight(Some("EDDF"), Some("EGLL"), T0 + 9_000, T0 + 15_000, "X"),
        ];
        assert_eq!(
            rotation_from_flights(DATE, gap, None),
            Err(ImportError::Invalid(RotationError::Disconnected(1)))
        );
    }
}
