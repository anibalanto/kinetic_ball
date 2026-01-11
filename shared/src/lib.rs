pub mod protocol;

pub use protocol::*;

pub const SERVER_PORT: u16 = 9000;
pub const TICK_RATE: u64 = 60;
pub const MAX_PLAYERS: usize = 16;
