use crate::{header::WrappedHeader, interface::HeaderConfig};
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use common_bitcoin::adapter::{Adapter, WrappedBinary};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

#[cw_serde]
pub struct Config {
    pub owner: Addr,
}

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {
    RelayHeaders { headers: Vec<WrappedHeader> },
    UpdateHeaderConfig { config: HeaderConfig },
    UpdateConfig { owner: Option<Addr> },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(HeaderConfig)]
    HeaderConfig {},
    #[returns(u32)]
    HeaderHeight {},
    #[returns(WrappedBinary<bitcoin::BlockHash>)]
    SidechainBlockHash {},
    #[returns(())]
    VerifyTxWithProof {
        btc_tx: Adapter<Transaction>,
        btc_height: u32,
        btc_proof: Adapter<PartialMerkleTree>,
    },
}

#[cw_serde]
pub enum MigrateMsg {}
