use std::error::Error;
use std::path::Path;
use std::process::ExitCode;

use daily::airports::AirportDb;
use daily::grade::grade_stop;
use daily::model::{Puzzle, Rotation};

const USAGE: &str = "usage:
  daily grade  <airports.csv> <guess> <answer>
  daily export <airports.csv> <rotation.json> <out_dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["grade", csv, guess, answer] => grade(csv, guess, answer),
        ["export", csv, rotation, out_dir] => export(csv, rotation, out_dir),
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

/// Writes `puzzle.json` and a compact `airports.json` for the web frontend.
fn export(csv: &str, rotation: &str, out_dir: &str) -> Result<(), Box<dyn Error>> {
    let db = AirportDb::load(csv)?;
    let rotation: Rotation = serde_json::from_str(&std::fs::read_to_string(rotation)?)?;
    let puzzle = Puzzle::from_rotation(1, rotation.date, &rotation, &db)?;

    // [iata, icao, name, city, country, lat, lon], coordinates to ~10 m.
    let round = |x: f64| (x * 1e4).round() / 1e4;
    let airports: Vec<_> = db
        .iter()
        .map(|a| (&a.iata, &a.icao, &a.name, &a.city, &a.country, round(a.coord.lat), round(a.coord.lon)))
        .collect();

    let out_dir = Path::new(out_dir);
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("puzzle.json"), serde_json::to_string_pretty(&puzzle)?)?;
    std::fs::write(out_dir.join("airports.json"), serde_json::to_string(&airports)?)?;
    println!("wrote puzzle #{} ({} legs) and {} airports to {}", puzzle.number, puzzle.clues.len(), airports.len(), out_dir.display());
    Ok(())
}
