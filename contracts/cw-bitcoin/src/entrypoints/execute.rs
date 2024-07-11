use crate::{
    app::Bitcoin,
    error::ContractResult,
    interface::{BitcoinConfig, CheckpointConfig, Dest},
    state::{BITCOIN_CONFIG, CHECKPOINT_CONFIG, HEADERS, HEADER_CONFIG},
};
use bitcoin::{consensus::Decodable, util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{from_binary, Binary, Env, Response, Storage};

/// TODO: check logic
pub fn update_checkpoint_config(
    store: &mut dyn Storage,
    config: CheckpointConfig,
) -> ContractResult<Response> {
    CHECKPOINT_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_checkpoint_config"))
}

/// TODO: check logic
pub fn update_bitcoin_config(
    store: &mut dyn Storage,
    config: BitcoinConfig,
) -> ContractResult<Response> {
    BITCOIN_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_bitcoin_config"))
}

/// TODO: check logic
pub fn update_header_config(store: &mut dyn Storage, config: Binary) -> ContractResult<Response> {
    let config = from_binary(&config)?;
    HEADER_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_header_config"))
}

/// TODO: check logic
pub fn add_work_header(store: &mut dyn Storage, header: Binary) -> ContractResult<Response> {
    // try verify header encoding
    let header = from_binary(&header)?;

    HEADERS.push_back(store, &header)?;
    Ok(Response::new().add_attribute("action", "add_work_header"))
}

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
