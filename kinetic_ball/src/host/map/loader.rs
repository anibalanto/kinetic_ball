use crate::shared::map::Map;
use std::path::Path;

#[derive(Debug)]
pub enum MapLoadError {
    FileNotFound(String),
    ParseError(String),
    InvalidGeometry(String),
}

impl std::fmt::Display for MapLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapLoadError::FileNotFound(path) => write!(f, "Map file not found: {}", path),
            MapLoadError::ParseError(err) => write!(f, "Failed to parse map: {}", err),
            MapLoadError::InvalidGeometry(err) => write!(f, "Invalid map geometry: {}", err),
        }
    }
}

impl std::error::Error for MapLoadError {}

/// Cargar un mapa de HaxBall desde un archivo JSON5 o JSON
pub fn load_map<P: AsRef<Path>>(path: P) -> Result<Map, MapLoadError> {
    let path_str = path.as_ref().to_string_lossy().to_string();

    // Leer archivo
    let content =
        std::fs::read_to_string(&path).map_err(|_| MapLoadError::FileNotFound(path_str.clone()))?;

    load_map_from_str(&content, &path_str)
}

/// Cargar un mapa desde un string (para mapas embebidos)
pub fn load_map_from_str(content: &str, name: &str) -> Result<Map, MapLoadError> {
    // Intentar JSON5 primero (estándar de HaxBall)
    if let Ok(map) = json5::from_str::<Map>(content) {
        validate_map(&map)?;
        println!("✅ Loaded map from JSON5: {}", name);
        return Ok(map);
    }

    // Fallback a JSON regular
    if let Ok(map) = serde_json::from_str::<Map>(content) {
        validate_map(&map)?;
        println!("✅ Loaded map from JSON: {}", name);
        return Ok(map);
    }

    Err(MapLoadError::ParseError(format!(
        "Failed to parse {} as JSON5 or JSON",
        name
    )))
}

/// Validar la geometría del mapa
fn validate_map(map: &Map) -> Result<(), MapLoadError> {
    // Verificar que los índices de vértices de los segmentos sean válidos
    for (i, seg) in map.segments.iter().enumerate() {
        if seg.v0 >= map.vertexes.len() || seg.v1 >= map.vertexes.len() {
            return Err(MapLoadError::InvalidGeometry(format!(
                "Segment {} references invalid vertex (v0={}, v1={}, total vertices={})",
                i,
                seg.v0,
                seg.v1,
                map.vertexes.len()
            )));
        }
    }

    // Verificar que los discos tengan radios positivos
    for (i, disc) in map.discs.iter().enumerate() {
        if disc.radius <= 0.0 {
            return Err(MapLoadError::InvalidGeometry(format!(
                "Disc {} has invalid radius: {}",
                i, disc.radius
            )));
        }
    }

    Ok(())
}
