// Módulo de abstracción de input para el servidor
// con NetworkInputSource

pub mod core;
pub mod network;

// Re-exportar los tipos principales
pub use core::{GameAction, InputSource};
pub use network::NetworkInputSource;
