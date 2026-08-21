//! Definicje gatunków drzew/krzewów + wbudowana biblioteka modeli vanilla DayZ
//! + pogrupowane presety stref roślinności.
//!
//! `model` to nazwa wpisu w Template Library Terrain Buildera — domyślnie
//! nazwa pliku .p3d bez ścieżki i rozszerzenia (konwencja Chernarus:
//! dz\plants\tree\*.p3d oraz dz\plants\bush\*.p3d).

use serde::{Deserialize, Serialize};

use crate::mask::Rgb8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeciesDef {
    /// Nazwa wyświetlana (np. "Brzoza wysoka").
    pub label: String,
    /// Nazwa modelu dla Terrain Buildera (bez .p3d).
    pub model: String,
    pub scale_min: f32,
    pub scale_max: f32,
    /// Maksymalny losowy przechył pitch/roll w stopniach.
    pub tilt_max_deg: f32,
    /// Grupa (do pogrupowanego widoku w UI), np. "Liściaste".
    #[serde(default)]
    pub group: String,
}

impl SpeciesDef {
    pub fn new(label: &str, model: &str, scale_min: f32, scale_max: f32) -> Self {
        Self {
            label: label.to_string(),
            model: model.to_string(),
            scale_min,
            scale_max,
            tilt_max_deg: 2.0,
            group: String::new(),
        }
    }

    pub fn grouped(label: &str, model: &str, smin: f32, smax: f32, group: &str) -> Self {
        Self {
            group: group.to_string(),
            ..Self::new(label, model, smin, smax)
        }
    }
}

/// Wbudowana biblioteka roślinności vanilla DayZ — WSZYSTKIE nazwy zweryfikowane
/// względem P:\DZ\plants\{tree,bush} (Chernarusplus). Nazwy muszą się zgadzać
/// z wpisami w Template Library użytkownika — można edytować w GUI.
///
/// UWAGA: indeksy z tej listy są używane przez `zone_presets()` — dodając
/// nowe gatunki, dopisz je na końcu, żeby nie przesunąć istniejących.
pub fn vanilla_library() -> Vec<SpeciesDef> {
    const L: &str = "Liściaste";
    const I: &str = "Iglaste";
    const K: &str = "Krzewy";
    const B: &str = "Bliss (lato)";
    const S: &str = "Sakhal (zima/mrok)";

    let mut v = Vec::new();
    // UWAGA: indeksy 0..=22 są używane przez zone_presets() — nie zmieniaj
    // kolejności istniejących wpisów, nowe dopisuj na końcu.
    let mut t = |label: &str, model: &str, smin: f32, smax: f32, group: &str| {
        v.push(SpeciesDef::grouped(label, model, smin, smax, group));
    };

    // --- drzewa liściaste (dz\plants\tree) ---
    t("Brzoza młoda", "t_BetulaPendula_1s", 0.85, 1.15, L);
    t("Brzoza", "t_BetulaPendula_2f", 0.9, 1.15, L);
    t("Brzoza wysoka", "t_BetulaPendula_3f", 0.9, 1.1, L);
    t("Dąb młody", "t_quercusRobur_1f", 0.9, 1.1, L);
    t("Dąb", "t_quercusRobur_2f", 0.9, 1.1, L);
    t("Dąb wysoki", "t_quercusRobur_3f", 0.95, 1.05, L);
    t("Buk", "t_FagusSylvatica_2f", 0.9, 1.1, L);
    t("Buk wysoki", "t_FagusSylvatica_3f", 0.95, 1.05, L);
    t("Jesion", "t_FraxinusExcelsior_2f", 0.9, 1.1, L);
    t("Modrzew", "t_LarixDecidua_2f", 0.9, 1.1, I);
    t("Robinia (akacja)", "t_robiniaPseudoacacia_2f", 0.9, 1.1, L);

    // --- drzewa iglaste ---
    t("Świerk młody", "t_PiceaAbies_1s", 0.85, 1.15, I);
    t("Świerk", "t_PiceaAbies_2f", 0.9, 1.15, I);
    t("Świerk wysoki", "t_PiceaAbies_3f", 0.95, 1.05, I);
    t("Sosna młoda", "t_PinusSylvestris_1s", 0.85, 1.15, I);
    t("Sosna", "t_PinusSylvestris_2f", 0.9, 1.15, I);
    t("Sosna wysoka", "t_PinusSylvestris_3f", 0.95, 1.05, I);

    // --- krzewy (dz\plants\bush) ---
    t("Leszczyna", "b_corylusAvellana_2s", 0.85, 1.2, K);
    t("Bez czarny", "b_sambucusNigra_2s", 0.85, 1.2, K);
    t("Róża dzika", "b_rosaCanina_2s", 0.85, 1.2, K);
    t("Tarnina", "b_prunusSpinosa_2s", 0.85, 1.2, K);
    t("Głóg", "b_crataegusLaevigata_2s", 0.85, 1.2, K);
    t("Brzoza karłowata", "b_betulaHumilis_1s", 0.85, 1.2, K);

    // --- Bliss / Livonia — warianty letnie (dz\plants_bliss) ---
    t("Klon", "t_acer_2s_summer", 0.9, 1.1, B);
    t("Brzoza E młoda", "t_BetulaPendulaE_1s_summer", 0.85, 1.15, B);
    t("Brzoza E", "t_BetulaPendulaE_2f_summer", 0.9, 1.15, B);
    t("Brzoza E wysoka", "t_BetulaPendulaE_3f_summer", 0.9, 1.1, B);
    t("Karagana", "b_caraganaArborescens_2s_summer", 0.85, 1.2, B);
    t("Leszczyna syberyjska", "b_corylusHeterophylla_2s_summer", 0.85, 1.2, B);
    t("Trzcina", "b_phragmitesAustralis_summer", 0.8, 1.3, B);
    t("Orzech włoski", "t_juglansRegia_3s_summer", 0.9, 1.1, B);
    t("Jabłoń dzika", "t_malusDomestica_3s_summer", 0.85, 1.15, B);
    t("Grusza dzika", "t_pyrusCommunis_3s_summer", 0.85, 1.15, B);
    t("Wierzba biała", "t_salixAlba_2sb_summer", 0.9, 1.15, B);
    t("Jarzębina", "t_sorbus_2s_summer", 0.85, 1.15, B);
    t("Topola czarna", "t_populusNigra_3sb_summer", 0.9, 1.1, B);

    // --- Sakhal — warianty mroczne/zimowe/jesienne (dz\plants_sakhal) ---
    t("Świerk mroczny młody", "t_PiceaAbies_1s_dark", 0.85, 1.15, S);
    t("Świerk mroczny", "t_PiceaAbies_2f_dark", 0.9, 1.15, S);
    t("Świerk zamrożony młody", "t_PiceaAbies_1s_frozen", 0.85, 1.15, S);
    t("Świerk zamrożony", "t_PiceaAbies_2f_frozen", 0.9, 1.15, S);
    t("Brzoza zimowa", "t_BetulaPendula_2f_winter", 0.9, 1.15, S);
    t("Brzoza zimowa wysoka", "t_BetulaPendula_3f_winter", 0.9, 1.1, S);
    t("Brzoza jesienna", "t_BetulaPendula_2f_latefall", 0.9, 1.15, S);
    t("Topola biała (jesień)", "t_populusAlba_2s_latefall", 0.9, 1.15, S);

    // --- Bliss — uzupełniające (dz\plants_bliss) ---
    t("Buk młody (lato)", "t_FagusSylvatica_1s_summer", 0.85, 1.15, B);
    t("Świerk krzewiasty (lato)", "b_PiceaAbies_1f_summer", 0.85, 1.2, B);
    t("Modrzew młody (lato)", "t_LarixDecidua_1s_summer", 0.85, 1.15, B);
    t("Sosna 1f (lato)", "t_PinusSylvestris_1f_summer", 0.85, 1.15, B);
    t("Orzech włoski 2s", "t_juglansRegia_2s_summer", 0.85, 1.15, B);
    t("Grusza dzika 2s", "t_pyrusCommunis_2s_summer", 0.85, 1.15, B);

    // --- Sakhal — uzupełniające (dz\plants_sakhal) ---
    t("Świerk krzewiasty mroczny", "b_PiceaAbies_1f_dark", 0.85, 1.2, S);
    t("Świerk krzewiasty zamrożony", "b_PiceaAbies_1f_frozen", 0.85, 1.2, S);
    t("Brzoza karłowata zimowa", "b_betulaNana_1s_winter", 0.85, 1.2, S);
    t("Topola biała młoda (jesień)", "t_populusAlba_1f_latefall", 0.85, 1.15, S);
    t("Karagana bezlistna", "b_caraganaArborescens_2s_leafless", 0.85, 1.2, S);

    v
}

/// Skanuje katalogi roślinności na P:\ i zwraca zestaw nazw modeli (bez .p3d,
/// z zachowaną wielkością liter jak na dysku). Obejmuje:
/// - P:\DZ\plants\tree + bush (Chernarus),
/// - P:\DZ\plants_bliss (Livonia/Bliss, warianty letnie),
/// - P:\DZ\plants_sakhal (Sakhal, mrok/zima/jesień).
/// Błąd, gdy workdrive nie jest zamontowany.
pub fn game_plant_models() -> Result<std::collections::HashSet<String>, String> {
    use std::collections::HashSet;

    fn scan_flat(dir: &str, out: &mut HashSet<String>) -> Result<(), String> {
        let rd = std::fs::read_dir(dir)
            .map_err(|e| format!("Nie mogę otworzyć {dir}: {e} (czy P:\\ jest zamontowany?)"))?;
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".p3d") {
                if stem.starts_with("t_") || stem.starts_with("b_") {
                    out.insert(stem.to_string());
                }
            }
        }
        Ok(())
    }

    fn scan_recursive(dir: &str, out: &mut HashSet<String>) -> Result<(), String> {
        let rd = std::fs::read_dir(dir)
            .map_err(|e| format!("Nie mogę otworzyć {dir}: {e}"))?;
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_recursive(&path.to_string_lossy(), out)?;
            } else if let Some(stem) = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .and_then(|n| n.strip_suffix(".p3d").map(|s| s.to_string()))
            {
                let low = stem.to_lowercase();
                if low.starts_with("t_") || low.starts_with("b_") {
                    out.insert(stem);
                }
            }
        }
        Ok(())
    }

    let mut out = HashSet::new();
    let mut any = false;
    for dir in [
        "P:\\DZ\\plants\\tree",
        "P:\\DZ\\plants\\bush",
    ] {
        scan_flat(dir, &mut out)?;
        any = true;
    }
    for root in [
        "P:\\DZ\\plants_bliss",
        "P:\\DZ\\plants_sakhal",
        "P:\\DZ\\plants\\clutter",
    ] {
        if std::path::Path::new(root).exists() {
            scan_recursive(root, &mut out)?;
            any = true;
        }
    }
    if !any || out.is_empty() {
        return Err("W P:\\DZ\\plants* nie znaleziono żadnych modeli t_/b_".into());
    }
    Ok(out)
}

/// Zwraca listę gatunków, których modelu nie ma wśród plików gry.
/// `available` — wynik `game_plant_models()`.
pub fn missing_in_game(species: &[SpeciesDef], available: &std::collections::HashSet<String>) -> Vec<String> {
    species
        .iter()
        .filter(|s| !available.contains(&s.model))
        .map(|s| format!("{} ({})", s.label, s.model))
        .collect()
}

/// Kolory podglądu dla gatunków (HSV -> RGB, rozłożone po kole barw).
pub fn species_preview_color(index: usize) -> [u8; 3] {
    let hue = (index as f64 * 0.618_033_988_7) % 1.0; // złoty kąt — dobrze rozróżnialne
    hsv_to_rgb(hue, 0.75, 0.95)
}

/// Gotowy szablon strefy lasu: gęstość + proporcje gatunków (indeksy wg
/// `vanilla_library()`). Używany przez GUI i CLI do szybkiego dodawania stref.
#[derive(Clone, Copy, Debug)]
pub struct ZonePreset {
    /// Grupa (np. "Iglaste") — używana tylko do prezentacji w UI.
    pub group: &'static str,
    pub name: &'static str,
    pub density_per_ha: f32,
    /// (indeks gatunku, waga)
    pub weights: &'static [(usize, f32)],
}

impl ZonePreset {
    pub fn to_zone_def(&self, color: Rgb8) -> crate::preset::ZoneDef {
        crate::preset::ZoneDef {
            color,
            label: self.name.to_string(),
            density_per_ha: self.density_per_ha,
            species_weights: self.weights.iter().map(|(i, w)| (*i, *w)).collect(),
            preset_mix: Vec::new(),
        }
    }
}

/// Pogrupowane presety roślinności (typy lasów jak w klimacie Chernarus).
/// Indeksy gatunków wg `vanilla_library()` powyżej.
pub fn zone_presets() -> Vec<ZonePreset> {
    use ZonePreset as P;
    vec![
        // --- Iglaste -----------------------------------------------------
        P { group: "Iglaste", name: "Bór świerkowy (góry)", density_per_ha: 260.0,
            weights: &[(13, 5.0), (12, 3.0), (16, 1.0), (11, 1.0)] },
        P { group: "Iglaste", name: "Bór sosnowy (niziny)", density_per_ha: 230.0,
            weights: &[(16, 4.0), (15, 4.0), (11, 1.0), (19, 1.0)] },
        P { group: "Iglaste", name: "Młodnik iglasty", density_per_ha: 340.0,
            weights: &[(11, 5.0), (15, 2.0)] },
        // --- Liściaste ---------------------------------------------------
        P { group: "Liściaste", name: "Dębowa puszcza", density_per_ha: 180.0,
            weights: &[(5, 3.0), (4, 3.0), (3, 1.0), (17, 1.0)] },
        P { group: "Liściaste", name: "Grąd (dąb-buk)", density_per_ha: 200.0,
            weights: &[(6, 3.0), (5, 2.0), (4, 2.0), (8, 1.0)] },
        P { group: "Liściaste", name: "Brzozowy zagajnik", density_per_ha: 190.0,
            weights: &[(2, 3.0), (1, 3.0), (0, 1.0), (17, 1.0)] },
        P { group: "Liściaste", name: "Łęg nadrzeczny", density_per_ha: 170.0,
            weights: &[(1, 3.0), (8, 3.0), (9, 2.0), (18, 1.0)] },
        // --- Mieszane ------------------------------------------------------
        P { group: "Mieszane", name: "Las mieszany nizinny", density_per_ha: 200.0,
            weights: &[(1, 3.0), (12, 2.0), (4, 2.0), (15, 1.0), (17, 1.0)] },
        P { group: "Mieszane", name: "Las mieszany wyżynny", density_per_ha: 210.0,
            weights: &[(12, 3.0), (6, 2.0), (1, 2.0), (13, 1.0), (18, 1.0)] },
        // --- Krzewy i zarośla -----------------------------------------------
        P { group: "Krzewy i zarośla", name: "Zarośla krzewiaste", density_per_ha: 420.0,
            weights: &[(19, 3.0), (17, 3.0), (18, 2.0), (20, 2.0)] },
        P { group: "Krzewy i zarośla", name: "Samosiewy (młodnik)", density_per_ha: 360.0,
            weights: &[(0, 3.0), (3, 2.0), (11, 2.0), (21, 1.0)] },
    ]
}

pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i64) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_library_has_unique_models() {
        let lib = vanilla_library();
        assert!(lib.len() >= 10);
        let mut models: Vec<&str> = lib.iter().map(|s| s.model.as_str()).collect();
        models.sort_unstable();
        models.dedup();
        assert_eq!(models.len(), lib.len(), "zduplikowane modele w bibliotece");
    }

    #[test]
    fn preview_colors_differ() {
        let a = species_preview_color(0);
        let b = species_preview_color(5);
        assert_ne!(a, b);
    }

    #[test]
    fn zone_presets_are_valid() {
        let lib_len = vanilla_library().len();
        let presets = zone_presets();
        assert!(presets.len() >= 8, "za mało presetów");
        let mut names: Vec<&str> = presets.iter().map(|p| p.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "zduplikowane nazwy presetów");

        for p in &presets {
            assert!(!p.group.is_empty());
            assert!(p.density_per_ha > 0.0, "{}: gęstość", p.name);
            assert!(!p.weights.is_empty(), "{}: brak gatunków", p.name);
            for (i, w) in p.weights {
                assert!((*i as usize) < lib_len, "{}: zły indeks gatunku {i}", p.name);
                assert!(*w > 0.0, "{}: waga <= 0", p.name);
            }
        }
    }

    #[test]
    fn preset_to_zone_def_roundtrip() {
        let p = &zone_presets()[0];
        let z = p.to_zone_def(Rgb8([1, 2, 3]));
        assert_eq!(z.color, Rgb8([1, 2, 3]));
        assert_eq!(z.label, p.name);
        assert_eq!(z.density_per_ha, p.density_per_ha);
        assert_eq!(z.species_weights.len(), p.weights.len());
    }
}
