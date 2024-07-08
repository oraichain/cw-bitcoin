use bitcoin::Script;
use cosmwasm_std::Storage;
use cw_storage_plus::{Item, Map};

use crate::{
    adapter::Adapter,
    error::ContractResult,
    interface::Xpub,
    interface::{BitcoinConfig, CheckpointConfig},
};

pub const CHECKPOINT_CONFIG: Item<CheckpointConfig> = Item::new("checkpoint_config");

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
