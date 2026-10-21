use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::model::Rotation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

// No per-flight FlightAware links: those URLs need the flight number and the
// scheduled departure time, and OpenSky only gives the ATC callsign (BAW7LN
// rather than BAW572) and when the aircraft was first heard.

/// FlightAware's page for an airframe, keyed by registration without the dash.
/// It lists the airframe's recent flights, including the puzzle's.
pub fn flightaware_tail(registration: &str) -> String {
    format!(
        "https://www.flightaware.com/live/flight/{}",
        registration.replace('-', "").to_ascii_uppercase()
    )
}

/// ADS-B Exchange's track replay for one airframe over one UTC day.
pub fn adsbx_trace(icao24: &str, date: NaiveDate) -> String {
    format!(
        "https://globe.adsbexchange.com/?icao={}&showTrace={}",
        icao24.trim().to_ascii_lowercase(),
        date.format("%Y-%m-%d"),
    )
}

/// Every link we can build from the identifiers the rotation carries:
/// the day's replay first, then the airframe.
pub fn for_rotation(rotation: &Rotation) -> Vec<Link> {
    let replay = rotation.icao24.as_deref().map(|hex| Link {
        label: "Replay the whole day on ADS-B Exchange".into(),
        url: adsbx_trace(hex, rotation.date),
    });
    let tail = rotation.registration.as_deref().map(|reg| Link {
        label: format!("{reg} on FlightAware"),
        url: flightaware_tail(reg),
    });
    replay.into_iter().chain(tail).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Leg;

    fn leg(callsign: Option<&str>) -> Leg {
        Leg {
            from: "EGLL".into(),
            to: "LFPG".into(),
            departure: "2026-10-20T07:00:00Z".parse().unwrap(),
            arrival: "2026-10-20T08:15:00Z".parse().unwrap(),
            callsign: callsign.map(String::from),
        }
    }

    fn rotation(legs: Vec<Leg>, registration: Option<&str>, icao24: Option<&str>) -> Rotation {
        Rotation {
            date: NaiveDate::from_ymd_opt(2026, 10, 20).unwrap(),
            aircraft_type: "A20N".into(),
            registration: registration.map(String::from),
            icao24: icao24.map(String::from),
            operator: None,
            legs,
            sample: false,
        }
    }

    #[test]
    fn url_formats() {
        assert_eq!(flightaware_tail("G-TTNA"), "https://www.flightaware.com/live/flight/GTTNA");
        assert_eq!(
            adsbx_trace("406B2F", NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
            "https://globe.adsbexchange.com/?icao=406b2f&showTrace=2026-10-20"
        );
    }

    #[test]
    fn links_follow_available_identifiers() {
        let full = rotation(vec![leg(Some("BAW304")), leg(None)], Some("G-TTNA"), Some("406b2f"));
        let labels: Vec<_> = for_rotation(&full).into_iter().map(|l| l.label).collect();
        assert_eq!(labels, ["Replay the whole day on ADS-B Exchange", "G-TTNA on FlightAware"]);

        assert!(for_rotation(&rotation(vec![leg(None)], None, None)).is_empty());
    }
}
