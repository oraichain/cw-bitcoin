use crate::{
    adapter::{Adapter, StringBinary},
    app::{Bitcoin, ConsensusKey},
    error::ContractResult,
    header::{HeaderList, HeaderQueue, WrappedHeader},
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, Xpub},
    state::{get_full_btc_denom, BITCOIN_CONFIG, CHECKPOINT_CONFIG, CONFIG, SIGNERS, VALIDATORS},
    threshold_sig::Signature,
};
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};

use cosmwasm_std::{
    to_json_binary, wasm_execute, Api, Env, MessageInfo, Response, Storage, WasmMsg,
};
use token_bindings::Metadata;

/// TODO: check logic
pub fn update_checkpoint_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: CheckpointConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    CHECKPOINT_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_checkpoint_config"))
}

/// TODO: check logic
pub fn update_bitcoin_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: BitcoinConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    BITCOIN_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_bitcoin_config"))
}

/// TODO: check logic
/// ONLY USE ONE
pub fn update_header_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: HeaderConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    let mut header_queue = HeaderQueue::default();
    let _ = header_queue.configure(store, config.clone())?;
    Ok(Response::new().add_attribute("action", "update_header_config"))
}

pub fn relay_headers(
    store: &mut dyn Storage,
    headers: Vec<WrappedHeader>,
) -> ContractResult<Response> {
    // let header_config = HEADER_CONFIG.load(store)?;
    let mut header_queue = HeaderQueue::default();
    header_queue.add(store, HeaderList::from(headers))?;
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
    let mut btc = Bitcoin::default();
    let response = Response::new().add_attribute("action", "relay_deposit");
    btc.relay_deposit(
        env.clone(),
        store,
        btc_tx,
        btc_height,
        btc_proof,
        btc_vout,
        sigset_index,
        dest,
    )?;

    Ok(response)
}

pub fn withdraw_to_bitcoin(
    store: &mut dyn Storage,
    info: MessageInfo,
    env: Env,
    script_pubkey: Adapter<bitcoin::Script>,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();

    let mut cosmos_msgs = vec![];

    let config = CONFIG.load(store)?;
    for fund in info.funds {
        let denom = get_full_btc_denom(store)?;
        if fund.denom == denom {
            let amount = fund.amount;
            btc.add_withdrawal(store, script_pubkey.clone(), amount)?;

            // burn here
            cosmos_msgs.push(WasmMsg::Execute {
                contract_addr: config.token_factory_addr.clone().into_string(),
                msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::BurnTokens {
                    amount,
                    denom,
                    burn_from_address: env.contract.address.to_string(),
                })?,
                funds: vec![],
            });
        }
    }

    let response = Response::new().add_attribute("action", "withdraw_to_bitcoin");
    Ok(response.add_messages(cosmos_msgs))
}

pub fn relay_checkpoint(
    store: &mut dyn Storage,
    btc_height: u32,
    btc_proof: Adapter<PartialMerkleTree>,
    cp_index: u32,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    let response = Response::new().add_attribute("action", "relay_checkpoint");
    btc.relay_checkpoint(store, btc_height, btc_proof, cp_index)?;
    Ok(response)
}

pub fn submit_checkpoint_signature(
    api: &dyn Api,
    store: &mut dyn Storage,
    xpub: StringBinary<Xpub>,
    sigs: Vec<Signature>,
    cp_index: u32,
    btc_height: u32,
) -> ContractResult<Response> {
    let btc = Bitcoin::default();
    let mut checkpoints = btc.checkpoints;
    checkpoints.sign(api, store, &xpub.0, sigs, cp_index, btc_height)?;
    let response = Response::new().add_attribute("action", "submit_checkpoint_signature");
    Ok(response)
}

pub fn submit_recovery_signature(
    api: &dyn Api,
    store: &mut dyn Storage,
    xpub: StringBinary<Xpub>,
    sigs: Vec<Signature>,
) -> ContractResult<Response> {
    let btc = Bitcoin::default();
    let mut recovery_txs = btc.recovery_txs;
    recovery_txs.sign(api, store, &xpub.0, sigs)?;
    let response = Response::new().add_attribute("action", "submit_recovery_signature");
    Ok(response)
}

pub fn set_signatory_key(
    store: &mut dyn Storage,
    info: MessageInfo,
    xpub: StringBinary<Xpub>,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    btc.set_signatory_key(store, info.sender, xpub.0)?;
    let response = Response::new().add_attribute("action", "set_signatory_key");
    Ok(response)
}

// TODO: Add check only owners of this contract can call
pub fn add_validators(
    store: &mut dyn Storage,
    info: MessageInfo,
    addrs: Vec<String>,
    voting_powers: Vec<u64>,
    consensus_keys: Vec<ConsensusKey>,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    assert_eq!(addrs.len(), voting_powers.len());
    assert_eq!(addrs.len(), consensus_keys.len());

    for i in 0..addrs.len() {
        let addr = &addrs[i];
        let power = voting_powers[i];
        let cons_key = &consensus_keys[i];

        SIGNERS.save(store, addr, cons_key)?;
        VALIDATORS.save(store, cons_key, &(power, addr.clone()))?;
    }
    let response = Response::new().add_attribute("action", "add_validators");
    Ok(response)
}

// TODO: Add check only owners of this contract can call
pub fn register_denom(
    store: &mut dyn Storage,
    info: MessageInfo,
    subdenom: String,
    metadata: Option<Metadata>,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);

    let config = CONFIG.load(store)?;
    let mut cosmos_msgs = vec![];
    cosmos_msgs.push(wasm_execute(
        config.token_factory_addr,
        &tokenfactory::msg::ExecuteMsg::CreateDenom {
            subdenom: subdenom,
            metadata: metadata,
        },
        info.funds,
    )?);

    Ok(Response::new()
        .add_messages(cosmos_msgs)
        .add_attribute("action", "register_denom"))
}
