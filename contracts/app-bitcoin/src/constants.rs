pub const MIN_FEE_RATE: u64 = 40; // in satoshis per vbytes
pub const MAX_FEE_RATE: u64 = 1000; // in satoshis per vbytes
pub const USER_FEE_FACTOR: u64 = 27000; // 2.7x. Calculate by USER_FEE_FACTOR / 10000
/// The default fee rate to be used to pay miner fees, in satoshis per virtual byte.
/// The default fee rate to be used to pay miner fees, in satoshis per virtual byte.
pub const DEFAULT_FEE_RATE: u64 = 35; // ~ 100 sat/vb
pub const TRANSFER_FEE: u64 = 0;

// checkpoints
pub const MAX_DEPOSIT_AGE: u64 = 60 * 60 * 24 * 7 * 2; // 2 weeks
pub const MAX_CHECKPOINT_INTERVAL: u64 = 60 * 60 * 24 * 12; // 12 days. This value should be smaller than max_deposit_age & MAX_CHECKPOINT_AGE
pub const MAX_CHECKPOINT_AGE: u64 = 60 * 60 * 24 * 7 * 3; // 3 weeks

// app constants
pub const MIN_DEPOSIT_AMOUNT: u64 = 5000; // in satoshis
pub const MIN_WITHDRAWAL_AMOUNT: u64 = 5000; // in satoshis

// TODO: move to config
pub const MAX_SIGNATORIES: u64 = 20;
pub const SIGSET_THRESHOLD: (u64, u64) = (2, 3);

pub const BTC_NATIVE_TOKEN_DENOM: &str = "obtc";
pub const VALIDATOR_ADDRESS_PREFIX: &str = "oraivaloper";
