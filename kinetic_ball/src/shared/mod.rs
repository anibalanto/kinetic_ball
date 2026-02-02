pub mod map;
pub mod match_slots;
pub mod movements;
pub mod protocol;

pub use match_slots::MatchSlots;
pub use protocol::*;

pub const TICK_RATE: u64 = 60;
