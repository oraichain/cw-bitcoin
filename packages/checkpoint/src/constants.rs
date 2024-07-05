pub const MAIN_NATIVE_TOKEN_DENOM: &str = "uoraibtc";
pub const BTC_NATIVE_TOKEN_DENOM: &str = "usat";
pub const MIN_FEE_RATE: u64 = 40; // in satoshis per vbytes
pub const MAX_FEE_RATE: u64 = 1000; // in satoshis per vbytes
pub const USER_FEE_FACTOR: u64 = 27000; // 2.7x. Calculate by USER_FEE_FACTOR / 10000
pub const IBC_FEE: u64 = 0;
/// The default fee rate to be used to pay miner fees, in satoshis per virtual byte.
pub const DEFAULT_FEE_RATE: u64 = 55; // ~ 100 sat/vb
pub const BRIDGE_FEE_RATE: f64 = 0.0;
pub const TRANSFER_FEE: u64 = 0;

// checkpoints
pub const MAX_CHECKPOINT_INTERVAL: u64 = 60 * 60 * 24 * 12; // 12 days. This value should be smaller than max_deposit_age & MAX_CHECKPOINT_AGE
pub const MAX_DEPOSIT_AGE: u64 = 60 * 60 * 24 * 7 * 2; // 2 weeks
pub const MAX_CHECKPOINT_AGE: u64 = 60 * 60 * 24 * 7 * 3; // 3 weeks

// app constants
pub const IBC_FEE_USATS: u64 = 0;
pub const DECLARE_FEE_USATS: u64 = 0;

pub const INITIAL_SUPPLY_ORAIBTC: u64 = 1_000_000_000_000; // 1 millions oraibtc
pub const INITIAL_SUPPLY_USATS_FOR_RELAYER: u64 = 1_000_000_000_000; // 1 millions usats

pub const MIN_DEPOSIT_AMOUNT: u64 = 5000; // in satoshis
pub const MIN_WITHDRAWAL_AMOUNT: u64 = 5000; // in satoshis

pub const MAX_VALIDATORS: u64 = 30;

// checkpoint constants
pub const DEFAULT_MAX_SCAN_CHECKPOINTS_CONFIRMATIONS: usize = 3000; // this variable is used for relaying checkpoint

// call fee usats
pub const CALL_FEE_USATS: u64 = 0;
