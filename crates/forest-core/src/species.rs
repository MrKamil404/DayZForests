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
}

impl SpeciesDef {
    pub fn new(label: &str, model: &str, scale_min: f32, scale_max: f32) -> Self {
        Self {
            label: label.to_string(),
            model: model.to_string(),
            scale_min,
            scale_max,
            tilt_max_deg: 2.0,
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
    let mut v = Vec::new();
    let mut t = |label: &str, model: &str, smin: f32, smax: f32| {
        v.push(SpeciesDef::new(label, model, smin, smax));
    };

    // --- drzewa liściaste (dz\plants\tree) ---
    t("Brzoza młoda", "t_BetulaPendula_1s", 0.85, 1.15);
    t("Brzoza", "t_BetulaPendula_2f", 0.9, 1.15);
    t("Brzoza wysoka", "t_BetulaPendula_3f", 0.9, 1.1);
    t("Dąb młody", "t_quercusRobur_1f", 0.9, 1.1);
    t("Dąb", "t_quercusRobur_2f", 0.9, 1.1);
    t("Dąb wysoki", "t_quercusRobur_3f", 0.95, 1.05);
    t("Buk", "t_FagusSylvatica_2f", 0.9, 1.1);
    t("Buk wysoki", "t_FagusSylvatica_3f", 0.95, 1.05);
    t("Jesion", "t_FraxinusExcelsior_2f", 0.9, 1.1);
    t("Modrzew", "t_LarixDecidua_2f", 0.9, 1.1);
    t("Robinia (akacja)", "t_robiniaPseudoacacia_2f", 0.9, 1.1);

    // --- drzewa iglaste (dz\plants\tree) ---
    t("Świerk młody", "t_PiceaAbies_1s", 0.85, 1.15);
    t("Świerk", "t_PiceaAbies_2f", 0.9, 1.15);
    t("Świerk wysoki", "t_PiceaAbies_3f", 0.95, 1.05);
    t("Sosna młoda", "t_PinusSylvestris_1s", 0.85, 1.15);
    t("Sosna", "t_PinusSylvestris_2f", 0.9, 1.15);
    t("Sosna wysoka", "t_PinusSylvestris_3f", 0.95, 1.05);

    // --- krzewy (dz\plants\bush) ---
    t("Leszczyna", "b_corylusAvellana_2s", 0.85, 1.2);
    t("Bez czarny", "b_sambucusNigra_2s", 0.85, 1.2);
    t("Róża dzika", "b_rosaCanina_2s", 0.85, 1.2);
    t("Tarnina", "b_prunusSpinosa_2s", 0.85, 1.2);
    t("Głóg", "b_crataegusLaevigata_2s", 0.85, 1.2);
    t("Brzoza karłowata", "b_betulaHumilis_1s", 0.85, 1.2);

    v
}

/// Skanuje P:\DZ\plants\{tree,bush} i zwraca zestaw nazw modeli (bez .p3d,
/// z zachowaną wielkością liter jak na dysku). Błąd, gdy workdrive nie jest
/// zamontowany albo brak katalogów.
pub fn game_plant_models() -> Result<std::collections::HashSet<String>, String> {
    use std::collections::HashSet;
    let mut out = HashSet::new();
    let mut any = false;
    for dir in ["P:\\DZ\\plants\\tree", "P:\\DZ\\plants\\bush"] {
        let rd = std::fs::read_dir(dir)
            .map_err(|e| format!("Nie mogę otworzyć {dir}: {e} (czy P:\\ jest zamontowany?)"))?;
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".p3d") {
                out.insert(stem.to_string());
                any = true;
            }
        }
    }
    if !any {
        return Err("W P:\\DZ\\plants nie znaleziono żadnych .p3d".into());
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
