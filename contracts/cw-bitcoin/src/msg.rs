use bitcoin::{util::merkleblock::PartialMerkleTree, Script, Transaction};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary};
use token_bindings::Metadata;

use crate::{
    adapter::{Adapter, WrappedBinary},
    app::ConsensusKey,
    checkpoint::Checkpoint,
    header::WrappedHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, Xpub},
    threshold_sig::Signature,
};

#[cw_serde]
pub struct InstantiateMsg {
    pub token_factory_addr: Addr,
    pub bridge_wasm_addr: Option<Addr>,
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
    RelayCheckpoint {
        btc_height: u32,
        btc_proof: Adapter<PartialMerkleTree>,
        cp_index: u32,
    },
    WithdrawToBitcoin {
        script_pubkey: Adapter<Script>,
    },
    SubmitCheckpointSignature {
        xpub: WrappedBinary<Xpub>,
        sigs: Vec<Signature>,
        checkpoint_index: u32,
        btc_height: u32,
    },
    SubmitRecoverySignature {
        xpub: WrappedBinary<Xpub>,
        sigs: Vec<Signature>,
    },
    SetSignatoryKey {
        xpub: WrappedBinary<Xpub>,
    },
    AddValidators {
        addrs: Vec<String>,
        voting_powers: Vec<u64>,
        consensus_keys: Vec<ConsensusKey>,
    },
    RegisterDenom {
        subdenom: String,
        metadata: Option<Metadata>,
    },
    ChangeBtcAdmin {
        new_admin: String,
    },
    TriggerBeginBlock {
        hash: Binary,
    },
}

#[cw_serde]
pub enum SudoMsg {
    ClockEndBlock { hash: Binary },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(BitcoinConfig)]
    BitcoinConfig {},
    #[returns(CheckpointConfig)]
    CheckpointConfig {},
    #[returns(HeaderConfig)]
    HeaderConfig {},
    #[returns(u32)]
    HeaderHeight {},
    #[returns(u64)]
    DepositFees { index: Option<u32> },
    #[returns(u64)]
    CheckpointFees { index: Option<u32> },
    #[returns(u64)]
    WithdrawalFees { address: String, index: Option<u32> },
    #[returns(Vec<Adapter<Transaction>>)]
    CompletedCheckpointTxs { limit: u32 },
    #[returns(Vec<Adapter<Transaction>>)]
    SignedRecoveryTxs {},
    #[returns(Adapter<Transaction>)]
    CheckpointTx { index: Option<u32> },
    #[returns(WrappedBinary<bitcoin::BlockHash>)]
    SidechainBlockHash {},
    #[returns(Checkpoint)]
    CheckpointByIndex { index: u32 },
    #[returns(Checkpoint)]
    BuildingCheckpoint {},
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningRecoveryTxs { xpub: WrappedBinary<Xpub> },
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningTxsAtCheckpointIndex {
        xpub: WrappedBinary<Xpub>,
        checkpoint_index: u32,
    },
    #[returns(bool)]
    ProcessedOutpoint { key: String },
    // Query index
    #[returns(Option<u32>)]
    ConfirmedIndex {},
    #[returns(u32)]
    BuildingIndex {},
    #[returns(u32)]
    CompletedIndex {},
    #[returns(Option<u32>)]
    UnhandledConfirmedIndex {},
    // End query index
}

#[cw_serde]
pub struct MigrateMsg {}
