#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    entrypoints::*,
    error::ContractError,
    interface::Config,
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg},
    state::CONFIG,
};

use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
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
        },
    )?;

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
        ExecuteMsg::UpdateHeaderConfig { config } => update_header_config(deps.storage, config),
        ExecuteMsg::AddWorkHeader { header } => add_work_header(deps.storage, header),
        ExecuteMsg::UpdateBitcoinConfig { config } => update_bitcoin_config(deps.storage, config),
        ExecuteMsg::UpdateCheckpointConfig { config } => {
            update_checkpoint_config(deps.storage, config)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositFees { index } => to_binary(&query_deposit_fees(deps.storage, index)?),
        QueryMsg::WithdrawalFees { address, index } => {
            to_binary(&query_withdrawal_fees(deps.storage, address, index)?)
        }
        QueryMsg::HeaderHeight {} => to_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => to_binary(&query_sidechain_block_hash(deps.storage)?),
        QueryMsg::CheckpointByIndex { index } => {
            to_binary(&query_checkpoint_by_index(deps.storage, index)?)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw_utils::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}
