use crate::{
    adapter::Adapter,
    app::Bitcoin,
    constants::BTC_NATIVE_TOKEN_DENOM,
    error::ContractResult,
    header::{HeaderList, HeaderQueue, WrappedHeader},
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, MintTokens},
    state::{BITCOIN_CONFIG, CHECKPOINT_CONFIG, CONFIG, HEADER_CONFIG},
};
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{wasm_execute, Env, Response, Storage};

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
/// ONLY USE ONE
pub fn update_header_config(
    store: &mut dyn Storage,
    config: HeaderConfig,
) -> ContractResult<Response> {
    HEADER_CONFIG.save(store, &config)?;
    let header_config = HEADER_CONFIG.load(store)?;
    let mut header_queue = HeaderQueue::new(header_config);
    let _ = header_queue.configure(store, config.clone())?;
    Ok(Response::new().add_attribute("action", "update_header_config"))
}

pub fn relay_headers(
    store: &mut dyn Storage,
    headers: Vec<WrappedHeader>,
) -> ContractResult<Response> {
    let header_config = HEADER_CONFIG.load(store)?;
    let mut header_queue = HeaderQueue::new(header_config);
    header_queue.add(store, HeaderList::from(headers)).unwrap();
    Ok(Response::new().add_attribute("action", "add_headers"))
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
    // dest validation?

    let header_config = HEADER_CONFIG.load(store)?;
    let mut btc = Bitcoin::new(header_config);
    let mut response = Response::new().add_attribute("action", "relay_deposit");
    if let Some(mint_amount) = btc.relay_deposit(
        env.clone(),
        store,
        btc_tx,
        btc_height,
        btc_proof,
        btc_vout,
        sigset_index,
        dest,
    )? {
        let config = CONFIG.load(store)?;
        response = response.add_message(wasm_execute(
            config.token_factory_addr,
            &MintTokens {
                denom: BTC_NATIVE_TOKEN_DENOM.to_string(),
                amount: mint_amount,
                mint_to_address: env.contract.address.to_string(),
            },
            vec![],
        )?);
    }

    Ok(response)
}

pub fn relay_checkpoint(
    store: &mut dyn Storage,
    btc_height: u32,
    btc_proof: Adapter<PartialMerkleTree>,
    cp_index: u32,
) -> ContractResult<Response> {
    let header_config = HEADER_CONFIG.load(store)?;
    let mut btc = Bitcoin::new(header_config);
    let response = Response::new().add_attribute("action", "relay_checkpoint");
    btc.relay_checkpoint(store, btc_height, btc_proof, cp_index)?;
    Ok(response)
}
