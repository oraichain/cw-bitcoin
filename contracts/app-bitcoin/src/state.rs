use crate::{
    app::ConsensusKey,
    checkpoint::Checkpoint,
    constants::BTC_NATIVE_TOKEN_DENOM,
    interface::{BitcoinConfig, CheckpointConfig, Validator},
    msg::Config,
    recovery::RecoveryTx,
};
use common_bitcoin::{deque::DequeExtension, error::ContractResult, xpub::Xpub};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Order, Storage};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Ratio {
    pub nominator: u64,
    pub denominator: u64,
}

pub const CONFIG: Item<Config> = Item::new("config");

/// TODO: store in smart contract
pub const CHECKPOINT_CONFIG: Item<CheckpointConfig> = Item::new("checkpoint_config");
pub const BITCOIN_CONFIG: Item<BitcoinConfig> = Item::new("bitcoin_config");

/// Mapping validator ConsensusKey => (power, Address)
pub const VALIDATORS: Map<&ConsensusKey, (u64, String)> = Map::new("validators");

/// Mapping validator Address => ConsensusKey
pub const SIGNERS: Map<&str, ConsensusKey> = Map::new("signers");

// by_cons Map<ConsensusKey, Xpub>
pub const SIG_KEYS: Map<&ConsensusKey, Xpub> = Map::new("sig_keys");

/// The collection also includes an set of all signatory extended public keys,
/// which is used to prevent duplicate keys from being submitted.
/// xpubs Map<Xpub::encode(), ()>
pub const XPUBS: Map<&[u8], ()> = Map::new("xpubs");

pub const RECOVERY_TXS: DequeExtension<RecoveryTx> = DequeExtension::new("recovery_txs");

/// A queue of outpoints to expire, sorted by expiration timestamp.
pub const EXPIRATION_QUEUE: Map<(u64, &str), ()> = Map::new("expiration_queue");

/// A set of outpoints.
pub const OUTPOINTS: Map<&str, ()> = Map::new("outpoints");

pub const FEE_POOL: Item<i64> = Item::new("fee_pool");

pub const CHECKPOINTS: DequeExtension<Checkpoint> = DequeExtension::new("checkpoints");
/// Checkpoint building index
pub const BUILDING_INDEX: Item<u32> = Item::new("building_index");
/// Checkpoint confirmed index
pub const CONFIRMED_INDEX: Item<u32> = Item::new("confirmed_index");
/// Checkpoint unhandled confirmed index
pub const FIRST_UNHANDLED_CONFIRMED_INDEX: Item<u32> = Item::new("first_unhandled_confirmed_index");

/// Fee
pub const TOKEN_FEE_RATIO: Item<Ratio> = Item::new("token_fee_ratio");

/// End block hash mapping, this is just unique hash string
pub const BLOCK_HASHES: Map<&[u8], ()> = Map::new("block_hashes");

pub fn get_validators(store: &dyn Storage) -> ContractResult<Vec<Validator>> {
    VALIDATORS
        .range(store, None, None, Order::Ascending)
        .map(|item| {
            let (k, (power, _)) = item?;
            Ok(Validator { power, pubkey: k })
        })
        .collect()
}

pub fn get_full_btc_denom(token_factory_addr: &str) -> String {
    format!("factory/{}/{}", token_factory_addr, BTC_NATIVE_TOKEN_DENOM)
}
