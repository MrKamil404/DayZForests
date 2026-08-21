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
use forest_core::preset::{ElevationMode, ForestProject, ZoneDef};
use forest_core::scatter::{generate, GenStats, PlacedObject};
use forest_core::species::species_preview_color;
use forest_core::user_presets::{UserPreset, UserPresetLibrary, DEFAULT_PRESETS_FILE};

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
    draw_preset: usize,

    /// Indeks strefy, której kolor jest właśnie edytowany.
    zone_color_edit: Option<usize>,

    // presety użytkownika (edytowalne kopie + własne)
    user_presets: Vec<UserPreset>,
    preset_edit_open: Option<usize>,
    presets_dirty: bool,
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
            draw_preset: 0,
            zone_color_edit: None,
            user_presets: Vec::new(),
            preset_edit_open: None,
            presets_dirty: false,
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
                let tex = preview_texture(ctx, "satellite", &m, TextureOptions::LINEAR);
                self.sat_tex = Some(tex);
                self.show_sat = true;
                self.info = Some(format!(
                    "Wczytano podkład satelitarny: {path} ({}x{} px)",
                    m.width, m.height
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
        if self.busy {
            return;
        }
        if self.mask.is_none() && self.project.areas.is_empty() {
            self.error =
                Some("Wczytaj maskę PNG albo narysuj przynajmniej jeden obszar (poligon).".into());
            return;
        }
        // anuluj niedokończony szkic przy starcie generowania
        self.draw_mode = false;
        self.draw_points.clear();
        self.project.sanitize();
        if let Err(e) = self.project.validate() {
            self.error = Some(e);
            return;
        }
        let proj = self.project.clone();
        let mask = self.mask.clone();
        let hm = self.heightmap.clone();
        let ex = self.exclusions.clone();
        let prog = self.progress.clone();
        *prog.lock().unwrap() = 0.0;

        let (tx, rx) = channel();
        self.gen_rx = Some(rx);
        self.busy = true;
        self.error = None;
        self.info = Some("Generowanie w toku...".into());

        std::thread::spawn(move || {
            let res = generate(&proj, mask.as_deref(), hm.as_deref(), ex.as_deref(), &|f| {
                *prog.lock().unwrap() = f;
            });
            tx.send(res).ok();
        });
        ctx.request_repaint();
    }

    fn poll_generation(&mut self, ctx: &egui::Context) {
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
                self.info = Some("Gotowe.".into());
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
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Projekt DayZForests", &["json"])
            .pick_file()
        {
            match ForestProject::load(&path) {
                Ok(mut p) => {
                    p.sanitize();
                    self.project = p;
                    self.overlay_tex = None;
                    self.objects.clear();
                    self.stats = None;
                    self.info = Some(format!("Wczytano projekt: {}", path.display()));
                    self.reload_project_files(ctx);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Doczytuje maskę/ASC/GeoJSON wskazane przez projekt (jeśli istnieją).
    fn reload_project_files(&mut self, ctx: &egui::Context) {
        if let Some(m) = self.project.paths.mask.clone() {
            if std::path::Path::new(&m).exists() && self.mask.is_none() {
                self.load_mask(ctx, &m);
            }
        }
        if let Some(a) = self.project.paths.heightmap_asc.clone() {
            if std::path::Path::new(&a).exists() && self.heightmap.is_none() {
                self.load_heightmap(&a);
            }
        }
        if let Some(g) = self.project.paths.exclusions_geojson.clone() {
            if std::path::Path::new(&g).exists() && self.exclusions.is_none() {
                self.load_exclusions(&g);
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
        let Some(snap) = self.resolve_preset(self.draw_preset) else {
            self.error = Some("Nieprawidłowy preset.".into());
            return;
        };
        let label = format!("Obszar {}", self.project.areas.len() + 1);
        let area_ha = forest_core::scatter::polygon_area_m2(&self.draw_points) / 10_000.0;
        // mapowanie modeli na indeksy bieżącej listy gatunków
        let mapped: Vec<(usize, f32)> = {
            let species = &self.project.species;
            snap.weights
                .iter()
                .filter_map(|(model, w)| {
                    species
                        .iter()
                        .position(|s| s.model.eq_ignore_ascii_case(model))
                        .map(|idx| (idx, *w))
                })
                .collect()
        };
        if mapped.is_empty() {
            self.error = Some(format!(
                "Preset '{}': żaden z jego modeli nie występuje na liście gatunków.",
                snap.name
            ));
            return;
        }
        self.project.areas.push(forest_core::preset::AreaDef {
            label,
            density_per_ha: snap.density_per_ha,
            species_weights: mapped,
            polygon: std::mem::take(&mut self.draw_points),
        });
        self.info = Some(format!(
            "Dodano obszar '{}' ({:.1} ha ≈ {} szt przy {:.0}/ha).",
            self.project.areas.last().unwrap().label,
            area_ha,
            (area_ha * f64::from(snap.density_per_ha)).round() as u64,
            snap.density_per_ha
        ));
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
    for o in objects {
        let px = (o.x * mx) as i64;
        let py = ((project.map_size_m - o.y) * my) as i64;
        let c = species_preview_color(o.species_index);
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

// --- App impl -----------------------------------------------------------------

impl ForestApp {
    /// Łączna liczba presetów: wbudowane + użytkownika.
    fn preset_count(&self) -> usize {
        forest_core::species::zone_presets().len() + self.user_presets.len()
    }

    /// Znormalizowany podgląd presetu o łącznym indeksie `i`.
    fn resolve_preset(&self, i: usize) -> Option<PSnap> {
        let builtins = forest_core::species::zone_presets();
        if let Some(p) = builtins.get(i) {
            let up = UserPreset::from_builtin(p);
            return Some(PSnap {
                name: up.name,
                density_per_ha: up.density_per_ha,
                weights: up.weights,
            });
        }
        self.user_presets
            .get(i - builtins.len())
            .map(|u| PSnap {
                name: u.name.clone(),
                density_per_ha: u.density_per_ha,
                weights: u.weights.clone(),
            })
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
        });
    }

    /// Wagi z presetu (łączny indeks) przypisane do obszaru.
    fn apply_preset_to_area_index(&mut self, ai: usize, pi: usize) {
        let Some(snap) = self.resolve_preset(pi) else { return };
        // najpierw zmapuj modele na indeksy (pożyczenie niemutowalne), potem mutacja
        let mapped: Vec<(usize, f32)> = {
            let species = &self.project.species;
            snap.weights
                .iter()
                .filter_map(|(model, w)| {
                    species
                        .iter()
                        .position(|s| s.model.eq_ignore_ascii_case(model))
                        .map(|idx| (idx, *w))
                })
                .collect()
        };
        if mapped.is_empty() {
            self.error = Some(format!(
                "Preset '{}': żaden z jego modeli nie występuje na liście gatunków.",
                snap.name
            ));
            return;
        }
        if let Some(a) = self.project.areas.get_mut(ai) {
            a.density_per_ha = snap.density_per_ha;
            a.species_weights = mapped;
        }
    }
}

impl eframe::App for ForestApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    ui.label("Generowanie...");
                } else if let Some(s) = &self.stats {
                    ui.label(format!(
                        "Obiektów: {} | granica: {} | gatunków >0: {} | {} ms",
                        s.total,
                        s.edge_count,
                        s.per_species.iter().filter(|(_, c)| *c > 0).count(),
                        s.elapsed_ms
                    ));
                } else {
                    ui.label("Gotowy.");
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
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled(!self.busy, egui::Button::new("▶ Generuj"))
                .clicked()
                .then(|| self.start_generation(ui.ctx()));
            ui.add_enabled(
                !self.objects.is_empty(),
                egui::Button::new("💾 Eksport TXT (TB)"),
            )
            .clicked()
            .then(|| self.export_txt());
            ui.separator();
            if ui.button("📂 Wczytaj projekt").clicked() {
                self.load_project(ui.ctx());
            }
            if ui.button("💾 Zapisz projekt").clicked() {
                self.save_project();
            }
            ui.separator();
            // tryb rysowania poligonów — preset z listy łącznej (wbudowane + moje)
            egui::ComboBox::from_id_source("draw_preset")
                .selected_text(format!(
                    "✏ {}",
                    self.resolve_preset(self.draw_preset)
                        .map_or("—".to_string(), |s| s.name)
                ))
                .show_ui(ui, |ui| {
                    let n_builtin = forest_core::species::zone_presets().len();
                    for i in 0..self.preset_count() {
                        let Some(s) = self.resolve_preset(i) else { continue };
                        let label = if i < n_builtin {
                            s.name.clone()
                        } else {
                            format!("★ {}", s.name)
                        };
                        ui.selectable_value(&mut self.draw_preset, i, label);
                    }
                });
            if ui
                .add(egui::Button::new(if self.draw_mode {
                    "✏ Rysowanie: WŁ"
                } else {
                    "✏ Rysuj obszar"
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
                    format!("pkt: {}", self.draw_points.len()),
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
            ui.checkbox(&mut self.show_sat, "Podkład");
            ui.checkbox(&mut self.show_mask, "Maska");
            ui.checkbox(&mut self.show_trees, "Drzewa");
            if ui.button("Dopasuj widok").clicked() {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
        });
        ui.separator();
    }

    fn ui_params(&mut self, ui: &mut egui::Ui) {
        // Pliki
        CollapsingHeader::new("📁 Pliki")
            .default_open(true)
            .show(ui, |ui| {
                if ui.button("Wczytaj maskę (PNG/BMP/TGA)...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Obrazy", &["png", "bmp", "tga", "jpg", "jpeg"])
                        .pick_file()
                    {
                        self.load_mask(ui.ctx(), &p.to_string_lossy());
                    }
                }
                ui.label(self.project.paths.mask.as_deref().unwrap_or("(brak maski)"))
                    .on_hover_text("Kolory = strefy lasu");

                if ui.button("Wczytaj podkład satelitarny (PNG/JPG)...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Obrazy", &["png", "jpg", "jpeg", "bmp", "tga"])
                        .pick_file()
                    {
                        self.load_satellite(ui.ctx(), &p.to_string_lossy());
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Krycie podkładu:");
                    ui.add(
                        egui::Slider::new(&mut self.sat_alpha, 0.0..=1.0).text(if self.sat_tex.is_some() { "" } else { "(brak)" }),
                    );
                });

                if ui.button("Wczytaj heightmapę (.asc)...").clicked() {
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
                        .unwrap_or("(brak — elevation=0, obiekty na terenie)"),
                );

                if ui.button("Wczytaj wykluczenia (.geojson)...").clicked() {
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
                        .unwrap_or("(brak wykluczeń wektorowych)"),
                );

                if let Some(m) = &self.mask {
                    ui.separator();
                    ui.small(format!("Maska: {}x{} px", m.width, m.height));
                }
            });

        // Mapa
        CollapsingHeader::new("🗺 Mapa / eksport")
            .default_open(true)
            .show(ui, |ui| {
                let p = &mut self.project;
                ui.horizontal(|ui| {
                    ui.label("Rozmiar [m]:");
                    ui.add(egui::DragValue::new(&mut p.map_size_m).speed(10.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Easting offset:");
                    ui.add(egui::DragValue::new(&mut p.easting_offset).speed(100.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Northing offset:");
                    ui.add(egui::DragValue::new(&mut p.northing_offset).speed(100.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Ziarno (seed):");
                    ui.add(egui::DragValue::new(&mut p.seed).speed(1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Wysokość:");
                    egui::ComboBox::from_id_source("elev_mode")
                        .selected_text(match p.elevation_mode {
                            ElevationMode::RelativeZero => "relative (0)",
                            ElevationMode::AbsoluteSampled => "absolute (z ASC)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut p.elevation_mode,
                                ElevationMode::RelativeZero,
                                "relative (0)",
                            );
                            ui.selectable_value(
                                &mut p.elevation_mode,
                                ElevationMode::AbsoluteSampled,
                                "absolute (z ASC)",
                            );
                        });
                });
            });

        // Źródła generowania
        CollapsingHeader::new("⚙ Źródła generowania")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(&mut self.project.use_mask_zones, "Strefy z maski (kolory)")
                    .on_hover_text("Generuj w obszarach wskazanych kolorami maski");
                ui.checkbox(&mut self.project.use_areas, "Obszary rysowane (poligony)")
                    .on_hover_text("Generuj wewnątrz narysowanych poligonów");
                ui.checkbox(&mut self.project.edges.enabled, "Granica lasu (krzewy)")
                    .on_hover_text("Pas krzewów/podrostu wzdłuż krawędzi lasu (parametry w sekcji 🧱)");
                ui.separator();
                ui.small(format!(
                    "Aktywne źródła: {}{}{}",
                    if self.project.use_mask_zones { "maska " } else { "" },
                    if self.project.use_areas { "poligony " } else { "" },
                    if self.project.edges.enabled { "+ granica" } else { "" }
                ));
                ui.small(format!(
                    "Ziarno: {} | mnożnik odstępów: {:.2} | polany: {:.0}%",
                    self.project.seed,
                    self.project.spacing_multiplier,
                    self.project.clearing_strength * 100.0
                ));
            });

        // Granica lasu (krzewy/podrost)
        CollapsingHeader::new("🧱 Granica lasu")
            .default_open(false)
            .show(ui, |ui| {
                let e = &mut self.project.edges;
                ui.checkbox(&mut e.enabled, "Włącz pas graniczny (krzewy wzdłuż krawędzi lasu)");
                ui.add_enabled_ui(e.enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Szerokość pasa [m]:");
                        ui.add(
                            egui::DragValue::new(&mut e.band_width_m)
                                .speed(1.0)
                                .clamp_range(1.0..=300.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Gęstość [szt/ha]:");
                        ui.add(
                            egui::DragValue::new(&mut e.density_per_ha)
                                .speed(1.0)
                                .clamp_range(1.0..=5000.0),
                        );
                    });
                    ui.checkbox(&mut e.blend, "Wtapianie pasa")
                        .on_hover_text("Gęstość zanika wraz z odległością od granicy \
                                        (najgęściej przy samej krawędzi lasu) — bez twardej linii krzaków");
                    ui.horizontal(|ui| {
                        ui.label("Poszarpanie granicy [m]:")
                            .on_hover_text("Szum przesuwający linię lasu — brzeg nie jest \
                                            równy jak od linijki. Dotyczy też obrysów poligonów.");
                        ui.add(
                            egui::DragValue::new(&mut e.jagged_m)
                                .speed(1.0)
                                .clamp_range(0.0..=200.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Wtapianie w las [m]:")
                            .on_hover_text("Jak głęboko od krawędzi gęstość drzew narasta \
                                            0 -> pełna. Otwarte, naturalne obrzeża lasu.");
                        ui.add(
                            egui::DragValue::new(&mut e.blend_inside_m)
                                .speed(1.0)
                                .clamp_range(0.0..=500.0),
                        );
                    });
                    ui.small("Wagi gatunków granicy:");
                    let n_species = self.project.species.len();
                    for si in 0..n_species {
                        let sp = &self.project.species[si];
                        let c = species_preview_color(si);
                        ui.horizontal(|ui| {
                            let (_r, resp) =
                                ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter_at(resp.rect).circle_filled(
                                resp.rect.center(),
                                4.0,
                                Color32::from_rgb(c[0], c[1], c[2]),
                            );
                            ui.label(sp.label.as_str());
                            let pos = e.species_weights.iter().position(|(i, _)| *i == si);
                            let mut w = pos.map_or(0.0, |p_| e.species_weights[p_].1);
                            let before = w;
                            ui.add(
                                egui::DragValue::new(&mut w)
                                    .speed(0.05)
                                    .clamp_range(0.0..=20.0),
                            );
                            if (w - before).abs() > f32::EPSILON {
                                match pos {
                                    Some(p_) => {
                                        if w <= 0.0 {
                                            e.species_weights.remove(p_);
                                        } else {
                                            e.species_weights[p_].1 = w;
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
            });

        // Filtry
        CollapsingHeader::new("⛰ Filtry środowiskowe")
            .default_open(false)
            .show(ui, |ui| {
                let p = &mut self.project;
                opt_f64(ui, "Min. wysokość [m]", &mut p.min_altitude, -1.0..=8000.0);
                opt_f64(ui, "Maks. wysokość [m]", &mut p.max_altitude, -1.0..=8000.0);
                opt_f64(ui, "Maks. spadek [°]", &mut p.max_slope_deg, 0.0..=90.0);
                ui.horizontal(|ui| {
                    ui.label("Tolerancja koloru:");
                    ui.add(egui::DragValue::new(&mut p.color_tolerance).clamp_range(0..=255));
                });
                ui.horizontal(|ui| {
                    ui.label("Margines krawędzi [m]:");
                    ui.add(egui::DragValue::new(&mut p.edge_padding_m).speed(1.0).clamp_range(0.0..=1000.0));
                });
                ui.separator();
                ui.small("Kolory wykluczone (np. drogi/woda):");
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
                            ui.small("Analizuję kolory maski...");
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
                                                if ui.button("+ Wyklucz").clicked() {
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
        CollapsingHeader::new("🎲 Rozrzut / polany")
            .default_open(false)
            .show(ui, |ui| {
                let p = &mut self.project;
                ui.horizontal(|ui| {
                    ui.label("Mnożnik odstępów:")
                        .on_hover_text(">1 = rzadszy las");
                    ui.add(egui::DragValue::new(&mut p.spacing_multiplier).speed(0.02).clamp_range(0.05..=5.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Skala polan [m]:")
                        .on_hover_text("Długość fali szumu polan; 0 = wyłączone");
                    ui.add(egui::DragValue::new(&mut p.clearing_scale_m).speed(5.0).clamp_range(0.0..=5000.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Siła polan:")
                        .on_hover_text("Jaka część obszaru ma być prześwitem");
                    ui.add(egui::Slider::new(&mut p.clearing_strength, 0.0..=0.9));
                });
            });

        // Gatunki
        CollapsingHeader::new("🌳 Gatunki")
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
                    ui.small("TB wymaga wpisu w Template Library dla każdego modelu.");
                });
                ui.separator();
                let mut remove: Option<usize> = None;
                let mut last_group = String::new();
                for (i, sp) in self.project.species.iter_mut().enumerate() {
                    // nagłówek grupy (puste pole grupy -> "Inne")
                    let group_label = if sp.group.is_empty() {
                        "Inne".to_string()
                    } else {
                        sp.group.clone()
                    };
                    if group_label != last_group {
                        ui.separator();
                        ui.strong(group_label.clone());
                        last_group = group_label;
                    }
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let c = species_preview_color(i);
                            let (_r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter_at(resp.rect).circle_filled(resp.rect.center(), 4.0, Color32::from_rgb(c[0], c[1], c[2]));
                            ui.text_edit_singleline(&mut sp.label);
                            if ui.button("✖").on_hover_text("Usuń gatunek").clicked() {
                                remove = Some(i);
                            }
                        });
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
        ScrollArea::vertical().show(ui, |ui| {
        // 📊 Wynik generowania
        CollapsingHeader::new("📊 Wygenerowane warstwy")
            .default_open(true)
            .show(ui, |ui| {
                match &self.stats {
                    Some(s) => {
                        ui.label(format!(
                            "Razem: {} obiektów | granica: {} | {} ms",
                            s.total, s.edge_count, s.elapsed_ms
                        ));
                        ui.separator();
                        if s.per_source.is_empty() {
                            ui.small("(brak źródeł)");
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
                        ui.small("Brak wyników — kliknij „▶ Generuj”. Po generowaniu tutaj pojawią się warstwy.");
                    }
                }
            });

        // Strefy
        CollapsingHeader::new("🌲 Strefy lasu")
            .default_open(true)
            .show(ui, |ui| {
                // --- presety roślinności (pogrupowane) ----------------------
                ui.small("Dodaj strefę z presetu (⧉ kopiuje do „Moje presety”):");
                let presets = forest_core::species::zone_presets();
                let mut last_group = "";
                ScrollArea::vertical()
                    .max_height(190.0)
                    .id_source("zone_presets")
                    .show(ui, |ui| {
                        for p in presets.iter() {
                            if p.group != last_group {
                                ui.separator();
                                ui.strong(p.group);
                                last_group = p.group;
                            }
                            ui.horizontal(|ui| {
                                if ui.button("+").on_hover_text(format!(
                                    "Dodaj strefę '{}' ({:.0} szt/ha)",
                                    p.name, p.density_per_ha
                                )) .clicked()
                                {
                                    self.add_zone_from_preset(*p);
                                }
                                ui.label(p.name);
                                ui.weak(format!("{:.0}/ha", p.density_per_ha));
                                if ui
                                    .button("⧉")
                                    .on_hover_text("Kopiuj do „Moje presety” (edytowalna kopia)")
                                    .clicked()
                                {
                                    self.user_presets.push(UserPreset::from_builtin(p));
                                    self.presets_dirty = true;
                                }
                            });
                        }
                    });
                ui.separator();

                let mut to_remove: Option<usize> = None;
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
                        if ui.button("✖").on_hover_text("Usuń strefę").clicked() {
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
                                ui.small("RGB ręcznie:");
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

                    ui.horizontal(|ui| {
                        ui.label("Gęstość [szt/ha]:");
                        ui.add(egui::DragValue::new(&mut z.density_per_ha).speed(1.0).clamp_range(1.0..=5000.0));
                    });
                    ui.indent(format!("zw{zi}"), |ui| {
                        ui.small("Wagi gatunków:");
                        let mut changed = false;
                        for wi in 0..z.species_weights.len() {
                            let (si, _) = z.species_weights[wi];
                            let Some(sp) = self.project.species.get(si) else { continue };
                            ui.horizontal(|ui| {
                                let c = species_preview_color(si);
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
                    ui.small("Dodaj strefę z koloru maski:");
                    match &self.histogram {
                        None => {
                            ui.small("Analizuję kolory maski...");
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
                                                if ui.button("+ Dodaj strefę").clicked() {
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
        CollapsingHeader::new("📚 Moje presety")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("+ Nowy pusty preset").clicked() {
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
                    if ui.button("💾 Zapisz").clicked() {
                        self.save_user_presets();
                    }
                    if self.presets_dirty {
                        ui.colored_label(Color32::YELLOW, "• niezapisane");
                    }
                });
                ui.small(format!("Plik: {} (katalog roboczy programu)", DEFAULT_PRESETS_FILE));
                ui.separator();

                if self.user_presets.is_empty() {
                    ui.small(
                        "Brak własnych presetów. Skopiuj wbudowane przyciskiem ⧉ \
                         w sekcji 🌲 Strefy lasu albo dodaj pusty powyżej.",
                    );
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
                        ui.strong(p.group.clone());
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
                            .on_hover_text("Edytuj preset")
                            .clicked()
                        {
                            self.preset_edit_open =
                                if editing { None } else { Some(i) };
                        }
                        ui.text_edit_singleline(&mut p.name);
                        if ui
                            .button("+ Strefa")
                            .on_hover_text("Dodaj strefę z tego presetu")
                            .clicked()
                        {
                            pending_add = Some(i);
                        }
                        let _ = &snap_for_actions;
                        if ui.button("⧉").on_hover_text("Duplikuj").clicked() {
                            pending_dup = Some(i);
                        }
                        if ui.button("🗑").on_hover_text("Usuń preset").clicked() {
                            to_remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Grupa:");
                        ui.add(egui::TextEdit::singleline(&mut p.group).desired_width(110.0));
                        ui.label("Gęstość [szt/ha]:");
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
                                let c = species_preview_color(si);
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
        CollapsingHeader::new("📐 Obszary (poligony)")
            .default_open(false)
            .show(ui, |ui| {
                ui.small(format!(
                    "Rysowane na mapie: wybierz preset na pasku → ✏ Rysuj obszar → klikaj wierzchołki (LPM), Enter/dwuklik = zakończ."
                ));
                ui.separator();
                // łączna lista presetów policzona PRZED iteracją (bez pożyczania self)
                let n_builtin = forest_core::species::zone_presets().len();
                let all_presets: Vec<Option<PSnap>> =
                    (0..self.preset_count()).map(|i| self.resolve_preset(i)).collect();
                let mut to_remove: Option<usize> = None;
                let mut pending_preset: Option<(usize, usize)> = None; // (area_idx, preset_idx)
                for (ai, a) in self.project.areas.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let col = [Color32::from_rgb(255, 170, 40), Color32::from_rgb(60, 210, 255)][ai % 2];
                            let (_r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter_at(resp.rect).circle_filled(resp.rect.center(), 4.0, col);
                            ui.text_edit_singleline(&mut a.label);
                            if ui.button("✖").on_hover_text("Usuń obszar").clicked() {
                                to_remove = Some(ai);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Gęstość [szt/ha]:");
                            ui.add(
                                egui::DragValue::new(&mut a.density_per_ha)
                                    .speed(1.0)
                                    .clamp_range(1.0..=5000.0),
                            );
                            let ha =
                                forest_core::scatter::polygon_area_m2(&a.polygon) / 10_000.0;
                            let est = (ha * f64::from(a.density_per_ha)).round() as u64;
                            ui.label(format!(
                                "{:.1} ha | pkt: {} | ≈ {} szt",
                                ha,
                                a.polygon.len(),
                                est
                            ));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Preset:");
                            egui::ComboBox::from_id_source(format!("area_preset_{ai}"))
                                .selected_text("(zachowaj)")
                                .show_ui(ui, |ui| {
                                    for (pi, s) in all_presets.iter().enumerate() {
                                        let Some(s) = s else { continue };
                                        let label = if pi < n_builtin {
                                            s.name.clone()
                                        } else {
                                            format!("★ {}", s.name)
                                        };
                                        if ui.selectable_label(false, label).clicked() {
                                            pending_preset = Some((ai, pi));
                                        }
                                    }
                                });
                        });
                    });
                }
                if let Some((ai, pi)) = pending_preset {
                    self.apply_preset_to_area_index(ai, pi);
                }
                if let Some(i) = to_remove {
                    self.project.areas.remove(i);
                }
                if !self.project.areas.is_empty()
                    && ui.button("Wyczyść wszystkie obszary").clicked()
                {
                    self.project.areas.clear();
                }
            });


        });
    }    // --- kanvas -------------------------------------------------------------

    fn ui_canvas(&mut self, ui: &mut egui::Ui) {
        if self.mask.is_none() && self.project.areas.is_empty() && !self.draw_mode {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("DayZ Forest Generator");
                    ui.add_space(12.0);
                    ui.label("1. Wczytaj maskę PNG (kolory = strefy lasu)");
                    ui.label("2. ...albo narysuj obszary: wybierz preset i klikaj wierzchołki na mapie");
                    ui.label("3. Opcjonalnie: heightmapa .asc, wykluczenia .geojson, granice lasu");
                    ui.label("4. Generuj → Eksport TXT → import w Terrain Builderze");
                });
            });
            return;
        }

        let map = self.project.map_size_m;
        // click_and_drag: drag-sense nie rejestruje kliknięć (wymagane dla
        // dodawania wierzchołków poligonu), a klik + lekki ruch = pan
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

        // interakcja: pan + zoom do kursora
        if resp.dragged_by(PointerButton::Primary) || resp.dragged_by(PointerButton::Secondary) {
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

fn opt_f64(ui: &mut egui::Ui, label: &str, val: &mut Option<f64>, range: std::ops::RangeInclusive<f64>) {
    ui.horizontal(|ui| {
        let mut enabled = val.is_some();
        ui.checkbox(&mut enabled, "");
        if enabled && val.is_none() {
            *val = Some(*range.start());
        }
        if !enabled {
            *val = None;
        }
        ui.add_enabled_ui(enabled, |ui| {
            ui.label(label);
            if let Some(v) = val {
                let mut d = *v;
                if ui.add(egui::DragValue::new(&mut d).speed(1.0).clamp_range(range.clone())).changed() {
                    *v = d;
                }
            }
        });
    });
}
