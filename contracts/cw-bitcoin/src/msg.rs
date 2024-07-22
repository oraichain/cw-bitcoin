use bitcoin::{util::{bip32::ExtendedPubKey, merkleblock::PartialMerkleTree}, Transaction};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary};

use crate::{
    adapter::Adapter,
    header::WrappedHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, Xpub},
};

#[cw_serde]
pub struct InstantiateMsg {
    pub token_factory_addr: Addr,
}

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
    RelayHeaders {
        headers: Vec<WrappedHeader>,
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
pub enum SudoMsg {
    ClockBeginBlock { hash: Binary },
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
