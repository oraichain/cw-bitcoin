// #![feature(trait_alias)]

mod app;
mod checkpoint;
mod constants;
pub mod entrypoints;
pub mod error;
mod header;
mod interface;
pub mod msg;

mod adapter;
mod outpoint_set;
mod recovery;
mod signatory;
mod state;
mod threshold_sig;

pub mod contract;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub mod integration_tests;
