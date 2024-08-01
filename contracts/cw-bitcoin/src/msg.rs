use bitcoin::{util::merkleblock::PartialMerkleTree, Script, Transaction};
use common::{adapter::Adapter, interface::Xpub};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary};
use token_bindings::Metadata;

use crate::{
    app::ConsensusKey,
    header::WrappedHeader,
    interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig},
    threshold_sig::Signature,
};
use common::adapter::HashBinary;

#[cw_serde]
pub struct InstantiateMsg {
    pub token_factory_addr: Addr,
    pub bitcoin_lib_addr: Addr,
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
    WithdrawToBitcoin {
        script_pubkey: Adapter<Script>,
    },
    SubmitCheckpointSignature {
        xpub: HashBinary<Xpub>,
        sigs: Vec<Signature>,
        checkpoint_index: u32,
        btc_height: u32,
    },
    SubmitRecoverySignature {
        xpub: HashBinary<Xpub>,
        sigs: Vec<Signature>,
    },
    SetSignatoryKey {
        xpub: HashBinary<Xpub>,
    },
    AddValidators {
        addrs: Vec<String>,
        infos: Vec<(u64, ConsensusKey)>,
    },
    RegisterDenom {
        subdenom: String,
        metadata: Option<Metadata>,
    },
    SetRecoveryScript {
        signatory_script: Adapter<Script>,
    },
}

#[cw_serde]
pub enum SudoMsg {
    ClockEndBlock { hash: Binary },
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
    #[returns(HashBinary<bitcoin::BlockHash>)]
    SidechainBlockHash {},
    #[returns(u64)]
    CheckpointByIndex { index: u32 },
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningRecoveryTxs { xpub: HashBinary<Xpub> },
    #[returns(Vec<([u8; 32], u32)>)] // Fix: Added closing angle bracket
    SigningTxsAtCheckpointIndex {
        xpub: HashBinary<Xpub>,
        checkpoint_index: u32,
    },
}

#[cw_serde]
pub struct MigrateMsg {}
