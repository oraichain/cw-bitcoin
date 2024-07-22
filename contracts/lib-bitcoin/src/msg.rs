use crate::{
    adapter::{Adapter, HashBinary},
    interface::Xpub,
};
use bitcoin::{
    util::bip32::{ExtendedPrivKey, ExtendedPubKey},
    Transaction,
};
use cosmwasm_schema::{cw_serde, QueryResponses};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(GetDerivePubkeyResponse)]
    GetDerivePubkey {
        xpub: HashBinary<Xpub>,
        sigset_index: u32,
    },
}

#[cw_serde]
pub struct GetDerivePubkeyResponse {}
