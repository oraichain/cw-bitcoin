use bitcoin::{util::merkleblock::PartialMerkleTree, Script, Transaction};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Uint128};
use oraiswap::asset::AssetInfo;
use token_bindings::Metadata;

use crate::{
    adapter::Adapter,
    app::ConsensusKey,
    header::WrappedHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, Xpub},
    state::Ratio,
    threshold_sig::Signature,
};

#[cw_serde]
pub struct FeeData {
    pub deducted_amount: Uint128,
    pub token_fee: Coin,
    pub relayer_fee: Coin,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub token_factory_addr: Addr,
    pub relayer_fee_token: AssetInfo,
    pub relayer_fee: Uint128, // This fee depends on the network type, not token type decimals of relayer fee should always be 10^6
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub swap_router_contract: Option<Addr>,
    pub osor_entry_point_contract: Option<Addr>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        relayer_fee_token: Option<AssetInfo>,
        token_fee_receiver: Option<Addr>,
        relayer_fee_receiver: Option<Addr>,
        relayer_fee: Option<Uint128>,
        swap_router_contract: Option<Addr>,
        token_fee: Option<Ratio>,
        token_factory_addr: Option<Addr>,
        owner: Option<Addr>,
    },
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
        xpub: Xpub,
        sigs: Vec<Signature>,
        checkpoint_index: u32,
        btc_height: u32,
    },
    SubmitRecoverySignature {
        xpub: Xpub,
        sigs: Vec<Signature>,
    },
    SetSignatoryKey {
        xpub: Xpub,
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
    #[returns(Option<Xpub>)]
    SignatoryKey { addr: Addr },
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
    #[returns(Adapter<bitcoin::BlockHash>)]
    SidechainBlockHash {},
    #[returns(crate::checkpoint::Checkpoint)]
    CheckpointByIndex { index: u32 },
    #[returns(crate::checkpoint::Checkpoint)]
    BuildingCheckpoint {},
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningRecoveryTxs { xpub: Xpub },
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningTxsAtCheckpointIndex { xpub: Xpub, checkpoint_index: u32 },
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
    #[returns(crate::interface::ChangeRates)]
    ChangeRates { interval: u64 },
}

#[cw_serde]
pub struct MigrateMsg {}
