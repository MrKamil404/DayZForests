//! Silnik rozrzutu roślinności: Poisson-disk (dart throwing z siatką przestrzenną),
//! szum polan, filtry wysokości/spadku, wykluczenia i proporcje gatunków.
//!
//! Źródła punktów (wspólna siatka odstępów):
//! - strefy z maski (kolory),
//! - obszary rysowane ręcznie (poligony, `areas`),
//! - pas graniczny lasu (`edges`) — krzewy/podrost wzdłuż krawędzi.

use std::collections::HashMap;

use rand::prelude::*;

use crate::geojson::{point_in_polygon, point_ring_distance, GeoJsonData, Polygon};
use crate::heightmap::AscHeightmap;
use crate::mask::{MaskImage, Rgb8};
use crate::preset::{ElevationMode, ForestProject};

#[derive(Clone, Debug)]
pub struct PlacedObject {
    pub model: String,
    /// Współrzędne świata w metrach (origin SW, bez offsetu easting).
    pub x: f64,
    pub y: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub scale: f64,
    pub elevation: f64,
    pub zone_index: usize,
    pub species_index: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GenStats {
    pub total: usize,
    pub per_species: Vec<(String, usize)>,
    pub per_source: Vec<(String, usize)>,
    /// Obiekty pasa granicznego.
    pub edge_count: usize,
    pub rejected_spacing: usize,
    pub rejected_filters: usize,
    pub rejected_exclusion: usize,
    pub rejected_clearing: usize,
    pub elapsed_ms: u128,
}

impl GenStats {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Wygenerowano {} obiektów w {} ms",
            self.total, self.elapsed_ms
        );
        if !self.per_source.is_empty() {
            s.push_str("\nStrefy/obszary:");
            for (n, c) in &self.per_source {
                s.push_str(&format!("\n  {n}: {c}"));
            }
        }
        if self.edge_count > 0 {
            s.push_str(&format!("\nGranica lasu: {}", self.edge_count));
        }
        s.push_str("\nGatunki:");
        for (n, c) in &self.per_species {
            if *c > 0 {
                s.push_str(&format!("\n  {n}: {c}"));
            }
        }
        let rej = self.rejected_spacing
            + self.rejected_filters
            + self.rejected_exclusion
            + self.rejected_clearing;
        if rej > 0 {
            s.push_str(&format!(
                "\nOdrzucone: {rej} (odstęp {}, filtry {}, wykluczenia {}, polany {})",
                self.rejected_spacing,
                self.rejected_filters,
                self.rejected_exclusion,
                self.rejected_clearing
            ));
        }
        s
    }
}

/// Callback postępu: argument = ułamek 0..1.
pub type Progress<'a> = &'a dyn Fn(f64);

const MAX_TOTAL_OBJECTS: usize = 2_000_000;
/// Limit liczby stref z maski (mapa identyfikatorów pikseli to u8).
const MAX_ZONES: usize = 254;

// --- Środowisko filtrów -----------------------------------------------------

enum Rej {
    Filters,
    Exclusion,
    Clearing,
}

struct Env<'a> {
    project: &'a ForestProject,
    hm: Option<&'a AscHeightmap>,
    ex: Option<&'a GeoJsonData>,
    noise_seed: u64,
    use_clearings: bool,
}

impl Env<'_> {
    fn eval(&self, wx: f64, wy: f64) -> Result<f64, Rej> {
        let p = self.project;
        let pad = p.edge_padding_m.max(0.0);
        let size = p.map_size_m;
        if wx < pad || wy < pad || wx > size - pad || wy > size - pad {
            return Err(Rej::Filters);
        }
        if self.use_clearings {
            let nx = wx / p.clearing_scale_m;
            let ny = wy / p.clearing_scale_m;
            if fbm2(nx, ny, self.noise_seed) < clearing_threshold(p.clearing_strength) {
                return Err(Rej::Clearing);
            }
        }
        let elevation;
        if let Some(hm) = self.hm {
            match hm.sample(wx, wy) {
                Some(h) => {
                    if let Some(mn) = p.min_altitude {
                        if h < mn {
                            return Err(Rej::Filters);
                        }
                    }
                    if let Some(mx) = p.max_altitude {
                        if h > mx {
                            return Err(Rej::Filters);
                        }
                    }
                    if let Some(max_slope) = p.max_slope_deg {
                        match hm.slope_deg(wx, wy) {
                            Some(s) if s <= max_slope => {}
                            _ => return Err(Rej::Filters),
                        }
                    }
                    elevation = match p.elevation_mode {
                        ElevationMode::AbsoluteSampled => h,
                        ElevationMode::RelativeZero => 0.0,
                    };
                }
                None => return Err(Rej::Filters),
            }
        } else {
            elevation = 0.0;
        }
        if let Some(ex) = self.ex {
            if ex.contains(wx, wy) {
                return Err(Rej::Exclusion);
            }
        }
        Ok(elevation)
    }
}

// --- Pomocnicze --------------------------------------------------------------

fn cumulative(weights: &[(usize, f32)]) -> Vec<(usize, f64)> {
    let total: f64 = weights.iter().map(|(_, w)| f64::from(*w)).sum();
    let mut acc = 0.0;
    weights
        .iter()
        .map(|(si, w)| {
            acc += f64::from(*w) / total;
            (*si, acc)
        })
        .collect()
}

fn pick_species(cum: &[(usize, f64)], rng: &mut StdRng) -> usize {
    let pick: f64 = rng.gen();
    cum.iter()
        .find(|(_, a)| pick <= *a)
        .map_or(cum[cum.len() - 1].0, |(si, _)| *si)
}

/// Pole poligonu (shoelace). Dla wielokątów samoprzecinających się wynik
/// jest bez sensu — używaj po sprawdzeniu `polygon_self_intersects`.
pub fn polygon_area_m2(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        sum += (ring[j][0] + ring[i][0]) * (ring[j][1] - ring[i][1]);
        j = i;
    }
    (sum / 2.0).abs()
}

fn polygon_area(ring: &[[f64; 2]]) -> f64 {
    polygon_area_m2(ring)
}

fn cross(ox: f64, oy: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - ox) * (by - oy) - (ay - oy) * (bx - ox)
}

fn on_segment(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> bool {
    // p leży na odcinku ab (zakładając collinearity)
    px.min(bx) >= ax.min(bx) - 1e-9
        && px.max(bx) <= ax.max(bx) + 1e-9
        && py.min(by) >= ay.min(by) - 1e-9
        && py.max(by) <= ay.max(by) + 1e-9
}

fn segments_intersect(a1: &[f64; 2], a2: &[f64; 2], b1: &[f64; 2], b2: &[f64; 2]) -> bool {
    let d1 = cross(b1[0], b1[1], b2[0], b2[1], a1[0], a1[1]);
    let d2 = cross(b1[0], b1[1], b2[0], b2[1], a2[0], a2[1]);
    let d3 = cross(a1[0], a1[1], a2[0], a2[1], b1[0], b1[1]);
    let d4 = cross(a1[0], a1[1], a2[0], a2[1], b2[0], b2[1]);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true; // klasyczne przecięcie
    }
    // styk (collinear / wspólny punkt) też traktujemy jako przecięcie
    if d1.abs() < 1e-12 && on_segment(b1[0], b1[1], b2[0], b2[1], a1[0], a1[1]) {
        return true;
    }
    if d2.abs() < 1e-12 && on_segment(b1[0], b1[1], b2[0], b2[1], a2[0], a2[1]) {
        return true;
    }
    if d3.abs() < 1e-12 && on_segment(a1[0], a1[1], a2[0], a2[1], b1[0], b1[1]) {
        return true;
    }
    if d4.abs() < 1e-12 && on_segment(a1[0], a1[1], a2[0], a2[1], b2[0], b2[1]) {
        return true;
    }
    false
}

/// Wykrywa samoprzecięcia obrysu (O(n²)). Wielokąty samoprzecinające się
/// łamią ray-casting i shoelace'a — muszą być poprawione przez użytkownika.
pub fn polygon_self_intersects(ring: &[[f64; 2]]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let a1 = ring[i];
        let a2 = ring[(i + 1) % n];
        if (a1[0] - a2[0]).abs() < 1e-12 && (a1[1] - a2[1]).abs() < 1e-12 {
            continue; // zerowy odcinek — pomijamy
        }
        for j in (i + 1)..n {
            // sąsiednie odcinki współdzielą wierzchołek — to legalne
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let b1 = ring[j];
            let b2 = ring[(j + 1) % n];
            if segments_intersect(&a1, &a2, &b1, &b2) {
                return true;
            }
        }
    }
    false
}

fn polygon_perimeter(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        sum += (ring[i][0] - ring[j][0]).hypot(ring[i][1] - ring[j][1]);
        j = i;
    }
    sum
}

fn bbox_of(ring: &[[f64; 2]]) -> (f64, f64, f64, f64) {
    let first = ring.first().copied().unwrap_or([0.0, 0.0]);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first[0], first[1], first[0], first[1]);
    for p in ring {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    (min_x, min_y, max_x, max_y)
}

/// Kandydat: losowy piksel zbioru + jitter w obrębie piksela -> współrzędne świata.
fn make_px_cand<'a>(
    set: &'a [u32],
    width: u32,
    size: f64,
    ps: f64,
) -> impl FnMut(&mut StdRng) -> Option<(f64, f64)> + 'a {
    move |rng: &mut StdRng| {
        if set.is_empty() {
            return None;
        }
        let idx = set[rng.gen_range(0..set.len())];
        let px = (idx % width) as f64;
        let row = (idx / width) as f64;
        Some((
            (px + 0.5 + rng.gen_range(-0.5..0.5)) * ps,
            size - (row + 0.5 + rng.gen_range(-0.5..0.5)) * ps,
        ))
    }
}

// --- Pas graniczny na masce (dylatacja BFS po pikselach strefy) ----------------

fn mask_edge_band(
    width: u32,
    height: u32,
    zone_id: &[u8],
    zi: u8,
    zone_pixels: &[u32],
    band_px: usize,
) -> Vec<(u32, u8)> {
    let mut dist: HashMap<u32, u8> = HashMap::new();
    let mut frontier: Vec<u32> = Vec::new();

    for &idx in zone_pixels {
        let x = idx % width;
        let y = idx / width;
        let i = idx as usize;
        let edge = x == 0
            || y == 0
            || x + 1 >= width
            || y + 1 >= height
            || zone_id[i - 1] != zi
            || zone_id[i + 1] != zi
            || zone_id[i - width as usize] != zi
            || zone_id[i + width as usize] != zi;
        if edge && dist.insert(idx, 0).is_none() {
            frontier.push(idx);
        }
    }

    for level in 0..band_px {
        if frontier.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for &idx in &frontier {
            let x = idx % width;
            let y = idx / width;
            let mut neigh: [Option<usize>; 4] = [None; 4];
            if x > 0 {
                neigh[0] = Some(idx as usize - 1);
            }
            if x + 1 < width {
                neigh[1] = Some(idx as usize + 1);
            }
            if y > 0 {
                neigh[2] = Some(idx as usize - width as usize);
            }
            if y + 1 < height {
                neigh[3] = Some(idx as usize + width as usize);
            }
            for n in neigh.into_iter().flatten() {
                if zone_id[n] == zi && dist.insert(n as u32, (level + 1) as u8).is_none() {
                    next.push(n as u32);
                }
            }
        }
        frontier = next;
    }

    dist.into_iter().collect()
}

/// Liczba warstw pasa granicznego (strefy o różnej gęstości — wtapianie).
const EDGE_STRIPS: usize = 5;

/// Waga warstwy `k` z `K` (t = odległość znormalizowana 0..1 od granicy).
/// Blend: najgęściej przy samej granicy, zanik do zewnątrz/podrostu.
fn strip_weight(k: usize, k_total: usize, blend: bool) -> f64 {
    if !blend || k_total <= 1 {
        return 1.0;
    }
    let t = (k as f64 + 0.5) / k_total as f64;
    0.5 * (1.0 + (std::f64::consts::PI * t).cos())
}

// --- Wspólny dart throwing ----------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_dart(
    env: &Env<'_>,
    rng: &mut StdRng,
    grid: &mut HashMap<(i64, i64), Vec<[f32; 2]>>,
    cell: f64,
    objects: &mut Vec<PlacedObject>,
    stats: &mut GenStats,
    target: usize,
    start_min_dist: f64,
    cum: &[(usize, f64)],
    source_index: usize,
    candidate: &mut dyn FnMut(&mut StdRng) -> Option<(f64, f64)>,
    progress: Progress,
    prog_base: f64,
    prog_width: f64,
) -> usize {
    let mut accepted = 0usize;
    let mut min_dist = start_min_dist.max(0.05);

    for _pass in 0..5 {
        let cap = target.saturating_mul(15).max(30_000);
        let mut attempts = 0usize;
        while accepted < target && attempts < cap {
            attempts += 1;
            if attempts % 8192 == 0 {
                progress(prog_base + prog_width * (accepted as f64 / target.max(1) as f64));
            }

            let Some((wx, wy)) = candidate(rng) else {
                continue;
            };

            let elevation = match env.eval(wx, wy) {
                Ok(e) => e,
                Err(Rej::Filters) => {
                    stats.rejected_filters += 1;
                    continue;
                }
                Err(Rej::Exclusion) => {
                    stats.rejected_exclusion += 1;
                    continue;
                }
                Err(Rej::Clearing) => {
                    stats.rejected_clearing += 1;
                    continue;
                }
            };

            let p = [wx as f32, wy as f32];
            if grid_has_neighbor(grid, cell, p, min_dist) {
                stats.rejected_spacing += 1;
                continue;
            }

            let si = pick_species(cum, rng);
            let sp = &env.project.species[si];

            grid.entry(world_to_cell(p, cell)).or_default().push(p);
            objects.push(PlacedObject {
                model: sp.model.clone(),
                x: wx,
                y: wy,
                yaw: rng.gen_range(0.0..360.0),
                pitch: rng.gen_range(-f64::from(sp.tilt_max_deg)..f64::from(sp.tilt_max_deg)),
                roll: rng.gen_range(-f64::from(sp.tilt_max_deg)..f64::from(sp.tilt_max_deg)),
                scale: rng.gen_range(f64::from(sp.scale_min)..=f64::from(sp.scale_max)),
                elevation,
                zone_index: source_index,
                species_index: si,
            });
            accepted += 1;
            stats.total += 1;
            stats.per_species[si].1 += 1;
        }
        if accepted >= target {
            break;
        }
        min_dist *= 0.85;
    }
    accepted
}

struct Job<'a> {
    label: String,
    target: usize,
    min_dist: f64,
    cum: Vec<(usize, f64)>,
    source_index: usize,
    is_edge: bool,
    cand: Box<dyn FnMut(&mut StdRng) -> Option<(f64, f64)> + 'a>,
}

/// Plan minimalnego odstępu dla celu gęstości (nasycenie RSA ~0.7*A/d^2).
fn plan_min_dist(spacing: f64, area_m2: f64, target: usize) -> f64 {
    spacing.max(0.05) * (0.7 * area_m2 / target.max(1) as f64).sqrt()
}

// --- Główne wejście -----------------------------------------------------------

pub fn generate(
    project: &ForestProject,
    mask: Option<&MaskImage>,
    heightmap: Option<&AscHeightmap>,
    exclusions: Option<&GeoJsonData>,
    progress: Progress,
) -> Result<(Vec<PlacedObject>, GenStats), String> {
    let t0 = std::time::Instant::now();
    project.validate()?;
    if !project.zones.is_empty() && mask.is_none() {
        return Err("Projekt zawiera strefy z maski, ale maska nie jest wczytana".into());
    }
    if let Some(m) = mask {
        if m.width < 2 || m.height < 2 {
            return Err("Maska jest zbyt mała".into());
        }
    }
    if project.zones.len() > MAX_ZONES {
        return Err(format!("Maks. liczba stref z maski: {MAX_ZONES}"));
    }

    let size = project.map_size_m;

    // --- Klasyfikacja pikseli maski ------------------------------------------
    let mut zone_pixels: Vec<Vec<u32>> = vec![Vec::new(); project.zones.len()];
    let mut zone_id: Vec<u8> = Vec::new();

    if let Some(mask) = mask {
        let palette: Vec<Rgb8> = project.zones.iter().map(|z| z.color).collect();
        let tol = project.color_tolerance;
        zone_id = vec![255u8; (mask.width as usize) * (mask.height as usize)];
        for y in 0..mask.height {
            for x in 0..mask.width {
                if mask.alpha(x, y) < 128 {
                    continue;
                }
                let c = mask.pixel(x, y);
                if project.exclusion_colors.iter().any(|e| e.dist(&c) <= tol) {
                    continue;
                }
                let mut best: Option<(usize, u32)> = None;
                for (i, p) in palette.iter().enumerate() {
                    let d = c.dist(p);
                    if d <= tol && best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((i, d));
                    }
                }
                if let Some((zi, _)) = best {
                    let idx = y * mask.width + x;
                    zone_pixels[zi].push(idx);
                    zone_id[idx as usize] = zi as u8;
                }
            }
        }
    }
    let ps = mask.map_or(1.0, |m| m.pixel_size_m(size));
    let pixel_area = ps * ps;

    let noise_seed = project.seed ^ 0x5DEECE66D;
    let use_clearings = project.clearing_strength > 0.0 && project.clearing_scale_m > 0.0;

    // poszarpanie granic (działa też dla wnętrza poligonów, niezależnie od pasa)
    let jag = project.edges.jagged_m.max(0.0);
    let wl = (project.edges.band_width_m * 3.0).max(80.0);
    let jag_seed = noise_seed ^ 0xA5A5_A5A5;
    let wob = |x: f64, y: f64| -> f64 {
        if jag <= 0.0 {
            0.0
        } else {
            jag * (2.0 * fbm2(x / wl, y / wl, jag_seed) - 1.0)
        }
    };

    let env = Env {
        project,
        hm: heightmap,
        ex: exclusions,
        noise_seed,
        use_clearings,
    };

    let nz = project.zones.len();
    let na = project.areas.len();
    // kolejność deklaracji ma znaczenie: dane pożyczane przez domknięcia w jobs
    // muszą żyć dłużej (drop jest w odwrotnej kolejności)
    let mut edge_bands: Vec<(usize, Vec<(u32, u8)>)> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut max_min_dist = 1.0f64;
    let mut total_target = 0usize;

    // --- Źródło: strefy z maski -------------------------------------------------
    if project.use_mask_zones {
        for (zi, zp) in zone_pixels.iter().enumerate() {
            let zone = &project.zones[zi];
            let area_m2 = zp.len() as f64 * pixel_area;
            let target =
                (area_m2 / 10_000.0 * f64::from(zone.density_per_ha)).round() as usize;
            if target == 0 || zp.is_empty() {
                continue;
            }
            let d = plan_min_dist(project.spacing_multiplier, area_m2, target);
            max_min_dist = max_min_dist.max(d);
            total_target += target;
            let cand = make_px_cand(zp, mask.unwrap().width, size, ps);
            jobs.push(Job {
                label: zone.label.clone(),
                target,
                min_dist: d,
                cum: cumulative(&zone.species_weights),
                source_index: zi,
                is_edge: false,
                cand: Box::new(cand),
            });
        }
    }
    // --- Źródło: obszary rysowane (poligony) --------------------------------------
    if project.use_areas {
        for (ai, area) in project.areas.iter().enumerate() {
            let ring = &area.polygon;
            if ring.len() < 3 {
                continue;
            }
            let area_m2 = polygon_area(ring);
            let target =
                (area_m2 / 10_000.0 * f64::from(area.density_per_ha)).round() as usize;
            if target == 0 || area_m2 <= 0.0 {
                continue;
            }
            let d = plan_min_dist(project.spacing_multiplier, area_m2, target);
            max_min_dist = max_min_dist.max(d);
            total_target += target;
            let poly = Polygon {
                rings: vec![ring.clone()],
            };
            let ring_c2 = ring.clone();
            // poszarpany brzeg: podpisana odległość od granicy + szum
            let (bmin_x, bmin_y, bmax_x, bmax_y) = bbox_of(ring);
            let (min_x, min_y, max_x, max_y) = (
                (bmin_x - jag).max(0.0),
                (bmin_y - jag).max(0.0),
                (bmax_x + jag).min(size),
                (bmax_y + jag).min(size),
            );
            jobs.push(Job {
                label: area.label.clone(),
                target,
                min_dist: d,
                cum: cumulative(&area.species_weights),
                source_index: nz + ai,
                is_edge: false,
                cand: Box::new(
                    move |rng: &mut StdRng| {
                        for _ in 0..24 {
                            let x = rng.gen_range(min_x..max_x);
                            let y = rng.gen_range(min_y..max_y);
                            let inside = point_in_polygon(&poly, x, y);
                            let dist = point_ring_distance(x, y, &ring_c2);
                            let signed = if inside { dist } else { -dist };
                            if signed > wob(x, y) {
                                return Some((x, y));
                            }
                        }
                        None
                    },
                ),
            });
        }
    }

    // --- Źródło: pas graniczny ------------------------------------------------------
    if project.edges.enabled {
        let band = project.edges.band_width_m;
        let band_px = ((band / ps).ceil() as usize).clamp(1, 128);
        let ecum = cumulative(&project.edges.species_weights);
        let blend = project.edges.blend;

        // najpierw policz pasy na masce (piksel + głębokość od krawędzi)
        if let Some(m) = mask {
            if project.use_mask_zones {
                for (zi, zp) in zone_pixels.iter().enumerate() {
                    if zp.is_empty() {
                        continue;
                    }
                    let band_set =
                        mask_edge_band(m.width, m.height, &zone_id, zi as u8, zp, band_px);
                    if !band_set.is_empty() {
                        edge_bands.push((zi, band_set));
                    }
                }
            }
        }

        // pasy na masce -> K warstw gęstości (wtapianie)
        for (zi, band_set) in &edge_bands {
            let total_area = band_set.len() as f64 * pixel_area;
            let base_target =
                (total_area / 10_000.0 * f64::from(project.edges.density_per_ha)).round()
                    as usize;
            if base_target == 0 {
                continue;
            }
            let wsum: f64 =
                (0..EDGE_STRIPS).map(|k| strip_weight(k, EDGE_STRIPS, blend)).sum();
            let m = mask.unwrap();
            let max_s = (band_px as f64 + 1.0) * ps;

            for k in 0..EDGE_STRIPS {
                let wk = strip_weight(k, EDGE_STRIPS, blend);
                let frac = wk / wsum;
                let strip_target = if k == EDGE_STRIPS - 1 {
                    base_target.saturating_sub(
                        (base_target as f64 * (wsum - wk) / wsum).round() as usize,
                    )
                } else {
                    (base_target as f64 * frac).round() as usize
                };
                if strip_target == 0 {
                    continue;
                }
                let strip_area = total_area * frac;
                let d = plan_min_dist(project.spacing_multiplier, strip_area.max(1.0), strip_target);
                max_min_dist = max_min_dist.max(d);
                total_target += strip_target;

                let s_lo = max_s * (k as f64) / EDGE_STRIPS as f64;
                let s_hi = max_s * ((k + 1) as f64) / EDGE_STRIPS as f64;
                let set: &[(u32, u8)] = band_set;
                let width = m.width;
                jobs.push(Job {
                    label: format!(
                        "Granica – {} ({}/{})",
                        project.zones[*zi].label,
                        k + 1,
                        EDGE_STRIPS
                    ),
                    target: strip_target,
                    min_dist: d,
                    cum: ecum.clone(),
                    source_index: nz + na,
                    is_edge: true,
                    cand: Box::new(move |rng: &mut StdRng| {
                        if set.is_empty() {
                            return None;
                        }
                        let (idx, lvl) = set[rng.gen_range(0..set.len())];
                        let px = (idx % width) as f64;
                        let row = (idx / width) as f64;
                        let wx = (px + 0.5 + rng.gen_range(-0.5..0.5)) * ps;
                        let wy = size - (row + 0.5 + rng.gen_range(-0.5..0.5)) * ps;
                        let s = (f64::from(lvl) + rng.gen_range(0.0..1.0)) * ps + wob(wx, wy);
                        if s >= s_lo && s < s_hi {
                            Some((wx, wy))
                        } else {
                            None
                        }
                    }),
                });
            }
        }

        // pas wokół granic poligonów (obie strony pierścienia), też w warstwach
        if project.use_areas {
            for (_ai, area) in project.areas.iter().enumerate() {
                let ring = &area.polygon;
                if ring.len() < 3 {
                    continue;
                }
                let perim = polygon_perimeter(ring);
                let inner = polygon_area(ring);
                let area_m2 = (perim * 2.0 * band).min(inner * 1.5);
                if area_m2 <= 0.0 {
                    continue;
                }
                let base_target =
                    (area_m2 / 10_000.0 * f64::from(project.edges.density_per_ha)).round()
                        as usize;
                if base_target == 0 {
                    continue;
                }
                let wsum: f64 = (0..EDGE_STRIPS)
                    .map(|k| strip_weight(k, EDGE_STRIPS, blend))
                    .sum();
                let (min_x, min_y, max_x, max_y) = bbox_of(ring);
                let margin = band + jag;
                let (min_x, min_y, max_x, max_y) = (
                    (min_x - margin).max(0.0),
                    (min_y - margin).max(0.0),
                    (max_x + margin).min(size),
                    (max_y + margin).min(size),
                );

                for k in 0..EDGE_STRIPS {
                    let wk = strip_weight(k, EDGE_STRIPS, blend);
                    let frac = wk / wsum;
                    let strip_target = if k == EDGE_STRIPS - 1 {
                        base_target.saturating_sub(
                            (base_target as f64 * (wsum - wk) / wsum).round() as usize,
                        )
                    } else {
                        (base_target as f64 * frac).round() as usize
                    };
                    if strip_target == 0 {
                        continue;
                    }
                    let strip_area = area_m2 * frac;
                    let d =
                        plan_min_dist(project.spacing_multiplier, strip_area.max(1.0), strip_target);
                    max_min_dist = max_min_dist.max(d);
                    total_target += strip_target;

                    let s_lo = band * (k as f64) / EDGE_STRIPS as f64;
                    let s_hi = band * ((k + 1) as f64) / EDGE_STRIPS as f64;
                    let ring_c = ring.clone();
                    jobs.push(Job {
                        label: format!(
                            "Granica – {} ({}/{})",
                            area.label,
                            k + 1,
                            EDGE_STRIPS
                        ),
                        target: strip_target,
                        min_dist: d,
                        cum: ecum.clone(),
                        source_index: nz + na,
                        is_edge: true,
                        cand: Box::new(move |rng: &mut StdRng| {
                            for _ in 0..24 {
                                let x = rng.gen_range(min_x..max_x);
                                let y = rng.gen_range(min_y..max_y);
                                let s = point_ring_distance(x, y, &ring_c) + wob(x, y);
                                if s >= s_lo && s < s_hi {
                                    return Some((x, y));
                                }
                            }
                            None
                        }),
                    });
                }
            }
        }
    }

    if total_target == 0 {
        return Err(
            "Brak obszarów do zasiedlenia (maska nie zawiera pikseli stref / brak poligonów)"
                .into(),
        );
    }
    if total_target > MAX_TOTAL_OBJECTS {
        return Err(format!(
            "Docelowa liczba obiektów ({total_target}) przekracza limit {MAX_TOTAL_OBJECTS} — zmniejsz gęstość"
        ));
    }

    // --- Wykonanie ------------------------------------------------------------
    let cell = max_min_dist.max(1.0);
    let mut grid: HashMap<(i64, i64), Vec<[f32; 2]>> = HashMap::new();
    let mut objects: Vec<PlacedObject> = Vec::with_capacity(total_target.min(1 << 20));
    let mut stats = GenStats {
        per_species: project.species.iter().map(|s| (s.label.clone(), 0)).collect(),
        ..Default::default()
    };

    let n_jobs = jobs.len();
    let span = 1.0 / n_jobs.max(1) as f64;
    let rng = &mut StdRng::seed_from_u64(project.seed);

    for (ji, job) in jobs.iter_mut().enumerate() {
        let prev_total = stats.total;
        let accepted = run_dart(
            &env,
            rng,
            &mut grid,
            cell,
            &mut objects,
            &mut stats,
            job.target,
            job.min_dist,
            &job.cum,
            job.source_index,
            &mut job.cand,
            progress,
            ji as f64 * span,
            span,
        );
        if job.is_edge {
            stats.edge_count += stats.total - prev_total;
        }
        stats.per_source.push((job.label.clone(), accepted));
    }

    progress(1.0);
    stats.elapsed_ms = t0.elapsed().as_millis();
    Ok((objects, stats))
}

// --- Siatka przestrzenna -------------------------------------------------------

#[inline]
fn world_to_cell(p: [f32; 2], cell: f64) -> (i64, i64) {
    (
        (p[0] as f64 / cell).floor() as i64,
        (p[1] as f64 / cell).floor() as i64,
    )
}

fn grid_has_neighbor(
    grid: &HashMap<(i64, i64), Vec<[f32; 2]>>,
    cell: f64,
    p: [f32; 2],
    radius: f64,
) -> bool {
    if radius <= 0.0 || grid.is_empty() {
        return false;
    }
    let r2 = (radius * radius) as f32;
    let span = (radius / cell).ceil() as i64;
    let (cx, cy) = world_to_cell(p, cell);
    for dy in -span..=span {
        for dx in -span..=span {
            if let Some(bucket) = grid.get(&(cx + dx, cy + dy)) {
                for q in bucket {
                    let ddx = q[0] - p[0];
                    let ddy = q[1] - p[1];
                    if ddx * ddx + ddy * ddy < r2 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// --- Szum wartościowy (polany) ---------------------------------------------

#[inline]
fn hash2(ix: f64, iy: f64, seed: u64) -> f64 {
    let mut h = ix
        .to_bits()
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ iy.to_bits().wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

#[inline]
fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(x: f64, y: f64, seed: u64) -> f64 {
    let ix = x.floor();
    let iy = y.floor();
    let tx = smoothstep(x - ix);
    let ty = smoothstep(y - iy);
    let v00 = hash2(ix, iy, seed);
    let v10 = hash2(ix + 1.0, iy, seed);
    let v01 = hash2(ix, iy + 1.0, seed);
    let v11 = hash2(ix + 1.0, iy + 1.0, seed);
    let a = v00 + (v10 - v00) * tx;
    let b = v01 + (v11 - v01) * tx;
    a + (b - a) * ty
}

/// 3-oktawowy fbm, wynik ~0..1.
fn fbm2(x: f64, y: f64, seed: u64) -> f64 {
    (value_noise(x, y, seed) * 0.5
        + value_noise(x * 2.03, y * 2.03, seed ^ 0x1234) * 0.25
        + value_noise(x * 4.07, y * 4.07, seed ^ 0xABCD) * 0.125)
        / 0.875
}

/// Próg fbm tak, aby odrzucona część obszaru ≈ `strength`.
/// LUT odpowiada przybliżonej dystrybuancie fbm (mu~0.5, sigma~0.12).
fn clearing_threshold(strength: f64) -> f64 {
    const LUT: [(f64, f64); 11] = [
        (0.00, 0.00),
        (0.10, 0.346),
        (0.20, 0.399),
        (0.30, 0.437),
        (0.40, 0.470),
        (0.50, 0.500),
        (0.60, 0.530),
        (0.70, 0.563),
        (0.80, 0.601),
        (0.90, 0.654),
        (0.95, 0.697),
    ];
    let s = strength.clamp(0.0, 0.95);
    for w in LUT.windows(2) {
        if s <= w[1].0 {
            let t = (s - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + t * (w[1].1 - w[0].1);
        }
    }
    0.697
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{AreaDef, EdgeSettings, ZoneDef};
    use crate::species::SpeciesDef;

    fn two_species() -> Vec<SpeciesDef> {
        vec![
            SpeciesDef::new("A", "a_1f", 0.9, 1.1),
            SpeciesDef::new("B", "b_1f", 0.9, 1.1),
        ]
    }

    fn zone_project(colors: &[Rgb8]) -> ForestProject {
        let mut p = ForestProject {
            map_size_m: 1000.0,
            clearing_scale_m: 0.0,
            edge_padding_m: 0.0,
            ..Default::default()
        };
        p.species = two_species();
        p.zones = colors
            .iter()
            .enumerate()
            .map(|(i, c)| ZoneDef {
                color: *c,
                label: format!("Z{i}"),
                density_per_ha: 300.0,
                species_weights: vec![(0, 3.0), (1, 1.0)],
            })
            .collect();
        p
    }

    fn mask_with_rect(w: u32, h: u32, rect: (u32, u32, u32, u32), c: Rgb8) -> MaskImage {
        let mut img = image::RgbaImage::new(w, h);
        let (x0, y0, rw, rh) = rect;
        if rw > 0 && rh > 0 {
            for y in y0..y0 + rh {
                for x in x0..x0 + rw {
                    img.put_pixel(x, y, image::Rgba([c.r(), c.g(), c.b(), 255]));
                }
            }
        }
        MaskImage::from_dynamic(image::DynamicImage::ImageRgba8(img))
    }

    #[test]
    fn generates_respecting_min_distance_and_proportions() {
        let green = Rgb8([0, 200, 0]);
        let proj = zone_project(&[green]);
        let mask = mask_with_rect(100, 100, (10, 10, 80, 80), green);
        // mapa 1000m, prostokat 800x800m = 64 ha * 300/ha = 19200 celów
        let (objs, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        assert!(stats.total >= 18_000, "total={}", stats.total);
        assert_eq!(objs.len(), stats.total);

        let mut min_d = f64::INFINITY;
        for (i, a) in objs.iter().enumerate() {
            for b in objs.iter().skip(i + 1).take(400) {
                min_d = min_d.min((a.x - b.x).hypot(a.y - b.y));
            }
        }
        let area: f64 = 640_000.0;
        let target: f64 = 64.0 * 300.0;
        let d_plan: f64 = proj.spacing_multiplier * (0.7 * area / target).sqrt();
        // dopuszczamy <=2 przebiegi zagęszczania (0.85^2)
        assert!(min_d >= d_plan * 0.70, "min_d={min_d}, d_plan={d_plan}");

        // proporcje 3:1 z tolerancją ±25%
        let na = stats.per_species[0].1 as f64;
        let nb = stats.per_species[1].1 as f64;
        let ratio = na / nb;
        assert!(ratio > 2.25 && ratio < 3.75, "ratio={ratio} (na={na}, nb={nb})");
    }

    #[test]
    fn deterministic_with_same_seed() {
        let red = Rgb8([255, 0, 0]);
        let proj = zone_project(&[red]);
        let mask = mask_with_rect(60, 60, (5, 5, 50, 50), red);
        let (o1, _) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        let (o2, _) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        assert_eq!(o1.len(), o2.len());
        for (a, b) in o1.iter().zip(&o2) {
            assert_eq!(a.model, b.model);
            assert!((a.x - b.x).abs() < 1e-9);
            assert!((a.y - b.y).abs() < 1e-9);
        }
    }

    #[test]
    fn exclusion_colors_and_geojson_block_placement() {
        let green = Rgb8([0, 200, 0]);
        let black = Rgb8([0, 0, 0]);
        let mut proj = zone_project(&[green]);
        proj.exclusion_colors = vec![black];

        let mut img = image::RgbaImage::new(100, 100);
        for y in 0..100 {
            for x in 0..100 {
                img.put_pixel(x, y, image::Rgba([0, 200, 0, 255]));
            }
        }
        for x in 40..60 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let mask = MaskImage::from_dynamic(image::DynamicImage::ImageRgba8(img));
        let (_, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();

        let (objs, _) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        for o in &objs {
            assert!(o.x < 400.0 || o.x >= 600.0, "obiekt w wykluczeniu x={}", o.x);
        }
        assert!(stats.total > 0);
    }

    #[test]
    fn geojson_exclusion_blocks_area() {
        let green = Rgb8([0, 200, 0]);
        let proj = zone_project(&[green]);
        let mask = mask_with_rect(100, 100, (0, 0, 100, 100), green);
        let gj = GeoJsonData::parse(
            r#"{"type":"Polygon","coordinates":[[[400,400],[600,400],[600,600],[400,600],[400,400]]]}"#,
        )
        .unwrap();
        let (objs, _) = generate(&proj, Some(&mask), None, Some(&gj), &|_| {}).unwrap();
        for o in &objs {
            assert!(o.x < 400.0 || o.x > 600.0 || o.y < 400.0 || o.y > 600.0);
        }
        assert!(!objs.is_empty());
    }

    #[test]
    fn altitude_filter_works() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.min_altitude = Some(55.0);
        let mask = mask_with_rect(30, 30, (0, 0, 30, 30), green);

        let asc_text = "ncols 3\nnrows 3\nxllcorner 0\nyllcorner 0\ncellsize 333.34\nNODATA_value -9999\n\
             100 110 120\n50 60 70\n0 10 20\n";
        let hm = AscHeightmap::parse(asc_text).unwrap();
        let (objs, stats) =
            generate(&proj, Some(&mask), Some(&hm), None, &|_| {}).unwrap();
        assert!(stats.rejected_filters > 0);
        for o in &objs {
            let h = hm.sample(o.x, o.y).unwrap();
            assert!(h >= 55.0);
        }
        assert!(!objs.is_empty());
    }

    #[test]
    fn absolute_elevation_written_when_requested() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.elevation_mode = ElevationMode::AbsoluteSampled;
        let mask = mask_with_rect(30, 30, (0, 0, 30, 30), green);
        let asc_text = "ncols 3\nnrows 3\nxllcorner 0\nyllcorner 0\ncellsize 333.34\nNODATA_value -9999\n\
             100 110 120\n50 60 70\n0 10 20\n";
        let hm = AscHeightmap::parse(asc_text).unwrap();
        let (objs, _) = generate(&proj, Some(&mask), Some(&hm), None, &|_| {}).unwrap();
        assert!(objs.iter().any(|o| o.elevation > 0.0));
    }

    #[test]
    fn clearings_shape_distribution() {
        let green = Rgb8([0, 200, 0]);
        let mut p = zone_project(&[green]);
        p.clearing_scale_m = 120.0;
        p.clearing_strength = 0.5;

        let mask = mask_with_rect(120, 120, (0, 0, 120, 120), green);
        let (objs, stats) = generate(&p, Some(&mask), None, None, &|_| {}).unwrap();
        assert!(stats.rejected_clearing > 0);
        assert!(stats.total >= 29_000, "total={}", stats.total);

        let noise_seed = p.seed ^ 0x5DEECE66D;
        let mean_at_points: f64 = objs
            .iter()
            .map(|o| fbm2(o.x / 120.0, o.y / 120.0, noise_seed))
            .sum::<f64>()
            / objs.len() as f64;
        let mut ref_sum = 0.0;
        let mut n = 0usize;
        for y in 0..120u32 {
            for x in 0..120u32 {
                ref_sum += fbm2(f64::from(x) / 12.0, f64::from(y) / 12.0, noise_seed);
                n += 1;
            }
        }
        let mean_ref = ref_sum / n as f64;
        assert!(
            mean_at_points > mean_ref + 0.02,
            "punkty={mean_at_points} referencja={mean_ref}"
        );
    }

    #[test]
    fn empty_mask_and_no_areas_is_error() {
        let green = Rgb8([0, 200, 0]);
        let proj = zone_project(&[green]);
        let mask = mask_with_rect(20, 20, (0, 0, 0, 0), green);
        assert!(generate(&proj, Some(&mask), None, None, &|_| {}).is_err());
    }

    #[test]
    fn zones_require_mask_when_present_in_project() {
        let green = Rgb8([0, 200, 0]);
        let proj = zone_project(&[green]);
        assert!(generate(&proj, None, None, None, &|_| {}).is_err());
    }

    // --- obszary (poligony) ------------------------------------------------

    fn square_ring(a: f64, b: f64, c: f64, d: f64) -> Vec<[f64; 2]> {
        vec![[a, b], [c, b], [c, d], [a, d]]
    }

    #[test]
    fn polygon_only_project_generates_inside_polygon() {
        let mut proj = zone_project(&[]);
        proj.zones.clear();
        proj.edges.jagged_m = 0.0; // ścisłe zawieranie dla tego testu
        proj.areas.push(AreaDef {
            label: "Dwór".into(),
            density_per_ha: 200.0,
            species_weights: vec![(0, 1.0)],
            polygon: square_ring(300.0, 300.0, 700.0, 700.0),
        });
        // 16 ha * 200/ha = 3200 celów
        let (objs, stats) = generate(&proj, None, None, None, &|_| {}).unwrap();
        assert!(stats.total >= 2_000, "total={}", stats.total);
        let poly = Polygon {
            rings: vec![square_ring(300.0, 300.0, 700.0, 700.0)],
        };
        for o in &objs {
            assert!(point_in_polygon(&poly, o.x, o.y), "poza poligonem");
        }
        assert_eq!(stats.per_source.len(), 1);
        assert_eq!(stats.per_source[0].0, "Dwór");
    }

    #[test]
    fn areas_coexist_with_mask_zones_sharing_spacing_grid() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.areas.push(AreaDef {
            label: "Sad".into(),
            density_per_ha: 150.0,
            species_weights: vec![(1, 1.0)], // model b_1f tylko z obszaru
            polygon: square_ring(350.0, 350.0, 650.0, 650.0),
        });
        let mask = mask_with_rect(100, 100, (10, 10, 80, 80), green);
        let (objs, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();

        let in_area_b = objs
            .iter()
            .filter(|o| o.model == "b_1f" && o.x > 350.0 && o.x < 650.0 && o.y > 350.0 && o.y < 650.0)
            .count();
        assert!(in_area_b > 500, "in_area_b={in_area_b}");
        assert_eq!(stats.per_source.len(), 2);
        // wspólny odstęp: brak dwóch obiektów bliżej niż ~d_plan*0.7 w próbce
        let mut min_d = f64::INFINITY;
        for (i, a) in objs.iter().enumerate().take(3000) {
            for b in objs.iter().skip(i + 1).take(200) {
                min_d = min_d.min((a.x - b.x).hypot(a.y - b.y));
            }
        }
        assert!(min_d > 1.0, "min_d={min_d}");
    }

    // --- pas graniczny -------------------------------------------------------

    #[test]
    fn forest_edge_band_placed_near_boundary_of_mask_zone() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.zones[0].species_weights = vec![(0, 1.0)]; // a_1f tylko wnętrze
        proj.edges = EdgeSettings {
            enabled: true,
            band_width_m: 40.0,
            density_per_ha: 900.0,
            species_weights: vec![(1, 1.0)], // b_1f tylko granica
            blend: false,
            jagged_m: 0.0, // deterministyczny pas do testów odległości
        };

        let mask = mask_with_rect(100, 100, (10, 10, 80, 80), green);
        let (objs, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        assert!(stats.edge_count > 0, "edge_count={}", stats.edge_count);

        let edge_objs: Vec<&PlacedObject> =
            objs.iter().filter(|o| o.model == "b_1f").collect();
        assert!(!edge_objs.is_empty());

        // granica prostokąta [100..900]^2; pas 40 m + luz na jitter/piksel
        for o in &edge_objs {
            let dx = if o.x < 100.0 {
                100.0 - o.x
            } else if o.x > 900.0 {
                o.x - 900.0
            } else {
                0.0
            };
            let dy = if o.y < 100.0 {
                100.0 - o.y
            } else if o.y > 900.0 {
                o.y - 900.0
            } else {
                0.0
            };
            let dist_to_boundary = dx.hypot(dy);
            assert!(
                dist_to_boundary <= 55.0,
                "obiekt granicy {dist_to_boundary}m od krawędzi"
            );
            // pas jest po wewnętrznej stronie strefy
            assert!(o.x > 90.0 && o.x < 910.0 && o.y > 90.0 && o.y < 910.0);
        }

        // środek lasu bez krzaków granicznych (model b_1f nie występuje głęboko)
        let deep_center = edge_objs
            .iter()
            .filter(|o| o.x > 250.0 && o.x < 750.0 && o.y > 250.0 && o.y < 750.0)
            .count();
        assert_eq!(deep_center, 0, "krzewy graniczne w środku lasu");
    }

    #[test]
    fn forest_edge_along_polygon_boundary() {
        let mut proj = zone_project(&[]);
        proj.zones.clear();
        proj.areas.push(AreaDef {
            label: "Gaj".into(),
            density_per_ha: 150.0,
            species_weights: vec![(0, 1.0)],
            polygon: square_ring(300.0, 300.0, 700.0, 700.0),
        });
        proj.edges = EdgeSettings {
            enabled: true,
            band_width_m: 25.0,
            density_per_ha: 900.0,
            species_weights: vec![(1, 1.0)],
            blend: false,
            jagged_m: 0.0,
        };
        let (objs, stats) = generate(&proj, None, None, None, &|_| {}).unwrap();
        assert!(stats.edge_count > 0);

        for o in objs.iter().filter(|o| o.model == "b_1f") {
            let d = point_ring_distance(o.x, o.y, &proj.areas[0].polygon);
            assert!(d <= 35.0, "odległość od granicy {d}m");
        }
    }

    #[test]
    fn polygon_self_intersects_detects_crossing() {
        // "motylek": obrys przecina sam siebie
        let bowtie = vec![[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]];
        assert!(polygon_self_intersects(&bowtie));
        let square = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        assert!(!polygon_self_intersects(&square));
        // wypukły pięciokąt
        let pent = vec![
            [50.0, 0.0],
            [100.0, 40.0],
            [80.0, 100.0],
            [20.0, 100.0],
            [0.0, 40.0],
        ];
        assert!(!polygon_self_intersects(&pent));
    }

    #[test]
    fn validate_rejects_self_intersecting_area() {
        let mut proj = zone_project(&[]);
        proj.zones.clear();
        proj.areas.push(AreaDef {
            label: "Motylek".into(),
            density_per_ha: 100.0,
            species_weights: vec![(0, 1.0)],
            polygon: vec![[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]],
        });
        let err = proj.validate().unwrap_err();
        assert!(err.contains("przecina sam siebie"), "{err}");
    }

    #[test]
    fn disabling_mask_zones_skips_them() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.use_mask_zones = false;
        proj.edges.jagged_m = 0.0;
        proj.areas.push(AreaDef {
            label: "Tylko poligon".into(),
            density_per_ha: 150.0,
            species_weights: vec![(1, 1.0)],
            polygon: square_ring(300.0, 300.0, 700.0, 700.0),
        });
        let mask = mask_with_rect(60, 60, (5, 5, 50, 50), green);
        let (objs, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        // wyłącznie gatunek z obszaru (b_1f); nic z maski (a_1f)
        assert!(!objs.iter().any(|o| o.model == "a_1f"));
        for o in &objs {
            assert!(o.x > 300.0 && o.x < 700.0 && o.y > 300.0 && o.y < 700.0);
        }
        assert_eq!(stats.per_source.len(), 1);
    }

    #[test]
    fn all_sources_disabled_is_error() {
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.use_mask_zones = false;
        proj.use_areas = false;
        assert!(proj.validate().is_err());
        let mask = mask_with_rect(30, 30, (5, 5, 20, 20), green);
        assert!(generate(&proj, Some(&mask), None, None, &|_| {}).is_err());
    }

    #[test]
    fn jagged_boundary_spills_outside_polygon() {
        // poszarpanie ma "rozlać" część lasu poza ścisły obrys
        let mut proj = zone_project(&[]);
        proj.zones.clear();
        proj.edges.jagged_m = 60.0;
        proj.areas.push(AreaDef {
            label: "Ragged".into(),
            density_per_ha: 200.0,
            species_weights: vec![(0, 1.0)],
            polygon: square_ring(300.0, 300.0, 700.0, 700.0),
        });
        let (objs, stats) = generate(&proj, None, None, None, &|_| {}).unwrap();
        assert!(stats.total > 1_000);

        let outside = objs
            .iter()
            .filter(|o| {
                !point_in_polygon(
                    &Polygon {
                        rings: vec![square_ring(300.0, 300.0, 700.0, 700.0)],
                    },
                    o.x,
                    o.y,
                )
            })
            .count();
        assert!(outside > 20, "poza obrysem: {outside}");
        // ale nie dalej niż poszarpanie + luz na jitter
        let strict = Polygon {
            rings: vec![square_ring(300.0, 300.0, 700.0, 700.0)],
        };
        for o in &objs {
            if !point_in_polygon(&strict, o.x, o.y) {
                let d = point_ring_distance(o.x, o.y, &proj.areas[0].polygon);
                assert!(d <= 90.0, "poza obrysem o {d}m (limit ~jag+jitter)");
            }
        }
    }

    #[test]
    fn edge_blend_concentrates_near_boundary() {
        // blend: warstwa przy granicy gęstsza niż najdalsza
        let green = Rgb8([0, 200, 0]);
        let mut proj = zone_project(&[green]);
        proj.zones[0].species_weights = vec![(0, 1.0)];
        proj.edges = EdgeSettings {
            enabled: true,
            band_width_m: 50.0,
            density_per_ha: 1200.0,
            species_weights: vec![(1, 1.0)],
            blend: true,
            jagged_m: 0.0,
        };
        let mask = mask_with_rect(100, 100, (10, 10, 80, 80), green);
        let (objs, _) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();

        // pas [100..900]^2; podział na 3 pierścienie głębokości
        let count_at = |lo: f64, hi: f64| -> usize {
            objs.iter()
                .filter(|o| o.model == "b_1f")
                .filter(|o| {
                    let d: f64 = (100.0f64)
                        .min(o.x - 100.0)
                        .min(900.0 - o.x)
                        .min(o.y - 100.0)
                        .min(900.0 - o.y);
                    d >= lo && d < hi
                })
                .count()
        };
        let near = count_at(0.0, 16.7);
        let far = count_at(33.4, 50.1);
        assert!(near > far * 2, "near={near} far={far}");
    }

    #[test]
    fn edge_disabled_by_default() {
        let green = Rgb8([0, 200, 0]);
        let proj = zone_project(&[green]);
        assert!(!proj.edges.enabled);
        let mask = mask_with_rect(60, 60, (10, 10, 40, 40), green);
        let (_, stats) = generate(&proj, Some(&mask), None, None, &|_| {}).unwrap();
        assert_eq!(stats.edge_count, 0);
    }
}
