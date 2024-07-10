use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{Env, Response, Storage};

use crate::{
    adapter::Adapter, app::Bitcoin, error::ContractResult, interface::Dest, state::HEADER_CONFIG,
};

pub fn relay_deposit(
    env: Env,
    store: &mut dyn Storage,
    btc_tx: Adapter<Transaction>,
    btc_height: u32,
    btc_proof: Adapter<PartialMerkleTree>,
    btc_vout: u32,
    sigset_index: u32,
    dest: Dest,
) -> ContractResult<Response> {
    let header_config = HEADER_CONFIG.load(store)?;
    let mut btc = Bitcoin::new(header_config);
    btc.relay_deposit(
        env,
        store,
        btc_tx,
        btc_height,
        btc_proof,
        btc_vout,
        sigset_index,
        dest,
    )?;

    Ok(Response::new().add_attribute("action", "relay_deposit"))
}
