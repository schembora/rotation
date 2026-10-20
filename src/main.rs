use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use chrono::NaiveDate;
use daily::aircraft;
use daily::airlines::AirlineDb;
use daily::airports::AirportDb;
use daily::grade::grade_stop;
use daily::import::rotation_from_flights;
use daily::model::{Puzzle, Rotation};
use daily::opensky::{Client, Credentials};
use serde::Serialize;

const USAGE: &str = "usage:
  daily grade  <airports.csv> <guess> <answer>
  daily build  <airports.csv> <puzzles_dir> <out_dir>
  daily scout  <airport-icao> <yyyy-mm-dd> [callsign-prefix]
  daily fetch  <icao24> <yyyy-mm-dd> <rotation.json>";

const AIRPORTS_CSV: &str = "data/airports.csv";
const AIRCRAFT_CSV: &str = "data/aircraftDatabase.csv";
const AIRLINES_DAT: &str = "data/airlines.dat";
/// Override with the OPENSKY_CREDENTIALS environment variable.
const CREDENTIALS: &str = "opensky-credentials.json";
/// Aircraft checked per scout run, to stay well inside OpenSky's rate limits.
const SCOUT_LIMIT: usize = 30;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["grade", csv, guess, answer] => grade(csv, guess, answer),
        ["build", csv, puzzles, out_dir] => build(csv, puzzles, out_dir),
        ["scout", airport, date] => scout(airport, date, None),
        ["scout", airport, date, prefix] => scout(airport, date, Some(prefix)),
        ["fetch", icao24, date, out] => fetch(icao24, date, out),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn grade(csv: &str, guess: &str, answer: &str) -> Result<(), Box<dyn Error>> {
    let db = AirportDb::load(csv)?;
    let (Some(guess), Some(answer)) = (db.get(guess), db.get(answer)) else {
        return Err("unknown airport code".into());
    };
    let fb = grade_stop(guess, answer);
    let arrow = fb.direction.map(|d| d.arrow().to_string()).unwrap_or_default();
    println!(
        "{} {} ({}) -> {:.0} km {arrow}",
        fb.verdict.emoji(),
        guess.name,
        guess.iata,
        fb.distance_km
    );
    Ok(())
}

#[derive(Serialize)]
struct IndexEntry {
    number: u32,
    date: NaiveDate,
}

/// Turns `puzzles_dir/YYYY-MM-DD.json` rotations (one per publish date) into
/// the site's data: numbered puzzles, an index of them, and the airport list.
fn build(csv: &str, puzzles_dir: &str, out_dir: &str) -> Result<(), Box<dyn Error>> {
    let db = AirportDb::load(csv)?;

    let mut sources: Vec<(NaiveDate, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(puzzles_dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let date = stem
            .parse()
            .map_err(|_| format!("puzzle files are named by publish date, like 2026-10-21.json: {}", path.display()))?;
        sources.push((date, path));
    }
    if sources.is_empty() {
        return Err(format!("no puzzles in {puzzles_dir}").into());
    }
    sources.sort();

    let out_dir = Path::new(out_dir);
    std::fs::create_dir_all(out_dir.join("puzzles"))?;
    let mut index = Vec::new();
    for (number, (date, path)) in (1..).zip(&sources) {
        let in_file = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let rotation: Rotation = serde_json::from_str(&std::fs::read_to_string(path)?).map_err(|e| in_file(&e))?;
        let puzzle = Puzzle::from_rotation(number, *date, &rotation, &db).map_err(|e| in_file(&e))?;
        std::fs::write(out_dir.join(format!("puzzles/{date}.json")), serde_json::to_string_pretty(&puzzle)?)?;
        index.push(IndexEntry { number, date: *date });
    }
    std::fs::write(
        out_dir.join("puzzles/index.json"),
        serde_json::to_string_pretty(&serde_json::json!({ "puzzles": index }))?,
    )?;

    // [iata, icao, name, city, country, lat, lon], coordinates to ~10 m.
    let round = |x: f64| (x * 1e4).round() / 1e4;
    let airports: Vec<_> = db
        .iter()
        .map(|a| (&a.iata, &a.icao, &a.name, &a.city, &a.country, round(a.coord.lat), round(a.coord.lon)))
        .collect();
    std::fs::write(out_dir.join("airports.json"), serde_json::to_string(&airports)?)?;

    let (first, last) = (sources[0].0, sources[sources.len() - 1].0);
    println!("built {} puzzles ({first} to {last}) into {}", index.len(), out_dir.display());
    Ok(())
}

fn opensky() -> Result<Client, Box<dyn Error>> {
    let path = std::env::var("OPENSKY_CREDENTIALS").unwrap_or_else(|_| CREDENTIALS.into());
    Ok(Client::login(&Credentials::load(&path)?)?)
}

fn parse_date(date: &str) -> Result<NaiveDate, Box<dyn Error>> {
    date.parse().map_err(|_| format!("dates look like 2026-10-19, got {date:?}").into())
}

/// Lists aircraft that left `airport` on `date` and whose whole day forms a
/// clean, puzzle-ready rotation.
fn scout(airport: &str, date: &str, prefix: Option<&str>) -> Result<(), Box<dyn Error>> {
    let date = parse_date(date)?;
    let airports = AirportDb::load(AIRPORTS_CSV)?;
    let client = opensky()?;

    let prefix = prefix.map(str::to_ascii_uppercase);
    let mut candidates: Vec<(String, String)> = Vec::new();
    for f in client.departures(airport, date)? {
        let callsign = f.callsign.as_deref().unwrap_or("").trim().to_string();
        let wanted = prefix.as_deref().is_none_or(|p| callsign.starts_with(p));
        if wanted && !candidates.iter().any(|(icao, _)| *icao == f.icao24) {
            candidates.push((f.icao24, callsign));
        }
    }
    println!(
        "{} aircraft departed {airport} on {date}; checking {}",
        candidates.len(),
        candidates.len().min(SCOUT_LIMIT)
    );

    let mut found = 0;
    for (icao24, callsign) in candidates.into_iter().take(SCOUT_LIMIT) {
        std::thread::sleep(Duration::from_millis(500));
        let flights = match client.flights_by_aircraft(&icao24, date) {
            Ok(flights) => flights,
            Err(e) => {
                eprintln!("  {icao24}: {e}");
                continue;
            }
        };
        let Ok(rotation) = rotation_from_flights(date, flights, None) else { continue };
        if !(2..=6).contains(&rotation.legs.len()) || Puzzle::from_rotation(0, date, &rotation, &airports).is_err() {
            continue;
        }
        let stops = rotation.stops();
        let returns = if stops.first() == stops.last() { "  returns to base" } else { "" };
        println!("  {icao24}  {callsign:<8}  {} legs  {}{returns}", rotation.legs.len(), stops.join(" > "));
        found += 1;
    }
    println!("{found} puzzle-ready. Next: daily fetch <icao24> {date} puzzles/<publish-date>.json");
    Ok(())
}

/// Downloads one airframe's day and writes it as a rotation file.
fn fetch(icao24: &str, date: &str, out: &str) -> Result<(), Box<dyn Error>> {
    let date = parse_date(date)?;
    let info = if Path::new(AIRCRAFT_CSV).exists() {
        aircraft::lookup(AIRCRAFT_CSV, icao24)?
    } else {
        eprintln!("warning: {AIRCRAFT_CSV} not found, so type, registration and airline will be blank");
        None
    };
    let flights = opensky()?.flights_by_aircraft(icao24, date)?;
    let mut rotation = rotation_from_flights(date, flights, info.as_ref())?;
    if rotation.operator.is_none() && Path::new(AIRLINES_DAT).exists() {
        let callsign = rotation.legs.iter().find_map(|leg| leg.callsign.as_deref());
        let airlines = AirlineDb::load(AIRLINES_DAT)?;
        rotation.operator = callsign.and_then(|c| airlines.for_callsign(c)).map(String::from);
    }
    Puzzle::from_rotation(0, date, &rotation, &AirportDb::load(AIRPORTS_CSV)?)?;

    std::fs::write(out, serde_json::to_string_pretty(&rotation)? + "\n")?;
    println!(
        "wrote {out}: {} {} legs, {}",
        rotation.registration.as_deref().unwrap_or(icao24),
        rotation.legs.len(),
        rotation.stops().join(" > ")
    );
    for (field, missing) in [
        ("aircraft_type", rotation.aircraft_type.is_empty()),
        ("registration", rotation.registration.is_none()),
        ("operator", rotation.operator.is_none()),
    ] {
        if missing {
            eprintln!("warning: {field} is unknown; fill it in by hand in {out}");
        }
    }
    Ok(())
}
