use std::borrow::BorrowMut;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    app::Bitcoin,
    entrypoints::*,
    error::ContractError,
    interface::{Config, HeaderConfig},
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg},
    state::{CONFIG, HEADER_CONFIG, VALIDATORS},
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
            bitcoin_lib_addr: msg.bitcoin_lib_addr,
        },
    )?;

    HEADER_CONFIG.save(deps.storage, &HeaderConfig::mainnet()?)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
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
        ExecuteMsg::RelayHeaders { headers } => relay_headers(deps.storage, headers),
        ExecuteMsg::UpdateHeaderConfig { config } => update_header_config(deps.storage, config),
        ExecuteMsg::UpdateBitcoinConfig { config } => update_bitcoin_config(deps.storage, config),
        ExecuteMsg::UpdateCheckpointConfig { config } => {
            update_checkpoint_config(deps.storage, config)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::ClockEndBlock { hash } => {
            let header_config = HEADER_CONFIG.load(deps.storage)?;
            let mut btc = Bitcoin::new(header_config);
            let querier = deps.querier;
            let storage = deps.storage;

            let external_outputs: Vec<bitcoin::TxOut> =
                if btc.should_push_checkpoint(env.clone(), querier, storage)? {
                    // TODO: build output
                    vec![]
                    // self.cosmos
                    //     .build_outputs(&self.ibc, btc.checkpoints.index)?
                } else {
                    vec![]
                };

            let offline_signers = btc.begin_block_step(
                env,
                querier,
                storage,
                external_outputs.into_iter().map(Ok),
                hash.to_vec(),
            )?;

            for cons_key in &offline_signers {
                let (_, address) = VALIDATORS.load(storage, cons_key)?;
                // punish_downtime(address)?;
                println!("need punish downtime for {}", address);
            }

            Ok(Response::new())
        }
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
        QueryMsg::HeaderHeight {} => to_json_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => {
            to_json_binary(&query_sidechain_block_hash(deps.storage)?)
        }
        QueryMsg::CheckpointByIndex { index } => {
            to_json_binary(&query_checkpoint_by_index(deps.storage, index)?)
        }
        QueryMsg::SigningRecoveryTxs { xpub } => to_binary(&query_signing_recovery_txs(
            deps.querier,
            deps.storage,
            xpub,
        )?),
        QueryMsg::SigningTxsAtCheckpointIndex {
            xpub,
            checkpoint_index,
        } => to_binary(&query_signing_txs_at_checkpoint_index(
            deps.querier,
            deps.storage,
            xpub,
            checkpoint_index,
        )?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}
