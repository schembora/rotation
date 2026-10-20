use std::fmt;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::airports::AirportDb;
use crate::links;

/// A single flight. Airports are stored as ICAO codes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leg {
    pub from: String,
    pub to: String,
    pub departure: DateTime<Utc>,
    pub arrival: DateTime<Utc>,
    /// ATC callsign as broadcast, e.g. `BAW304`.
    #[serde(default)]
    pub callsign: Option<String>,
}

impl Leg {
    pub fn block_time(&self) -> TimeDelta {
        self.arrival - self.departure
    }
}

/// One airframe's flying for one day: the raw material for a puzzle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    pub date: NaiveDate,
    /// ICAO type designator, e.g. `A20N`, `B738`.
    pub aircraft_type: String,
    pub registration: Option<String>,
    /// 24-bit transponder address in hex, e.g. `406b2f`.
    #[serde(default)]
    pub icao24: Option<String>,
    pub operator: Option<String>,
    pub legs: Vec<Leg>,
    /// A made-up day for demos rather than one flown for real.
    #[serde(default)]
    pub sample: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationError {
    NoLegs,
    /// Leg `n` arrives at or before it departs.
    NonPositiveBlockTime(usize),
    /// Leg `n` departs from somewhere other than where leg `n - 1` landed.
    Disconnected(usize),
    /// Leg `n` departs before leg `n - 1` has arrived.
    Overlap(usize),
}

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RotationError::NoLegs => write!(f, "rotation has no legs"),
            RotationError::NonPositiveBlockTime(n) => write!(f, "leg {n} has non-positive block time"),
            RotationError::Disconnected(n) => write!(f, "leg {n} does not depart from previous arrival"),
            RotationError::Overlap(n) => write!(f, "leg {n} departs before previous leg arrives"),
        }
    }
}

impl std::error::Error for RotationError {}

impl Rotation {
    /// Every airport the aircraft touches, in order: origin, then each arrival.
    pub fn stops(&self) -> Vec<&str> {
        let Some(first) = self.legs.first() else {
            return Vec::new();
        };
        std::iter::once(first.from.as_str())
            .chain(self.legs.iter().map(|leg| leg.to.as_str()))
            .collect()
    }

    pub fn validate(&self) -> Result<(), RotationError> {
        if self.legs.is_empty() {
            return Err(RotationError::NoLegs);
        }
        for (i, leg) in self.legs.iter().enumerate() {
            if leg.block_time() <= TimeDelta::zero() {
                return Err(RotationError::NonPositiveBlockTime(i));
            }
            if let Some(prev) = i.checked_sub(1).map(|p| &self.legs[p]) {
                if leg.from != prev.to {
                    return Err(RotationError::Disconnected(i));
                }
                if leg.departure < prev.arrival {
                    return Err(RotationError::Overlap(i));
                }
            }
        }
        Ok(())
    }
}

/// What the player is shown about one leg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegClue {
    /// Departure in the origin airport's local time. Comparing it with the
    /// previous leg's block and ground time reveals time-zone crossings.
    pub departure_local: NaiveTime,
    pub block_minutes: i64,
    /// `None` for the final leg.
    pub ground_minutes_after: Option<i64>,
}

/// A day's published puzzle: clues for the player plus the hidden answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Puzzle {
    pub number: u32,
    /// The day the puzzle is published, not the day it was flown.
    pub date: NaiveDate,
    pub aircraft_type: String,
    /// Airline name, offered to the player as a hint.
    pub operator: Option<String>,
    pub clues: Vec<LegClue>,
    /// ICAO codes of every stop, origin first.
    pub answer: Vec<String>,
    pub reveal: Reveal,
    pub sample: bool,
}

/// Shown once the game ends: which aircraft this was, with links to look the
/// day up on flight trackers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reveal {
    pub flown: NaiveDate,
    pub registration: Option<String>,
    pub links: Vec<links::Link>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PuzzleError {
    InvalidRotation(RotationError),
    UnknownAirport(String),
}

impl fmt::Display for PuzzleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PuzzleError::InvalidRotation(e) => write!(f, "invalid rotation: {e}"),
            PuzzleError::UnknownAirport(code) => write!(f, "unknown airport {code}"),
        }
    }
}

impl std::error::Error for PuzzleError {}

impl From<RotationError> for PuzzleError {
    fn from(e: RotationError) -> Self {
        PuzzleError::InvalidRotation(e)
    }
}

impl Puzzle {
    pub fn from_rotation(
        number: u32,
        date: NaiveDate,
        rotation: &Rotation,
        airports: &AirportDb,
    ) -> Result<Self, PuzzleError> {
        rotation.validate()?;
        let legs = &rotation.legs;
        let clues = legs
            .iter()
            .enumerate()
            .map(|(i, leg)| {
                let origin = airports
                    .get(&leg.from)
                    .ok_or_else(|| PuzzleError::UnknownAirport(leg.from.clone()))?;
                Ok(LegClue {
                    departure_local: leg.departure.with_timezone(&origin.tz).time(),
                    block_minutes: leg.block_time().num_minutes(),
                    ground_minutes_after: legs.get(i + 1).map(|next| (next.departure - leg.arrival).num_minutes()),
                })
            })
            .collect::<Result<_, PuzzleError>>()?;
        if let Some(last) = legs.last().filter(|leg| airports.get(&leg.to).is_none()) {
            return Err(PuzzleError::UnknownAirport(last.to.clone()));
        }
        Ok(Puzzle {
            number,
            date,
            aircraft_type: rotation.aircraft_type.clone(),
            operator: rotation.operator.clone(),
            clues,
            answer: rotation.stops().into_iter().map(String::from).collect(),
            reveal: Reveal {
                flown: rotation.date,
                registration: rotation.registration.clone(),
                links: links::for_rotation(rotation),
            },
            sample: rotation.sample,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg(from: &str, to: &str, dep: &str, arr: &str) -> Leg {
        Leg {
            from: from.into(),
            to: to.into(),
            departure: dep.parse().unwrap(),
            arrival: arr.parse().unwrap(),
            callsign: None,
        }
    }

    fn rotation(legs: Vec<Leg>) -> Rotation {
        Rotation {
            date: NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
            aircraft_type: "A20N".into(),
            registration: None,
            icao24: None,
            operator: None,
            legs,
            sample: false,
        }
    }

    fn out_and_back() -> Rotation {
        rotation(vec![
            leg("EGKK", "LEMG", "2026-10-19T05:10:00Z", "2026-10-19T07:45:00Z"),
            leg("LEMG", "EGKK", "2026-10-19T08:30:00Z", "2026-10-19T11:05:00Z"),
        ])
    }

    #[test]
    fn stops_include_origin() {
        assert_eq!(out_and_back().stops(), ["EGKK", "LEMG", "EGKK"]);
    }

    #[test]
    fn valid_rotation_passes() {
        assert_eq!(out_and_back().validate(), Ok(()));
    }

    #[test]
    fn detects_broken_rotations() {
        assert_eq!(rotation(vec![]).validate(), Err(RotationError::NoLegs));

        let mut r = out_and_back();
        r.legs[1].from = "LEPA".into();
        assert_eq!(r.validate(), Err(RotationError::Disconnected(1)));

        let mut r = out_and_back();
        r.legs[1].departure = "2026-10-19T07:00:00Z".parse().unwrap();
        assert_eq!(r.validate(), Err(RotationError::Overlap(1)));

        let mut r = out_and_back();
        r.legs[0].arrival = r.legs[0].departure;
        assert_eq!(r.validate(), Err(RotationError::NonPositiveBlockTime(0)));
    }

    const AIRPORTS: &str = r#""ident","type","name","latitude_deg","longitude_deg","iso_country","municipality","scheduled_service","icao_code","iata_code"
"EGKK","large_airport","London Gatwick Airport",51.148102,-0.190278,"GB","London","yes","EGKK","LGW"
"LEMG","large_airport","Malaga-Costa del Sol Airport",36.6749,-4.49911,"ES","Malaga","yes","LEMG","AGP"
"#;

    fn hhmm(s: &str) -> NaiveTime {
        NaiveTime::parse_from_str(s, "%H:%M").unwrap()
    }

    #[test]
    fn puzzle_clues_use_local_departure_times() {
        let db = AirportDb::from_reader(AIRPORTS.as_bytes()).unwrap();
        let r = out_and_back();
        let p = Puzzle::from_rotation(1, r.date, &r, &db).unwrap();
        // London is on BST (UTC+1) and Malaga on CEST (UTC+2) in mid-October,
        // so the 45 min turn shows up as a 06:10 -> 10:30 local gap of 4h20.
        assert_eq!(
            p.clues,
            [
                LegClue { departure_local: hhmm("06:10"), block_minutes: 155, ground_minutes_after: Some(45) },
                LegClue { departure_local: hhmm("10:30"), block_minutes: 155, ground_minutes_after: None },
            ]
        );
        assert_eq!(p.answer, ["EGKK", "LEMG", "EGKK"]);
    }

    #[test]
    fn puzzle_rejects_unknown_airport() {
        let db = AirportDb::from_reader(AIRPORTS.as_bytes()).unwrap();
        let mut r = out_and_back();
        r.legs[1].to = "LEPA".into();
        r.legs.push(leg("LEPA", "EGKK", "2026-10-19T12:00:00Z", "2026-10-19T14:20:00Z"));
        assert_eq!(
            Puzzle::from_rotation(1, r.date, &r, &db),
            Err(PuzzleError::UnknownAirport("LEPA".into()))
        );
    }
}
