use crate::{
    adapter::Adapter,
    app::Bitcoin,
    error::ContractResult,
    header::WorkHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig},
    state::{BITCOIN_CONFIG, CHECKPOINT_CONFIG, HEADERS, HEADER_CONFIG},
};
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{Env, Response, Storage};

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
pub fn update_header_config(
    store: &mut dyn Storage,
    config: HeaderConfig,
) -> ContractResult<Response> {
    HEADER_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_header_config"))
}

/// TODO: check logic
pub fn add_work_header(store: &mut dyn Storage, header: WorkHeader) -> ContractResult<Response> {
    // try verify header encoding

    HEADERS.push_back(store, &header)?;
    Ok(Response::new().add_attribute("action", "add_work_header"))
}

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
