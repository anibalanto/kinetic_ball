mod setup;
mod input;
mod interpolation;

pub use setup::{setup, spawn_key_visual_2d};
pub use input::handle_multi_player_input;
pub use interpolation::{interpolate_entities, process_movements, animate_keys, is_gamepad_binding_active};
