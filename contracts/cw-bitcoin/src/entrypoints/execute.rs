use crate::{app::Bitcoin, error::ContractResult, interface::Dest, state::HEADER_CONFIG};
use bitcoin::{consensus::Decodable, util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{Binary, Env, Response, Storage};

pub fn relay_deposit(
    env: Env,
    store: &mut dyn Storage,
    btc_tx: Binary,
    btc_height: u32,
    btc_proof: Binary,
    btc_vout: u32,
    sigset_index: u32,
    dest: Dest,
) -> ContractResult<Response> {
    let btc_tx: Transaction = Decodable::consensus_decode(&mut btc_tx.as_slice())?;
    let btc_proof: PartialMerkleTree = Decodable::consensus_decode(&mut btc_proof.as_slice())?;
    let header_config = HEADER_CONFIG.load(store)?;
    let mut btc = Bitcoin::new(header_config);
    btc.relay_deposit(
        env,
        store,
        btc_tx.into(),
        btc_height,
        btc_proof.into(),
        btc_vout,
        sigset_index,
        dest,
    )?;

    Ok(Response::new().add_attribute("action", "relay_deposit"))
}
