pub const MAX_LENGTH: u64 = 24_192; // ~6 months
pub const MAX_HEADERS_RELAY_ONE_TIME: u64 = 1000;
pub const MAX_TIME_INCREASE: u32 = 2 * 60 * 60;
pub const RETARGET_INTERVAL: u32 = 2016;
pub const TARGET_SPACING: u32 = 10 * 60;
pub const TARGET_TIMESPAN: u32 = RETARGET_INTERVAL * TARGET_SPACING;
pub const MAX_TARGET: u32 = 0x1d00ffff;
