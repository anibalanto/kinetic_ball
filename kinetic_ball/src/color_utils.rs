use bevy::prelude::*;

use crate::resources::PlayerColors;

// ============================================================================
// FUNCIONES HELPER DE COLORES
// ============================================================================

/// Convierte RGB a HSV
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max == 0.0 { 0.0 } else { delta / max };

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };

    (h, s, v)
}

/// Convierte HSV a RGB
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (v, v, v);
    }

    let h = h * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Calcula el color complementario rotando el Hue 180 grados en HSV
pub fn complementary_color(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let h_opposite = (h + 0.5).rem_euclid(1.0);
    hsv_to_rgb(h_opposite, s, v)
}

/// Genera un color único para un jugador, evitando la zona verde del fondo del minimapa.
/// Usa golden ratio para distribución uniforme y evita hues 80°-160° (zona verde).
pub fn generate_unique_player_color(player_colors: &mut PlayerColors) -> Color {
    const GOLDEN_RATIO: f32 = 0.618033988749895;

    // Calcular el hue base usando golden ratio para máxima separación
    let raw_hue = player_colors.next_hue_offset;
    player_colors.next_hue_offset = (player_colors.next_hue_offset + GOLDEN_RATIO) % 1.0;

    // Evitar zona verde (80°-160° = 0.222-0.444 en rango 0-1)
    // Mapear el hue a los rangos válidos: 0°-80° (0.0-0.222) y 160°-360° (0.444-1.0)
    // Rango válido total: 0.222 + 0.556 = 0.778 del espectro
    let valid_range = 0.778;
    let scaled_hue = raw_hue * valid_range;

    let final_hue = if scaled_hue < 0.222 {
        // Zona roja/naranja/amarilla (0° - 80°)
        scaled_hue
    } else {
        // Zona azul/magenta/rosa (160° - 360°)
        scaled_hue + 0.222 // Saltar la zona verde
    };

    // Saturación alta (0.85) y Value alto (0.95) para visibilidad
    let (r, g, b) = hsv_to_rgb(final_hue, 0.85, 0.95);
    Color::srgb(r, g, b)
}

/// Calcula el color del jugador y su color opuesto para barras/texto
/// basándose en el índice de equipo y los colores definidos en la configuración
pub fn get_team_colors(team_index: u8, team_colors: &[(f32, f32, f32)]) -> (Color, Color) {
    let team_color = team_colors
        .get(team_index as usize)
        .copied()
        .unwrap_or((0.5, 0.5, 0.5));

    let player_color = Color::srgb(team_color.0, team_color.1, team_color.2);
    let (r, g, b) = complementary_color(team_color.0, team_color.1, team_color.2);
    let opposite_color = Color::srgb(r, g, b);

    (player_color, opposite_color)
}
