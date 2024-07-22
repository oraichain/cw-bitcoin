use crate::{adapter::Adapter, interface::Xpub};
use bitcoin::Transaction;
use cosmwasm_schema::{cw_serde, QueryResponses};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(GetDerivePubkeyResponse)]
    GetDerivePubkey { xpub: Adapter<Xpub> },
}

#[cw_serde]
pub struct GetDerivePubkeyResponse {}
