#![feature(trait_alias)]

mod adapter;
mod app;
mod checkpoint;
mod constants;
pub mod entrypoints;
pub mod error;
mod header;
mod interface;
mod msg;
mod outpoint_set;
mod recovery;
mod signatory;
mod state;
mod threshold_sig;

/// libraries for relayer
#[cfg(not(target_arch = "wasm32"))]
mod signer;

pub mod contract;

#[cfg(test)]
mod tests;
