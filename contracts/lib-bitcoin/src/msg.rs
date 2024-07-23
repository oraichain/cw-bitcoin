use common::{adapter::HashBinary, interface::Xpub};
use cosmwasm_schema::{cw_serde, QueryResponses};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(HashBinary<bitcoin::secp256k1::PublicKey>)]
    GetDerivePubkey {
        xpub: HashBinary<Xpub>,
        sigset_index: u32,
    },
}
