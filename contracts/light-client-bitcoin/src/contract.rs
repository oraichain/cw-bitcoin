use crate::{
    entrypoints::{
        query_header_config, query_header_height, query_sidechain_block_hash,
        query_verify_tx_with_proof, relay_headers, update_config, update_header_config,
    },
    header::HeaderQueue,
    interface::HeaderConfig,
    msg::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg},
    state::CONFIG,
};
use common_bitcoin::error::ContractError;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:light-client-bitcoin";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &Config { owner: info.sender })?;

    // Set up header
    let header_config = HeaderConfig::mainnet()?;
    let mut header_queue = HeaderQueue::default();
    header_queue.configure(deps.storage, header_config.clone())?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RelayHeaders { headers } => relay_headers(deps.storage, headers),
        ExecuteMsg::UpdateHeaderConfig { config } => {
            update_header_config(deps.storage, info, config)
        }
        ExecuteMsg::UpdateConfig { owner } => update_config(deps.storage, info, owner),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::HeaderConfig {} => to_json_binary(&query_header_config(deps.storage)?),
        QueryMsg::HeaderHeight {} => to_json_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => {
            to_json_binary(&query_sidechain_block_hash(deps.storage)?)
        }
        QueryMsg::VerifyTxWithProof {
            btc_tx,
            btc_height,
            btc_proof,
        } => to_json_binary(&query_verify_tx_with_proof(
            deps.storage,
            btc_tx,
            btc_height,
            btc_proof,
        )?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}

#[cfg(test)]
mod tests {}
