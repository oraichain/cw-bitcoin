use crate::{
    app::{Bitcoin, ConsensusKey},
    constants::VALIDATOR_ADDRESS_PREFIX,
    helper::{convert_addr_by_prefix, fetch_staking_validator},
    interface::{BitcoinConfig, CheckpointConfig, Dest},
    state::{
        get_full_btc_denom, Ratio, BITCOIN_CONFIG, CHECKPOINT_CONFIG, CONFIG, SIGNERS,
        TOKEN_FEE_RATIO, VALIDATORS,
    },
    threshold_sig::Signature,
};
use bech32::Bech32;
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use common_bitcoin::{
    adapter::{Adapter, WrappedBinary},
    error::{ContractError, ContractResult},
    xpub::Xpub,
};
use ibc_proto::cosmos::staking::v1beta1::{BondStatus, QueryValidatorResponse};
use prost::Message;
use std::str::FromStr;

use cosmwasm_std::{
    to_json_binary, wasm_execute, Addr, Api, Binary, Env, MessageInfo, QuerierWrapper, Response,
    Storage, Uint128, WasmMsg,
};
use oraiswap::asset::AssetInfo;
use token_bindings::Metadata;

pub fn update_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    owner: Option<Addr>,
    relayer_fee_token: Option<AssetInfo>,
    token_fee_receiver: Option<Addr>,
    relayer_fee_receiver: Option<Addr>,
    relayer_fee: Option<Uint128>,
    token_fee: Option<Ratio>,
    light_client_contract: Option<Addr>,
    swap_router_contract: Option<Addr>,
    token_factory_contract: Option<Addr>,
    osor_entry_point_contract: Option<Addr>,
) -> ContractResult<Response> {
    let mut config = CONFIG.load(store)?;
    assert_eq!(info.sender, config.owner);

    if let Some(owner) = owner {
        config.owner = owner;
    }

    if let Some(relayer_fee_token) = relayer_fee_token {
        config.relayer_fee_token = relayer_fee_token;
    }

    if let Some(token_fee_receiver) = token_fee_receiver {
        config.token_fee_receiver = token_fee_receiver;
    }

    if let Some(relayer_fee_receiver) = relayer_fee_receiver {
        config.relayer_fee_receiver = relayer_fee_receiver;
    }

    if let Some(relayer_fee) = relayer_fee {
        config.relayer_fee = relayer_fee;
    }

    if let Some(token_fee) = token_fee {
        TOKEN_FEE_RATIO.save(store, &token_fee)?;
    }

    if let Some(token_factory_contract) = token_factory_contract {
        config.token_factory_contract = token_factory_contract;
    }

    if let Some(light_client_contract) = light_client_contract {
        config.light_client_contract = light_client_contract;
    }

    if let Some(swap_router_contract) = swap_router_contract {
        config.swap_router_contract = Some(swap_router_contract);
    }

    if let Some(osor_entry_point_contract) = osor_entry_point_contract {
        config.osor_entry_point_contract = Some(osor_entry_point_contract);
    }

    CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}

pub fn update_checkpoint_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: CheckpointConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    CHECKPOINT_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_checkpoint_config"))
}

pub fn update_bitcoin_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: BitcoinConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    BITCOIN_CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_bitcoin_config"))
}

pub fn relay_deposit(
    querier: &QuerierWrapper,
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
        querier,
        &env,
        store,
        btc_tx,
        btc_height,
        btc_proof,
        btc_vout,
        sigset_index,
        dest,
        false,
    )?;

    Ok(response)
}

pub fn withdraw_to_bitcoin(
    store: &mut dyn Storage,
    info: MessageInfo,
    env: Env,
    btc_address: String,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();

    let mut cosmos_msgs = vec![];

    let config = CONFIG.load(store)?;
    let denom = get_full_btc_denom(config.token_factory_contract.as_str());
    let script_pubkey = bitcoin::Address::from_str(btc_address.as_str())
        .map_err(|err| ContractError::App(err.to_string()))?
        .script_pubkey();
    for fund in info.funds {
        if fund.denom == denom {
            let amount = fund.amount;
            btc.add_withdrawal(store, Adapter::new(script_pubkey.clone()), amount)?;

            // burn here
            cosmos_msgs.push(WasmMsg::Execute {
                contract_addr: config.token_factory_contract.clone().into_string(),
                msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::BurnTokens {
                    amount,
                    denom: fund.denom,
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
    querier: &QuerierWrapper,
    store: &mut dyn Storage,
    btc_height: u32,
    btc_proof: Adapter<PartialMerkleTree>,
    cp_index: u32,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    let response = Response::new().add_attribute("action", "relay_checkpoint");
    btc.relay_checkpoint(querier, store, btc_height, btc_proof, cp_index, false)?;
    Ok(response)
}

pub fn submit_checkpoint_signature(
    api: &dyn Api,
    store: &mut dyn Storage,
    xpub: WrappedBinary<Xpub>,
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
    xpub: WrappedBinary<Xpub>,
    sigs: Vec<Signature>,
) -> ContractResult<Response> {
    let btc = Bitcoin::default();
    let mut recovery_txs = btc.recovery_txs;
    recovery_txs.sign(api, store, &xpub.0, sigs)?;
    let response = Response::new().add_attribute("action", "submit_recovery_signature");
    Ok(response)
}

pub fn set_signatory_key(
    querier: &QuerierWrapper,
    store: &mut dyn Storage,
    info: MessageInfo,
    xpub: WrappedBinary<Xpub>,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();
    btc.set_signatory_key(querier, store, info.sender, xpub.0)?;
    let response = Response::new().add_attribute("action", "set_signatory_key");
    Ok(response)
}

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

pub fn register_validator(
    store: &mut dyn Storage,
    querier: &QuerierWrapper,
    info: MessageInfo,
) -> ContractResult<Response> {
    let sender = info.sender;
    let val_addr = convert_addr_by_prefix(sender.as_str(), VALIDATOR_ADDRESS_PREFIX);
    let binary_validator_result = fetch_staking_validator(querier, val_addr).unwrap();
    let validator_response =
        QueryValidatorResponse::decode(binary_validator_result.as_slice()).unwrap();
    let validator: ibc_proto::cosmos::staking::v1beta1::Validator =
        validator_response.validator.unwrap();
    if validator.jailed {
        return Err(ContractError::ValidatorJailed {});
    }
    if validator.consensus_pubkey.is_none() {
        return Err(ContractError::ValidatorNoConsensusPubKey {});
    }
    if validator.status != BondStatus::Bonded as i32 {
        return Err(ContractError::ValidatorNotBonded {});
    }
    let cons_key: [u8; 32] = validator
        .consensus_pubkey
        .unwrap()
        .value
        .try_into()
        .expect("Consensus keys must have only 32 elements");
    let voting_power: u64 = validator.tokens.parse().expect("Cannot parse voting power");
    SIGNERS.save(store, sender.as_str(), &cons_key)?;
    VALIDATORS.save(
        store,
        &cons_key,
        &(voting_power, sender.clone().into_string()),
    )?;
    let response = Response::new()
        .add_attribute("action", "register_validator")
        .add_attribute("sender", sender)
        .add_attribute("consensus_key", Binary::from(cons_key).to_string())
        .add_attribute("voting_power", voting_power.to_string());
    Ok(response)
}

pub fn register_denom(
    store: &mut dyn Storage,
    info: MessageInfo,
    subdenom: String,
    metadata: Option<Metadata>,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);

    let config = CONFIG.load(store)?;
    let msg = wasm_execute(
        config.token_factory_contract,
        &tokenfactory::msg::ExecuteMsg::CreateDenom { subdenom, metadata },
        info.funds,
    )?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "register_denom"))
}

// USE THIS WHEN WE HAVE TO CHANGE TO ANOTHER BRIDGE CONTRACT
pub fn change_btc_denom_owner(
    store: &mut dyn Storage,
    info: MessageInfo,
    new_owner: String,
) -> ContractResult<Response> {
    let config = CONFIG.load(store)?;
    assert_eq!(info.sender, config.owner);

    let denom = get_full_btc_denom(config.token_factory_contract.as_str());
    let msg = wasm_execute(
        config.token_factory_contract,
        &tokenfactory::msg::ExecuteMsg::ChangeDenomOwner {
            denom,
            new_admin_address: new_owner,
        },
        info.funds,
    )?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "change_btc_denom_owner"))
}
