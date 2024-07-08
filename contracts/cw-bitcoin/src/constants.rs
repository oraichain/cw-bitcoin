pub const MIN_FEE_RATE: u64 = 40; // in satoshis per vbytes
pub const MAX_FEE_RATE: u64 = 1000; // in satoshis per vbytes
pub const USER_FEE_FACTOR: u64 = 27000; // 2.7x. Calculate by USER_FEE_FACTOR / 10000
/// The default fee rate to be used to pay miner fees, in satoshis per virtual byte.
pub const DEFAULT_FEE_RATE: u64 = 55; // ~ 100 sat/vb

// checkpoints
pub const MAX_CHECKPOINT_INTERVAL: u64 = 60 * 60 * 24 * 12; // 12 days. This value should be smaller than max_deposit_age & MAX_CHECKPOINT_AGE
pub const MAX_CHECKPOINT_AGE: u64 = 60 * 60 * 24 * 7 * 3; // 3 weeks

pub const MAX_LENGTH: u64 = 24_192; // ~6 months
pub const MAX_RELAY: u64 = 1000;
pub const MAX_TIME_INCREASE: u32 = 2 * 60 * 60;
pub const RETARGET_INTERVAL: u32 = 2016;
pub const TARGET_SPACING: u32 = 10 * 60;
pub const TARGET_TIMESPAN: u32 = RETARGET_INTERVAL * TARGET_SPACING;
pub const MAX_TARGET: u32 = 0x1d00ffff;
