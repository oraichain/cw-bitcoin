#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    entrypoints::*,
    error::ContractError,
    interface::{BitcoinConfig, CheckpointConfig, Config, HeaderConfig},
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg},
    state::{
        BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, CONFIG, FEE_POOL,
        FIRST_UNHANDLED_CONFIRMED_INDEX, HEADER_CONFIG, VALIDATORS,
    },
};

use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, WasmMsg,
};
use cw2::set_contract_version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cw_bitcoin";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    CONFIG.save(
        deps.storage,
        &Config {
            owner: info.sender,
            token_factory_addr: msg.token_factory_addr,
            bridge_wasm_addr: msg.bridge_wasm_addr,
        },
    )?;

    HEADER_CONFIG.save(deps.storage, &HeaderConfig::mainnet()?)?;
    CHECKPOINT_CONFIG.save(deps.storage, &CheckpointConfig::default())?;
    BITCOIN_CONFIG.save(deps.storage, &&BitcoinConfig::default())?;
    FEE_POOL.save(deps.storage, &0)?;
    BUILDING_INDEX.save(deps.storage, &0)?;
    FIRST_UNHANDLED_CONFIRMED_INDEX.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RelayDeposit {
            btc_tx,
            btc_height,
            btc_proof,
            btc_vout,
            sigset_index,
            dest,
        } => relay_deposit(
            env,
            deps.storage,
            btc_tx,
            btc_height,
            btc_proof,
            btc_vout,
            sigset_index,
            dest,
        ),
        ExecuteMsg::RelayCheckpoint {
            btc_height,
            btc_proof,
            cp_index,
        } => relay_checkpoint(deps.storage, btc_height, btc_proof, cp_index),
        ExecuteMsg::WithdrawToBitcoin { script_pubkey } => {
            withdraw_to_bitcoin(deps.storage, info, env, script_pubkey)
        }
        ExecuteMsg::RelayHeaders { headers } => relay_headers(deps.storage, headers),
        ExecuteMsg::UpdateHeaderConfig { config } => {
            update_header_config(deps.storage, info, config)
        }
        ExecuteMsg::UpdateBitcoinConfig { config } => {
            update_bitcoin_config(deps.storage, info, config)
        }
        ExecuteMsg::UpdateCheckpointConfig { config } => {
            update_checkpoint_config(deps.storage, info, config)
        }
        ExecuteMsg::SubmitCheckpointSignature {
            xpub,
            sigs,
            checkpoint_index,
            btc_height,
        } => submit_checkpoint_signature(
            deps.api,
            deps.storage,
            xpub,
            sigs,
            checkpoint_index,
            btc_height,
        ),
        ExecuteMsg::SubmitRecoverySignature { xpub, sigs } => {
            submit_recovery_signature(deps.api, deps.storage, xpub, sigs)
        }
        ExecuteMsg::SetSignatoryKey { xpub } => set_signatory_key(deps.storage, info, xpub),
        ExecuteMsg::AddValidators { addrs, infos } => {
            add_validators(deps.storage, info, addrs, infos)
        }
        ExecuteMsg::RegisterDenom { subdenom, metadata } => {
            register_denom(deps.storage, info, subdenom, metadata)
        }
        #[cfg(test)]
        ExecuteMsg::TriggerBeginBlock { hash } => clock_end_block(&env, deps.storage, hash),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::ClockEndBlock { hash } => clock_end_block(&env, deps.storage, hash),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositFees { index } => {
            to_json_binary(&query_deposit_fees(deps.storage, index)?)
        }
        QueryMsg::WithdrawalFees { address, index } => {
            to_json_binary(&query_withdrawal_fees(deps.storage, address, index)?)
        }
        QueryMsg::CompletedTxs { limit } => {
            to_json_binary(&query_complete_txs(deps.storage, limit)?)
        }
        QueryMsg::HeaderHeight {} => to_json_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => {
            to_json_binary(&query_sidechain_block_hash(deps.storage)?)
        }
        QueryMsg::CheckpointByIndex { index } => {
            to_json_binary(&query_checkpoint_by_index(deps.storage, index)?)
        }
        QueryMsg::SigningRecoveryTxs { xpub } => to_json_binary(&query_signing_recovery_txs(
            deps.querier,
            deps.storage,
            xpub,
        )?),
        QueryMsg::SigningTxsAtCheckpointIndex {
            xpub,
            checkpoint_index,
        } => to_json_binary(&query_signing_txs_at_checkpoint_index(
            deps.storage,
            xpub,
            checkpoint_index,
        )?),
        QueryMsg::CompletedIndex {} => to_json_binary(&query_completed_index(deps.storage)?),
        QueryMsg::BuildingIndex {} => to_json_binary(&query_building_index(deps.storage)?),
        QueryMsg::ConfirmedIndex {} => to_json_binary(&query_comfirmed_index(deps.storage)?),
        QueryMsg::UnhandledConfirmedIndex {} => {
            to_json_binary(&query_first_unconfirmed_index(deps.storage)?)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}
