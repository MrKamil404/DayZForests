//! Tłumaczenia interfejsu: PL (źródłowy) / EN / DE.
//!
//! Słownik kluczowany POLSKIM tekstem źródłowym — brak wpisu oznacza
//! powrót do tekstu polskiego, więc interfejs zawsze działa.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Pl,
    En,
    De,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Pl, Lang::En, Lang::De];
    pub fn name(&self) -> &'static str {
        match self {
            Lang::Pl => "Polski",
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }
    pub fn code(&self) -> &'static str {
        match self {
            Lang::Pl => "pl",
            Lang::En => "en",
            Lang::De => "de",
        }
    }
    pub fn from_code(s: &str) -> Option<Lang> {
        match s.to_lowercase().as_str() {
            "pl" => Some(Lang::Pl),
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            _ => None,
        }
    }
}

/// Preferencje UI (język) — zapisywane obok presetów.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiPrefs {
    #[serde(default)]
    pub language: String,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self { language: "pl".into() }
    }
}

pub const PREFS_FILE: &str = "ui_settings.json";

impl UiPrefs {
    pub fn load(path: impl AsRef<std::path::Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<UiPrefs>(&s).ok())
            .and_then(|p| {
                // walidacja kodu języka
                Lang::from_code(&p.language)?;
                Some(p)
            })
            .unwrap_or_default()
    }
    pub fn save(&self, path: impl AsRef<std::path::Path>) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Wpis słownika: (polski oryginał, en, de)
type Entry = (&'static str, &'static str, &'static str);

const DICT: &[Entry] = &[
    // --- pasek narzędzi ---
    ("▶ Generuj", "▶ Generate", "▶ Generieren"),
    ("💾 Eksport TXT (TB)", "💾 Export TXT (TB)", "💾 TXT-Export (TB)"),
    ("🖼 Eksport PNG (drzewa)", "🖼 Export PNG (trees)", "🖼 PNG-Export (Bäume)"),
    ("📂 Wczytaj projekt", "📂 Open project", "📂 Projekt öffnen"),
    ("💾 Zapisz projekt", "💾 Save project", "💾 Projekt speichern"),
    ("✏ Rysuj obszar", "✏ Draw area", "✏ Fläche zeichnen"),
    ("✏ Rysowanie: WŁ", "✏ Drawing: ON", "✏ Zeichnen: EIN"),
    ("Podkład", "Satellite", "Satellit"),
    ("Maska", "Mask", "Maske"),
    ("Drzewa", "Trees", "Bäume"),
    ("Dopasuj widok", "Fit view", "Ansicht anpassen"),
    // --- sekcje ---
    ("📁 Pliki", "📁 Files", "📁 Dateien"),
    ("🗺 Mapa / eksport", "🗺 Map / export", "🗺 Karte / Export"),
    ("⚙ Źródła generowania", "⚙ Generation sources", "⚙ Generierungsquellen"),
    ("🌲 Strefy lasu", "🌲 Forest zones", "🌲 Waldzonen"),
    ("📚 Moje presety", "📚 My presets", "📚 Meine Presets"),
    ("📐 Obszary (poligony)", "📐 Areas (polygons)", "📐 Bereiche (Polygone)"),
    ("🧱 Granica lasu", "🧱 Forest edge", "🧱 Waldrand"),
    ("⛰ Filtry środowiskowe", "⛰ Environment filters", "⛰ Umgebung filter"),
    ("🎲 Rozrzut / polany", "🎲 Scatter / clearings", "🎲 Streuung / Lichtungen"),
    ("🌳 Gatunki", "🌳 Species", "🌳 Arten"),
    ("✂ Wycinanie (kolor + bufor)", "✂ Cut-out (color + buffer)", "✂ Ausschneiden (Farbe + Puffer)"),
    ("📊 Wygenerowane warstwy", "📊 Generated layers", "📊 Erzeugte Ebenen"),
    // --- ekran powitalny / kanwa ---
    (
        "1. Wczytaj maskę PNG (kolory = strefy lasu)",
        "1. Load a PNG mask (colors = forest zones)",
        "1. PNG-Maske laden (Farben = Waldzonen)",
    ),
    (
        "2. ...albo narysuj obszary: wybierz preset i klikaj wierzchołki na mapie",
        "2. ...or draw areas: pick a preset and click vertices on the map",
        "2. ...oder Flächen zeichnen: Preset wählen und Eckpunkte klicken",
    ),
    (
        "3. Opcjonalnie: heightmapa .asc, wykluczenia .geojson, granice lasu",
        "3. Optional: .asc heightmap, .geojson exclusions, forest edges",
        "3. Optional: Höhenkarte .asc, Ausschlüsse .geojson, Waldränder",
    ),
    (
        "4. Generuj → Eksport TXT → import w Terrain Builderze",
        "4. Generate → Export TXT → import in Terrain Builder",
        "4. Generieren → TXT-Export → Import in den Terrain Builder",
    ),
    // --- pasek stanu ---
    ("Gotowy.", "Ready.", "Bereit."),
    ("Generowanie...", "Generating...", "Generiere..."),
    (
        "Razem: {0} obiektów | granica: {1} | wycięto: {2} | {3} ms",
        "Total: {0} objects | edge: {1} | cut: {2} | {3} ms",
        "Gesamt: {0} Objekte | Rand: {1} | geschnitten: {2} | {3} ms",
    ),
    ("(brak źródeł)", "(no sources)", "(keine Quellen)"),
    // --- pliki ---
    (
        "Wczytaj maskę (PNG/BMP/TGA)...",
        "Load mask (PNG/BMP/TGA)...",
        "Maske laden (PNG/BMP/TGA)...",
    ),
    ("(brak maski)", "(no mask)", "(keine Maske)"),
    (
        "Wczytaj podkład satelitarny (PNG/JPG)...",
        "Load satellite layer (PNG/JPG)...",
        "Satellitenbild laden (PNG/JPG)...",
    ),
    ("Krycie podkładu:", "Layer opacity:", "Deckkraft:"),
    ("(brak)", "(none)", "(keine)"),
    (
        "Wczytaj heightmapę (.asc)...",
        "Load heightmap (.asc)...",
        "Höhenkarte laden (.asc)...",
    ),
    (
        "Wczytaj wykluczenia (.geojson)...",
        "Load exclusions (.geojson)...",
        "Ausschlüsse laden (.geojson)...",
    ),
    ("Maska: {0}x{1} px", "Mask: {0}x{1} px", "Maske: {0}x{1} px"),
    // --- źródła ---
    ("Strefy z maski (kolory)", "Mask zones (colors)", "Maskenzonen (Farben)"),
    ("Obszary rysowane (poligony)", "Drawn areas (polygons)", "Gezeichnete Bereiche (Polygone)"),
    ("Granica lasu (krzewy)", "Forest edge (bushes)", "Waldrand (Sträucher)"),
    // --- mapa ---
    ("Rozmiar [m]:", "Size [m]:", "Größe [m]:"),
    ("Easting offset:", "Easting offset:", "Ost-Versatz:"),
    ("Northing offset:", "Northing offset:", "Nord-Versatz:"),
    ("Ziarno (seed):", "Seed:", "Seed:"),
    ("Wysokość:", "Elevation:", "Höhe:"),
    ("relative (0)", "relative (0)", "relativ (0)"),
    ("absolute (z ASC)", "absolute (from ASC)", "absolut (aus ASC)"),
    // --- granica ---
    (
        "Włącz pas graniczny (krzewy wzdłuż krawędzi lasu)",
        "Enable forest edge band (bushes along the treeline)",
        "Waldrand-Streifen aktivieren (Sträucher an der Baumlinie)",
    ),
    ("Szerokość pasa [m]:", "Band width [m]:", "Streifenbreite [m]:"),
    ("Gęstość [szt/ha]:", "Density [pcs/ha]:", "Dichte [Stk./ha]:"),
    ("Wtapianie pasa", "Blend band", "Streifen verblenden"),
    ("Poszarpanie granicy [m]:", "Edge raggedness [m]:", "Randrauheit [m]:"),
    ("Wtapianie w las [m]:", "Blend into forest [m]:", "Verblendung in Wald [m]:"),
    ("Wagi gatunków granicy:", "Edge species weights:", "Arten-Gewichte am Rand:"),
    // --- filtry ---
    ("Min. wysokość [m]:", "Min. altitude [m]:", "Min. Höhe [m]:"),
    ("Maks. wysokość [m]:", "Max. altitude [m]:", "Max. Höhe [m]:"),
    ("Maks. spadek [°]:", "Max. slope [°]:", "Max. Neigung [°]:"),
    ("Tolerancja koloru:", "Color tolerance:", "Farbtoleranz:"),
    ("Margines krawędzi [m]:", "Edge margin [m]:", "Randabstand [m]:"),
    (
        "Kolory wykluczone (np. drogi/woda):",
        "Excluded colors (e.g. roads/water):",
        "Ausgeschlossene Farben (z.B. Straßen/Wasser):",
    ),
    // --- rozrzut ---
    ("Mnożnik odstępów:", "Spacing multiplier:", "Abstands-Multiplikator:"),
    ("Skala polan [m]:", "Clearing scale [m]:", "Lichtungsskala [m]:"),
    ("Siła polan:", "Clearing strength:", "Lichtungsstärke:"),
    // --- wycinanie ---
    (
        "✂ Wycinanie (kolor + bufor)",
        "✂ Cut-out (color + buffer)",
        "✂ Ausschneiden (Farbe + Puffer)",
    ),
    ("Bufor [m]:", "Buffer [m]:", "Puffer [m]:"),
    (
        "Dodaj strefę z koloru maski:",
        "Add zone from mask color:",
        "Zone aus Maskenfarbe hinzufügen:",
    ),
    ("Brak stref wycinania.", "No cut zones.", "Keine Schnittzonen."),
    // --- gatunki ---
    (
        "🔍 Sprawdź modele na P:\\",
        "🔍 Check models on P:\\",
        "🔍 Modelle auf P:\\ prüfen",
    ),
    (
        "TB wymaga wpisu w Template Library dla każdego modelu.",
        "TB requires a Template Library entry for every model.",
        "Der TB verlangt einen Template-Library-Eintrag pro Modell.",
    ),
    // --- strefy ---
    ("+ Dodaj strefę", "+ Add zone", "+ Zone hinzufügen"),
    ("Analizuję kolory maski...", "Analyzing mask colors...", "Analysiere Maskenfarben..."),
    ("Usuń strefę", "Delete zone", "Zone löschen"),
    ("RGB ręcznie:", "Manual RGB:", "Manuell RGB:"),
    ("Wagi gatunków:", "Species weights:", "Arten-Gewichte:"),
    ("Kolor własny:", "Custom color:", "Eigene Farbe:"),
    ("Kolor z maski:", "Color from mask:", "Farbe aus Maske:"),
    ("⟲ auto", "⟲ auto", "⟲ auto"),
    ("(wczytaj maskę, aby wybierać kolory)", "(load a mask to pick colors)", "(Maske laden, um Farben zu wählen)"),
    ("Gęstość [szt/ha]:", "Density [pcs/ha]:", "Dichte [Stk./ha]:"),
    // --- moje presety ---
    ("+ Nowy pusty preset", "+ New empty preset", "+ Neues leeres Preset"),
    ("💾 Zapisz", "💾 Save", "💾 Speichern"),
    ("• niezapisane", "• unsaved", "• ungespeichert"),
    ("Grupa:", "Group:", "Gruppe:"),
    ("+ Strefa", "+ Zone", "+ Zone"),
    ("Duplikuj", "Duplicate", "Duplizieren"),
    ("Usuń preset", "Delete preset", "Preset löschen"),
    ("Edytuj preset", "Edit preset", "Preset bearbeiten"),
    // --- obszary ---
    ("Usuń obszar", "Delete area", "Bereich löschen"),
    ("Własna granica", "Own edge", "Eigener Waldrand"),
    (
        "Wyczyść wszystkie obszary",
        "Clear all areas",
        "Alle Bereiche löschen",
    ),
    // --- komunikaty ---
    ("Gotowy.", "Ready.", "Fertig."),
    ("Generowanie w toku...", "Generation in progress...", "Generierung läuft..."),
    (
        "Wczytaj maskę PNG albo narysuj przynajmniej jeden obszar (poligon).",
        "Load a PNG mask or draw at least one area (polygon).",
        "PNG-Maske laden oder mindestens einen Bereich (Polygon) zeichnen.",
    ),
    (
        "Odrzucone: {0} (odstęp {1}, filtry {2}, wykluczenia {3}, polany {4})",
        "Rejected: {0} (spacing {1}, filters {2}, exclusions {3}, clearings {4})",
        "Verworfen: {0} (Abstand {1}, Filter {2}, Ausschlüsse {3}, Lichtungen {4})",
    ),
    (
        "Dodano obszar '{0}' ({1:.1} ha). Przypisz presety w panelu 📐 Obszary, aby generował drzewa.",
        "Added area '{0}' ({1:.1} ha). Assign presets in the 📐 Areas panel to grow trees.",
        "Bereich '{0}' ({1:.1} ha) hinzugefügt. Weisen Sie im Panel 📐 Bereiche Presets zu, um Bäume zu erzeugen.",
    ),
    (
        "Wczytano projekt: {0}",
        "Project loaded: {0}",
        "Projekt geladen: {0}",
    ),
];

/// Tłumaczy tekst źródłowy (PL). Brak wpisu -> zwraca oryginał.
pub fn tr(lang: Lang, key: &str) -> String {
    if lang == Lang::Pl {
        return key.to_string();
    }
    let idx = if lang == Lang::En { 1usize } else { 2usize };
    for e in DICT {
        if e.0 == key {
            return match idx {
                1 => e.1.to_string(),
                _ => e.2.to_string(),
            };
        }
    }
    key.to_string()
}

/// Szablon z tokenami {0} {1} ... — podmienia po kolei.
pub fn tf(lang: Lang, key: &str, args: &[&str]) -> String {
    let mut s = tr(lang, key);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}
