//! forest-gui — desktopowy generator lasów DayZ (egui/eframe).

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, CollapsingHeader, Color32, ColorImage, PointerButton, Rect, ScrollArea,
    Sense, TextureHandle, TextureOptions, Vec2,
};

use forest_core::export_tb::write_tb_file;
use forest_core::geojson::GeoJsonData;
use forest_core::heightmap::AscHeightmap;
use forest_core::mask::{MaskImage, Rgb8};
use forest_core::preset::{ElevationMode, EdgeSettings, ForestProject, ZoneDef};
use forest_core::species::SpeciesDef;
use forest_core::scatter::{generate, GenStats, PlacedObject};
use forest_core::user_presets::{UserPreset, UserPresetLibrary, DEFAULT_PRESETS_FILE};
pub mod i18n;
use i18n::{tr, tf, Lang};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("DayZ Forest Generator"),
        ..Default::default()
    };
    eframe::run_native(
        "DayZ Forest Generator",
        options,
        Box::new(|_cc| {
            let mut app = ForestApp::new();
            let prefs = i18n::UiPrefs::load(i18n::PREFS_FILE);
            if let Some(l) = Lang::from_code(&prefs.language) {
                app.lang = l;
            }
            app.load_user_presets();
            Box::new(app)
        }),
    )
}

type GenRx = Receiver<Result<(Vec<PlacedObject>, GenStats), String>>;

struct ForestApp {
    project: ForestProject,
    mask: Option<Arc<MaskImage>>,
    heightmap: Option<Arc<AscHeightmap>>,
    exclusions: Option<Arc<GeoJsonData>>,
    mask_tex: Option<TextureHandle>,
    sat_tex: Option<TextureHandle>,
    sat_image: Option<Arc<MaskImage>>,
    sat_alpha: f32,
    overlay_tex: Option<TextureHandle>,
    show_mask: bool,
    show_sat: bool,
    show_trees: bool,

    zoom: f32,
    pan: Vec2,

    gen_rx: Option<GenRx>,
    progress: Arc<Mutex<f64>>,
    histogram: Option<Arc<Vec<(Rgb8, usize)>>>,
    hist_rx: Option<Receiver<Arc<Vec<(Rgb8, usize)>>>>,
    objects: Vec<PlacedObject>,
    stats: Option<GenStats>,
    busy: bool,
    error: Option<String>,
    info: Option<String>,

    // tryb rysowania poligonów (obszary)
    draw_mode: bool,
    draw_points: Vec<[f64; 2]>,

    /// Indeks obszaru w trakcie edycji (przesuwanie/dodawanie wierzchołków).
    editing_area: Option<usize>,
    /// Indeks wybranego/przesuwanego wierzchołka w edytowanym obszarze.
    editing_vertex: Option<usize>,

    /// Indeks strefy, której kolor jest właśnie edytowany.
    zone_color_edit: Option<usize>,
    /// Indeks gatunku, którego kolor jest edytowany.
    species_color_edit: Option<usize>,
    /// Obszar, dla którego pobierane są próbki kolorów z podkładu.
    sampling_area: Option<usize>,
    /// Wybrany źródłowy obszar do kopiowania ustawień (per obszar).
    area_copy_src: Vec<Option<usize>>,

    // presety użytkownika (edytowalne kopie + własne)
    user_presets: Vec<UserPreset>,
    preset_edit_open: Option<usize>,
    presets_dirty: bool,

    // język interfejsu
    lang: Lang,
}

/// Znormalizowany podgląd presetu (wbudowanego lub użytkownika).
/// Znormalizowany podgląd presetu (wbudowanego lub użytkownika).
struct PSnap {
    name: String,
    density_per_ha: f32,
    /// wagi wg nazwy modelu
    weights: Vec<(String, f32)>,
}

impl Default for ForestApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ForestApp {
    fn new() -> Self {
        Self {
            project: ForestProject::default(),
            mask: None,
            heightmap: None,
            exclusions: None,
            mask_tex: None,
            sat_tex: None,
            sat_image: None,
            sat_alpha: 1.0,
            overlay_tex: None,
            show_mask: true,
            show_sat: true,
            show_trees: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            gen_rx: None,
            progress: Arc::new(Mutex::new(0.0)),
            histogram: None,
            hist_rx: None,
            objects: Vec::new(),
            stats: None,
            busy: false,
            error: None,
            info: None,
            draw_mode: false,
            draw_points: Vec::new(),
            editing_area: None,
            editing_vertex: None,
            zone_color_edit: None,
            sampling_area: None,
            area_copy_src: Vec::new(),
            species_color_edit: None,
            user_presets: Vec::new(),
            preset_edit_open: None,
            presets_dirty: false,
            lang: Lang::Pl,
        }
    }

    // --- wczytywanie plików -------------------------------------------------

    fn load_mask(&mut self, ctx: &egui::Context, path: &str) {
        match MaskImage::load(path) {
            Ok(m) => {
                let tex = preview_texture(ctx, "mask", &m, TextureOptions::LINEAR);
                self.mask_tex = Some(tex);
                self.overlay_tex = None;
                self.mask = Some(Arc::new(m));
                self.project.paths.mask = Some(path.to_string());
                self.info = Some(format!("Wczytano maskę: {path}"));
                self.error = None;

                // Histogram kolorów liczony raz, w tle (iteracja po wszystkich
                // pikselach w UI co klatkę zamrażała program przy dużych maskach).
                self.histogram = None;
                self.hist_rx = None;
                let mask_for_hist = self.mask.clone().unwrap();
                let (tx, rx) = channel();
                self.hist_rx = Some(rx);
                std::thread::spawn(move || {
                    let mut hist = mask_for_hist.color_histogram(1);
                    hist.truncate(4096);
                    tx.send(Arc::new(hist)).ok();
                });
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn load_satellite(&mut self, ctx: &egui::Context, path: &str) {
        match MaskImage::load(path) {
            Ok(m) => {
                let (sw, sh) = (m.width, m.height);
                let tex = preview_texture(ctx, "satellite", &m, TextureOptions::LINEAR);
                self.sat_image = Some(Arc::new(m));
                self.sat_tex = Some(tex);
                self.show_sat = true;
                self.project.paths.satellite = Some(path.to_string());
                self.info = Some(format!(
                    "Wczytano podkład satelitarny: {path} ({sw}x{sh} px)"
                ));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn load_heightmap(&mut self, path: &str) {
        match AscHeightmap::load(path) {
            Ok(h) => {
                let (mn, mx) = h.min_max();
                self.info =
                    Some(format!("Wczytano ASC: {}x{}, cellsize {}, wysokości {:.0}..{:.0} m",
                        h.ncols, h.nrows, h.cellsize, mn, mx));
                self.heightmap = Some(Arc::new(h));
                self.project.paths.heightmap_asc = Some(path.to_string());
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn load_exclusions(&mut self, path: &str) {
        match GeoJsonData::load(path) {
            Ok(mut gj) => {
                gj.normalize_easting(self.project.easting_offset);
                self.info = Some(format!(
                    "Wczytano wykluczenia: {} poligonów",
                    gj.polygons.len()
                ));
                self.exclusions = Some(Arc::new(gj));
                self.project.paths.exclusions_geojson = Some(path.to_string());
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    // --- generowanie ----------------------------------------------------------

    fn start_generation(&mut self, ctx: &egui::Context) {
        let lang = self.lang;

        if self.busy {
            return;
        }
        if self.mask.is_none() && self.project.areas.is_empty() {
            self.error =
                Some(tr(lang, "Wczytaj maskę PNG albo narysuj przynajmniej jeden obszar (poligon).").into());
            return;
        }
        // anuluj niedokończony szkic przy starcie generowania
        self.draw_mode = false;
        self.draw_points.clear();
        self.sampling_area = None;
        self.project.sanitize();
        if let Err(e) = self.project.validate() {
            self.error = Some(e);
            return;
        }
        let proj = self.project.clone();
        let mask = self.mask.clone();
        let sat_img = self.sat_image.clone();
        let hm = self.heightmap.clone();
        let ex = self.exclusions.clone();
        let prog = self.progress.clone();
        *prog.lock().unwrap() = 0.0;

        let (tx, rx) = channel();
        self.gen_rx = Some(rx);
        self.busy = true;
        self.error = None;
        self.info = Some(tr(lang, "Generowanie w toku...").into());

        std::thread::spawn(move || {
            let res = generate(&proj, mask.as_deref(), sat_img.as_deref(), hm.as_deref(), ex.as_deref(), &|f| {
                *prog.lock().unwrap() = f;
            });
            tx.send(res).ok();
        });
        ctx.request_repaint();
    }

    fn poll_generation(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let Some(rx) = &self.gen_rx else { return };
        match rx.try_recv() {
            Ok(Ok((objects, stats))) => {
                self.objects = objects;
                self.stats = Some(stats);
                self.busy = false;
                self.gen_rx = None;
                // podgląd działa też bez maski (np. generowanie z samych poligonów)
                let (dw, dh) = match &self.mask {
                    Some(m) => preview_dims(m, PREVIEW_MAX_DIM),
                    None => (PREVIEW_MAX_DIM, PREVIEW_MAX_DIM),
                };
                let overlay = build_overlay(dw, dh, &self.objects, &self.project);
                self.overlay_tex =
                    Some(ctx.load_texture("overlay", overlay, TextureOptions::NEAREST));
                self.info = Some(tr(lang, "Gotowe.").into());
            }
            Ok(Err(e)) => {
                self.busy = false;
                self.gen_rx = None;
                self.error = Some(e);
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.gen_rx = None;
            }
        }
    }

    fn poll_histogram(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.hist_rx else { return };
        match rx.try_recv() {
            Ok(h) => {
                self.histogram = Some(h);
                self.hist_rx = None;
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(TryRecvError::Disconnected) => {
                self.hist_rx = None;
            }
        }
    }

    // --- eksport / projekt ------------------------------------------------------

    fn export_txt(&mut self) {
        if self.objects.is_empty() {
            self.error = Some("Brak wygenerowanych obiektów.".into());
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Zapisz plik dla Terrain Buildera")
            .add_filter("Terrain Builder TXT", &["txt"])
            .set_file_name("obiekty_tb.txt")
            .save_file()
        {
            match write_tb_file(&self.objects, &self.project, path) {
                Ok(n) => self.info = Some(format!("Zapisano {n} obiektów.")),
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Eksport warstwy drzew jako przezroczysty PNG (rozdzielczość wg rozmiaru mapy).
    fn export_trees_png(&mut self) {
        if self.objects.is_empty() {
            self.error = Some("Brak wygenerowanych obiektów.".into());
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Eksport warstwy drzew (PNG)")
            .add_filter("PNG", &["png"])
            .set_file_name("warstwa_drzew.png")
            .save_file()
        else {
            return;
        };
        // rozdzielczość = rozmiar mapy w metrach (1 px = 1 m), z ograniczeniem
        let res = (self.project.map_size_m as u32).clamp(64, 16384);
        let colors: Vec<[u8; 3]> = self
            .project
            .species
            .iter()
            .enumerate()
            .map(|(i, s)| s.effective_color(i))
            .collect();
        match forest_core::export_trees_png(&self.objects, &self.project, &colors, res, path) {
            Ok(n) => self.info = Some(format!("Zapisano warstwę drzew ({n} obiektów, {res} px).")),
            Err(e) => self.error = Some(e),
        }
    }

    fn save_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Projekt DayZForests", &["json"])
            .set_file_name("projekt_lasu.json")
            .save_file()
        {
            let path = with_extension(path, "json");
            match self.project.save(&path) {
                Ok(()) => self.info = Some(format!("Zapisano projekt: {}", path.display())),
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn load_project(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Projekt DayZForests", &["json"])
            .pick_file()
        {
            match ForestProject::load(&path) {
                Ok(mut p) => {
                    p.sanitize();
                    self.project = p;
                    // wyczyść stan z poprzedniego projektu, żeby pliki nowego
                    // zawsze się przeładowały (nawet gdy ścieżki są inne)
                    self.mask = None;
                    self.mask_tex = None;
                    self.sat_tex = None;
                    self.sat_image = None;
                    self.heightmap = None;
                    self.exclusions = None;
                    self.histogram = None;
                    self.hist_rx = None;
                    self.overlay_tex = None;
                    self.objects.clear();
                    self.stats = None;
                    self.zone_color_edit = None;
                    self.sampling_area = None;
                    self.area_copy_src = vec![None; self.project.areas.len()];
                    self.editing_area = None;
                    self.editing_vertex = None;
                    self.draw_mode = false;
                    self.draw_points.clear();
                    self.info = Some(tf(lang, "Wczytano projekt: {0}", &[path.display().to_string().as_str()]));
                    self.reload_project_files(ctx);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Doczytuje maskę/podkład/ASC/GeoJSON wskazane przez projekt (jeśli istnieją).
    fn reload_project_files(&mut self, ctx: &egui::Context) {
        if let Some(m) = self.project.paths.mask.clone() {
            if std::path::Path::new(&m).exists() {
                self.load_mask(ctx, &m);
            } else {
                self.error = Some(format!("Plik maski nie istnieje: {m}"));
            }
        }
        if let Some(s) = self.project.paths.satellite.clone() {
            if std::path::Path::new(&s).exists() {
                self.load_satellite(ctx, &s);
            } else {
                self.info = Some(format!(
                    "Podkład satelitarny nie istnieje (pomijam): {s}"
                ));
            }
        }
        if let Some(a) = self.project.paths.heightmap_asc.clone() {
            if std::path::Path::new(&a).exists() {
                self.load_heightmap(&a);
            } else {
                self.error = Some(format!("Plik heightmapy nie istnieje: {a}"));
            }
        }
        if let Some(g) = self.project.paths.exclusions_geojson.clone() {
            if std::path::Path::new(&g).exists() {
                self.load_exclusions(&g);
            } else {
                self.error = Some(format!("Plik wykluczeń nie istnieje: {g}"));
            }
        }
    }

    fn add_zone_from_color(&mut self, color: Rgb8) {
        let n_species = self.project.species.len();
        self.project.zones.push(ZoneDef {
            color,
            label: format!("Strefa {}", self.project.zones.len() + 1),
            density_per_ha: 220.0,
            species_weights: (0..n_species).map(|i| (i, 1.0)).collect(),
            preset_mix: Vec::new(),
        });
    }

    /// Pierwszy wolny kolor: nieużywany przez strefy/wykluczenia, z histogramu
    /// maski; awaryjnie z palety syntetycznej.
    fn pick_zone_color(&self) -> Option<Rgb8> {
        let used: Vec<Rgb8> = self
            .project
            .zones
            .iter()
            .map(|z| z.color)
            .chain(self.project.exclusion_colors.iter().copied())
            .collect();
        if let Some(h) = &self.histogram {
            for (c, _) in h.iter() {
                if !used.contains(c) {
                    return Some(*c);
                }
            }
        }
        for c in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [255, 0, 255],
            [0, 255, 255],
            [128, 0, 0],
            [0, 128, 0],
            [0, 0, 128],
            [255, 128, 0],
            [128, 0, 255],
            [0, 128, 128],
        ] {
            let c = Rgb8(c);
            if !used.contains(&c) {
                return Some(c);
            }
        }
        None
    }

    fn add_zone_from_preset(&mut self, preset: forest_core::species::ZonePreset) {
        let snap = UserPreset::from_builtin(&preset);
        self.add_zone_from_snap(&PSnap {
            name: snap.name,
            density_per_ha: snap.density_per_ha,
            weights: snap.weights,
        });
    }

    // --- obszary rysowane (poligony) -----------------------------------------

    fn finish_area(&mut self) {
        let lang = self.lang;
        if self.draw_points.len() < 3 {
            self.error = Some("Poligon wymaga >= 3 punktów.".into());
            return;
        }
        if forest_core::scatter::polygon_self_intersects(&self.draw_points) {
            self.error = Some(
                "Obrys przecina sam siebie (odcinki się krzyżują) — cofnij (Backspace) \
                 lub przesuń wierzchołki tak, aby linia nie przecinała samej siebie."
                    .into(),
            );
            return;
        }
        let label = format!("{} {}", tr(self.lang, "Obszar"), self.project.areas.len() + 1);
        let area_ha = forest_core::scatter::polygon_area_m2(&self.draw_points) / 10_000.0;
        // obszar startuje nieskonfigurowany: 0 obiektów, dopóki nie dodasz
        // presetów (🧩 Miks presetów) w panelu Obszary
        self.project.areas.push(forest_core::preset::AreaDef {
            enabled: true,
            label,
            density_per_ha: 0.0,
            species_weights: Vec::new(),
            preset_mix: Vec::new(),
            edges: None,
            color_filter: None,
            polygon: std::mem::take(&mut self.draw_points),
        });
        self.area_copy_src.push(None);
        let area_label = self.project.areas.last().unwrap().label.clone();
        let ha_str = format!("{:.1}", area_ha);
        self.info = Some(tf(lang, "Dodano obszar '{0}' ({1:.1} ha). Przypisz presety w panelu 📐 Obszary, aby generował drzewa.", &[area_label.as_str(), ha_str.as_str()]));
    }

    fn cancel_drawing(&mut self) {
        self.draw_mode = false;
        self.draw_points.clear();
    }
}

// --- tekstury / nakładka ------------------------------------------------------

/// Maksymalny bok tekstur podglądu (VRAM/GPU-safe). Generowanie zawsze używa
/// pełnej rozdzielczości maski — to dotyczy tylko wyświetlania.
const PREVIEW_MAX_DIM: u32 = 4096;

fn preview_dims(m: &MaskImage, max_dim: u32) -> (u32, u32) {
    let scale = (f64::from(max_dim) / f64::from(m.width.max(m.height))).min(1.0);
    let w = ((f64::from(m.width) * scale).round() as u32).max(1);
    let h = ((f64::from(m.height) * scale).round() as u32).max(1);
    (w, h)
}

fn thumbnail_rgba(m: &MaskImage, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        let sy = ((u64::from(y) * u64::from(m.height)) / u64::from(h)) as u32;
        for x in 0..w {
            let sx = ((u64::from(x) * u64::from(m.width)) / u64::from(w)) as u32;
            let si = ((sy * m.width + sx) * 4) as usize;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&m.rgba[si..si + 4]);
        }
    }
    out
}

fn preview_texture(
    ctx: &egui::Context,
    name: &str,
    m: &MaskImage,
    options: TextureOptions,
) -> TextureHandle {
    let (w, h) = preview_dims(m, PREVIEW_MAX_DIM);
    let rgba = thumbnail_rgba(m, w, h);
    ctx.load_texture(name, ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), options)
}

fn build_overlay(
    disp_w: u32,
    disp_h: u32,
    objects: &[PlacedObject],
    project: &ForestProject,
) -> ColorImage {
    let mut rgba = vec![0u8; (disp_w as usize) * (disp_h as usize) * 4];
    // świat -> piksele podglądu (origin SW, wiersz 0 tekstury = północ)
    let mx = f64::from(disp_w) / project.map_size_m;
    let my = f64::from(disp_h) / project.map_size_m;
    let stencil = dot_stencil(2.0_f32);
    // kolory efektywne gatunków (indywidualne lub automatyczne)
    let colors: Vec<[u8; 3]> = project
        .species
        .iter()
        .enumerate()
        .map(|(i, s)| s.effective_color(i))
        .collect();
    for o in objects {
        let px = (o.x * mx) as i64;
        let py = ((project.map_size_m - o.y) * my) as i64;
        let c = colors.get(o.species_index).copied().unwrap_or([255; 3]);
        for &(dx, dy) in &stencil {
            let x = px + dx;
            let y = py + dy;
            if x >= 0 && y >= 0 && (x as u32) < disp_w && (y as u32) < disp_h {
                let idx = ((y as usize) * (disp_w as usize) + (x as usize)) * 4;
                rgba[idx] = c[0];
                rgba[idx + 1] = c[1];
                rgba[idx + 2] = c[2];
                rgba[idx + 3] = 255;
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([disp_w as usize, disp_h as usize], &rgba)
}

/// Odległość punktu od odcinka (ekran), do chwytania krawędzi poligonu.
fn seg_dist_screen(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let denom = abx * abx + aby * aby;
    let t = if denom <= f32::EPSILON {
        0.0
    } else {
        ((apx * abx + apy * aby) / denom).clamp(0.0, 1.0)
    };
    let dx = apx - t * abx;
    let dy = apy - t * aby;
    dx.hypot(dy)
}
/// Wzorzec dysku (koła) do rysowania obiektów na nakładce podglądu.
fn dot_stencil(radius: f32) -> Vec<(i64, i64)> {
    let ri = radius.ceil() as i64;
    let r2 = radius * radius;
    let mut v = Vec::new();
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            if ((dx * dx + dy * dy) as f32) <= r2 + 0.25 {
                v.push((dx, dy));
            }
        }
    }
    v
}

fn with_extension(mut path: std::path::PathBuf, ext: &str) -> std::path::PathBuf {
    path.set_extension(ext);
    path
}

/// Wiersze miksu presetów: udział + usuwanie, poniżej combo dodawania.
/// Zwraca true, gdy coś zmieniono. `presets` = łączna lista (nazwa → snap).
fn ui_mix_rows(
    ui: &mut egui::Ui,
    lang: Lang,
    id: &str,
    mix: &mut Vec<(String, f32)>,
    presets: &[(String, PSnap)],
) -> bool {
    let mut changed = false;
    let mut remove: Option<usize> = None;
    for (i, (name, share)) in mix.iter_mut().enumerate() {
        let density = presets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.density_per_ha);
        ui.horizontal(|ui| {
            ui.monospace(tr(lang, name.as_str()));
            if let Some(d) = density {
                ui.weak(format!("{d:.0}/ha"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let before = *share;
                ui.add(
                    egui::DragValue::new(share)
                        .speed(0.01)
                        .clamp_range(0.05..=1.0)
                        .suffix("×"),
                );
                if (*share - before).abs() > f32::EPSILON {
                    changed = true;
                }
                if ui.button("✖").clicked() {
                    remove = Some(i);
                    changed = true;
                }
            });
        });
    }
    if let Some(i) = remove {
        mix.remove(i);
    }

    // dodawanie presetu do miksu — jedno kliknięcie na pozycji listy
    ui.horizontal(|ui| {
        ui.label(tr(lang, "Dodaj preset:"));
        egui::ComboBox::from_id_source(egui::Id::new(format!("mix_add_{id}")))
            .selected_text(tr(lang, "wybierz z listy..."))
            .show_ui(ui, |ui| {
                for (n, s) in presets.iter() {
                    let already = mix.iter().any(|(m, _)| m == n);
                    let label = if already {
                        format!("✓ {}", tr(lang, n))
                    } else {
                        format!("{}  ({:.0}/ha)", tr(lang, n), s.density_per_ha)
                    };
                    if ui
                        .add_enabled(!already, egui::Button::new(label))
                        .clicked()
                    {
                        mix.push((n.clone(), 1.0));
                        changed = true;
                    }
                }
            });
        if mix.len() > 1 {
            ui.small(tr(
                lang,
                "(udziały → sumują się do dowolnej wartości — liczone proporcjonalnie)",
            ));
        }
    });
    changed
}

/// Efekt miksu: średnia ważona gęstość + suma wag gatunków (wg modelu).
fn compute_mix_snap(
    mix: &[(String, f32)],
    presets: &[(String, PSnap)],
) -> Option<PSnap> {
    let mut total_share = 0.0f32;
    let mut density = 0.0f32;
    use std::collections::HashMap as HM;
    let mut wmap: HM<String, f32> = HM::new();
    for (name, share) in mix {
        let Some(p) = presets.iter().find(|(n, _)| n == name) else { continue };
        total_share += share;
        density += p.1.density_per_ha * share;
        for (model, w) in &p.1.weights {
            *wmap.entry(model.clone()).or_insert(0.0) += w * share;
        }
    }
    if total_share <= 0.0 || wmap.is_empty() {
        return None;
    }
    Some(PSnap {
        name: "Miks".into(),
        density_per_ha: density / total_share,
        weights: wmap.into_iter().filter(|(_, w)| *w > 0.0).collect(),
    })
}

// --- pola ustawień z indywidualnym przywracaniem domyślnych -------------------

/// Pomarańczowy kolor zmienionego ustawienia.
const ORANGE_MOD: Color32 = Color32::from_rgb(255, 150, 40);

fn lbl_mod(ui: &mut egui::Ui, label: String, modified: bool) {
    if modified {
        ui.colored_label(ORANGE_MOD, label);
    } else {
        ui.label(label);
    }
}

/// Przycisk ⟲ widoczny tylko gdy wartość ≠ domyślnej. Zwraca true po kliknięciu.
fn reset_btn(ui: &mut egui::Ui, modified: bool) -> bool {
    modified
        && ui
            .small_button("⟲")
            .on_hover_text("Przywróć wartość domyślną")
            .clicked()
}

/// ui.horizontal z tooltipem na całym wierszu.
fn horiz_tip<R>(
    ui: &mut egui::Ui,
    tip: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let r = ui.horizontal(add);
    if !tip.is_empty() {
        r.response.on_hover_text(tip);
    }
    r.inner
}

fn setting_bool(ui: &mut egui::Ui, label: String, tip: &str, v: &mut bool, d: bool) {
    ui.horizontal(|ui| {
        let m = *v != d;
        let _ = ui.checkbox(v, label).on_hover_text(tip).changed();
        if reset_btn(ui, m) {
            *v = d;
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn setting_f64(
    ui: &mut egui::Ui,
    label: String,
    tip: &str,
    v: &mut f64,
    d: f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) {
    ui.horizontal(|ui| {
        let m = (*v - d).abs() > 1e-9;
        lbl_mod(ui, label, m);
        let resp = ui.add(
            egui::DragValue::new(v).speed(speed).clamp_range(range),
        );
        if !tip.is_empty() {
            resp.on_hover_text(tip);
        }
        if reset_btn(ui, m) {
            *v = d;
        }
    });
}

fn setting_u64(ui: &mut egui::Ui, label: String, v: &mut u64, d: u64) {
    ui.horizontal(|ui| {
        let m = *v != d;
        lbl_mod(ui, label, m);
        ui.add(egui::DragValue::new(v).speed(1.0).clamp_range(0..=u64::MAX));
        if reset_btn(ui, m) {
            *v = d;
        }
    });
}

fn setting_u32(ui: &mut egui::Ui, label: String, v: &mut u32, d: u32, range: std::ops::RangeInclusive<u32>) {
    ui.horizontal(|ui| {
        let m = *v != d;
        lbl_mod(ui, label, m);
        ui.add(egui::DragValue::new(v).clamp_range(range));
        if reset_btn(ui, m) {
            *v = d;
        }
    });
}

/// Wspólny edytor ustawień granicy (globalnej lub per-obszar).
/// `d` = wartości odniesienia dla przycisków ⟲.
fn ui_edge_settings(
        ui: &mut egui::Ui,
        lang: Lang,
    e: &mut EdgeSettings,
    d: &EdgeSettings,
    species: &[forest_core::species::SpeciesDef],
) {
    let n_species = species.len();
    setting_bool(
        ui,
        tr(lang, "Włącz pas graniczny (krzewy wzdłuż krawędzi lasu)"),
        "",
        &mut e.enabled,
        d.enabled,
    );
    ui.add_enabled_ui(e.enabled, |ui| {
        ui.horizontal(|ui| {
            let m = (e.band_width_m - d.band_width_m).abs() > 1e-9;
            lbl_mod(ui, tr(lang, "Szerokość pasa [m]:"), m);
            ui.add(egui::DragValue::new(&mut e.band_width_m).speed(1.0).clamp_range(1.0..=300.0));
            if reset_btn(ui, m) {
                e.band_width_m = d.band_width_m;
            }
        });
        ui.horizontal(|ui| {
            let m = (e.density_per_ha - d.density_per_ha).abs() > 1e-9;
            lbl_mod(ui, tr(lang, "Gęstość [szt/ha]:"), m);
            ui.add(egui::DragValue::new(&mut e.density_per_ha).speed(1.0).clamp_range(1.0..=5000.0));
            if reset_btn(ui, m) {
                e.density_per_ha = d.density_per_ha;
            }
        });
        ui.horizontal(|ui| {
            let m = e.blend != d.blend;
            let _ = ui
                .checkbox(&mut e.blend, tr(lang, "Wtapianie pasa"))
                .on_hover_text(
                    "Gęstość zanika wraz z odległością od granicy \
                     (najgęściej przy samej krawędzi lasu) — bez twardej linii krzaków",
                )
                .changed();
            if reset_btn(ui, m) {
                e.blend = d.blend;
            }
        });
        horiz_tip(
            ui,
            "Szum przesuwający linię lasu — brzeg nie jest równy jak od linijki. \
             Dotyczy też obrysów poligonów.",
            |ui| {
                let m = (e.jagged_m - d.jagged_m).abs() > 1e-9;
                lbl_mod(ui, tr(lang, "Poszarpanie granicy [m]:"), m);
                ui.add(egui::DragValue::new(&mut e.jagged_m).speed(1.0).clamp_range(0.0..=200.0));
                if reset_btn(ui, m) {
                    e.jagged_m = d.jagged_m;
                }
            },
        );
        horiz_tip(
            ui,
            "Jak głęboko od krawędzi gęstość drzew narasta 0 -> pełna. \
             Otwarte, naturalne obrzeża lasu.",
            |ui| {
                let m = (e.blend_inside_m - d.blend_inside_m).abs() > 1e-9;
                lbl_mod(ui, tr(lang, "Wtapianie w las [m]:"), m);
                ui.add(
                    egui::DragValue::new(&mut e.blend_inside_m)
                        .speed(1.0)
                        .clamp_range(0.0..=500.0),
                );
                if reset_btn(ui, m) {
                    e.blend_inside_m = d.blend_inside_m;
                }
            },
        );
        ui.horizontal(|ui| {
            ui.small(tr(lang, "Wagi gatunków granicy:"));
            let m = e.species_weights != d.species_weights;
            if reset_btn(ui, m) {
                // przywróć domyślne krzewy, odfiltrowując spoza listy gatunków
                e.species_weights = d
                    .species_weights
                    .iter()
                    .filter(|(i, _)| (*i as usize) < n_species)
                    .copied()
                    .collect();
            }
        });
        for si in 0..n_species {
            let sp = &species[si];
            let c = sp.effective_color(si);
            ui.horizontal(|ui| {
                let (_r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter_at(resp.rect).circle_filled(
                    resp.rect.center(),
                    4.0,
                    Color32::from_rgb(c[0], c[1], c[2]),
                );
                ui.label(sp.label.as_str());
                let pos = e.species_weights.iter().position(|(i, _)| *i == si);
                let mut w = pos.map_or(0.0, |k| e.species_weights[k].1);
                let before = w;
                ui.add(egui::DragValue::new(&mut w).speed(0.05).clamp_range(0.0..=20.0));
                if (w - before).abs() > f32::EPSILON {
                    match pos {
                        Some(k) => {
                            if w <= 0.0 {
                                e.species_weights.remove(k);
                            } else {
                                e.species_weights[k].1 = w;
                            }
                        }
                        None => {
                            if w > 0.0 {
                                e.species_weights.push((si, w));
                            }
                        }
                    }
                }
            });
        }
    });
}

// --- App impl -----------------------------------------------------------------

impl ForestApp {
    
    


    /// Łączna lista presetów jako snapshoty — UŻYTKOWNIKA NAJPIERW,
    /// dzięki czemu wyszukiwanie po nazwie preferuje edytowane kopie.
    fn all_presets(&self) -> Vec<PSnap> {
        let mut v: Vec<PSnap> = self
            .user_presets
            .iter()
            .map(|u| PSnap {
                name: u.name.clone(),
                density_per_ha: u.density_per_ha,
                weights: u.weights.clone(),
            })
            .collect();
        for p in forest_core::species::zone_presets() {
            let up = UserPreset::from_builtin(&p);
            v.push(PSnap {
                name: up.name,
                density_per_ha: up.density_per_ha,
                weights: up.weights,
            });
        }
        v
    }

    
    fn load_user_presets(&mut self) {
        match UserPresetLibrary::load(DEFAULT_PRESETS_FILE) {
            Ok(lib) => {
                if !lib.presets.is_empty() {
                    self.info = Some(format!(
                        "Wczytano {} presetów użytkownika ({}).",
                        lib.presets.len(),
                        DEFAULT_PRESETS_FILE
                    ));
                }
                self.user_presets = lib.presets;
            }
            Err(e) => {
                self.error = Some(format!("{e} — pomijam plik."));
            }
        }
    }

    fn save_user_presets(&mut self) {
        let lib = UserPresetLibrary {
            presets: self.user_presets.clone(),
        };
        match lib.save(DEFAULT_PRESETS_FILE) {
            Ok(()) => {
                self.presets_dirty = false;
                self.info = Some(format!("Presety zapisane: {DEFAULT_PRESETS_FILE}"));
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn export_species(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("species.json")
            .save_file()
        {
            match serde_json::to_string_pretty(&self.project.species) {
                Ok(json) => match std::fs::write(&path, json) {
                    Ok(()) => {
                        self.info = Some(tf(
                            lang,
                            "Wyeksportowano {0} gatunków do {1}.",
                            &[&self.project.species.len().to_string(), &path.display().to_string()],
                        ));
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                },
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn import_species(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // spróbuj Vec<SpeciesDef> lub UserPresetLibrary-like wrapper
                    let parsed: Result<Vec<SpeciesDef>, _> = serde_json::from_str(&text);
                    match parsed {
                        Ok(imported) => {
                            let mut added = 0;
                            let mut skipped = 0;
                            for sp in imported {
                                if self
                                    .project
                                    .species
                                    .iter()
                                    .any(|s| s.model.eq_ignore_ascii_case(&sp.model))
                                {
                                    skipped += 1;
                                } else {
                                    self.project.species.push(sp);
                                    added += 1;
                                }
                            }
                            self.info = Some(tf(
                                lang,
                                "Zaimportowano {0} gatunków (pominięto {1} duplikatów) z {2}.",
                                &[
                                    &added.to_string(),
                                    &skipped.to_string(),
                                    &path.display().to_string(),
                                ],
                            ));
                            self.error = None;
                        }
                        Err(e) => self.error = Some(format!("Parse: {e}")),
                    }
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn export_zones(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("zones.json")
            .save_file()
        {
            match serde_json::to_string_pretty(&self.project.zones) {
                Ok(json) => match std::fs::write(&path, json) {
                    Ok(()) => {
                        self.info = Some(tf(
                            lang,
                            "Wyeksportowano {0} stref do {1}.",
                            &[&self.project.zones.len().to_string(), &path.display().to_string()],
                        ));
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                },
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn import_zones(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<Vec<ZoneDef>>(&text) {
                    Ok(mut imported) => {
                        let n = imported.len();
                        // walidacja kolorów: pomiń duplikaty kolorów już istniejących
                        let existing_colors: Vec<Rgb8> =
                            self.project.zones.iter().map(|z| z.color).collect();
                        imported.retain(|z| !existing_colors.contains(&z.color));
                        let added = imported.len();
                        let skipped = n - added;
                        self.project.zones.extend(imported);
                        self.info = Some(tf(
                            lang,
                            "Zaimportowano {0} stref (pominięto {1} duplikatów) z {2}.",
                            &[&added.to_string(), &skipped.to_string(), &path.display().to_string()],
                        ));
                        self.error = None;
                    }
                    Err(e) => self.error = Some(format!("Parse: {e}")),
                },
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn export_presets_dialog(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("presets.json")
            .save_file()
        {
            let lib = UserPresetLibrary {
                presets: self.user_presets.clone(),
            };
            match serde_json::to_string_pretty(&lib) {
                Ok(json) => match std::fs::write(&path, json) {
                    Ok(()) => {
                        self.info = Some(tf(
                            lang,
                            "Wyeksportowano {0} presetów do {1}.",
                            &[&self.user_presets.len().to_string(), &path.display().to_string()],
                        ));
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                },
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn import_presets_dialog(&mut self) {
        let lang = self.lang;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // spróbuj jako UserPresetLibrary lub Vec<UserPreset>
                    let parsed_lib: Result<UserPresetLibrary, _> = serde_json::from_str(&text);
                    let imported = if let Ok(lib) = parsed_lib {
                        lib.presets
                    } else {
                        match serde_json::from_str::<Vec<UserPreset>>(&text) {
                            Ok(v) => v,
                            Err(e) => {
                                self.error = Some(format!("Parse: {e}"));
                                return;
                            }
                        }
                    };
                    let n = imported.len();
                    let mut added = 0;
                    let mut skipped = 0;
                    for p in imported {
                        if self
                            .user_presets
                            .iter()
                            .any(|u| u.name == p.name && u.group == p.group)
                        {
                            skipped += 1;
                        } else {
                            self.user_presets.push(p);
                            added += 1;
                        }
                    }
                    self.presets_dirty = true;
                    self.info = Some(tf(
                        lang,
                        "Zaimportowano {0} presetów (pominięto {1} duplikatów) z {2}.",
                        &[&added.to_string(), &skipped.to_string(), &path.display().to_string()],
                    ));
                    self.error = None;
                    let _ = n;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    /// Dodaje strefę na bazie znormalizowanego presetu (modele → indeksy).
    fn add_zone_from_snap(&mut self, snap: &PSnap) {
        let Some(color) = self.pick_zone_color() else {
            self.error =
                Some("Brak wolnych kolorów na nową strefę (maska wyczerpana).".into());
            return;
        };
        let species = self.project.species.clone();
        // mapowanie nazwa modelu -> indeks
        let mut zw = Vec::new();
        for (model, w) in &snap.weights {
            if let Some(idx) = species
                .iter()
                .position(|s| s.model.eq_ignore_ascii_case(model))
            {
                zw.push((idx, *w));
            }
        }
        if zw.is_empty() {
            self.error = Some(format!(
                "Preset '{}': żaden z jego modeli nie występuje na liście gatunków.",
                snap.name
            ));
            return;
        }
        self.project.zones.push(ZoneDef {
            color,
            label: snap.name.clone(),
            density_per_ha: snap.density_per_ha,
            species_weights: zw,
            preset_mix: Vec::new(),
        });
    }
}

impl eframe::App for ForestApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let lang = self.lang;

        self.poll_generation(ctx);
        self.poll_histogram(ctx);

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    let p = *self.progress.lock().unwrap();
                    ui.add(
                        egui::ProgressBar::new(p as f32)
                            .show_percentage()
                            .desired_width(220.0),
                    );
                    ui.label(tr(lang, "Generowanie..."));
                } else if let Some(s) = &self.stats {
                    ui.label(format!(
                        "Obiektów: {} | granica: {} | gatunków >0: {} | {} ms",
                        s.total,
                        s.edge_count,
                        s.per_species.iter().filter(|(_, c)| *c > 0).count(),
                        s.elapsed_ms
                    ));
                } else {
                    ui.label(tr(lang, "Gotowy."));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(e) = &self.error {
                        ui.colored_label(Color32::RED, format!("⚠ {e}"));
                    } else if let Some(i) = &self.info {
                        ui.weak(i);
                    }
                });
            });
        });

        egui::SidePanel::left("params")
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    self.ui_params(ui);
                });
            });

 
        egui::SidePanel::right("presety_warstwy")
            .resizable(true)
            .default_width(370.0)
            .show(ctx, |ui| {
                self.ui_presets_layers(ui);
            });       egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_toolbar(ui);
            self.ui_canvas(ui);
        });
    }
}

impl ForestApp {
    fn ui_toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.horizontal_wrapped(|ui| {
            // 🌐 język
            egui::ComboBox::from_id_source("lang_sel")
                .selected_text(format!("🌐 {}", lang.name()))
                .show_ui(ui, |ui| {
                    for l in Lang::ALL {
                        let selected = l == lang;
                        if ui
                            .selectable_label(selected, format!("🌐 {}", l.name()))
                            .clicked()
                            && !selected
                        {
                            self.lang = l;
                            i18n::UiPrefs {
                                language: l.code().into(),
                            }
                            .save(i18n::PREFS_FILE);
                        }
                    }
                });
            ui.separator();
            ui.add_enabled(!self.busy, egui::Button::new(tr(lang, "▶ Generuj")))
                .clicked()
                .then(|| self.start_generation(ui.ctx()));
            ui.add_enabled(
                !self.objects.is_empty(),
                egui::Button::new(tr(lang, "💾 Eksport TXT (TB)")),
            )
            .clicked()
            .then(|| self.export_txt());
            ui.add_enabled(
                !self.objects.is_empty(),
                egui::Button::new(tr(lang, "🖼 Eksport PNG (drzewa)")),
            )
            .clicked()
            .then(|| self.export_trees_png());
            ui.separator();
            if ui.button(tr(lang, "📂 Wczytaj projekt")).clicked() {
                self.load_project(ui.ctx());
            }
            if ui.button(tr(lang, "💾 Zapisz projekt")).clicked() {
                self.save_project();
            }
            ui.separator();
            // obszar powstaje "pusty" — presety przypisuje się w panelu Obszary
            if ui
                .add(egui::Button::new(if self.draw_mode {
                    tr(lang, "✏ Rysowanie: WŁ")
                } else {
                    tr(lang, "✏ Rysuj obszar")
                }))
                .clicked()
            {
                self.draw_mode = !self.draw_mode;
                if !self.draw_mode {
                    self.draw_points.clear();
                }
            }
            if !self.draw_points.is_empty() {
                ui.colored_label(
                    Color32::YELLOW,
                    format!("{} {}", tr(self.lang, "pkt:"), self.draw_points.len()),
                );
                if ui.button("✔ Zakończ").clicked() {
                    self.finish_area();
                }
                if ui.button("⟲ Cofnij pkt").clicked() {
                    self.draw_points.pop();
                }
                if ui.button("✖ Anuluj").clicked() {
                    self.cancel_drawing();
                }
            }
            ui.separator();
            ui.checkbox(&mut self.show_sat, tr(lang, "Podkład"));
            ui.checkbox(&mut self.show_mask, tr(lang, "Maska"));
            ui.checkbox(&mut self.show_trees, tr(lang, "Drzewa"));
            if ui.button(tr(lang, "Dopasuj widok")).clicked() {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
        });
        ui.separator();
    }

    fn ui_params(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;

        // Pliki
        CollapsingHeader::new(tr(lang, "📁 Pliki"))
            .default_open(true)
            .show(ui, |ui| {
                if ui.button(tr(lang, "Wczytaj maskę (PNG/BMP/TGA)...")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Obrazy", &["png", "bmp", "tga", "jpg", "jpeg"])
                        .pick_file()
                    {
                        self.load_mask(ui.ctx(), &p.to_string_lossy());
                    }
                }
                ui.label(match self.project.paths.mask.as_deref() {
                    Some(p) => p.to_string(),
                    None => tr(lang, "(brak maski)"),
                })
                    .on_hover_text("Kolory = strefy lasu");

                if ui.button(tr(lang, "Wczytaj podkład satelitarny (PNG/JPG)...")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Obrazy", &["png", "jpg", "jpeg", "bmp", "tga"])
                        .pick_file()
                    {
                        self.load_satellite(ui.ctx(), &p.to_string_lossy());
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Krycie podkładu:"));
                    ui.add(
                        egui::Slider::new(&mut self.sat_alpha, 0.0..=1.0).text(if self.sat_tex.is_some() { "".to_string() } else { tr(lang, "(brak)") }),
                    );
                });

                if ui.button(tr(lang, "Wczytaj heightmapę (.asc)...")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("ESRI ASCII Grid", &["asc"])
                        .pick_file()
                    {
                        self.load_heightmap(&p.to_string_lossy());
                    }
                }
                ui.label(
                    self.project
                        .paths
                        .heightmap_asc
                        .as_deref()
                        .map(|s| s.to_string())
                        .unwrap_or(tr(lang, "(brak — elevation=0, obiekty na terenie)")),
                );

                if ui.button(tr(lang, "Wczytaj wykluczenia (.geojson)...")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("GeoJSON", &["geojson", "json"])
                        .pick_file()
                    {
                        self.load_exclusions(&p.to_string_lossy());
                    }
                }
                ui.label(
                    self.project
                        .paths
                        .exclusions_geojson
                        .as_deref()
                        .map(|s| s.to_string())
                        .unwrap_or(tr(lang, "(brak wykluczeń wektorowych)")),
                );

                if let Some(m) = &self.mask {
                    ui.separator();
                    ui.small(tf(
                        lang,
                        "Maska: {0}x{1} px",
                        &[&m.width.to_string(), &m.height.to_string()],
                    ));
                }
            });

        // Mapa
        CollapsingHeader::new(tr(lang, "🗺 Mapa / eksport"))
            .default_open(true)
            .show(ui, |ui| {
                let d = ForestProject::default();
                let p = &mut self.project;
                ui.horizontal(|ui| {
                    let m = (p.map_size_m - d.map_size_m).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Rozmiar [m]:"), m);
                    ui.add(egui::DragValue::new(&mut p.map_size_m).speed(10.0));
                    if reset_btn(ui, m) {
                        p.map_size_m = d.map_size_m;
                    }
                });
                ui.horizontal(|ui| {
                    let m = (p.easting_offset - d.easting_offset).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Easting offset:"), m);
                    ui.add(egui::DragValue::new(&mut p.easting_offset).speed(100.0));
                    if reset_btn(ui, m) {
                        p.easting_offset = d.easting_offset;
                    }
                });
                ui.horizontal(|ui| {
                    let m = (p.northing_offset - d.northing_offset).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Northing offset:"), m);
                    ui.add(egui::DragValue::new(&mut p.northing_offset).speed(100.0));
                    if reset_btn(ui, m) {
                        p.northing_offset = d.northing_offset;
                    }
                });
                setting_u64(ui, tr(lang, "Ziarno (seed):"), &mut p.seed, d.seed);
                ui.horizontal(|ui| {
                    let m = p.elevation_mode != d.elevation_mode;
                    lbl_mod(ui, tr(lang, "Wysokość:"), m);
                    egui::ComboBox::from_id_source("elev_mode")
                        .selected_text(match p.elevation_mode {
                            ElevationMode::RelativeZero => tr(lang, "relative (0)"),
                            ElevationMode::AbsoluteSampled => tr(lang, "absolute (z ASC)"),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut p.elevation_mode,
                                ElevationMode::RelativeZero,
                                tr(lang, "relative (0)"),
                            );
                            ui.selectable_value(
                                &mut p.elevation_mode,
                                ElevationMode::AbsoluteSampled,
                                tr(lang, "absolute (z ASC)"),
                            );
                        });
                    if reset_btn(ui, m) {
                        p.elevation_mode = d.elevation_mode;
                    }
                });
            });

        // Źródła generowania
        CollapsingHeader::new(tr(lang, "⚙ Źródła generowania"))
            .default_open(true)
            .show(ui, |ui| {
                let d = ForestProject::default();
                setting_bool(
                    ui,
                    tr(lang, "Strefy z maski (kolory)"),
                    "Generuj w obszarach wskazanych kolorami maski",
                    &mut self.project.use_mask_zones,
                    d.use_mask_zones,
                );
                setting_bool(
                    ui,
                    tr(lang, "Obszary rysowane (poligony)"),
                    "Generuj wewnątrz narysowanych poligonów",
                    &mut self.project.use_areas,
                    d.use_areas,
                );
                setting_bool(
                    ui,
                    tr(lang, "Granica lasu (krzewy)"),
                    "Pas krzewów/podrostu wzdłuż krawędzi lasu (parametry w sekcji 🧱)",
                    &mut self.project.edges.enabled,
                    false,
                );
                ui.separator();
                {
                    let src0 = format!(
                        "{} {}{}{}",
                        tr(lang, "Aktywne źródła:"),
                        if self.project.use_mask_zones {
                            tr(lang, "maska")
                        } else {
                            String::new()
                        },
                        if self.project.use_areas {
                            format!(" {}", tr(lang, "poligony"))
                        } else {
                            String::new()
                        },
                        if self.project.edges.enabled {
                            format!(" {}", tr(lang, "+ granica"))
                        } else {
                            String::new()
                        }
                    );
                    ui.small(src0);
                }
                ui.small(tf(
                    lang,
                    "Ziarno: {0} | mnożnik odstępów: {1} | polany: {2}",
                    &[
                        &self.project.seed.to_string(),
                        &format!("{:.2}", self.project.spacing_multiplier),
                        &format!("{:.0}%", self.project.clearing_strength * 100.0),
                    ],
                ));
            });


        // Granica lasu (krzewy/podrost)
        CollapsingHeader::new(tr(lang, "🧱 Granica lasu"))
            .default_open(false)
            .show(ui, |ui| {
                let d_edges = EdgeSettings::default();
                let species_snap = self.project.species.clone();
                ui_edge_settings(ui, self.lang, &mut self.project.edges, &d_edges, &species_snap);
            });

        // Wycinanie po kolorach maski (+ bufor) — usuwa wygenerowane obiekty
        CollapsingHeader::new(tr(lang, "✂ Wycinanie (kolor + bufor)"))
            .default_open(false)
            .show(ui, |ui| {
                ui.small(tr(
                    lang,
                    "Usuwa WYGENEROWANE obiekty w buforze wokół pikseli danego koloru (działa też na obszary rysowane). Nakłada się ponownie przy każdym generowaniu. Bufor zaokrąglany do piksela maski.",
                ));
                ui.separator();
                let d = ForestProject::default();
                let p = &mut self.project;
                if p.cut_zones.is_empty() {
                    ui.small(tr(lang, "Brak stref wycinania."));
                }
                let mut to_remove: Option<usize> = None;
                for (ci, cz) in p.cut_zones.iter_mut().enumerate() {
                    let [r, g, b] = cz.color.0;
                    ui.horizontal(|ui| {
                        let (_rect, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        ui.painter_at(resp.rect)
                            .rect_filled(resp.rect, 2.0, Color32::from_rgb(r, g, b));
                        ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
                        ui.label(tr(lang, "Bufor [m]:"));
                        ui.add(
                            egui::DragValue::new(&mut cz.margin_m)
                                .speed(1.0)
                                .clamp_range(0.0..=500.0),
                        );
                        if ui.button("✖").clicked() {
                            to_remove = Some(ci);
                        }
                    });
                }
                if let Some(i) = to_remove {
                    p.cut_zones.remove(i);
                }

                // dodawanie z kolorów maski — histogram z cache (liczony raz przy wczytaniu)
                if self.mask.is_some() {
                    ui.small(tr(lang, "Dodaj strefę z koloru maski:"));
                    match &self.histogram {
                        None => {
                            ui.small(tr(lang, "Analizuję kolory maski..."));
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(150));
                        }
                        Some(hist) => {
                            let used: Vec<Rgb8> =
                                p.cut_zones.iter().map(|c| c.color).collect();
                            let cands: Vec<(Rgb8, usize)> = hist
                                .iter()
                                .filter(|(c, _)| !used.contains(c))
                                .take(12)
                                .cloned()
                                .collect();
                            if cands.is_empty() {
                                ui.small("(brak nowych kolorów)");
                            } else {
                                ScrollArea::vertical()
                                    .max_height(160.0)
                                    .id_source("cut_colors")
                                    .show(ui, |ui| {
                                        for (c, n) in &cands {
                                            let cc = *c;
                                            let [r, g, b] = cc.0;
                                            ui.horizontal(|ui| {
                                                let (_rect, resp) = ui.allocate_exact_size(
                                                    Vec2::splat(18.0),
                                                    Sense::click(),
                                                );
                                                ui.painter_at(resp.rect).rect_filled(
                                                    resp.rect,
                                                    2.0,
                                                    Color32::from_rgb(r, g, b),
                                                );
                                                ui.monospace(format!(
                                                    "#{r:02X}{g:02X}{b:02X}"
                                                ));
                                                ui.weak(format!("{n}"));
                                                if ui
                                                    .button(tr(lang, "+ Wyklucz"))
                                                    .on_hover_text(
                                                        "Dodaj strefę wycinania",
                                                    )
                                                    .clicked()
                                                {
                                                    p.cut_zones
                                                        .push(forest_core::preset::CutZone {
                                                            color: cc,
                                                            margin_m: 10.0,
                                                        });
                                                }
                                            });
                                        }
                                    });
                            }
                        }
                    }
                } else {
                    ui.small(tr(lang, "(wczytaj maskę, aby wybierać kolory)"));
                }
                if p.cut_zones.len() != d.cut_zones.len() {
                    ui.small("Strefy działają po kliknięciu „▶ Generuj”.");
                }
            });

        // Filtry
        CollapsingHeader::new(tr(lang, "⛰ Filtry środowiskowe"))
            .default_open(false)
            .show(ui, |ui| {
                let d = ForestProject::default();
                let p = &mut self.project;
                opt_f64(ui, tr(lang, "Min. wysokość [m]"), &mut p.min_altitude, -1.0..=8000.0, d.min_altitude);
                opt_f64(ui, tr(lang, "Maks. wysokość [m]"), &mut p.max_altitude, -1.0..=8000.0, d.max_altitude);
                opt_f64(ui, tr(lang, "Maks. spadek [°]"), &mut p.max_slope_deg, 0.0..=90.0, d.max_slope_deg);
                setting_u32(ui, tr(lang, "Tolerancja koloru:"), &mut p.color_tolerance, d.color_tolerance, 0..=255);
                setting_f64(
                    ui,
                    tr(lang, "Margines krawędzi [m]:"),
                    "",
                    &mut p.edge_padding_m,
                    d.edge_padding_m,
                    1.0,
                    0.0..=1000.0,
                );
                ui.separator();
                ui.horizontal(|ui| {
                    let m = p.exclusion_colors != d.exclusion_colors;
                    lbl_mod(ui, tr(lang, "Kolory wykluczone (np. drogi/woda):"), m);
                    if reset_btn(ui, m) {
                        p.exclusion_colors = d.exclusion_colors.clone();
                    }
                });
                let mut remove: Option<usize> = None;
                for (i, c) in p.exclusion_colors.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let [r, g, b] = c.0;
                        let (_rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        ui.painter_at(resp.rect).rect_filled(resp.rect, 2.0, Color32::from_rgb(r, g, b));
                        ui.label(format!("#{:02X}{:02X}{:02X}", r, g, b));
                        if ui.button("✖").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    p.exclusion_colors.remove(i);
                }
                if self.mask.is_some() {
                    match &self.histogram {
                        None => {
                            ui.small(tr(lang, "Analizuję kolory maski..."));
                        }
                        Some(hist) => {
                            let cands: Vec<(Rgb8, usize)> = hist
                                .iter()
                                .filter(|(c, _)| {
                                    !p.exclusion_colors.contains(c)
                                        && !p.zones.iter().any(|z| z.color == *c)
                                })
                                .take(24)
                                .cloned()
                                .collect();
                            if cands.is_empty() {
                                ui.small("Brak nowych kolorów do wykluczenia.");
                            } else {
                                ScrollArea::vertical()
                                    .max_height(140.0)
                                    .id_source("excl_colors")
                                    .show(ui, |ui| {
                                        for (c, n) in cands {
                                            let [r, g, b] = c.0;
                                            ui.horizontal(|ui| {
                                                let (_rect, resp) = ui.allocate_exact_size(
                                                    Vec2::splat(16.0),
                                                    Sense::click(),
                                                );
                                                ui.painter_at(resp.rect).rect_filled(
                                                    resp.rect,
                                                    2.0,
                                                    Color32::from_rgb(r, g, b),
                                                );
                                                ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
                                                ui.weak(format!("{n} px"));
                                                if ui.button(tr(lang, "+ Wyklucz")).clicked() {
                                                    p.exclusion_colors.push(c);
                                                }
                                            });
                                        }
                                    });
                            }
                        }
                    }
                }
            });

        // Rozrzut
        CollapsingHeader::new(tr(lang, "🎲 Rozrzut / polany"))
            .default_open(false)
            .show(ui, |ui| {
                let d = ForestProject::default();
                let p = &mut self.project;
                horiz_tip(ui, ">1 = rzadszy las", |ui| {
                    let m = (p.spacing_multiplier - d.spacing_multiplier).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Mnożnik odstępów:"), m);
                    ui.add(egui::DragValue::new(&mut p.spacing_multiplier).speed(0.02).clamp_range(0.05..=5.0));
                    if reset_btn(ui, m) {
                        p.spacing_multiplier = d.spacing_multiplier;
                    }
                });
                horiz_tip(ui, "Długość fali szumu polan; 0 = wyłączone", |ui| {
                    let m = (p.clearing_scale_m - d.clearing_scale_m).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Skala polan [m]:"), m);
                    ui.add(egui::DragValue::new(&mut p.clearing_scale_m).speed(5.0).clamp_range(0.0..=5000.0));
                    if reset_btn(ui, m) {
                        p.clearing_scale_m = d.clearing_scale_m;
                    }
                });
                horiz_tip(ui, "Jaka część obszaru ma być prześwitem", |ui| {
                    let m = (p.clearing_strength - d.clearing_strength).abs() > 1e-9;
                    lbl_mod(ui, tr(lang, "Siła polan:"), m);
                    ui.add(egui::Slider::new(&mut p.clearing_strength, 0.0..=0.9));
                    if reset_btn(ui, m) {
                        p.clearing_strength = d.clearing_strength;
                    }
                });
            });

        // Gatunki
        CollapsingHeader::new(tr(lang, "🌳 Gatunki"))
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("🔍 Sprawdź modele na P:\\")
                        .on_hover_text("Porównaj modele z plikami gry (P:\\DZ\\plants). \
                                        Terrain Builder odrzuci import, gdy jakiegokolwiek \
                                        brakuje w Template Library.")
                        .clicked()
                    {
                        match forest_core::species::game_plant_models() {
                            Ok(available) => {
                                let missing =
                                    forest_core::species::missing_in_game(&self.project.species, &available);
                                if missing.is_empty() {
                                    self.info = Some(
                                        "✓ Wszystkie modele istnieją w grze (P:\\DZ\\plants).".into(),
                                    );
                                    self.error = None;
                                } else {
                                    self.error = Some(format!(
                                        "Brakujące modele (TB odrzuci import): {}",
                                        missing.join(", ")
                                    ));
                                    self.info = None;
                                }
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    ui.small(tr(lang, "TB wymaga wpisu w Template Library dla każdego modelu."));
                });
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "📥 Import gatunków...")).clicked() {
                        self.import_species();
                    }
                    if ui.button(tr(lang, "📤 Eksport gatunków...")).clicked() {
                        self.export_species();
                    }
                });
                ui.separator();
                let mut remove: Option<usize> = None;
                let mut last_group = String::new();
                for (i, sp) in self.project.species.iter_mut().enumerate() {
                    // nagłówek grupy (puste pole grupy -> "Inne")
                    let group_label = if sp.group.is_empty() {
                        tr(lang, "Inne")
                    } else {
                        tr(lang, &sp.group)
                    };
                    if group_label != last_group {
                        ui.separator();
                        ui.strong(group_label.clone());
                        last_group = group_label;
                    }
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let c = sp.effective_color(i);
                            let (_r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter_at(resp.rect).circle_filled(resp.rect.center(), 4.0, Color32::from_rgb(c[0], c[1], c[2]));
ui.text_edit_singleline(&mut sp.label);
                            // kolor: własny (picker) lub z maski
                            if ui
                                .add(egui::Button::new("🎨").min_size(Vec2::splat(20.0)))
                                .on_hover_text("Kolor gatunku (własny lub z maski)")
                                .clicked()
                            {
                                self.species_color_edit = if self.species_color_edit == Some(i) {
                                    None
                                } else {
                                    Some(i)
                                };
                            }
                            if ui.button("✖").on_hover_text("Usuń gatunek").clicked() {
                                remove = Some(i);
                            }
                        });
                        if self.species_color_edit == Some(i) {
                            ui.indent(format!("spcol{i}"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.small("Kolor własny:");
                                    let mut rgb = sp.color.unwrap_or(Rgb8([255, 255, 255]));
                                    if ui.color_edit_button_srgb(&mut rgb.0).changed() {
                                        sp.color = Some(rgb);
                                    }
                                });
                                ui.small("Kolor z maski:");
                                match &self.histogram {
                                    None => {
                                        ui.small(tr(lang, "Analizuję kolory maski..."));
                                        ui.ctx().request_repaint_after(
                                            std::time::Duration::from_millis(150),
                                        );
                                    }
                                    Some(hist) => {
                                        ScrollArea::vertical()
                                            .max_height(150.0)
                                            .id_source(format!("spcol_list{i}"))
                                            .show(ui, |ui| {
                                                for (c, n) in hist.iter().take(24) {
                                                    let cc = c.0;
                                                    ui.horizontal(|ui| {
                                                        let (_rect, resp) = ui
                                                            .allocate_exact_size(
                                                                Vec2::splat(18.0),
                                                                Sense::click(),
                                                            );
                                                        ui.painter_at(resp.rect).rect_filled(
                                                            resp.rect,
                                                            2.0,
                                                            Color32::from_rgb(
                                                                cc[0], cc[1], cc[2],
                                                            ),
                                                        );
                                                        ui.monospace(format!(
                                                            "#{:02X}{:02X}{:02X}",
                                                            cc[0], cc[1], cc[2]
                                                        ));
                                                        ui.weak(format!("{n}"));
                                                        if ui
                                                            .button(tr(lang, "+ Użyj"))
                                                            .on_hover_text(
                                                                "Ustaw kolor gatunku",
                                                            )
                                                            .clicked()
                                                        {
                                                            sp.color = Some(*c);
                                                        }
                                                    });
                                                }
                                            });
                                    }
                                }
                                if ui.small_button("⟲ auto").clicked() {
                                    sp.color = None;
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.label("Model TB:");
                            ui.text_edit_singleline(&mut sp.model);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Skala:");
                            ui.add(egui::DragValue::new(&mut sp.scale_min).speed(0.01).clamp_range(0.1..=5.0));
                            ui.label("..");
                            ui.add(egui::DragValue::new(&mut sp.scale_max).speed(0.01).clamp_range(0.1..=5.0));
                            ui.label("Przechył [°]:");
                            ui.add(egui::DragValue::new(&mut sp.tilt_max_deg).speed(0.1).clamp_range(0.0..=45.0));
                        });
                    });
                }
                if let Some(i) = remove {
                    self.project.species.remove(i);
                    self.species_color_edit = None;
                    for z in &mut self.project.zones {
                        z.species_weights.retain(|(si, _)| *si != i);
                        for (si, _) in z.species_weights.iter_mut() {
                            if *si > i {
                                *si -= 1;
                            }
                        }
                    }
                }
                if ui.button("+ Dodaj gatunek").clicked() {
                    self.project
                        .species
                        .push(forest_core::species::SpeciesDef::new("Nowy", "model_1f", 0.9, 1.1));
                    for z in &mut self.project.zones {
                        z.species_weights.push((self.project.species.len() - 1, 1.0));
                    }
                }
            });
    }


    // --- prawy panel: presety i wygenerowane warstwy ----------------------------

    fn ui_presets_layers(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;

        ScrollArea::vertical().show(ui, |ui| {
        // 📊 Wynik generowania
        CollapsingHeader::new(tr(lang, "📊 Wygenerowane warstwy"))
            .default_open(true)
            .show(ui, |ui| {
                match &self.stats {
                    Some(s) => {
                        ui.label(format!(
                            "Razem: {} obiektów | granica: {} | wycięto: {} | {} ms",
                            s.total, s.edge_count, s.removed_cut, s.elapsed_ms
                        ));
                        ui.separator();
                        if s.per_source.is_empty() {
                            ui.small(tr(lang, "(brak źródeł)"));
                        }
                        for (n, cnt) in &s.per_source {
                            ui.horizontal(|ui| {
                                ui.label(n.as_str());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.monospace(format!("{cnt}"));
                                });
                            });
                        }
                        let rej = s.rejected_spacing + s.rejected_filters + s.rejected_exclusion + s.rejected_clearing;
                        if rej > 0 {
                            ui.separator();
                            ui.small(format!(
                                "Odrzucone: {rej} (odstęp {}, filtry {}, wykluczenia {}, polany {})",
                                s.rejected_spacing, s.rejected_filters,
                                s.rejected_exclusion, s.rejected_clearing
                            ));
                        }
                    }
                    None => {
                        ui.small(tr(lang, "Brak wyników - kliknij '▶ Generuj'. Po generowaniu tutaj pojawią się warstwy."));
                    }
                }
            });

        // Strefy
        CollapsingHeader::new(tr(lang, "🌲 Strefy lasu"))
            .default_open(true)
            .show(ui, |ui| {
                // --- presety roślinności (pogrupowane) ----------------------
                ui.small(tr(lang, "Dodaj strefę z presetu (⊕ kopiuje do \"Moje presety\"):"));
                let presets = forest_core::species::zone_presets();
                let mut last_group = "";
                ScrollArea::vertical()
                    .max_height(190.0)
                    .id_source("zone_presets")
                    .show(ui, |ui| {
                        for p in presets.iter() {
                            if p.group != last_group {
                                ui.separator();
                                ui.strong(tr(lang, p.group));
                                last_group = p.group;
                            }
                            ui.horizontal(|ui| {
                                if ui.button("+").on_hover_text(tf(
                                    lang,
                                    "Dodaj strefę '{0}' ({1} szt/ha)",
                                    &[&tr(lang, p.name), &format!("{:.0}", p.density_per_ha)],
                                )) .clicked()
                                {
                                    self.add_zone_from_preset(*p);
                                }
                                ui.label(tr(lang, p.name));
                                ui.weak(format!("{:.0}/ha", p.density_per_ha));
                                if ui
                                    .button("⧉")
                                    .on_hover_text(tr(lang, "Kopiuj do „Moje presety” (edytowalna kopia)"))
                                    .clicked()
                                {
                                    self.user_presets.push(UserPreset::from_builtin(p));
                                    self.presets_dirty = true;
                                }
                            });
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "📥 Import stref...")).clicked() {
                        self.import_zones();
                    }
                    if ui.button(tr(lang, "📤 Eksport stref...")).clicked() {
                        self.export_zones();
                    }
                });
                ui.separator();

                let mut to_remove: Option<usize> = None;
                // dane dla miksu presetów (klonowane przed pożyczkiem &mut stref)
                let species_snap = self.project.species.clone();
                let preset_list: Vec<(String, PSnap)> = self
                    .all_presets()
                    .into_iter()
                    .map(|s| (s.name.clone(), s))
                    .collect();
                for zi in 0..self.project.zones.len() {
                    let editing = self.zone_color_edit == Some(zi);

                    // dane edytora koloru — zbierane przed pożyczkiem &mut strefy
                    let mut cand_colors: Vec<(Rgb8, bool)> = Vec::new(); // (kolor, czy wolny)
                    if editing {
                        let excl = self.project.exclusion_colors.clone();
                        let others: Vec<Rgb8> = self
                            .project
                            .zones
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != zi)
                            .map(|(_, z)| z.color)
                            .collect();
                        if let Some(h) = &self.histogram {
                            for (c, _) in h.iter() {
                                if cand_colors.len() >= 32 {
                                    break;
                                }
                                let free = !others.contains(c) && !excl.contains(c);
                                if free || !cand_colors.iter().any(|(cc, _)| cc == c) {
                                    cand_colors.push((*c, free));
                                }
                            }
                        }
                        // aktualny kolor strefy zawsze dostępny na początku listy
                        let cur = self.project.zones[zi].color;
                        cand_colors.retain(|(c, _)| *c != cur);
                        cand_colors.insert(0, (cur, !others.contains(&cur)));
                    }

                    let z = &mut self.project.zones[zi];
                    let [r, g, b] = z.color.0;
                    ui.horizontal(|ui| {
                        let (_rect, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        ui.painter_at(resp.rect).rect_filled(resp.rect, 3.0, Color32::from_rgb(r, g, b));
                        if ui
                            .button("🎨")
                            .on_hover_text("Zmień kolor strefy (kliknij ponownie, aby zamknąć)")
                            .clicked()
                        {
                            self.zone_color_edit =
                                if self.zone_color_edit == Some(zi) { None } else { Some(zi) };
                        }
                        ui.text_edit_singleline(&mut z.label);
                        if ui.button("✖").on_hover_text(tr(lang, "Usuń strefę")).clicked() {
                            to_remove = Some(zi);
                        }
                    });

                    if editing {
                        let mut picked: Option<Rgb8> = None;
                        ui.indent(format!("zc{zi}"), |ui| {
                            ui.small("Kolor z maski (szare = zajęte przez inne strefy/wykluczenia):");
                            ui.horizontal_wrapped(|ui| {
                                for (c, free) in &cand_colors {
                                    let [cr, cg, cb] = c.0;
                                    let col = Color32::from_rgb(cr, cg, cb);
                                    let btn = egui::Button::new("").min_size(Vec2::splat(20.0))
                                        .fill(if *free { col } else { col.gamma_multiply(0.25) })
                                        .stroke(egui::Stroke::new(
                                            1.5_f32,
                                            if *free { Color32::WHITE } else { Color32::DARK_RED },
                                        ));
                                    if ui.add(btn).on_hover_text(format!(
                                        "#{cr:02X}{cg:02X}{cb:02X}{}",
                                        if *free { "" } else { " (zajęty)" }
                                    )).clicked() && *free {
                                        picked = Some(*c);
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.small(tr(lang, "RGB ręcznie:"));
                                for ch in 0..3usize {
                                    let mut v = z.color.0[ch];
                                    if ui
                                        .add(egui::DragValue::new(&mut v).clamp_range(0..=255))
                                        .changed()
                                    {
                                        z.color.0[ch] = v;
                                    }
                                }
                            });
                        });
                        if let Some(c) = picked {
                            z.color = c;
                            self.zone_color_edit = None;
                            self.info = Some(format!(
                                "Strefa '{}' ma teraz kolor #{:02X}{:02X}{:02X}.",
                                z.label,
                                c.r(),
                                c.g(),
                                c.b()
                            ));
                        }
                    }

                    // 🧩 miks presetów — kilka szablonów na tej samej strefie
                    ui.indent(format!("zmix{zi}"), |ui| {
                        let changed =
                            ui_mix_rows(ui, lang, &format!("zone{zi}"), &mut z.preset_mix, &preset_list);
                        if changed {
                            if let Some(snap) = compute_mix_snap(&z.preset_mix, &preset_list) {
                                let mapped: Vec<(usize, f32)> = snap
                                    .weights
                                    .iter()
                                    .filter_map(|(model, w)| {
                                        species_snap
                                            .iter()
                                            .position(|s| s.model.eq_ignore_ascii_case(model))
                                            .map(|idx| (idx, *w))
                                    })
                                    .collect();
                                if !mapped.is_empty() {
                                    z.density_per_ha = snap.density_per_ha;
                                    z.species_weights = mapped;
                                }
                            }
                        }
                        if !z.preset_mix.is_empty() {
                            ui.small(tf(
                                lang,
                                "Efekt: {0} szt/ha, {1} gatunków (ręczna edycja wag niżej czyści miks)",
                                &[
                                    &format!("{:.0}", z.density_per_ha),
                                    &z.species_weights.len().to_string(),
                                ],
                            ));
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Gęstość [szt/ha]:"));
                        ui.add(egui::DragValue::new(&mut z.density_per_ha).speed(1.0).clamp_range(1.0..=5000.0));
                    });
                    ui.indent(format!("zw{zi}"), |ui| {
                        ui.small(tr(lang, "Wagi gatunków:"));
                        let mut changed = false;
                        for wi in 0..z.species_weights.len() {
                            let (si, _) = z.species_weights[wi];
                            let Some(sp) = self.project.species.get(si) else { continue };
                            ui.horizontal(|ui| {
                                let c = sp.effective_color(si);
                                let (_r, resp) =
                                    ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                ui.painter_at(resp.rect).circle_filled(resp.rect.center(), 4.0, Color32::from_rgb(c[0], c[1], c[2]));
                                ui.label(sp.label.as_str());
                                let mut w = z.species_weights[wi].1;
                                let before = w;
                                ui.add(egui::DragValue::new(&mut w).speed(0.05).clamp_range(0.0..=20.0));
                                if (w - before).abs() > f32::EPSILON {
                                    z.species_weights[wi].1 = w;
                                    changed = true;
                                }
                            });
                        }
                        let _ = changed;
                    });
                    ui.separator();
                }
                if let Some(zi) = to_remove {
                    self.project.zones.remove(zi);
                    self.zone_color_edit = None;
                }

                if self.mask.is_some() {
                    ui.small(tr(lang, "Dodaj strefę z koloru maski:"));
                    match &self.histogram {
                        None => {
                            ui.small(tr(lang, "Analizuję kolory maski..."));
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(150));
                        }
                        Some(hist) => {
                            let used: Vec<Rgb8> =
                                self.project.zones.iter().map(|z| z.color).collect();
                            let candidates: Vec<(Rgb8, usize)> = hist
                                .iter()
                                .filter(|(c, _)| !used.contains(c))
                                .take(24)
                                .cloned()
                                .collect();
                            if candidates.is_empty() {
                                ui.small("Brak nowych kolorów — wszystkie największe obszary są już strefami.");
                            } else {
                                ScrollArea::vertical()
                                    .max_height(200.0)
                                    .id_source("zone_colors")
                                    .show(ui, |ui| {
                                        for (c, n) in candidates {
                                            let [r, g, b] = c.0;
                                            ui.horizontal(|ui| {
                                                let (_rect, resp) = ui
                                                    .allocate_exact_size(
                                                        Vec2::splat(16.0),
                                                        Sense::click(),
                                                    );
                                                ui.painter_at(resp.rect).rect_filled(
                                                    resp.rect,
                                                    3.0,
                                                    Color32::from_rgb(r, g, b),
                                                );
                                                ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
                                                ui.weak(format!("{n} px"));
                                                if ui.button(tr(lang, "+ Dodaj strefę")).clicked() {
                                                    self.add_zone_from_color(c);
                                                }
                                            });
                                        }
                                    });
                            }
                        }
                    }
                } else {
                    ui.small("Wczytaj maskę, aby dodawać strefy klikając kolory.");
                }
            });


        // Moje presety — edytowalne szablony stref (zapisywane do JSON)
        CollapsingHeader::new(tr(lang, "📚 Moje presety"))
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "+ Nowy pusty preset")).clicked() {
                        self.user_presets.push(UserPreset {
                            group: "Moje".into(),
                            name: format!("Mój preset {}", self.user_presets.len() + 1),
                            density_per_ha: 200.0,
                            weights: (0..self.project.species.len())
                                .map(|i| (self.project.species[i].model.clone(), 1.0))
                                .collect(),
                        });
                        self.preset_edit_open = Some(self.user_presets.len() - 1);
                        self.presets_dirty = true;
                    }
                    if ui.button(tr(lang, "💾 Zapisz")).clicked() {
                        self.save_user_presets();
                    }
                    if self.presets_dirty {
                        ui.colored_label(Color32::YELLOW, tr(lang, "• niezapisane"));
                    }
                });
                ui.small(tf(
                    lang,
                    "Plik: {0} (katalog roboczy programu)",
                    &[DEFAULT_PRESETS_FILE],
                ));
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "📥 Import presetów...")).clicked() {
                        self.import_presets_dialog();
                    }
                    if ui.button(tr(lang, "📤 Eksport presetów...")).clicked() {
                        self.export_presets_dialog();
                    }
                });
                ui.separator();

                if self.user_presets.is_empty() {
                    ui.small(tr(
                        lang,
                        "Brak własnych presetów. Skopiuj wbudowane przyciskiem ⊕ w sekcji \"🌲 Strefy lasu\" albo dodaj pusty powyżej.",
                    ));
                }

                let n_species = self.project.species.len();
                let mut to_remove: Option<usize> = None;
                let mut pending_add: Option<usize> = None;
                let mut pending_dup: Option<usize> = None;
                let mut edited = false;
                let mut last_group = String::new();
                for (i, p) in self.user_presets.iter_mut().enumerate() {
                    if p.group != last_group {
                        ui.separator();
                        ui.strong(tr(lang, &p.group));
                        last_group = p.group.clone();
                    }
                    let editing = self.preset_edit_open == Some(i);
                    // snapshot do akcji odroczonych (bez pożyczania self w closure)
                    let snap_for_actions = PSnap {
                        name: p.name.clone(),
                        density_per_ha: p.density_per_ha,
                        weights: p.weights.clone(),
                    };
                    ui.horizontal(|ui| {
                        if ui
                            .button(if editing { "▾" } else { "✎" })
                            .on_hover_text(tr(lang, "Edytuj preset"))
                            .clicked()
                        {
                            self.preset_edit_open =
                                if editing { None } else { Some(i) };
                        }
                        ui.text_edit_singleline(&mut p.name);
                        if ui
                            .button(tr(lang, "+ Strefa"))
                            .on_hover_text(tr(lang, "Dodaj strefę z tego presetu"))
                            .clicked()
                        {
                            pending_add = Some(i);
                        }
                        let _ = &snap_for_actions;
                        if ui.button("⧉").on_hover_text(tr(lang, "Duplikuj")).clicked() {
                            pending_dup = Some(i);
                        }
                        if ui.button("🗑").on_hover_text(tr(lang, "Usuń preset")).clicked() {
                            to_remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Grupa:"));
                        ui.add(egui::TextEdit::singleline(&mut p.group).desired_width(110.0));
                        ui.label(tr(lang, "Gęstość [szt/ha]:"));
                        ui.add(
                            egui::DragValue::new(&mut p.density_per_ha)
                                .speed(1.0)
                                .clamp_range(1.0..=5000.0),
                        );
                    });

                    if self.preset_edit_open == Some(i) {
                        ui.indent(format!("up{i}"), |ui| {
                            ui.small("Wagi gatunków (zapisywane po nazwie modelu):");
                            for si in 0..n_species {
                                let Some(sp) = self.project.species.get(si) else { continue };
                                let c = sp.effective_color(si);
                                ui.horizontal(|ui| {
                                    let (_r, resp) = ui
                                        .allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                    ui.painter_at(resp.rect).circle_filled(
                                        resp.rect.center(),
                                        4.0,
                                        Color32::from_rgb(c[0], c[1], c[2]),
                                    );
                                    ui.label(sp.label.as_str());
                                    let pos = p
                                        .weights
                                        .iter()
                                        .position(|(m, _)| m.eq_ignore_ascii_case(&sp.model));
                                    let mut w = pos.map_or(0.0, |k| p.weights[k].1);
                                    let before = w;
                                    ui.add(
                                        egui::DragValue::new(&mut w)
                                            .speed(0.05)
                                            .clamp_range(0.0..=20.0),
                                    );
                                    if (w - before).abs() > f32::EPSILON {
                                        match pos {
                                            Some(k) => {
                                                if w <= 0.0 {
                                                    p.weights.remove(k);
                                                } else {
                                                    p.weights[k].1 = w;
                                                }
                                            }
                                            None => {
                                                if w > 0.0 {
                                                    p.weights.push((sp.model.clone(), w));
                                                }
                                            }
                                        }
                                        edited = true;
                                    }
                                });
                            }
                            if p.weights.is_empty() {
                                ui.colored_label(
                                    Color32::RED,
                                    "⚠ Brak gatunków — taki preset nie doda strefy.",
                                );
                            }
                        });
                    }
                }
                // odroczone akcje (po zakończeniu iteracji z &mut)
                if let Some(i) = pending_dup {
                    let mut copy = self.user_presets[i].clone();
                    copy.name = format!("{} kopia", copy.name);
                    self.user_presets.insert(i + 1, copy);
                    edited = true;
                }
                if let Some(i) = to_remove {
                    self.user_presets.remove(i);
                    self.preset_edit_open = None;
                    edited = true;
                }
                if let Some(i) = pending_add {
                    let snap = PSnap {
                        name: self.user_presets[i].name.clone(),
                        density_per_ha: self.user_presets[i].density_per_ha,
                        weights: self.user_presets[i].weights.clone(),
                    };
                    self.add_zone_from_snap(&snap);
                }
                if edited {
                    self.presets_dirty = true;
                }
            });


        // Obszary rysowane (poligony) — alternatywa dla maski
        CollapsingHeader::new(tr(lang, "📐 Obszary (poligony)"))
            .default_open(false)
            .show(ui, |ui| {
                ui.small(tr(
                    lang,
                    "Rysowane na mapie: wybierz preset na pasku → ✏ Rysuj obszar → klikaj wierzchołki (LPM), Enter/dwuklik = zakończ.",
                ));
                ui.separator();
                let mut to_remove: Option<usize> = None;
                let species_snap_a = self.project.species.clone();
                let global_edges_snap = self.project.edges.clone();
                let preset_list_a: Vec<(String, PSnap)> = self
                    .all_presets()
                    .into_iter()
                    .map(|s| (s.name.clone(), s))
                    .collect();
                if self.area_copy_src.len() != self.project.areas.len() {
                    self.area_copy_src.resize(self.project.areas.len(), None);
                }
                let area_labels: Vec<String> =
                    self.project.areas.iter().map(|ar| ar.label.clone()).collect();
                let mut pending_copy: Option<(usize, usize)> = None;
                for (ai, a) in self.project.areas.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let col = [Color32::from_rgb(255, 170, 40), Color32::from_rgb(60, 210, 255)][ai % 2];
                            let (_r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter_at(resp.rect).circle_filled(resp.rect.center(), 4.0, col);
                            ui.text_edit_singleline(&mut a.label);
                            ui.checkbox(&mut a.enabled, tr(lang, "Aktywny"))
                                .on_hover_text(tr(lang, "Bierze udział w generowaniu"));
                            if ui
                                .button(if self.editing_area == Some(ai) { "✏ WŁ" } else { "✏" })
                                .on_hover_text(tr(lang, "Edytuj obszar (wierzchołki)"))
                                .clicked()
                            {
                                if self.editing_area == Some(ai) {
                                    self.editing_area = None;
                                    self.editing_vertex = None;
                                } else {
                                    self.editing_area = Some(ai);
                                    self.editing_vertex = None;
                                    self.draw_mode = false;
                                    self.draw_points.clear();
                                }
                            }
                            if ui.button("✖").on_hover_text(tr(lang, "Usuń obszar")).clicked() {
                                to_remove = Some(ai);
                            }
                        });
                        // kopiowanie ustawień z innego obszaru
                        if area_labels.len() > 1 {
                            ui.horizontal(|ui| {
                                ui.label(tr(lang, "Kopiuj z:"));
                                let sel_text = self.area_copy_src[ai]
                                    .and_then(|idx| area_labels.get(idx))
                                    .cloned()
                                    .unwrap_or_else(|| tr(lang, "— wybierz —"));
                                egui::ComboBox::from_id_source(format!("copy_src{ai}"))
                                    .selected_text(sel_text)
                                    .width(140.0)
                                    .show_ui(ui, |ui| {
                                        for (j, label) in area_labels.iter().enumerate() {
                                            if j == ai {
                                                continue;
                                            }
                                            let is_selected = self.area_copy_src[ai] == Some(j);
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                self.area_copy_src[ai] = Some(j);
                                            }
                                        }
                                    });
                                let can_copy = self.area_copy_src[ai].is_some_and(|src| {
                                    src < area_labels.len() && src != ai
                                });
                                if ui
                                    .add_enabled(can_copy, egui::Button::new(tr(lang, "Kopiuj")))
                                    .on_hover_text(tr(lang, "Kopiuj ustawienia z wybranego obszaru"))
                                    .clicked()
                                {
                                    if let Some(src) = self.area_copy_src[ai] {
                                        pending_copy = Some((src, ai));
                                    }
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr(lang, "Gęstość [szt/ha]:"));
                            ui.add(
                                egui::DragValue::new(&mut a.density_per_ha)
                                    .speed(1.0)
                                    .clamp_range(1.0..=5000.0),
                            );
                            let ha =
                                forest_core::scatter::polygon_area_m2(&a.polygon) / 10_000.0;
                            let est = (ha * f64::from(a.density_per_ha)).round() as u64;
                            ui.label(tf(
                                lang,
                                "{0} ha | pkt: {1} | ≈ {2} szt",
                                &[
                                    &format!("{:.1}", ha),
                                    &a.polygon.len().to_string(),
                                    &est.to_string(),
                                ],
                            ));
                        });
                        // 🧩 miks presetów na tym obszarze
                        ui.indent(format!("amix{ai}"), |ui| {
                            let changed = ui_mix_rows(
                                ui,
                                lang,
                                &format!("area{ai}"),
                                &mut a.preset_mix,
                                &preset_list_a,
                            );
                            if changed {
                                if let Some(snap) =
                                    compute_mix_snap(&a.preset_mix, &preset_list_a)
                                {
                                    let mapped: Vec<(usize, f32)> = snap
                                        .weights
                                        .iter()
                                        .filter_map(|(model, w)| {
                                            species_snap_a
                                                .iter()
                                                .position(|s| {
                                                    s.model.eq_ignore_ascii_case(model)
                                                })
                                                .map(|idx| (idx, *w))
                                        })
                                        .collect();
                                    if !mapped.is_empty() {
                                        a.density_per_ha = snap.density_per_ha;
                                        a.species_weights = mapped;
                                    }
                                }
                            }
                            if !a.preset_mix.is_empty() {
                                ui.small(tf(
                                    lang,
                                    "Efekt: {0} szt/ha, {1} gatunków",
                                    &[
                                        &format!("{:.0}", a.density_per_ha),
                                        &a.species_weights.len().to_string(),
                                    ],
                                ));
                            }
                        });

                        // 🎯 inteligentne generowanie — kolory z podkładu
                        ui.separator();
                        let n_samp = a
                            .color_filter
                            .as_ref()
                            .map(|f| f.samples.len())
                            .unwrap_or(0);
                        ui.horizontal(|ui| {
                            let s_on = self.sampling_area == Some(ai);
                            if ui
                                .button(if s_on { "🎯 Pobieranie: WŁ" } else { "🎯" })
                                .on_hover_text(
                                    "Inteligentne generowanie: pobierz próbki kolorów \
                                     lasu z podkładu klikając na mapie (Esc = koniec)",
                                )
                                .clicked()
                            {
                                if s_on {
                                    self.sampling_area = None;
                                } else if self.sat_image.is_some() {
                                    self.sampling_area = Some(ai);
                                    self.draw_mode = false;
                                    self.draw_points.clear();
                                    self.editing_area = None;
                                    self.editing_vertex = None;
                                    self.info =
                                        Some("Klikaj na podkładzie w miejsca z lasem...".into());
                                } else {
                                    self.error =
                                        Some("Najpierw wczytaj podkład satelitarny (📁 Pliki).".into());
                                }
                            }
                            ui.label(tr(lang, "Tryb próbkowania"));
                            if s_on {
                                ui.colored_label(Color32::YELLOW, "●");
                            }
                        });
                        egui::CollapsingHeader::new(format!(
                            "{} {}",
                            tr(lang, "Próbki kolorów:"),
                            n_samp
                        ))
                        .id_source(format!("cs{ai}"))
                        .default_open(n_samp > 0)
                        .show(ui, |ui| {
                            if let Some(cf) = a.color_filter.as_mut() {
                                if !cf.samples.is_empty() && self.sat_image.is_none() {
                                    ui.colored_label(
                                        ORANGE_MOD,
                                        "⚠ Brak podkładu — filtr nie zadziała",
                                    );
                                }
                                let mut to_remove: Option<usize> = None;
                                for (si_, sm) in cf.samples.iter().enumerate() {
                                    let [r, g, b] = sm.0;
                                    ui.horizontal(|ui| {
                                        let (_rect, resp) = ui
                                            .allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                                        ui.painter_at(resp.rect).rect_filled(
                                            resp.rect,
                                            2.0,
                                            Color32::from_rgb(r, g, b),
                                        );
                                        ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("✖").clicked() {
                                                    to_remove = Some(si_);
                                                }
                                            },
                                        );
                                    });
                                }
                                if let Some(i) = to_remove {
                                    cf.samples.remove(i);
                                }
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Tolerancja koloru:"));
                                    ui.add(
                                        egui::DragValue::new(&mut cf.tolerance)
                                            .speed(1.0)
                                            .clamp_range(0..=255),
                                    );
                                    if ui.button(tr(lang, "Wyczyść")).clicked() {
                                        cf.samples.clear();
                                    }
                                });
                            } else {
                                ui.small(tr(lang, "(brak próbek — kliknij 🎯 i próbkuj na mapie)"));
                            }
                        });

                        // granica tego obszaru (nadpisuje globalną)
                        let mut own = a.edges.is_some();
                        if ui
                            .checkbox(&mut own, tr(lang, "Własna granica"))
                            .on_hover_text(
                                "Nadpisuje globalne ustawienia granicy dla tego obszaru",
                            )
                            .changed()
                        {
                            a.edges = if own {
                                Some(global_edges_snap.clone())
                            } else {
                                None
                            };
                        }
                        match a.edges.as_mut() {
                            Some(ge) => {
                                ui.indent(format!("aedge{ai}"), |ui| {
                                    ui.small("⟲ przywraca wartości z globalnej granicy");
                                    ui_edge_settings(ui, self.lang, ge, &global_edges_snap, &species_snap_a);
                                });
                            }
                            None => {
                                ui.small(tf(
                                    lang,
                                    "Używa granicy globalnej (pas {0} m, {1}/ha)",
                                    &[
                                        &format!("{:.0}", global_edges_snap.band_width_m),
                                        &format!("{:.0}", global_edges_snap.density_per_ha),
                                    ],
                                ));
                            }
                        }
                    });
                }
                if let Some((src, dst)) = pending_copy {
                    if src < self.project.areas.len()
                        && dst < self.project.areas.len()
                        && src != dst
                    {
                        let src_label = self.project.areas[src].label.clone();
                        let dst_label = self.project.areas[dst].label.clone();
                        let src_clone = self.project.areas[src].clone();
                        let dst_area = &mut self.project.areas[dst];
                        dst_area.enabled = src_clone.enabled;
                        dst_area.density_per_ha = src_clone.density_per_ha;
                        dst_area.species_weights = src_clone.species_weights;
                        dst_area.preset_mix = src_clone.preset_mix;
                        dst_area.edges = src_clone.edges;
                        dst_area.color_filter = src_clone.color_filter;
                        self.info = Some(tf(
                            lang,
                            "Skopiowano ustawienia z '{0}' do '{1}'.",
                            &[&src_label, &dst_label],
                        ));
                    }
                }
                if let Some(i) = to_remove {
                    self.project.areas.remove(i);
                    if i < self.area_copy_src.len() {
                        self.area_copy_src.remove(i);
                    }
                    for v in self.area_copy_src.iter_mut() {
                        if let Some(idx) = v {
                            if *idx == i {
                                *v = None;
                            } else if *idx > i {
                                *v = Some(*idx - 1);
                            }
                        }
                    }
                    if self.editing_area == Some(i) {
                        self.editing_area = None;
                        self.editing_vertex = None;
                    }
                    if self.sampling_area == Some(i) {
                        self.sampling_area = None;
                    } else if let Some(sa) = self.sampling_area {
                        if sa > i {
                            self.sampling_area = Some(sa - 1);
                        }
                    }
                    if let Some(ea) = self.editing_area {
                        if ea > i {
                            self.editing_area = Some(ea - 1);
                        }
                    }
                }
                if !self.project.areas.is_empty()
                    && ui.button(tr(lang, "Wyczyść wszystkie obszary")).clicked()
                {
                    self.project.areas.clear();
                    self.area_copy_src.clear();
                    self.editing_area = None;
                    self.editing_vertex = None;
                    self.sampling_area = None;
                }
            });


        });
    }    // --- kanvas -------------------------------------------------------------

    fn ui_canvas(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;

        if self.mask.is_none() && self.project.areas.is_empty() && !self.draw_mode {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("DayZ Forest Generator");
                    ui.add_space(12.0);
                    ui.label(tr(lang, "1. Wczytaj maskę PNG (kolory = strefy lasu)"));
                    ui.label(tr(lang, "2. ...albo narysuj obszary: wybierz preset i klikaj wierzchołki na mapie"));
                    ui.label(tr(lang, "3. Opcjonalnie: heightmapa .asc, wykluczenia .geojson, granice lasu"));
                    ui.label(tr(lang, "4. Generuj → Eksport TXT → import w Terrain Builderze"));
                });
            });
            return;
        }

        let map = self.project.map_size_m;
        // click_and_drag: drag-sense nie rejestruje kliknięć (wymagane dla
        // dodawania wierzchołków poligonu), a klik + lekki ruch = pan
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

        // interakcja: pan + zoom do kursora
        // (w edycji wierzchołka drag przesuwa wierzchołek, nie panuje)
        let editing_vertex_active = self.editing_area.is_some() && self.editing_vertex.is_some();
        if !editing_vertex_active
            && (resp.dragged_by(PointerButton::Primary) || resp.dragged_by(PointerButton::Secondary))
        {
            self.pan += resp.drag_delta();
        }
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 && resp.hovered() {
            let mapf = map as f32;
            let base = rect.size().min_elem() / mapf;
            let old_scale = base * self.zoom;
            let origin = canvas_origin(rect, map, old_scale, self.pan);
            if let Some(hover) = resp.hover_pos() {
                // punkt świata pod kursorem przed zoomem...
                let wx = (hover.x - origin.x) / old_scale;
                let wy = mapf - (hover.y - origin.y) / old_scale;
                self.zoom = (self.zoom * (1.0 + scroll / 400.0)).clamp(0.2, 60.0);
                // ...musi po zoomie trafić z powrotem dokładnie w kursor
                let new_scale = base * self.zoom;
                let base_o = canvas_origin(rect, map, new_scale, Vec2::ZERO);
                self.pan = egui::vec2(
                    hover.x - wx * new_scale - base_o.x,
                    hover.y - (mapf - wy) * new_scale - base_o.y,
                );
            }
        }
        // dwuklik: w trybie rysowania kończy poligon, poza nim dopasowuje widok
        if resp.double_clicked() {
            if self.draw_mode {
                self.finish_area();
            } else if self.editing_area.is_some() {
                self.editing_area = None;
                self.editing_vertex = None;
            } else {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
        }

        // twardy clamp: mapa nigdy nie może całkiem opuścić widoku
        let scale = rect.size().min_elem() / map as f32 * self.zoom;
        self.pan = clamp_pan_to_view(rect, map as f32, scale, self.pan);
        let origin = canvas_origin(rect, map, scale, self.pan);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 26, 30));

        let to_screen = |wx: f64, wy: f64| -> egui::Pos2 {
            egui::pos2(
                origin.x + (wx as f32) * scale,
                origin.y + ((map - wy) as f32) * scale,
            )
        };

        // rysowanie poligonu: LPM = wierzchołek, Enter = zakończ, Esc = anuluj
        if self.draw_mode {
            if resp.clicked_by(PointerButton::Primary) {
                if let Some(hover) = resp.hover_pos() {
                    let wx = ((hover.x - origin.x) / scale) as f64;
                    let wy = map - (((hover.y - origin.y) / scale) as f64);
                    self.draw_points.push([wx.clamp(0.0, map), wy.clamp(0.0, map)]);
                }
            }
            let (enter, esc, back) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Backspace),
                )
            });
            if enter && !self.draw_points.is_empty() {
                self.finish_area();
            }
            if esc {
                self.cancel_drawing();
            }
            if back {
                self.draw_points.pop();
            }
        }

        // edycja istniejącego obszaru: przesuwanie/dodawanie/usuwanie wierzchołków
        if let Some(ai) = self.editing_area {
            let n_pts = self.project.areas.get(ai).map(|a| a.polygon.len());
            if let Some(_n) = n_pts {
                // start przeciągania na wierzchołku -> wybierz go
                if resp.drag_started_by(PointerButton::Primary) {
                    if let Some(hover) = resp.hover_pos() {
                        let hit_px = 10.0_f32;
                        let best_v = self.project.areas[ai]
                            .polygon
                            .iter()
                            .enumerate()
                            .map(|(vi, p)| {
                                let s = to_screen(p[0], p[1]);
                                (vi, (s.x - hover.x).hypot(s.y - hover.y))
                            })
                            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                            .filter(|(_, d)| *d <= hit_px)
                            .map(|(vi, _)| vi);
                        self.editing_vertex = best_v;
                    }
                }
                // przeciąganie wybranego wierzchołka
                if resp.dragged_by(PointerButton::Primary) && self.editing_vertex.is_some() {
                    if let Some(hover) = resp.hover_pos() {
                        let wx = ((hover.x - origin.x) / scale) as f64;
                        let wy = map - (((hover.y - origin.y) / scale) as f64);
                        let vi = self.editing_vertex.unwrap();
                        if let Some(a) = self.project.areas.get_mut(ai) {
                            if vi < a.polygon.len() {
                                a.polygon[vi] = [wx.clamp(0.0, map), wy.clamp(0.0, map)];
                            }
                        }
                    }
                }
                // klik bez przeciągnięcia: wybierz wierzchołek albo dodaj na odcinku
                if resp.clicked_by(PointerButton::Primary) {
                    if let Some(hover) = resp.hover_pos() {
                        let hit_px = 10.0_f32;
                        let wx = ((hover.x - origin.x) / scale) as f64;
                        let wy = map - (((hover.y - origin.y) / scale) as f64);
                        let best_v = self.project.areas[ai]
                            .polygon
                            .iter()
                            .enumerate()
                            .map(|(vi, p)| {
                                let s = to_screen(p[0], p[1]);
                                (vi, (s.x - hover.x).hypot(s.y - hover.y))
                            })
                            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                            .filter(|(_, d)| *d <= hit_px)
                            .map(|(vi, _)| vi);
                        if let Some(vi) = best_v {
                            self.editing_vertex = Some(vi);
                        } else {
                            // dodaj nowy wierzchołek na najbliższym odcinku
                            let n = self.project.areas[ai].polygon.len();
                            let mut best: Option<(usize, f32)> = None;
                            for i in 0..n {
                                let a_s = to_screen(
                                    self.project.areas[ai].polygon[i][0],
                                    self.project.areas[ai].polygon[i][1],
                                );
                                let b_s = to_screen(
                                    self.project.areas[ai].polygon[(i + 1) % n][0],
                                    self.project.areas[ai].polygon[(i + 1) % n][1],
                                );
                                let d = seg_dist_screen(hover, a_s, b_s);
                                if d <= hit_px && best.map_or(true, |(_, bd)| d < bd) {
                                    best = Some((i + 1, d));
                                }
                            }
                            if let Some((ins, _)) = best {
                                if let Some(a) = self.project.areas.get_mut(ai) {
                                    a.polygon.insert(
                                        ins,
                                        [wx.clamp(0.0, map), wy.clamp(0.0, map)],
                                    );
                                }
                                self.editing_vertex = Some(ins);
                            }
                        }
                    }
                }
                // klawiatura: Backspace usuwa wybrany, Esc/Enter kończy edycję
                let (esc, enter, back) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::Escape),
                        i.key_pressed(egui::Key::Enter),
                        i.key_pressed(egui::Key::Backspace),
                    )
                });
                if back {
                    if let Some(vi) = self.editing_vertex {
                        if let Some(a) = self.project.areas.get_mut(ai) {
                            if a.polygon.len() > 3 && vi < a.polygon.len() {
                                a.polygon.remove(vi);
                                self.editing_vertex = None;
                            }
                        }
                    }
                }
                if esc || enter {
                    self.editing_area = None;
                    self.editing_vertex = None;
                }
            }
        }

        // 🎯 pobieranie próbek kolorów z podkładu satelitarnego
        if let Some(ai) = self.sampling_area {
            let (esc, clicked) = (
                ui.input(|i| i.key_pressed(egui::Key::Escape)),
                resp.clicked_by(PointerButton::Primary),
            );
            if esc {
                self.sampling_area = None;
            } else if clicked {
                if let Some(hover) = resp.hover_pos() {
                    let wx = ((hover.x - origin.x) / scale) as f64;
                    let wy = map - (((hover.y - origin.y) / scale) as f64);
                    if wx >= 0.0 && wy >= 0.0 && wx <= map && wy <= map {
                        if let Some(sat) = &self.sat_image {
                            let sx = (wx / map * f64::from(sat.width)) as u32;
                            let sy = ((map - wy) / map * f64::from(sat.height)) as u32;
                            let sx = sx.min(sat.width - 1);
                            let sy = sy.min(sat.height - 1);
                            let color = sat.pixel(sx, sy);
                            if let Some(a) = self.project.areas.get_mut(ai) {
                                let cf = a.color_filter.get_or_insert_with(|| {
                                    forest_core::preset::ColorFilter {
                                        samples: Vec::new(),
                                        tolerance: 60,
                                    }
                                });
                                cf.samples.push(color);
                                self.info = Some(format!(
                                    "Próbka #{:02X}{:02X}{:02X} dodana (razem: {}). Esc = koniec.",
                                    color.0[0],
                                    color.0[1],
                                    color.0[2],
                                    cf.samples.len()
                                ));
                            }
                        } else {
                            self.error =
                                Some("Brak podkładu satelitarnego — wczytaj go w 📁 Pliki.".into());
                            self.sampling_area = None;
                        }
                    }
                }
            }
        }

        let map_rect = Rect::from_min_size(
            egui::pos2(origin.x, origin.y),
            Vec2::splat(map as f32 * scale),
        );

        if self.show_sat {
            if let Some(tex) = &self.sat_tex {
                let alpha = (self.sat_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
                painter.image(
                    tex.id(),
                    map_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_white_alpha(alpha),
                );
            }
        }
        if self.show_mask {
            if let Some(tex) = &self.mask_tex {
                painter.image(
                    tex.id(),
                    map_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        if self.show_trees {
            if let Some(tex) = &self.overlay_tex {
                painter.image(
                    tex.id(),
                    map_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }

        // wykluczenia wektorowe
        if let Some(ex) = &self.exclusions {
            for poly in &ex.polygons {
                for ring in &poly.rings {
                    if ring.len() < 2 {
                        continue;
                    }
                    let pts: Vec<egui::Pos2> =
                        ring.iter().map(|c| to_screen(c[0], c[1])).collect();
                    painter.add(egui::Shape::line(
                        pts,
                        egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 80, 80)),
                    ));
                }
            }
        }

        // ramka mapy
        painter.rect_stroke(map_rect, 0.0, egui::Stroke::new(1.0_f32, Color32::GRAY));

        // obszary rysowane (poligony)
        let area_colors = [
            Color32::from_rgb(255, 170, 40),
            Color32::from_rgb(60, 210, 255),
            Color32::from_rgb(215, 120, 255),
            Color32::from_rgb(150, 255, 90),
        ];
        for (ai, a) in self.project.areas.iter().enumerate() {
            if a.polygon.len() < 2 {
                continue;
            }
            let col = area_colors[ai % area_colors.len()];
            let mut pts: Vec<egui::Pos2> =
                a.polygon.iter().map(|c| to_screen(c[0], c[1])).collect();
            pts.push(pts[0]); // domknięcie
            painter.add(egui::Shape::line(
                pts.clone(),
                egui::Stroke::new(2.0_f32, col),
            ));
            for p in &pts[..pts.len() - 1] {
                painter.circle_filled(*p, 3.0, col);
            }
            // etykieta w środku ciężkości obrysu
            let cx = a.polygon.iter().map(|p| p[0]).sum::<f64>() / a.polygon.len() as f64;
            let cy = a.polygon.iter().map(|p| p[1]).sum::<f64>() / a.polygon.len() as f64;
            painter.text(
                to_screen(cx, cy),
                egui::Align2::CENTER_CENTER,
                &a.label,
                egui::FontId::proportional(13.0),
                col,
            );

            // edycja tego obszaru: uchwyty wierzchołków + punkty środków krawędzi
            if self.editing_area == Some(ai) {
                for (vi, p) in a.polygon.iter().enumerate() {
                    let s = to_screen(p[0], p[1]);
                    let selected = self.editing_vertex == Some(vi);
                    painter.circle_filled(
                        s,
                        if selected { 7.0 } else { 5.0 },
                        if selected { Color32::YELLOW } else { Color32::WHITE },
                    );
                    painter.circle_stroke(
                        s,
                        if selected { 7.0 } else { 5.0 },
                        egui::Stroke::new(1.5_f32, Color32::from_rgb(40, 40, 40)),
                    );
                }
                // punkty do dodawania wierzchołka (środki odcinków)
                let n = a.polygon.len();
                for i in 0..n {
                    let a_s = to_screen(a.polygon[i][0], a.polygon[i][1]);
                    let b_s = to_screen(a.polygon[(i + 1) % n][0], a.polygon[(i + 1) % n][1]);
                    let mid = egui::pos2((a_s.x + b_s.x) / 2.0, (a_s.y + b_s.y) / 2.0);
                    painter.circle_filled(
                        mid,
                        3.0,
                        Color32::from_rgba_premultiplied(120, 220, 120, 180),
                    );
                }
            }
        }

        // szkic rysowanego właśnie poligonu
        if !self.draw_points.is_empty() {
            let yellow = Color32::YELLOW;
            let pts: Vec<egui::Pos2> =
                self.draw_points.iter().map(|p| to_screen(p[0], p[1])).collect();
            if pts.len() >= 2 {
                let mut closed = pts.clone();
                closed.push(pts[0]);
                painter.add(egui::Shape::line(
                    closed,
                    egui::Stroke::new(1.5_f32, yellow.gamma_multiply(0.55)),
                ));
            }
            for p in &pts {
                painter.circle_filled(*p, 3.5, yellow);
            }
            if let Some(hover) = resp.hover_pos() {
                if let Some(last) = pts.last() {
                    painter.line_segment(
                        [*last, hover],
                        egui::Stroke::new(1.0_f32, yellow.gamma_multiply(0.4)),
                    );
                }
            }
        }

        // legenda skali
        let km_px = 1000.0 * scale;
        painter.line_segment(
            [egui::pos2(rect.min.x + 12.0, rect.max.y - 16.0),
             egui::pos2(rect.min.x + 12.0 + km_px, rect.max.y - 16.0)],
            egui::Stroke::new(2.0_f32, Color32::WHITE),
        );
        painter.text(
            egui::pos2(rect.min.x + 16.0 + km_px, rect.max.y - 20.0),
            egui::Align2::LEFT_TOP,
            "1 km",
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
    }
}

fn canvas_origin(rect: Rect, map: f64, scale: f32, pan: Vec2) -> egui::Vec2 {
    egui::vec2(
        rect.min.x + (rect.size().x - map as f32 * scale) / 2.0 + pan.x,
        rect.min.y + (rect.size().y - map as f32 * scale) / 2.0 + pan.y,
    )
}

/// Twardy clamp widoku: mapa mniejsza od kanwy jest wycentrowana (pan = 0),
/// a większa zawsze musi zachować nakładanie się z kanwą (margines 32 px) —
/// obraz nie może "uciec" poza ekran.
fn clamp_pan_to_view(rect: Rect, mapf: f32, scale: f32, pan: Vec2) -> Vec2 {
    const MARGIN: f32 = 32.0;
    let mut p = pan;
    let half = mapf * scale * 0.5;

    // oś X
    if mapf * scale <= rect.width() + 2.0 * MARGIN {
        p.x = 0.0;
    } else {
        let lo = rect.left() + MARGIN - half;
        let hi = rect.right() - MARGIN + half;
        let cx = rect.center().x + p.x;
        p.x = cx.clamp(lo.min(hi), hi.max(lo)) - rect.center().x;
    }
    // oś Y
    if mapf * scale <= rect.height() + 2.0 * MARGIN {
        p.y = 0.0;
    } else {
        let lo = rect.top() + MARGIN - half;
        let hi = rect.bottom() - MARGIN + half;
        let cy = rect.center().y + p.y;
        p.y = cy.clamp(lo.min(hi), hi.max(lo)) - rect.center().y;
    }
    p
}

fn opt_f64(
    ui: &mut egui::Ui,
    label: String,
    val: &mut Option<f64>,
    range: std::ops::RangeInclusive<f64>,
    default: Option<f64>,
) {
    ui.horizontal(|ui| {
        let modified = *val != default;
        let enabled = val.is_some();
        if enabled {
            lbl_mod(ui, label, modified);
        } else {
            ui.label(label);
        }
        if enabled && val.is_none() {
            *val = Some(*range.start());
        }
        if !enabled {
            *val = None;
        }
        ui.add_enabled_ui(enabled, |ui| {
            if let Some(v) = val {
                let mut d = *v;
                if ui
                    .add(egui::DragValue::new(&mut d).speed(1.0).clamp_range(range.clone()))
                    .changed()
                {
                    *v = d;
                }
            }
        });
        if reset_btn(ui, modified) {
            *val = default;
            // zsynchronizuj checkbox z faktycznym stanem
        }
    });
}
