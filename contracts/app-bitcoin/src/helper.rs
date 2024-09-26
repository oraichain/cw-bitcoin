use bech32::Bech32;
use common_bitcoin::error::ContractResult;
use cosmwasm_std::{to_json_vec, Api, Binary, Empty, QuerierWrapper, QueryRequest};
use ibc_proto::cosmos::staking::v1beta1::QueryValidatorRequest;
use oraiswap::asset::AssetInfo;
use prost::Message;

use crate::constants::VALIDATOR_ADDRESS_PREFIX;

pub fn denom_to_asset_info(api: &dyn Api, denom: &str) -> AssetInfo {
    if let Ok(contract_addr) = api.addr_validate(denom) {
        AssetInfo::Token { contract_addr }
    } else {
        AssetInfo::NativeToken {
            denom: denom.to_string(),
        }
    }
}

pub fn fetch_staking_validator(querier: &QuerierWrapper, addr: String) -> ContractResult<Binary> {
    let bin_request = to_json_vec(&QueryRequest::<Empty>::Stargate {
        path: "/cosmos.staking.v1beta1.Query/Validator".to_string(),
        data: QueryValidatorRequest {
            validator_addr: addr,
        }
        .encode_to_vec()
        .into(),
    })?;
    let buf = querier.raw_query(&bin_request).unwrap().unwrap();
    Ok(buf)
}

pub fn convert_addr_by_prefix(address: &str, prefix: &str) -> String {
    let (_hrp, bech32_data) = bech32::decode(address).unwrap();
    let val_addr =
        bech32::encode::<Bech32>(bech32::Hrp::parse(prefix).unwrap(), &bech32_data).unwrap();
    val_addr
}
