use bitcoin::Script;
use cosmwasm_std::Storage;
use cw_storage_plus::Map;

use crate::{adapter::Adapter, error::ContractResult, msg::Xpub};

pub const RECOVERY_SCRIPTS: Map<String, Adapter<bitcoin::Script>> = Map::new("recovery_scripts");

pub const VALIDATORS: Map<&[u8], u64> = Map::new("validators");

pub const SIG_KEYS: Map<&[u8], Xpub> = Map::new("sig_keys");

pub fn to_output_script(store: &dyn Storage, dest: String) -> ContractResult<Option<Script>> {
    Ok(RECOVERY_SCRIPTS
        .load(store, dest)
        .ok()
        .map(|script| script.clone().into_inner()))
}
