//! Minimalny parser GeoJSON (Polygon / MultiPolygon) + test punktu w poligonie.
//!
//! Współrzędne oczekiwane w metrach mapy (origin SW). Jeśli plik używa
//! współrzędnych z offsetem easting (>= 100000), offset jest automatycznie
//! usuwany podczas normalizacji.

#[derive(Clone, Debug, Default)]
pub struct Polygon {
    /// ring 0 = obrys, kolejne = dziury
    pub rings: Vec<Vec<[f64; 2]>>,
}

#[derive(Clone, Debug, Default)]
pub struct GeoJsonData {
    pub polygons: Vec<Polygon>,
}

impl GeoJsonData {
    pub fn parse(text: &str) -> Result<Self, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("GeoJSON: {e}"))?;
        let mut out = Self::default();
        match json.get("type").and_then(|t| t.as_str()) {
            Some("FeatureCollection") => {
                let feats = json
                    .get("features")
                    .and_then(|f| f.as_array())
                    .ok_or("GeoJSON: brak 'features'")?;
                for f in feats {
                    if let Some(geom) = f.get("geometry") {
                        push_geometry(geom, &mut out)?;
                    }
                }
            }
            Some("Feature") => {
                if let Some(geom) = json.get("geometry") {
                    push_geometry(geom, &mut out)?;
                }
            }
            Some("Polygon") | Some("MultiPolygon") => push_geometry(&json, &mut out)?,
            other => return Err(format!("GeoJSON: nieobsługiwany typ {other:?}")),
        }
        Ok(out)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Nie udało się wczytać GeoJSON: {e}"))?;
        Self::parse(&text)
    }

    /// Normalizuje współrzędne: jeśli max x > 100000, odejmuje `easting_offset`.
    pub fn normalize_easting(&mut self, easting_offset: f64) {
        let mut max_x = f64::NEG_INFINITY;
        for p in &self.polygons {
            for r in &p.rings {
                for c in r {
                    max_x = max_x.max(c[0]);
                }
            }
        }
        if max_x.is_finite() && max_x > 100_000.0 && easting_offset > 0.0 {
            for p in &mut self.polygons {
                for r in &mut p.rings {
                    for c in r {
                        c[0] -= easting_offset;
                    }
                }
            }
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.polygons.iter().any(|p| point_in_polygon(p, x, y))
    }

    pub fn total_vertices(&self) -> usize {
        self.polygons
            .iter()
            .map(|p| p.rings.iter().map(|r| r.len()).sum::<usize>())
            .sum()
    }
}

use std::path::Path;

fn push_geometry(geom: &serde_json::Value, out: &mut GeoJsonData) -> Result<(), String> {
    let ty = geom
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or("GeoJSON: geometria bez 'type'")?;
    let coords = geom
        .get("coordinates")
        .ok_or("GeoJSON: geometria bez 'coordinates'")?;
    match ty {
        "Polygon" => {
            let rings = parse_rings(coords)?;
            if rings.is_empty() {
                return Ok(());
            }
            out.polygons.push(Polygon { rings });
            Ok(())
        }
        "MultiPolygon" => {
            let polys = coords
                .as_array()
                .ok_or("GeoJSON: MultiPolygon bez tablicy")?;
            for poly in polys {
                let rings = parse_rings(poly)?;
                if !rings.is_empty() {
                    out.polygons.push(Polygon { rings });
                }
            }
            Ok(())
        }
        other => Err(format!("GeoJSON: pomijam geometrię '{other}'")),
    }
}

fn parse_rings(v: &serde_json::Value) -> Result<Vec<Vec<[f64; 2]>>, String> {
    let arr = v
        .as_array()
        .ok_or("GeoJSON: pierścień bez tablicy współrzędnych")?;
    let mut rings = Vec::with_capacity(arr.len());
    for ring in arr {
        let pts = ring
            .as_array()
            .ok_or("GeoJSON: pierścień bez punktów")?;
        let mut r = Vec::with_capacity(pts.len());
        for pt in pts {
            let c = pt
                .as_array()
                .ok_or("GeoJSON: punkt bez współrzędnych")?;
            if c.len() < 2 {
                return Err("GeoJSON: punkt wymaga >= 2 współrzędnych".into());
            }
            let x = c[0]
                .as_f64()
                .ok_or("GeoJSON: x nie jest liczbą")?;
            let y = c[1]
                .as_f64()
                .ok_or("GeoJSON: y nie jest liczbą")?;
            r.push([x, y]);
        }
        if r.len() >= 3 {
            rings.push(r);
        }
    }
    Ok(rings)
}

/// Ray casting na zewnętrznym pierścieniu minus dziury.
pub fn point_in_polygon(poly: &Polygon, x: f64, y: f64) -> bool {
    let Some(outer) = poly.rings.first() else {
        return false;
    };
    if !ring_contains(outer, x, y) {
        return false;
    }
    for hole in poly.rings.iter().skip(1) {
        if ring_contains(hole, x, y) {
            return false;
        }
    }
    true
}

fn ring_contains(ring: &[[f64; 2]], x: f64, y: f64) -> bool {
    let n = ring.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i][0];
        let yi = ring[i][1];
        let xj = ring[j][0];
        let yj = ring[j][1];
        let intersects = (yi > y) != (yj > y)
            && x < (xj - xi) * (y - yi) / (yj - yi) + xi;
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Odległość punktu od odcinka AB (kwadrat).
fn point_segment_distance2(px: f64, py: f64, a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let apx = px - a[0];
    let apy = py - a[1];
    let denom = abx * abx + aby * aby;
    let t = if denom <= f64::EPSILON {
        0.0
    } else {
        ((apx * abx + apy * aby) / denom).clamp(0.0, 1.0)
    };
    let dx = apx - t * abx;
    let dy = apy - t * aby;
    dx * dx + dy * dy
}

/// Minimalna odległość punktu od granicy pierścienia (zamkniętego).
pub fn point_ring_distance(x: f64, y: f64, ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    let mut j = n - 1;
    for i in 0..n {
        let d2 = point_segment_distance2(x, y, &ring[j], &ring[i]);
        best = best.min(d2);
        j = i;
    }
    best.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FC: &str = r#"{
      "type": "FeatureCollection",
      "features": [
        { "type": "Feature", "properties": {}, "geometry": {
            "type": "Polygon",
            "coordinates": [[[100,100],[900,100],[900,900],[100,900],[100,100]]] } },
        { "type": "Feature", "properties": {}, "geometry": {
            "type": "MultiPolygon",
            "coordinates": [[[[2000,2000],[3000,2000],[3000,3000],[2000,3000],[2000,2000]]]] } }
      ]
    }"#;

    #[test]
    fn parses_feature_collection() {
        let gj = GeoJsonData::parse(FC).unwrap();
        assert_eq!(gj.polygons.len(), 2);
        assert_eq!(gj.total_vertices(), 10);
    }

    #[test]
    fn point_in_polygon_basic() {
        let gj = GeoJsonData::parse(FC).unwrap();
        assert!(gj.contains(500.0, 500.0));
        assert!(!gj.contains(950.0, 500.0));
        assert!(gj.contains(2500.0, 2500.0));
        assert!(!gj.contains(1500.0, 1500.0));
    }

    #[test]
    fn holes_are_excluded() {
        let gj = GeoJsonData::parse(
            r#"{"type":"Polygon","coordinates":[
                 [[0,0],[100,0],[100,100],[0,100],[0,0]],
                 [[40,40],[60,40],[60,60],[40,60],[40,40]]
               ]}"#,
        )
        .unwrap();
        assert!(gj.contains(10.0, 10.0));
        assert!(!gj.contains(50.0, 50.0));
    }

    #[test]
    fn normalizes_tb_easting() {
        let mut gj = GeoJsonData::parse(
            r#"{"type":"Polygon","coordinates":[
                 [[200100,100],[200900,100],[200900,900],[200100,900],[200100,100]]
               ]}"#,
        )
        .unwrap();
        gj.normalize_easting(200000.0);
        assert!(gj.contains(500.0, 500.0));
        assert!(!gj.contains(1500.0, 500.0));
    }

    #[test]
    fn rejects_garbage() {
        assert!(GeoJsonData::parse("{ not json").is_err());
        assert!(GeoJsonData::parse(r#"{"type":"LineString","coordinates":[[0,0],[1,1]]}"#).is_err());
    }

    #[test]
    fn ring_distance() {
        let square = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        assert!((point_ring_distance(50.0, 50.0, &square) - 50.0).abs() < 1e-9);
        assert!(point_ring_distance(0.0, 50.0, &square).abs() < 1e-9); // na krawędzi
        assert!((point_ring_distance(5.0, 50.0, &square) - 5.0).abs() < 1e-9);
        // róg (100,100): odległość od (103,104) = 5
        assert!((point_ring_distance(103.0, 104.0, &square) - 5.0).abs() < 1e-9);
    }
}
