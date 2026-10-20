use serde::{Deserialize, Serialize};

use crate::airports::Airport;
use crate::geo::{distance_km, initial_bearing_deg, Compass};

/// Guesses within this distance of the answer count as "close".
pub const CLOSE_KM: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Correct,
    /// Same country, or within [`CLOSE_KM`].
    Close,
    Wrong,
}

impl Verdict {
    pub fn emoji(self) -> char {
        match self {
            Verdict::Correct => '🟩',
            Verdict::Close => '🟨',
            Verdict::Wrong => '⬛',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StopFeedback {
    pub verdict: Verdict,
    pub distance_km: f64,
    /// Direction from the guess towards the answer; `None` when correct.
    pub direction: Option<Compass>,
}

pub fn grade_stop(guess: &Airport, answer: &Airport) -> StopFeedback {
    if guess.icao == answer.icao {
        return StopFeedback { verdict: Verdict::Correct, distance_km: 0.0, direction: None };
    }
    let distance = distance_km(guess.coord, answer.coord);
    let verdict = if guess.country == answer.country || distance <= CLOSE_KM {
        Verdict::Close
    } else {
        Verdict::Wrong
    };
    StopFeedback {
        verdict,
        distance_km: distance,
        direction: Some(Compass::from_bearing(initial_bearing_deg(guess.coord, answer.coord))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::Coord;

    fn airport(icao: &str, country: &str, lat: f64, lon: f64) -> Airport {
        Airport {
            icao: icao.into(),
            iata: String::new(),
            name: String::new(),
            city: String::new(),
            country: country.into(),
            coord: Coord::new(lat, lon),
            tz: chrono_tz::UTC,
        }
    }

    #[test]
    fn verdicts() {
        let agp = airport("LEMG", "ES", 36.675, -4.499);
        let alc = airport("LEAL", "ES", 38.282, -0.558);
        let gib = airport("LXGB", "GI", 36.151, -5.349);
        let cdg = airport("LFPG", "FR", 49.010, 2.548);

        assert_eq!(grade_stop(&agp, &agp).verdict, Verdict::Correct);
        assert_eq!(grade_stop(&alc, &agp).verdict, Verdict::Close); // same country
        assert_eq!(grade_stop(&gib, &agp).verdict, Verdict::Close); // ~95 km, across a border
        let paris = grade_stop(&cdg, &agp);
        assert_eq!(paris.verdict, Verdict::Wrong);
        assert_eq!(paris.direction, Some(Compass::SW));
    }
}
