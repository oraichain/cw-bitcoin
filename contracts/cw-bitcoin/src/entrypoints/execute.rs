use crate::{
    app::{Bitcoin, ConsensusKey},
    constants::BTC_NATIVE_TOKEN_DENOM,
    error::ContractResult,
    header::{HeaderList, HeaderQueue, WrappedHeader},
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, MintTokens},
    state::{
        get_full_btc_denom, BITCOIN_CONFIG, CHECKPOINT_CONFIG, CONFIG, HEADER_CONFIG, SIGNERS,
        VALIDATORS,
    },
    threshold_sig::Signature,
};
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use common::{
    adapter::{Adapter, HashBinary},
    interface::Xpub,
};
use cosmwasm_std::{
    to_binary, wasm_execute, Env, MessageInfo, QuerierWrapper, Response, Storage, WasmMsg,
};
use token_bindings::Metadata;

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
    // let header_config = HEADER_CONFIG.load(store)?;
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
    let mut btc = Bitcoin::default();
    let mut response = Response::new().add_attribute("action", "relay_deposit");
    btc.relay_deposit(
        env.clone(),
        store,
        btc_tx,
        btc_height,
        btc_proof,
        btc_vout,
        sigset_index,
        dest,
    )
    .unwrap();

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
        if fund.denom == BTC_NATIVE_TOKEN_DENOM {
            let amount = fund.amount;
            btc.add_withdrawal(store, script_pubkey.clone(), amount)
                .unwrap();

            // burn here
            cosmos_msgs.push(WasmMsg::Execute {
                contract_addr: config.token_factory_addr.clone().into_string(),
                msg: to_binary(&tokenfactory::msg::ExecuteMsg::BurnTokens {
                    amount,
                    denom: get_full_btc_denom(store)?,
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
    querier: QuerierWrapper,
    store: &mut dyn Storage,
    xpub: HashBinary<Xpub>,
    sigs: Vec<Signature>,
    cp_index: u32,
    btc_height: u32,
) -> ContractResult<Response> {
    let btc = Bitcoin::default();
    let mut checkpoints = btc.checkpoints;
    let _ = checkpoints.sign(querier, store, &xpub.0, sigs, cp_index, btc_height);
    let response = Response::new().add_attribute("action", "submit_checkpoint_signature");
    Ok(response)
}

pub fn submit_recovery_signature(
    querier: QuerierWrapper,
    store: &mut dyn Storage,
    xpub: HashBinary<Xpub>,
    sigs: Vec<Signature>,
) -> ContractResult<Response> {
    let btc = Bitcoin::default();
    let mut recovery_txs = btc.recovery_txs;
    let _ = recovery_txs.sign(querier, store, &xpub.0, sigs);
    let response = Response::new().add_attribute("action", "submit_recovery_signature");
    Ok(response)
}

pub fn set_signatory_key(
    store: &mut dyn Storage,
    info: MessageInfo,
    xpub: HashBinary<Xpub>,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    let _ = btc.set_signatory_key(store, info.sender, xpub.0);
    let response = Response::new().add_attribute("action", "set_signatory_key");
    Ok(response)
}

pub fn set_recovery_script(
    store: &mut dyn Storage,
    info: MessageInfo,
    script: Adapter<bitcoin::Script>,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    let _ = btc.set_recovery_script(store, info.sender, script);
    let response = Response::new().add_attribute("action", "set_recovery_script");
    Ok(response)
}

// TODO: Add check only owners of this contract can call
pub fn add_validators(
    store: &mut dyn Storage,
    _info: MessageInfo,
    addrs: Vec<String>,
    infos: Vec<(u64, ConsensusKey)>,
) -> ContractResult<Response> {
    for (index, addr) in addrs.iter().enumerate() {
        let info = infos.get(index).unwrap();
        let (power, cons_key) = info;
        SIGNERS.save(store, addr, &cons_key).unwrap();
        VALIDATORS
            .save(store, &cons_key, &(power.to_owned(), addr.to_owned()))
            .unwrap();
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
    // OWNER.assert_admin(deps.as_ref(), &info.sender)?;

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
