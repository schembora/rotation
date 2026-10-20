//! Minimal client for the OpenSky Network REST API's historical flight endpoints.
//! These need an account: create an API client on your OpenSky account page and
//! save its credentials file locally (never commit it).

use std::fmt;
use std::path::Path;

use chrono::NaiveDate;
use serde::Deserialize;

const TOKEN_URL: &str = "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token";
const API_URL: &str = "https://opensky-network.org/api";

#[derive(Debug, Deserialize)]
pub struct Credentials {
    #[serde(alias = "clientId")]
    pub client_id: String,
    #[serde(alias = "clientSecret")]
    pub client_secret: String,
}

impl Credentials {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(Error::Credentials)?;
        serde_json::from_str(&text).map_err(Error::CredentialsFormat)
    }
}

/// One flight as OpenSky reports it. Times are when receivers first and last
/// heard the aircraft, not gate times; airports are inferred from position.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flight {
    pub icao24: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub est_departure_airport: Option<String>,
    pub est_arrival_airport: Option<String>,
    pub callsign: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    Credentials(std::io::Error),
    CredentialsFormat(serde_json::Error),
    Http(ureq::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Credentials(e) => write!(
                f,
                "reading OpenSky credentials: {e}. Create an API client on your OpenSky account page and save its credentials.json as opensky-credentials.json in the project root"
            ),
            Error::CredentialsFormat(e) => write!(f, "OpenSky credentials file should hold clientId and clientSecret: {e}"),
            Error::Http(ureq::Error::StatusCode(401 | 403)) => {
                write!(f, "OpenSky refused the request; check the credentials file and that the API client is active")
            }
            Error::Http(ureq::Error::StatusCode(429)) => write!(f, "OpenSky rate limit reached; try again later"),
            Error::Http(e) => write!(f, "OpenSky request failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Error::Http(e)
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

pub struct Client {
    token: String,
}

impl Client {
    pub fn login(creds: &Credentials) -> Result<Self, Error> {
        let token: Token = ureq::post(TOKEN_URL)
            .send_form([
                ("grant_type", "client_credentials"),
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
            ])?
            .body_mut()
            .read_json()?;
        Ok(Self { token: token.access_token })
    }

    /// Every flight by one airframe that started during the given UTC day.
    pub fn flights_by_aircraft(&self, icao24: &str, date: NaiveDate) -> Result<Vec<Flight>, Error> {
        self.flights("aircraft", "icao24", &icao24.to_ascii_lowercase(), date)
    }

    /// Every flight that departed the given airport (ICAO code) during the UTC day.
    pub fn departures(&self, airport: &str, date: NaiveDate) -> Result<Vec<Flight>, Error> {
        self.flights("departure", "airport", &airport.to_ascii_uppercase(), date)
    }

    fn flights(&self, endpoint: &str, key: &str, value: &str, date: NaiveDate) -> Result<Vec<Flight>, Error> {
        let (begin, end) = day_bounds(date);
        let result = ureq::get(format!("{API_URL}/flights/{endpoint}"))
            .header("Authorization", format!("Bearer {}", self.token))
            .query(key, value)
            .query("begin", begin.to_string())
            .query("end", end.to_string())
            .call();
        match result {
            Ok(mut resp) => Ok(resp.body_mut().read_json()?),
            // OpenSky answers 404 when nothing matched.
            Err(ureq::Error::StatusCode(404)) => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Unix seconds for the start and end of a UTC day.
pub fn day_bounds(date: NaiveDate) -> (i64, i64) {
    let begin = date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    (begin, begin + 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_flight_shape() {
        let json = r#"[{"icao24":"406b2f","firstSeen":1792479600,"estDepartureAirport":"EGLL",
            "lastSeen":1792484100,"estArrivalAirport":"LFPG","callsign":"BAW304  ",
            "estDepartureAirportHorizDistance":1200,"estDepartureAirportVertDistance":40,
            "estArrivalAirportHorizDistance":900,"estArrivalAirportVertDistance":30,
            "departureAirportCandidatesCount":1,"arrivalAirportCandidatesCount":2}]"#;
        let flights: Vec<Flight> = serde_json::from_str(json).unwrap();
        assert_eq!(flights[0].est_arrival_airport.as_deref(), Some("LFPG"));
        assert_eq!(flights[0].callsign.as_deref(), Some("BAW304  "));
    }

    #[test]
    fn credentials_accept_both_spellings() {
        let a: Credentials = serde_json::from_str(r#"{"clientId":"a","clientSecret":"b"}"#).unwrap();
        let b: Credentials = serde_json::from_str(r#"{"client_id":"a","client_secret":"b"}"#).unwrap();
        assert_eq!((a.client_id, b.client_secret), ("a".to_string(), "b".to_string()));
    }

    #[test]
    fn day_bounds_cover_one_utc_day() {
        let (begin, end) = day_bounds(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap());
        assert_eq!(begin, 1_792_454_400);
        assert_eq!(end - begin, 86_400);
    }
}
