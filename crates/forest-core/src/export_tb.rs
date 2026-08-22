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

/// Eksportuje warstwę drzew jako przezroczysty PNG (rozdzielczość `res`x`res`).
/// `colors[i]` = kolor gatunku o indeksie `i`. Zwraca liczbę narysowanych obiektów.
pub fn export_trees_png(
    objects: &[PlacedObject],
    project: &ForestProject,
    colors: &[[u8; 3]],
    res: u32,
    path: impl AsRef<std::path::Path>,
) -> Result<u32, String> {
    if res == 0 {
        return Err("Rozdzielczość musi być > 0".into());
    }
    let mut rgba = vec![0u8; (res as usize) * (res as usize) * 4];
    let map = project.map_size_m;
    let sx = f64::from(res) / map;
    let sy = f64::from(res) / map;

    // promień punktu ~1.5 px, nie mniejszy niż 1
    let r = ((res as f64 / map) * 1.5).round().max(1.0);
    let ri = r.ceil() as i64;
    let r2 = r * r;

    let mut drawn: u32 = 0;
    for o in objects {
        let px = (o.x * sx) as i64;
        let py = ((map - o.y) * sy) as i64;
        let c = colors.get(o.species_index).copied().unwrap_or([255, 255, 255]);
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                if ((dx * dx + dy * dy) as f64) <= r2 + 0.25 {
                    let x = px + dx;
                    let y = py + dy;
                    if x >= 0 && y >= 0 && (x as u32) < res && (y as u32) < res {
                        let idx = ((y as usize) * (res as usize) + (x as usize)) * 4;
                        rgba[idx] = c[0];
                        rgba[idx + 1] = c[1];
                        rgba[idx + 2] = c[2];
                        rgba[idx + 3] = 255;
                    }
                }
            }
        }
        drawn += 1;
    }

    image::save_buffer(
        path,
        &rgba,
        res,
        res,
        image::ColorType::Rgba8,
    )
    .map_err(|e| format!("Zapis PNG: {e}"))?;
    Ok(drawn)
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

    #[test]
    fn trees_png_transparent_with_colored_dots() {
        let proj = ForestProject {
            map_size_m: 1000.0,
            zones: vec![ZoneDef { color: Rgb8([0,0,0]), ..Default::default() }],
            ..Default::default()
        };
        let objs = vec![
            PlacedObject { model: "a".into(), x: 500.0, y: 500.0, yaw: 0.0, pitch: 0.0, roll: 0.0, scale: 1.0, elevation: 0.0, zone_index: 0, species_index: 0 },
        ];
        let colors = [[255, 0, 0]];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trees.png");
        let n = export_trees_png(&objs, &proj, &colors, 100, &path).unwrap();
        assert_eq!(n, 1);

        let img = image::open(&path).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (100, 100));
        // środek ma czerwoną kropkę, a daleki róg jest przezroczysty (alpha=0)
        let center = img.get_pixel(50, 50);
        assert_eq!([center[0], center[1], center[2], center[3]], [255, 0, 0, 255]);
        let corner = img.get_pixel(5, 5);
        assert_eq!(corner[3], 0);
    }
}
