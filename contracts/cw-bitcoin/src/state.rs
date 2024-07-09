use bitcoin::Script;
use cosmwasm_std::Storage;
use cw_storage_plus::{Item, Map};

use crate::{
    adapter::Adapter,
    error::ContractResult,
    header::WorkHeader,
    interface::{BitcoinConfig, CheckpointConfig, DequeExtension, HeaderConfig, Xpub},
    recovery::RecoveryTx,
};

pub const CHECKPOINT_CONFIG: Item<CheckpointConfig> = Item::new("checkpoint_config");
pub const HEADER_CONFIG: Item<HeaderConfig> = Item::new("header");
pub const BITCOIN_CONFIG: Item<BitcoinConfig> = Item::new("bitcoin_config");

pub const RECOVERY_SCRIPTS: Map<String, Adapter<bitcoin::Script>> = Map::new("recovery_scripts");

pub const VALIDATORS: Map<&[u8], u64> = Map::new("validators");

pub const SIG_KEYS: Map<&[u8], Xpub> = Map::new("sig_keys");

pub fn to_output_script(store: &dyn Storage, dest: String) -> ContractResult<Option<Script>> {
    Ok(RECOVERY_SCRIPTS
        .load(store, dest)
        .ok()
        .map(|script| script.into_inner()))
}

/// A queue of Bitcoin block headers, along with the total estimated amount of
/// work (measured in hashes) done in the headers included in the queue.
///
/// The header queue is used to validate headers as they are received from the
/// Bitcoin network, ensuring each header is associated with a valid
/// proof-of-work and that the chain of headers is valid.
///
/// The queue is able to reorg if a new chain of headers is received that
/// contains more work than the current chain, however it can not process reorgs
/// that are deeper than the length of the queue (the length will be at the
/// configured pruning level based on the `max_length` config parameter).
pub const HEADERS: DequeExtension<WorkHeader> = DequeExtension::new("headers");

pub const RECOVERY_TXS: DequeExtension<RecoveryTx> = DequeExtension::new("recovery_txs");
