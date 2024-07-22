use bitcoin::secp256k1::PublicKey;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    adapter::HashBinary,
    error::{ContractError, ContractResult},
    interface::Xpub,
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg},
};
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw2::set_contract_version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:lib_bitcoin";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _: MessageInfo,
    _: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    _: DepsMut,
    _env: Env,
    _: MessageInfo,
    _: ExecuteMsg,
) -> Result<Response, ContractError> {
    unimplemented!()
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(_: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetDerivePubkey { xpub, sigset_index } => {
            to_binary(&get_derive_pubkey(xpub, sigset_index)?)
        }
    }
}

fn get_derive_pubkey(
    xpub: HashBinary<Xpub>,
    sigset_index: u32,
) -> ContractResult<HashBinary<PublicKey>> {
    let secp = bitcoin::secp256k1::Secp256k1::verification_only();

    Ok(HashBinary(
        xpub.derive_pub(
            &secp,
            &[bitcoin::util::bip32::ChildNumber::from_normal_idx(sigset_index).unwrap()],
        )
        .unwrap()
        .public_key,
    ))
}
