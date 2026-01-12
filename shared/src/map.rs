use serde::{Deserialize, Serialize};

/// Mapa completo de HaxBall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    pub name: String,
    #[serde(default)]
    pub bg: BgConfig,
    #[serde(default)]
    pub vertexes: Vec<Vertex>,
    #[serde(default)]
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub discs: Vec<Disc>,
    #[serde(default)]
    pub goals: Vec<Goal>,
}

/// Configuración del fondo (opcional, para futura renderización del cliente)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BgConfig {
    #[serde(rename = "type")]
    pub bg_type: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub color: Option<String>,
}

/// Vértice: punto en el espacio que marca posiciones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    #[serde(default = "default_bcoef")]
    #[serde(rename = "bCoef")]
    pub b_coef: f32, // Coeficiente de rebote
    #[serde(default)]
    #[serde(rename = "cMask")]
    pub c_mask: Option<Vec<String>>, // Máscara de colisión
    #[serde(default)]
    #[serde(rename = "cGroup")]
    pub c_group: Option<Vec<String>>, // Grupo de colisión
}

/// Segmento: línea (recta o curva) entre dos vértices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub v0: usize, // Índice del primer vértice
    pub v1: usize, // Índice del segundo vértice
    #[serde(default = "default_bcoef")]
    #[serde(rename = "bCoef")]
    pub b_coef: f32,
    #[serde(default)]
    pub curve: Option<f32>, // Radio de curvatura (0 = recto)
    #[serde(default)]
    #[serde(rename = "curveF")]
    pub curve_f: Option<f32>, // Campo alternativo para curvas
    #[serde(default)]
    pub bias: Option<f32>,
    #[serde(default)]
    pub vis: Option<bool>, // Visibilidad
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    #[serde(rename = "cMask")]
    pub c_mask: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "cGroup")]
    pub c_group: Option<Vec<String>>,
}

/// Disco: objeto circular estático
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disc {
    pub pos: [f32; 2], // Posición [x, y]
    pub radius: f32,
    #[serde(default = "default_bcoef")]
    #[serde(rename = "bCoef")]
    pub b_coef: f32,
    #[serde(default)]
    #[serde(rename = "cMask")]
    pub c_mask: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "cGroup")]
    pub c_group: Option<Vec<String>>,
    #[serde(default)]
    pub color: Option<String>,
}

/// Gol: línea de gol para detección de puntuación (futura implementación)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub team: String, // "red" o "blue"
}

/// Configuración para aproximación de curvas
#[derive(Debug, Clone)]
pub struct CurveConfig {
    pub segments_per_curve: usize,
}

impl Default for CurveConfig {
    fn default() -> Self {
        Self {
            segments_per_curve: 16, // 16 puntos por curva por defecto
        }
    }
}

fn default_bcoef() -> f32 {
    1.0 // Coeficiente de rebote por defecto
}
