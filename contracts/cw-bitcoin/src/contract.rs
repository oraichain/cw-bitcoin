#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    entrypoints::*,
    error::ContractError,
    header::HeaderQueue,
    interface::{BitcoinConfig, CheckpointConfig, Config, HeaderConfig},
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg},
    state::{
        BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, CONFIG, FEE_POOL,
        FIRST_UNHANDLED_CONFIRMED_INDEX,
    },
};

use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
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
            relayer_fee_receiver: msg.relayer_fee_receiver,
            token_fee_receiver: msg.token_fee_receiver,
            relayer_fee_token: msg.relayer_fee_token,
            relayer_fee: msg.relayer_fee,
            swap_router_contract: msg.swap_router_contract,
            osor_entry_point_contract: msg.osor_entry_point_contract,
        },
    )?;

    // Set up header
    let header_config = HeaderConfig::mainnet()?;
    let mut header_queue = HeaderQueue::default();
    header_queue.configure(deps.storage, header_config.clone())?;

    // Set up config
    CHECKPOINT_CONFIG.save(deps.storage, &CheckpointConfig::default())?;
    BITCOIN_CONFIG.save(deps.storage, &BitcoinConfig::default())?;
    FEE_POOL.save(deps.storage, &0)?;

    // Set up checkpoint index
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
        ExecuteMsg::UpdateConfig {
            relayer_fee_token,
            token_fee_receiver,
            relayer_fee_receiver,
            relayer_fee,
            swap_router_contract,
            token_fee,
            token_factory_addr,
            owner,
        } => update_config(
            deps.storage,
            info,
            relayer_fee_token,
            token_fee_receiver,
            relayer_fee_receiver,
            relayer_fee,
            swap_router_contract,
            token_fee,
            token_factory_addr,
            owner,
        ),
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
        ExecuteMsg::WithdrawToBitcoin { btc_address } => {
            withdraw_to_bitcoin(deps.storage, info, env, btc_address)
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
        ExecuteMsg::AddValidators {
            addrs,
            voting_powers,
            consensus_keys,
        } => add_validators(deps.storage, info, addrs, voting_powers, consensus_keys),
        ExecuteMsg::RegisterDenom { subdenom, metadata } => {
            register_denom(deps.storage, info, subdenom, metadata)
        }
        ExecuteMsg::ChangeBtcDenomOwner { new_owner } => {
            change_btc_denom_owner(deps.storage, info, new_owner)
        }
        ExecuteMsg::TriggerBeginBlock { hash } => {
            clock_end_block(&env, deps.storage, &deps.querier, deps.api, hash)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::BitcoinConfig {} => to_json_binary(&query_bitcoin_config(deps.storage)?),
        QueryMsg::CheckpointConfig {} => to_json_binary(&query_checkpoint_config(deps.storage)?),
        QueryMsg::HeaderConfig {} => to_json_binary(&query_header_config(deps.storage)?),
        QueryMsg::SignatoryKey { addr } => {
            to_json_binary(&query_signatory_key(deps.storage, addr)?)
        }
        QueryMsg::DepositFees { index } => {
            to_json_binary(&query_deposit_fees(deps.storage, index)?)
        }
        QueryMsg::WithdrawalFees { address, index } => {
            to_json_binary(&query_withdrawal_fees(deps.storage, address, index)?)
        }
        QueryMsg::CheckpointFees { index } => {
            to_json_binary(&query_checkpoint_fees(deps.storage, index)?)
        }
        QueryMsg::CompletedCheckpointTxs { limit } => {
            to_json_binary(&query_complete_checkpoint_txs(deps.storage, limit)?)
        }
        QueryMsg::CheckpointTx { index } => {
            to_json_binary(&query_checkpoint_tx(deps.storage, index)?)
        }
        QueryMsg::SignedRecoveryTxs {} => to_json_binary(&query_signed_recovery_txs(deps.storage)?),
        QueryMsg::HeaderHeight {} => to_json_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => {
            to_json_binary(&query_sidechain_block_hash(deps.storage)?)
        }
        QueryMsg::CheckpointByIndex { index } => {
            to_json_binary(&query_checkpoint_by_index(deps.storage, index)?)
        }
        QueryMsg::BuildingCheckpoint {} => {
            to_json_binary(&query_building_checkpoint(deps.storage)?)
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
        QueryMsg::ProcessedOutpoint { key } => {
            to_json_binary(&query_process_outpoints(deps.storage, key)?)
        }
        QueryMsg::CompletedIndex {} => to_json_binary(&query_completed_index(deps.storage)?),
        QueryMsg::BuildingIndex {} => to_json_binary(&query_building_index(deps.storage)?),
        QueryMsg::ConfirmedIndex {} => to_json_binary(&query_comfirmed_index(deps.storage)?),
        QueryMsg::UnhandledConfirmedIndex {} => {
            to_json_binary(&query_first_unconfirmed_index(deps.storage)?)
        }
        QueryMsg::ChangeRates { interval } => {
            to_json_binary(&query_change_rates(deps.storage, _env, interval)?)
        }
        QueryMsg::ValueLocked {} => to_json_binary(&query_value_locked(deps.storage)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}
