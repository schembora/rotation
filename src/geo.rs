use serde::{Deserialize, Serialize};

/// Mean Earth radius in kilometres (IUGG).
const EARTH_RADIUS_KM: f64 = 6371.0088;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coord {
    pub lat: f64,
    pub lon: f64,
}

impl Coord {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

/// Great-circle distance between two points, via the haversine formula.
pub fn distance_km(a: Coord, b: Coord) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

/// Initial bearing from `a` towards `b`, in degrees clockwise from north (0..360).
pub fn initial_bearing_deg(a: Coord, b: Coord) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlon = (b.lon - a.lon).to_radians();
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compass {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Compass {
    pub fn from_bearing(deg: f64) -> Self {
        const POINTS: [Compass; 8] = [
            Compass::N,
            Compass::NE,
            Compass::E,
            Compass::SE,
            Compass::S,
            Compass::SW,
            Compass::W,
            Compass::NW,
        ];
        let idx = ((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8;
        POINTS[idx]
    }

    pub fn arrow(self) -> char {
        match self {
            Compass::N => '⬆',
            Compass::NE => '↗',
            Compass::E => '➡',
            Compass::SE => '↘',
            Compass::S => '⬇',
            Compass::SW => '↙',
            Compass::W => '⬅',
            Compass::NW => '↖',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LHR: Coord = Coord { lat: 51.4707, lon: -0.4599 };
    const JFK: Coord = Coord { lat: 40.6398, lon: -73.7789 };

    #[test]
    fn lhr_to_jfk_distance() {
        let d = distance_km(LHR, JFK);
        assert!((d - 5540.0).abs() < 15.0, "got {d}");
    }

    #[test]
    fn distance_to_self_is_zero() {
        assert_eq!(distance_km(LHR, LHR), 0.0);
    }

    #[test]
    fn bearings() {
        // Great-circle route to New York leaves London heading west-northwest.
        let b = initial_bearing_deg(LHR, JFK);
        assert!((b - 288.0).abs() < 2.0, "got {b}");
        assert_eq!(Compass::from_bearing(b), Compass::W);

        let due_east = initial_bearing_deg(Coord::new(0.0, 0.0), Coord::new(0.0, 10.0));
        assert!((due_east - 90.0).abs() < 1e-9);
    }

    #[test]
    fn compass_boundaries() {
        assert_eq!(Compass::from_bearing(0.0), Compass::N);
        assert_eq!(Compass::from_bearing(22.4), Compass::N);
        assert_eq!(Compass::from_bearing(22.6), Compass::NE);
        assert_eq!(Compass::from_bearing(359.0), Compass::N);
        assert_eq!(Compass::from_bearing(-90.0), Compass::W);
    }
}
