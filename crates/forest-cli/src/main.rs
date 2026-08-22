//! forest-cli — generowanie lasów z wiersza poleceń.
//!
//! Użycie:
//!   forest-cli init <projekt.json>              — zapisuje przykładowy projekt
//!   forest-cli run <projekt.json> [-o out.txt] [--seed N] [--quiet]
//!   forest-cli validate <projekt.json>

use std::process::ExitCode;

use forest_core::export_tb::write_tb_file;
use forest_core::geojson::GeoJsonData;
use forest_core::heightmap::AscHeightmap;
use forest_core::mask::MaskImage;
use forest_core::preset::ForestProject;
use forest_core::species::vanilla_library;
use forest_core::scatter::generate;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("BŁĄD: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() -> Result<String, String> {
    Ok(
        "forest-cli — generator lasów DayZ\n\n\
         Użycie:\n  \
         forest-cli init <projekt.json>            przykładowy projekt do edycji\n  \
         forest-cli validate <projekt.json>        sprawdza konfigurację\n  \
         forest-cli check-models <projekt.json>    czy modele istnieją w P:\\DZ\\plants\n  \
         forest-cli run <projekt.json> [opcje]     generuje i eksportuje TXT\n\n\
         Opcje run:\n  \
         -o, --output <plik.txt>   plik wyjściowy (domyślnie obiekty_tb.txt)\n  \
         --seed <N>                nadpisuje ziarno losowe\n  \
         --quiet                   bez podsumowania"
            .to_string(),
    )
}

fn run(args: &[String]) -> Result<String, String> {
    let Some(cmd) = args.first() else {
        return print_usage();
    };
    match cmd.as_str() {
        "init" => {
            let path = args
                .get(1)
                .ok_or("init wymaga ścieżki projektu, np. projekt.json")?;
            let mut p = ForestProject::default();
            p.species = vanilla_library();

            // strefy startowe z wbudowanych presetów (kolory przykładowej maski)
            let presets = forest_core::species::zone_presets();
            let mut zones = Vec::new();
            for (name, color) in [
                ("Bór świerkowy (góry)", forest_core::mask::Rgb8([0, 170, 0])),
                ("Las mieszany nizinny", forest_core::mask::Rgb8([0, 220, 80])),
            ] {
                match presets.iter().find(|pr| pr.name == name) {
                    Some(pr) => zones.push(pr.to_zone_def(color)),
                    None => return Err(format!("preset '{name}' nie istnieje")),
                }
            }
            p.zones = zones;
            p.save(path)?;
            Ok(format!("Zapisano przykładowy projekt: {path}"))
        }
        "validate" => {
            let path = args.get(1).ok_or("validate wymaga ścieżki projektu")?;
            let p = ForestProject::load(path)?;
            p.validate()?;
            Ok(format!(
                "Projekt OK: {} stref, {} gatunków, mapa {:.0} m",
                p.zones.len(),
                p.species.len(),
                p.map_size_m
            ))
        }
        "check-models" => {
            let path = args.get(1).ok_or("check-models wymaga ścieżki projektu")?;
            let p = ForestProject::load(path)?;
            let available = forest_core::species::game_plant_models()?;
            let missing = forest_core::species::missing_in_game(&p.species, &available);
            if missing.is_empty() {
                Ok(format!(
                    "✓ Wszystkie {} modeli istnieją w P:\\DZ\\plants.",
                    p.species.len()
                ))
            } else {
                Err(format!(
                    "Brakujące modele ({}):\n  {}",
                    missing.len(),
                    missing.join("\n  ")
                ))
            }
        }
        "run" => cmd_run(args.get(1).ok_or("run wymaga ścieżki projektu")?, &args[2..]),
        "-h" | "--help" | "help" => print_usage(),
        other => Err(format!(
            "Nieznana komenda '{other}'. Użyj 'forest-cli help'."
        )),
    }
}

fn cmd_run(project_path: &str, opts: &[String]) -> Result<String, String> {
    let mut output = String::from("obiekty_tb.txt");
    let mut seed_override: Option<u64> = None;
    let mut quiet = false;

    let mut it = opts.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => {
                output = it.next().ok_or("--output wymaga ścieżki")?.clone();
            }
            "--seed" => {
                let v = it.next().ok_or("--seed wymaga liczby")?;
                seed_override = Some(v.parse().map_err(|_| "--seed: nieprawidłowa liczba")?);
            }
            "--quiet" | "-q" => quiet = true,
            other => return Err(format!("Nieznana opcja '{other}'")),
        }
    }

    let mut project = ForestProject::load(project_path)?;
    if let Some(s) = seed_override {
        project.seed = s;
    }
    project.sanitize();
    project.validate()?;

    let mask = match &project.paths.mask {
        Some(p) => Some(MaskImage::load(p)?),
        None => None,
    };

    let satellite = match &project.paths.satellite {
        Some(p) => Some(MaskImage::load(p)?),
        None => None,
    };
    let heightmap = match &project.paths.heightmap_asc {
        Some(p) => Some(AscHeightmap::load(p)?),
        None => None,
    };
    let exclusions = match &project.paths.exclusions_geojson {
        Some(p) => {
            let mut gj = GeoJsonData::load(p)?;
            gj.normalize_easting(project.easting_offset);
            Some(gj)
        }
        None => None,
    };

    if !quiet {
        match &mask {
            Some(m) => println!(
                "Maska {}x{} px | mapa {:.0} m | stref: {} | obszarów: {}",
                m.width,
                m.height,
                project.map_size_m,
                project.zones.len(),
                project.areas.len()
            ),
            None => println!(
                "Bez maski | mapa {:.0} m | obszarów: {}",
                project.map_size_m,
                project.areas.len()
            ),
        }
        if project.edges.enabled {
            println!(
                "Granica lasu: pas {:.0} m, {:.0} szt/ha",
                project.edges.band_width_m, project.edges.density_per_ha
            );
        }
        println!("Generowanie...");
    }

    let start = std::time::Instant::now();
    let (objects, stats) =
        generate(&project, mask.as_ref(), satellite.as_ref(), heightmap.as_ref(), exclusions.as_ref(), &|f| {
            if !quiet {
                print!("\r  postęp: {:>3}%", (f * 100.0) as u32);
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
        })?;
    if !quiet {
        println!();
    }

    let n = write_tb_file(&objects, &project, &output)?;
    let _ = start.elapsed();

    if !quiet {
        println!("Zapisano {n} obiektów -> {output}");
        println!("{}", stats.summary());
    }
    Ok(String::new())
}
