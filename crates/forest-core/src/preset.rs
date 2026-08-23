//! Konfiguracja projektu (serializowana do JSON) — most między GUI/CLI a silnikiem.

use crate::mask::Rgb8;
use crate::species::SpeciesDef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevationMode {
    /// Elevation = 0 -> obiekty "przyklejone" do terenu przy imporcie (relative).
    RelativeZero,
    /// Elevation = wysokość z heightmapy ASC (import jako absolute).
    AbsoluteSampled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZoneDef {
    /// Kolor strefy na masce (RGB).
    pub color: Rgb8,
    pub label: String,
    /// Docelowa gęstość drzew na hektar.
    pub density_per_ha: f32,
    /// Wagi gatunków w strefie: (indeks do `species`, waga > 0).
    pub species_weights: Vec<(usize, f32)>,
    /// Miks presetów: (nazwa presetu, udział 0..1]. Gdy niepuste —
    /// gęstość/wagi są wynikiem zmieszania tych presetów proporcjonalnie
    /// do udziałów (edytowalne w GUI; edycja ręczna wag czyści miks).
    #[serde(default)]
    pub preset_mix: Vec<(String, f32)>,
}

impl Default for ZoneDef {
    fn default() -> Self {
        Self {
            color: Rgb8([255, 0, 0]),
            label: "Strefa".into(),
            density_per_ha: 220.0,
            species_weights: Vec::new(),
            preset_mix: Vec::new(),
        }
    }
}

/// Obszar rysowany ręcznie (poligon) — alternatywa dla stref z maski.
/// Współrzędne świata w metrach (origin SW), obrys zamknięty automatycznie.
/// Strefa wycinania: obiekty w promieniu `margin_m` od pikseli o kolorze
/// `color` są usuwane PO generowaniu (niezależnie od źródła).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CutZone {
    pub color: Rgb8,
    pub margin_m: f32,
}

/// Inteligentne generowanie: filtr kolorów z warstwy satelitarnej.
/// Drzewa powstają tylko tam, gdzie piksel podkładu pasuje do jednej
/// z próbek (Manhattan <= tolerancja).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorFilter {
    /// Próbki referencyjne (RGB) pobrane z podkładu.
    pub samples: Vec<Rgb8>,
    /// Tolerancja dopasowania (suma różnic kanałów).
    #[serde(default = "default_cf_tol")]
    pub tolerance: u32,
}

fn default_cf_tol() -> u32 {
    60
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AreaDef {
    /// Czy obszar bierze udział w generowaniu.
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub label: String,
    pub density_per_ha: f32,
    /// Wagi gatunków: (indeks do `species`, waga > 0).
    pub species_weights: Vec<(usize, f32)>,
    /// Pierścień poligonu, >= 3 punkty.
    pub polygon: Vec<[f64; 2]>,
    /// Dziury poligonu (wycięcia) — pierścienie >= 3 punkty. Obiekty nie są
    /// generowane wewnątrz dziur (import SHP z wieloczęściowymi poligonami).
    #[serde(default)]
    pub holes: Vec<Vec<[f64; 2]>>,
    /// Miks presetów jak w ZoneDef.
    #[serde(default)]
    pub preset_mix: Vec<(String, f32)>,
    /// Indywidualna granica lasu dla tego obszaru (None = użyj globalnej).
    #[serde(default)]
    pub edges: Option<EdgeSettings>,
    /// Inteligentne generowanie: tylko na kolorach podkładu z próbek.
    /// None lub puste próbki = brak filtra (normalne generowanie).
    #[serde(default)]
    pub color_filter: Option<ColorFilter>,
}

/// Pas graniczny lasu — krzewy/podrost sadzone wzdłuż krawędzi stref i obszarów.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeSettings {
    pub enabled: bool,
    /// Szerokość pasa po obu stronach granicy [m].
    pub band_width_m: f64,
    pub density_per_ha: f32,
    pub species_weights: Vec<(usize, f32)>,
    /// Wtapianie: gęstość pasa zanika wraz z odległością od granicy
    /// (zamiast równomiernego, twardego pasa).
    #[serde(default = "default_true")]
    pub blend: bool,
    /// Poszarpanie granicy [m] — szum przesuwający efektywną linię lasu,
    /// dzięki czemu brzeg nie jest równy jak od linijki. 0 = wyłączone.
    #[serde(default = "default_jagged")]
    pub jagged_m: f64,
    /// Odległość wtapiania w głąb lasu [m]: gęstość drzew narasta 0 -> pełna
    /// na tej głębokości od krawędzi (otwarte, naturalne obrzeża). 0 = wył.
    #[serde(default = "default_blend_inside")]
    pub blend_inside_m: f64,
}

fn default_jagged() -> f64 {
    25.0
}

fn default_blend_inside() -> f64 {
    40.0
}

impl Default for EdgeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            band_width_m: 15.0,
            density_per_ha: 160.0,
            // leszczyna (17), róża (19), bez (18), tarnina (20) — wg vanilla_library
            species_weights: vec![(17, 3.0), (19, 3.0), (18, 2.0), (20, 1.0)],
            blend: true,
            jagged_m: 25.0,
            blend_inside_m: 40.0,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectPaths {
    #[serde(default)]
    pub mask: Option<String>,
    /// Podkład satelitarny (tylko podgląd, nie wpływa na generowanie).
    #[serde(default)]
    pub satellite: Option<String>,
    #[serde(default)]
    pub heightmap_asc: Option<String>,
    #[serde(default)]
    pub exclusions_geojson: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForestProject {
    pub version: u32,
    /// Rozmiar mapy w metrach (kwadrat), np. 15360 dla Chernarus.
    pub map_size_m: f64,
    /// Offset easting dodawany przy eksporcie TB (klasycznie 200000).
    pub easting_offset: f64,
    pub northing_offset: f64,
    pub seed: u64,

    /// Mnożnik odstępów Poisson-disk (>1 = rzadziej).
    pub spacing_multiplier: f64,
    /// Długość fali szumu polan w metrach (0 = wyłączone).
    pub clearing_scale_m: f64,
    /// Siła polan 0..0.95 (jaka część obszaru ma być prześwitem).
    pub clearing_strength: f64,

    pub min_altitude: Option<f64>,
    pub max_altitude: Option<f64>,
    pub max_slope_deg: Option<f64>,
    /// Margines od krawędzi mapy w metrach.
    pub edge_padding_m: f64,

    /// Tolerancja dopasowania koloru maski (suma różnic kanałów).
    pub color_tolerance: u32,
    /// Kolory maski traktowane jako zakazane (np. drogi, woda).
    pub exclusion_colors: Vec<Rgb8>,
    pub elevation_mode: ElevationMode,

    pub species: Vec<SpeciesDef>,
    pub zones: Vec<ZoneDef>,

    /// Włącz/wyłącz źródło: strefy z maski.
    #[serde(default = "default_true")]
    pub use_mask_zones: bool,
    /// Włącz/wyłącz źródło: obszary rysowane (poligony).
    #[serde(default = "default_true")]
    pub use_areas: bool,

    /// Obszary rysowane ręcznie (poligony) — alternatywa/uzupełnienie maski.
    #[serde(default)]
    pub areas: Vec<AreaDef>,

    /// Pas graniczny (krzewy itp.) wzdłuż krawędzi lasu.
    #[serde(default)]
    pub edges: EdgeSettings,

    /// Strefy wycinania po kolorze maski + bufor [m] — usuwają wygenerowane
    /// obiekty PO generowaniu (działa też na obszary rysowane).
    #[serde(default)]
    pub cut_zones: Vec<CutZone>,

    #[serde(default)]
    pub paths: ProjectPaths,
}

impl Default for ForestProject {
    fn default() -> Self {
        Self {
            version: 1,
            map_size_m: 15360.0,
            easting_offset: 200_000.0,
            northing_offset: 0.0,
            seed: 1337,
            spacing_multiplier: 1.0,
            clearing_scale_m: 350.0,
            clearing_strength: 0.35,
            min_altitude: None,
            max_altitude: None,
            max_slope_deg: Some(35.0),
            edge_padding_m: 16.0,
            color_tolerance: 12,
            exclusion_colors: Vec::new(),
            elevation_mode: ElevationMode::RelativeZero,
            species: crate::species::vanilla_library(),
            zones: Vec::new(),
            use_mask_zones: true,
            use_areas: true,
            cut_zones: Vec::new(),
            areas: Vec::new(),
            edges: EdgeSettings::default(),
            paths: ProjectPaths::default(),
        }
    }
}

impl ForestProject {
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serializacja projektu: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("Zapis projektu: {e}"))
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Odczyt projektu: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("Parse projektu: {e}"))
    }

    /// Usuwa z stref/obszarów odwołania do nieistniejących gatunków.
    pub fn sanitize(&mut self) {
        let n = self.species.len();
        for z in &mut self.zones {
            z.species_weights.retain(|(i, w)| *i < n && *w > 0.0);
        }
        self.zones.retain(|z| !z.species_weights.is_empty());
        for a in &mut self.areas {
            // obszary bez wag mogą czekać na przypisanie presetu — nie usuwamy
            if !a.species_weights.is_empty() {
                a.species_weights.retain(|(i, w)| *i < n && *w > 0.0);
            }
            a.holes.retain(|h| h.len() >= 3);
        }
        self.areas.retain(|a| a.polygon.len() >= 3);
        if self.edges.enabled {
            self.edges
                .species_weights
                .retain(|(i, w)| *i < n && *w > 0.0);
        }
    }

    /// Walidacja przed generowaniem; zwraca listę problemów.
    pub fn validate(&self) -> Result<(), String> {
        if self.map_size_m <= 0.0 {
            return Err("Rozmiar mapy musi być > 0".into());
        }
        if !self.use_mask_zones && !self.use_areas {
            return Err(
                "Wszystkie źródła są wyłączone — włącz w '⚙ Źródła generowania' \
                 przynajmniej maskę albo obszary (poligony)."
                    .into(),
            );
        }
        if self.zones.is_empty() && self.areas.is_empty() {
            return Err("Brak stref lasu (dodaj kolory z maski albo narysuj obszary)".into());
        }
        for (zi, z) in self.zones.iter().enumerate() {
            if z.density_per_ha <= 0.0 {
                return Err(format!("Strefa {}: gęstość musi być > 0", zi + 1));
            }
            if z.species_weights.is_empty() {
                return Err(format!("Strefa '{}': brak gatunków", z.label));
            }
            check_weights(&z.species_weights, self.species.len())
                .map_err(|e| format!("Strefa '{}': {e}", z.label))?;
        }
        for (ai, a) in self.areas.iter().enumerate() {
            if !a.enabled {
                continue;
            }
            if a.polygon.len() < 3 {
                return Err(format!("Obszar {}: poligon wymaga >= 3 punktów", ai + 1));
            }
            // obszar nieskonfigurowany (bez presetu) — legalny, generuje 0 obiektów
            if a.species_weights.is_empty() && a.density_per_ha <= 0.0 {
                continue;
            }
            for (pi, pt) in a.polygon.iter().enumerate() {
                if !pt[0].is_finite() || !pt[1].is_finite() {
                    return Err(format!("Obszar {}: punkt {pi} ma nieprawidłowe współrzędne", ai + 1));
                }
            }
            if let Some(ix) = crate::scatter::find_self_intersection(&a.polygon) {
                return Err(format!(
                    "Obszar '{}': obrys przecina sam siebie — odcinki #{}–#{} i #{}–#{} \
                     krzyżują się w punkcie ({:.1}, {:.1}). Zobacz czerwony znacznik na mapie; \
                     usuń lub przesuń wierzchołki tak, aby obrys był prostym wielokątem.",
                    a.label,
                    ix.seg_a,
                    ix.seg_a + 1,
                    ix.seg_b,
                    ix.seg_b + 1,
                    ix.point[0],
                    ix.point[1]
                ));
            }
            for (hi, hole) in a.holes.iter().enumerate() {
                if let Some(ix) = crate::scatter::find_self_intersection(hole) {
                    return Err(format!(
                        "Obszar '{}': dziura {} przecina samą siebie — odcinki #{}–#{} i #{}–#{} \
                         krzyżują się w punkcie ({:.1}, {:.1}). Popraw obrys dziury.",
                        a.label,
                        hi + 1,
                        ix.seg_a,
                        ix.seg_a + 1,
                        ix.seg_b,
                        ix.seg_b + 1,
                        ix.point[0],
                        ix.point[1]
                    ));
                }
            }
            if a.density_per_ha <= 0.0 {
                return Err(format!("Obszar '{}': gęstość musi być > 0", a.label));
            }
            if a.species_weights.is_empty() {
                return Err(format!("Obszar '{}': brak gatunków", a.label));
            }
            check_weights(&a.species_weights, self.species.len())
                .map_err(|e| format!("Obszar '{}': {e}", a.label))?;
        }
        if self.edges.enabled {
            if self.edges.band_width_m <= 0.0 {
                return Err("Granica lasu: szerokość pasa musi być > 0".into());
            }
            if self.edges.density_per_ha <= 0.0 {
                return Err("Granica lasu: gęstość musi być > 0".into());
            }
            if self.edges.species_weights.is_empty() {
                return Err("Granica lasu: brak gatunków".into());
            }
            check_weights(&self.edges.species_weights, self.species.len())
                .map_err(|e| format!("Granica lasu: {e}"))?;
        }
        Ok(())
    }
}

fn check_weights(weights: &[(usize, f32)], species_len: usize) -> Result<(), String> {
    for (i, _) in weights {
        if *i >= species_len {
            return Err(format!("wskazuje nieistniejący gatunek #{}", i));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_project_serializes_roundtrip() {
        let p = ForestProject::default();
        let json = serde_json::to_string(&p).unwrap();
        let back: ForestProject = serde_json::from_str(&json).unwrap();
        assert_eq!(back.map_size_m, 15360.0);
        assert_eq!(back.easting_offset, 200000.0);
        assert!(!back.species.is_empty());
    }

    #[test]
    fn save_and_load_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proj.json");
        let mut p = ForestProject::default();
        p.zones.push(ZoneDef {
            color: Rgb8([0, 128, 0]),
            label: "Las mieszany".into(),
            ..Default::default()
        });
        p.save(&path).unwrap();
        let q = ForestProject::load(&path).unwrap();
        assert_eq!(q.zones.len(), 1);
        assert_eq!(q.zones[0].color, Rgb8([0, 128, 0]));
    }

    #[test]
    fn sanitize_drops_dead_references() {
        let mut p = ForestProject::default();
        p.zones.push(ZoneDef {
            species_weights: vec![(0, 1.0), (9999, 2.0), (1, 0.0)],
            ..Default::default()
        });
        p.sanitize();
        assert_eq!(p.zones[0].species_weights, vec![(0, 1.0)]);
    }

    #[test]
    fn validate_catches_empty_zones() {
        let p = ForestProject::default();
        assert!(p.validate().is_err());
    }
}
