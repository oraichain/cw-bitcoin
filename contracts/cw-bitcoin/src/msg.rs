use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::{
    adapter::Adapter,
    header::WorkHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig},
};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateBitcoinConfig {
        config: BitcoinConfig,
    },
    UpdateCheckpointConfig {
        config: CheckpointConfig,
    },
    UpdateHeaderConfig {
        config: HeaderConfig,
    },
    AddWorkHeader {
        header: WorkHeader,
    },
    RelayDeposit {
        btc_tx: Adapter<Transaction>,
        btc_height: u32,
        btc_proof: Adapter<PartialMerkleTree>,
        btc_vout: u32,
        sigset_index: u32,
        dest: Dest,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(u32)]
    HeaderHeight {},
    #[returns(u64)]
    DepositFees { index: Option<u32> },
    #[returns(u64)]
    WithdrawalFees { address: String, index: Option<u32> },
    #[returns(crate::adapter::HashBinary<bitcoin::BlockHash>)]
    SidechainBlockHash {},
    #[returns(u64)]
    CheckpointByIndex { index: u32 },
}

#[cw_serde]
pub struct MigrateMsg {}
