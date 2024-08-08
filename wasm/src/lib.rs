#![allow(non_snake_case)]

extern crate alloc;
extern crate paste;

mod adapter;
mod deposit_index;
mod error;
mod header_queue;
mod helpers;
mod interface;
mod relayer;
mod signatory;
mod threshold_sig;
mod utils;

use wasm_bindgen::prelude::*;

pub const NETWORK: ::bitcoin::Network = ::bitcoin::Network::Bitcoin;

pub const BRIDGE_FEE_RATE: f64 = 0.0;
pub const SIGSET_THRESHOLD: (u64, u64) = (2, 3);
pub const HEADER_BATCH_SIZE: usize = 250;
pub const MAX_SIGNATORIES: u64 = 20;

pub const MAX_LENGTH: u64 = 24_192; // ~6 months
pub const MAX_RELAY: u64 = 1000;
pub const MAX_TIME_INCREASE: u32 = 2 * 60 * 60;
pub const RETARGET_INTERVAL: u32 = 2016;
pub const TARGET_SPACING: u32 = 10 * 60;
pub const TARGET_TIMESPAN: u32 = RETARGET_INTERVAL * TARGET_SPACING;
pub const MAX_TARGET: u32 = 0x1d00ffff;

#[wasm_bindgen]
pub fn getGlobalBridgeFeeRate() -> f64 {
    BRIDGE_FEE_RATE
}

#[wasm_bindgen]
pub fn getGlobalSigsetThreshold() -> Vec<u64> {
    vec![SIGSET_THRESHOLD.0, SIGSET_THRESHOLD.1]
}

#[wasm_bindgen]
pub fn getGlobalHeaderBatchSize() -> usize {
    HEADER_BATCH_SIZE
}

#[wasm_bindgen]
pub fn getMaxSignatories() -> u64 {
    MAX_SIGNATORIES
}
