use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cosmwasm_std::Coin;
use serde::{Deserialize, Serialize};
// use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ContractResult;

// pub trait DequeExtension<'a, T: Serialize + DeserializeOwned> {
//     fn retain_unordered<F>(&self, store: &mut dyn Storage, f: F) -> StdResult<u64>
//     where
//         F: FnMut(&T) -> bool;
// }

// impl<'a, T: Serialize + DeserializeOwned> DequeExtension<'a, T> for Deque<'a, T> {
//     fn retain_unordered<F>(&self, store: &mut dyn Storage, mut f: F) -> StdResult<u64>
//     where
//         F: FnMut(&T) -> bool,
//     {
//         let mut temp = vec![];
//         while let Some(item) = self.pop_front(store)? {
//             temp.push(item);
//         }
//         let mut size = 0;
//         for item in temp {
//             if f(&item) {
//                 self.push_back(store, &item)?;
//                 size += 1;
//             }
//         }

//         Ok(size)
//     }
// }

#[cw_serde]
pub struct Accounts {
    transfers_allowed: bool,
    transfer_exceptions: Vec<String>,
    accounts: Vec<(String, Coin)>,
}

impl Accounts {
    pub fn balance(&self, address: String) -> Option<Coin> {
        self.accounts
            .iter()
            .find(|item| item.0 == address)
            .map(|item| item.1.clone())
    }
}

#[cw_serde]
pub struct IbcDest {
    pub source_port: String,
    pub source_channel: String,
    #[serde(skip)]
    pub receiver: String,
    #[serde(skip)]
    pub sender: String,
    pub timeout_timestamp: u64,
    pub memo: String,
}

#[cw_serde]
pub enum Dest {
    Address(Addr),
    Ibc(IbcDest),
}

impl Dest {
    pub fn to_receiver_addr(&self) -> String {
        match self {
            Self::Address(addr) => addr.to_string(),
            Self::Ibc(dest) => dest.receiver.to_string(),
        }
    }

    pub fn commitment_bytes(&self) -> ContractResult<Vec<u8>> {
        let bytes = match self {
            Self::Address(addr) => addr.as_bytes().into(),
            Self::Ibc(dest) => Sha256::digest(dest.receiver.as_bytes()).to_vec(),
        };

        Ok(bytes)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Validator {
    pub pubkey: Vec<u8>,
    pub power: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BitcoinConfig {
    /// The minimum number of checkpoints that must be produced before
    /// withdrawals are enabled.
    pub min_withdrawal_checkpoints: u32,
    /// The minimum amount of BTC a deposit must send to be honored, in
    /// satoshis.
    pub min_deposit_amount: u64,
    /// The minimum amount of BTC a withdrawal must withdraw, in satoshis.
    pub min_withdrawal_amount: u64,
    /// TODO: remove this, not used
    pub max_withdrawal_amount: u64,
    /// The maximum length of a withdrawal output script, in bytes.
    pub max_withdrawal_script_length: u64,
    /// The fee charged for an nBTC transfer, in micro-satoshis.
    pub transfer_fee: u64,
    /// The minimum number of confirmations a Bitcoin block must have before it
    /// is considered finalized. Note that in the current implementation, the
    /// actual number of confirmations required is `min_confirmations + 1`.
    pub min_confirmations: u32,
    /// The number which amounts in satoshis are multiplied by to get the number
    /// of units held in nBTC accounts. In other words, the amount of
    /// subdivisions of satoshis which nBTC accounting uses.
    pub units_per_sat: u64,

    // (These fields were moved to `checkpoint::Config`)
    pub emergency_disbursal_min_tx_amt: u64,

    pub emergency_disbursal_lock_time_interval: u32,

    pub emergency_disbursal_max_tx_size: u64,

    /// If a signer does not submit signatures for this many consecutive
    /// checkpoints, they are considered offline and are removed from the
    /// signatory set (jailed) and slashed.    
    pub max_offline_checkpoints: u32,
    /// The minimum number of confirmations a checkpoint must have on the
    /// Bitcoin network before it is considered confirmed. Note that in the
    /// current implementation, the actual number of confirmations required is
    /// `min_checkpoint_confirmations + 1`.    
    pub min_checkpoint_confirmations: u32,
    /// The maximum amount of BTC that can be held in the network, in satoshis.    
    pub capacity_limit: u64,

    pub max_deposit_age: u64,

    pub fee_pool_target_balance: u64,

    pub fee_pool_reward_split: (u64, u64),
}
