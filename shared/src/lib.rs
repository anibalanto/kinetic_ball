pub mod protocol;
pub mod map;

pub use protocol::*;
pub use map::*;

pub const SERVER_PORT: u16 = 9000;
pub const TICK_RATE: u64 = 60;
pub const MAX_PLAYERS: usize = 16;
