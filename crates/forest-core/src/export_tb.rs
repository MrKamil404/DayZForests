//! Eksport obiektów do pliku TXT dla Terrain Buildera.
//!
//! Format wiersza (potwierdzony dla TB / DayZ Tools):
//!   "nazwa_modelu";X;Y;Yaw;Pitch;Roll;Scale;Elevation;
//! X zawiera offset easting (klasycznie +200000). Nazwa modelu musi odpowiadać
//! wpisowi w Template Library. Przy imporcie wybierz odpowiednio
//! "relative" (Elevation=0) lub "absolute" (wysokość z heightmapy).

use std::io::Write;

use crate::preset::ForestProject;
use crate::scatter::PlacedObject;

pub fn write_tb_txt(
    objects: &[PlacedObject],
    project: &ForestProject,
    out: &mut dyn Write,
) -> std::io::Result<usize> {
    let mut n = 0usize;
    for o in objects {
        writeln!(
            out,
            "\"{}\";{:.6};{:.6};{:.6};{:.6};{:.6};{:.6};{:.6};",
            o.model,
            o.x + project.easting_offset,
            o.y + project.northing_offset,
            o.yaw,
            o.pitch,
            o.roll,
            o.scale,
            o.elevation
        )?;
        n += 1;
    }
    Ok(n)
}

pub fn write_tb_file(
    objects: &[PlacedObject],
    project: &ForestProject,
    path: impl AsRef<std::path::Path>,
) -> Result<usize, String> {
    let file = std::fs::File::create(path)
        .map_err(|e| format!("Nie można utworzyć pliku wyjściowego: {e}"))?;
    let mut buf = std::io::BufWriter::new(file);
    write_tb_txt(objects, project, &mut buf).map_err(|e| format!("Zapis TXT: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{ForestProject, ZoneDef};
    use crate::mask::Rgb8;

    #[test]
    fn line_format_matches_tb_spec() {
        let proj = ForestProject {
            easting_offset: 200_000.0,
            northing_offset: 0.0,
            zones: vec![ZoneDef { color: Rgb8([0,0,0]), ..Default::default() }],
            ..Default::default()
        };
        let obj = PlacedObject {
            model: "t_BetulaPendula_2f".into(),
            x: 456.296875,
            y: 1032.300049,
            yaw: 250.0,
            pitch: -1.5,
            roll: 2.25,
            scale: 1.0,
            elevation: 5.686351,
            zone_index: 0,
            species_index: 0,
        };
        let mut buf: Vec<u8> = Vec::new();
        let n = write_tb_txt(&[obj], &proj, &mut buf).unwrap();
        assert_eq!(n, 1);
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "\"t_BetulaPendula_2f\";200456.296875;1032.300049;250.000000;-1.500000;2.250000;1.000000;5.686351;\n"
        );
    }

    #[test]
    fn northing_offset_applied() {
        let proj = ForestProject {
            easting_offset: 200_000.0,
            northing_offset: 500_000.0,
            zones: vec![ZoneDef { color: Rgb8([0,0,0]), ..Default::default() }],
            ..Default::default()
        };
        let obj = PlacedObject {
            model: "x".into(),
            x: 1.0,
            y: 2.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            scale: 1.0,
            elevation: 0.0,
            zone_index: 0,
            species_index: 0,
        };
        let mut buf: Vec<u8> = Vec::new();
        write_tb_txt(&[obj], &proj, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parts: Vec<&str> = s.trim_end().split(';').collect();
        assert_eq!(parts[1], "200001.000000");
        assert_eq!(parts[2], "500002.000000");
    }
}
