use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use common_bitcoin::error::ContractError;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:proxy-bitcoin";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    unimplemented!()
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::ValidatorInfo { val_addr } => to_json_binary(
            &query::query_staking_validator_info(deps.querier, val_addr)?,
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}

pub mod query {
    use std::str::FromStr;

    use common_bitcoin::{error::ContractResult, msg::ValidatorInfo};
    use cosmwasm_std::{Binary, QuerierWrapper, QueryRequest, Uint128};
    use ibc_proto::cosmos::staking::v1beta1::{QueryValidatorRequest, QueryValidatorResponse};
    use prost::Message;

    pub fn query_staking_validator_info(
        api: QuerierWrapper,
        addr: String,
    ) -> ContractResult<Option<ValidatorInfo>> {
        let query_validator_request = QueryValidatorRequest {
            validator_addr: addr,
        };
        let encode_query_validator_request =
            QueryValidatorRequest::encode_to_vec(&query_validator_request);
        let query_validator_request_binary = Binary::from(encode_query_validator_request);
        let query_validator_response: Binary = api
            .query(&QueryRequest::Stargate {
                path: "/cosmos.staking.v1beta1.Query/Validator".to_string(),
                data: query_validator_request_binary,
            })
            .unwrap();
        let decode_validator_response =
            QueryValidatorResponse::decode(query_validator_response.as_slice()).unwrap();
        let option_validator = decode_validator_response.validator;
        if option_validator.is_none() {
            return Ok(None);
        }

        let validator = option_validator.unwrap();
        if validator.consensus_pubkey.is_none() {
            return Ok(None);
        }

        Ok(Some(ValidatorInfo {
            operator_address: validator.operator_address,
            consensus_pubkey: Some(validator.consensus_pubkey.unwrap().value),
            jailed: validator.jailed,
            status: validator.status,
            tokens: Uint128::from_str(validator.tokens.as_str()).unwrap(),
        }))
    }
}

#[cfg(test)]
mod tests {}
