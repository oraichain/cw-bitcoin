use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Uint128};
use oraiswap::asset::AssetInfo;
use token_bindings::Metadata;

use crate::{
    app::ConsensusKey,
    interface::{BitcoinConfig, CheckpointConfig, Dest},
    state::Ratio,
    threshold_sig::Signature,
};
use common_bitcoin::adapter::{Adapter, WrappedBinary};
use common_bitcoin::xpub::Xpub;

#[cw_serde]
pub struct FeeData {
    pub deducted_amount: Uint128,
    pub token_fee: Coin,
    pub relayer_fee: Coin,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub relayer_fee_token: AssetInfo,
    pub relayer_fee: Uint128, // This fee depends on the network type, not token type decimals of relayer fee should always be 10^6
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub token_factory_contract: Addr,
    pub light_client_contract: Addr,
    pub swap_router_contract: Option<Addr>,
    pub osor_entry_point_contract: Option<Addr>,
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub relayer_fee_token: AssetInfo,
    pub relayer_fee: Uint128, // This fee depends on the network type, not token type decimals of relayer fee should always be 10^6
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub token_factory_contract: Addr,
    pub light_client_contract: Addr,
    pub swap_router_contract: Option<Addr>,
    pub osor_entry_point_contract: Option<Addr>,
}

#[cw_serde]
pub struct ConfigResponse {
    pub owner: Addr,
    pub relayer_fee_token: AssetInfo,
    pub token_fee: Ratio,
    pub relayer_fee: Uint128, // This fee depends on the network type, not token type decimals of relayer fee should always be 10^6
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub token_factory_contract: Addr,
    pub light_client_contract: Addr,
    pub swap_router_contract: Option<Addr>,
    pub osor_entry_point_contract: Option<Addr>,
}

#[cw_serde]
pub enum OsorMsg {
    UniversalSwap { memo: String },
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<Addr>,
        relayer_fee_token: Option<AssetInfo>,
        token_fee_receiver: Option<Addr>,
        relayer_fee_receiver: Option<Addr>,
        relayer_fee: Option<Uint128>,
        token_fee: Option<Ratio>,
        light_client_contract: Option<Addr>,
        swap_router_contract: Option<Addr>,
        token_factory_contract: Option<Addr>,
        osor_entry_point_contract: Option<Addr>,
    },
    UpdateBitcoinConfig {
        config: BitcoinConfig,
    },
    UpdateCheckpointConfig {
        config: CheckpointConfig,
    },
    #[cfg(feature = "native-validator")]
    RegisterValidator {},
    #[cfg(not(feature = "native-validator"))]
    AddValidators {
        addrs: Vec<String>,
        voting_powers: Vec<u64>,
        consensus_keys: Vec<ConsensusKey>,
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
        btc_address: String,
        fee: Option<u64>,
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
    RegisterDenom {
        subdenom: String,
        metadata: Option<Metadata>,
    },
    ChangeBtcDenomOwner {
        new_owner: String,
    },
    TriggerBeginBlock {
        hash: Binary,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(BitcoinConfig)]
    BitcoinConfig {},
    #[returns(CheckpointConfig)]
    CheckpointConfig {},
    #[returns(Option<WrappedBinary<Xpub>>)]
    SignatoryKey { addr: Addr },
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
    #[returns(crate::checkpoint::Checkpoint)]
    CheckpointByIndex { index: u32 },
    #[returns(crate::checkpoint::Checkpoint)]
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
    #[returns(crate::interface::ChangeRates)]
    ChangeRates { interval: u64 },
    #[returns(u64)]
    ValueLocked {},
    #[returns(bool)]
    CheckEligibleValidator { val_addr: String },
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub enum SudoMsg {
    ClockEndBlock { hash: Binary },
}
