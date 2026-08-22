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
    ("(brak nowych kolorów)", "(no new colors)", "(keine neuen Farben)"),
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
    ("+ Wyklucz", "+ Exclude", "+ Ausschließen"),
    ("+ Użyj", "+ Use", "+ Verwenden"),
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
    ("Edytuj obszar (wierzchołki)", "Edit area (vertices)", "Bereich bearbeiten (Eckpunkte)"),
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
    // --- inteligentne generowanie (kolory z podkładu satelitarnego) ---
    (
        "Próbka #{0} dodana (razem: {1}). Esc = koniec.",
        "Sample #{0} added (together: {1}). Esc = end.",
        "Beispiel #{0} hinzugefügt (zusammen: {1}). Esc = Ende.",
    ),
    (
        "Klikaj na podkładzie w miejsca z lasem...",
        "Click on the satellite map in forest areas...",
        "Klicken Sie auf das Satellitenbild in Waldbereichen...",
    ),
    (
        "Najpierw wczytaj podkład satelitarny (Pliki).",
        "First load the satellite layer (Files).",
        "Zuerst Satellitenbild laden (Dateien).",
    ),
    (
        "Brak podkładu satelitarnego - wczytaj go w Pliki.",
        "No satellite layer - load it from Files.",
        "Kein Satellitenbild vorhanden - laden Sie es über Dateien.",
    ),
    (
        "Brak podkładu - filtr nie zadziała.",
        "No layer - filter will not work.",
        "Kein Bild - Filter funktioniert nicht.",
    ),
    // --- poprawki: warianty kluczy używanych w kodzie (bez dwukropka) ---
    ("Gotowe.", "Done.", "Fertig."),
    ("Min. wysokość [m]", "Min. altitude [m]", "Min. Höhe [m]"),
    ("Maks. wysokość [m]", "Max. altitude [m]", "Max. Höhe [m]"),
    ("Maks. spadek [°]", "Max. slope [°]", "Max. Neigung [°]"),
    ("Próbki kolorów:", "Color samples:", "Farbproben:"),
    ("Wyczyść", "Clear", "Löschen"),
    ("Inne", "Other", "Andere"),
    ("Kopiuj do „Moje presety” (edytowalna kopia)", "Copy to “My presets” (editable copy)", "Nach „Meine Presets“ kopieren (editierbare Kopie)"),
    // --- napisy wcześniej zahardkodowane (widoczne w UI) ---
    (
        "(brak — elevation=0, obiekty na terenie)",
        "(none — elevation=0, objects on terrain)",
        "(keine — Höhe=0, Objekte auf Gelände)",
    ),
    (
        "(brak wykluczeń wektorowych)",
        "(no vector exclusions)",
        "(keine Vektorausschlüsse)",
    ),
    ("Aktywne źródła:", "Active sources:", "Aktive Quellen:"),
    ("maska", "mask", "Maske"),
    ("poligony", "polygons", "Polygone"),
    ("+ granica", "+ edge", "+ Rand"),
    (
        "Ziarno: {0} | mnożnik odstępów: {1} | polany: {2}",
        "Seed: {0} | spacing mult.: {1} | clearings: {2}",
        "Seed: {0} | Abstands-Multiplikator: {1} | Lichtungen: {2}",
    ),
    (
        "Usuwa WYGENEROWANE obiekty w buforze wokół pikseli danego koloru (działa też na obszary rysowane). Nakłada się ponownie przy każdym generowaniu. Bufor zaokrąglany do piksela maski.",
        "Removes GENERATED objects in a buffer around pixels of the given color (also affects drawn areas). Reapplied on every generation. Buffer rounded to mask pixel.",
        "Entfernt GENERIERTE Objekte im Puffer um Pixel der angegebenen Farbe (betrifft auch gezeichnete Bereiche). Wird bei jeder Generierung erneut angewendet. Puffer auf Maskenpixel gerundet.",
    ),
    // --- UI chrome zahardkodowane w main.rs (widoczne w UI) ---
    ("Obszar", "Area", "Gebiet"),
    ("pkt:", "pts:", "Pkt.:"),
    ("szt", "pcs", "Stk."),
    (
        "Brak wyników - kliknij '▶ Generuj'. Po generowaniu tutaj pojawią się warstwy.",
        "No results - click '▶ Generate'. Layers will appear here after generation.",
        "Keine Ergebnisse - auf '▶ Generieren' klicken. Ebenen erscheinen hier nach der Generierung.",
    ),
    (
        "Dodaj strefę z presetu (⊕ kopiuje do \"Moje presety\"):",
        "Add zone from preset (⊕ copies to \"My presets\"):",
        "Zone aus Preset hinzufügen (⊕ kopiert nach \"Meine Presets\"):",
    ),
    (
        "Dodaj strefę '{0}' ({1} szt/ha)",
        "Add zone '{0}' ({1} pcs/ha)",
        "Zone '{0}' hinzufügen ({1} Stk./ha)",
    ),
    (
        "Efekt: {0} szt/ha, {1} gatunków (ręczna edycja wag niżej czyści miks)",
        "Effect: {0} pcs/ha, {1} species (manual weight edit below clears mix)",
        "Effekt: {0} Stk./ha, {1} Arten (manuelle Gewichtsänderung löscht Mix)",
    ),
    (
        "Efekt: {0} szt/ha, {1} gatunków",
        "Effect: {0} pcs/ha, {1} species",
        "Effekt: {0} Stk./ha, {1} Arten",
    ),
    (
        "Brak własnych presetów. Skopiuj wbudowane przyciskiem ⊕ w sekcji \"🌲 Strefy lasu\" albo dodaj pusty powyżej.",
        "No custom presets. Copy built-ins with ⊕ in \"🌲 Forest zones\" or add empty above.",
        "Keine eigenen Presets. Eingebaute mit ⊕ in \"🌲 Waldzonen\" kopieren oder oben leeres hinzufügen.",
    ),
    ("Dodaj strefę z tego presetu", "Add zone from this preset", "Zone aus diesem Preset hinzufügen"),
    (
        "Rysowane na mapie: wybierz preset na pasku → ✏ Rysuj obszar → klikaj wierzchołki (LPM), Enter/dwuklik = zakończ.",
        "Drawn on map: pick preset on toolbar → ✏ Draw area → click vertices (LMB), Enter/double-click = finish.",
        "Auf Karte gezeichnet: Preset in Leiste wählen → ✏ Fläche zeichnen → Eckpunkte klicken (LMB), Enter/Doppelklick = beenden.",
    ),
    ("{0} ha | pkt: {1} | ≈ {2} szt", "{0} ha | pts: {1} | ≈ {2} pcs", "{0} ha | Pkt.: {1} | ≈ {2} Stk."),
    (
        "Używa granicy globalnej (pas {0} m, {1}/ha)",
        "Uses global edge (band {0} m, {1}/ha)",
        "Nutzt globalen Rand (Streifen {0} m, {1}/ha)",
    ),
    ("Dodaj preset:", "Add preset:", "Preset hinzufügen:"),
    ("wybierz z listy...", "pick from list...", "aus Liste wählen..."),
    (
        "(udziały → sumują się do dowolnej wartości — liczone proporcjonalnie)",
        "(shares → sum to any value — counted proportionally)",
        "(Anteile → summieren sich beliebig — proportional gerechnet)",
    ),
    (
        "Plik: presets_user.json (katalog roboczy programu)",
        "File: presets_user.json (program working directory)",
        "Datei: presets_user.json (Arbeitsverzeichnis)",
    ),
    (
        "Plik: {0} (katalog roboczy programu)",
        "File: {0} (program working directory)",
        "Datei: {0} (Arbeitsverzeichnis)",
    ),
    ("Iglaste", "Coniferous", "Nadelbäume"),
    ("Liściaste", "Deciduous", "Laubbäume"),
    ("Mieszane", "Mixed", "Gemischt"),
    ("Krzewy i zarośla", "Shrubs and thickets", "Sträucher und Dickicht"),
    ("Krzewy", "Shrubs", "Sträucher"),
    ("Bliss (lato)", "Bliss (summer)", "Bliss (Sommer)"),
    ("Sakhal (zima/mrok)", "Sakhal (winter/dark)", "Sakhal (Winter/Dunkel)"),
    ("Bór świerkowy (góry)", "Spruce forest (mountains)", "Fichtenwald (Berge)"),
    ("Bór sosnowy (niziny)", "Pine forest (lowlands)", "Kiefernwald (Tiefland)"),
    ("Młodnik iglasty", "Coniferous thicket", "Nadel-Dickicht"),
    ("Dębowa puszcza", "Oak primeval forest", "Eichen-Urwald"),
    ("Grąd (dąb-buk)", "Oak-hornbeam (oak-beech)", "Eichen-Hainbuchenwald"),
    ("Brzozowy zagajnik", "Birch grove", "Birkenhain"),
    ("Łęg nadrzeczny", "Riparian forest", "Auwald"),
    ("Las mieszany nizinny", "Mixed lowland forest", "Mischwald Tiefland"),
    ("Las mieszany wyżynny", "Mixed upland forest", "Mischwald Hochland"),
    ("Zarośla krzewiaste", "Shrub thickets", "Gebüsch"),
    ("Samosiewy (młodnik)", "Natural regrowth (thicket)", "Naturverjüngung"),
    ("Brzoza młoda", "Young birch", "Junge Birke"),
    ("Brzoza", "Birch", "Birke"),
    ("Brzoza wysoka", "Tall birch", "Hohe Birke"),
    ("Dąb młody", "Young oak", "Junge Eiche"),
    ("Dąb", "Oak", "Eiche"),
    ("Dąb wysoki", "Tall oak", "Hohe Eiche"),
    ("Buk", "Beech", "Buche"),
    ("Buk wysoki", "Tall beech", "Hohe Buche"),
    ("Jesion", "Ash", "Esche"),
    ("Modrzew", "Larch", "Lärche"),
    ("Robinia (akacja)", "Black locust (acacia)", "Robinie (Akazie)"),
    ("Świerk młody", "Young spruce", "Junge Fichte"),
    ("Świerk", "Spruce", "Fichte"),
    ("Świerk wysoki", "Tall spruce", "Hohe Fichte"),
    ("Sosna młoda", "Young pine", "Junge Kiefer"),
    ("Sosna", "Pine", "Kiefer"),
    ("Sosna wysoka", "Tall pine", "Hohe Kiefer"),
    ("Leszczyna", "Hazel", "Hasel"),
    ("Bez czarny", "Black elder", "Schwarzer Holunder"),
    ("Róża dzika", "Wild rose", "Heckenrose"),
    ("Tarnina", "Blackthorn", "Schlehe"),
    ("Głóg", "Hawthorn", "Weißdorn"),
    ("Brzoza karłowata", "Dwarf birch", "Zwergbirke"),
    ("Klon", "Maple", "Ahorn"),
    ("Brzoza E młoda", "E-Birch young", "E-Birke jung"),
    ("Brzoza E", "E-Birch", "E-Birke"),
    ("Brzoza E wysoka", "Tall E-birch", "Hohe E-Birke"),
    ("Karagana", "Caragana", "Erbsenstrauch"),
    ("Leszczyna syberyjska", "Siberian hazel", "Sibirische Hasel"),
    ("Trzcina", "Reed", "Schilf"),
    ("Orzech włoski", "Walnut", "Walnuss"),
    ("Jabłoń dzika", "Wild apple", "Wildapfel"),
    ("Grusza dzika", "Wild pear", "Wildbirne"),
    ("Wierzba biała", "White willow", "Silberweide"),
    ("Jarzębina", "Rowan", "Eberesche"),
    ("Topola czarna", "Black poplar", "Schwarzpappel"),
    ("Świerk mroczny młody", "Dark spruce young", "Dunkle Fichte jung"),
    ("Świerk mroczny", "Dark spruce", "Dunkle Fichte"),
    ("Świerk zamrożony młody", "Frozen spruce young", "Gefrorene Fichte jung"),
    ("Świerk zamrożony", "Frozen spruce", "Gefrorene Fichte"),
    ("Brzoza zimowa", "Winter birch", "Winterbirke"),
    ("Brzoza zimowa wysoka", "Tall winter birch", "Hohe Winterbirke"),
    ("Brzoza jesienna", "Autumn birch", "Herbstbirke"),
    ("Topola biała (jesień)", "White poplar (autumn)", "Silberpappel (Herbst)"),
    ("Buk młody (lato)", "Young beech (summer)", "Junge Buche (Sommer)"),
    ("Świerk krzewiasty (lato)", "Shrubby spruce (summer)", "Buschfichte (Sommer)"),
    ("Modrzew młody (lato)", "Young larch (summer)", "Junge Lärche (Sommer)"),
    ("Sosna 1f (lato)", "Pine 1f (summer)", "Kiefer 1f (Sommer)"),
    ("Orzech włoski 2s", "Walnut 2s", "Walnuss 2s"),
    ("Grusza dzika 2s", "Wild pear 2s", "Wildbirne 2s"),
    ("Świerk krzewiasty mroczny", "Dark shrubby spruce", "Dunkle Buschfichte"),
    ("Świerk krzewiasty zamrożony", "Frozen shrubby spruce", "Gefrorene Buschfichte"),
    ("Brzoza karłowata zimowa", "Winter dwarf birch", "Zwergbirke Winter"),
    ("Topola biała młoda (jesień)", "Young white poplar (autumn)", "Junge Silberpappel (Herbst)"),
    ("Karagana bezlistna", "Leafless caragana", "Blattlose Erbsenstrauch"),
    // --- presety / gatunki (vanilla) ---
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
