//! forest-core — silnik generowania lasów dla DayZ (Terrain Builder).
//!
//! Pipeline: maska PNG + heightmapa ASC + wykluczenia GeoJSON
//! -> rozrzut Poisson-disk z proporcjami gatunków
//! -> plik TXT do importu w Terrain Builder.

pub mod export_tb;
pub mod geojson;
pub mod heightmap;
pub mod mask;
pub mod preset;
pub mod scatter;
pub mod species;
pub mod user_presets;

pub use export_tb::write_tb_txt;
pub use preset::ForestProject;
pub use scatter::{generate, GenStats, PlacedObject};
