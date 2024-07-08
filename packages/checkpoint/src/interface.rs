use bitcoin::Script;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary};
use cosmwasm_std::{Coin, Storage};
use cw_storage_plus::Map;
use serde::{Deserialize, Serialize};
// use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};

use crate::adapter::Adapter;
use crate::error::ContractResult;
use crate::signatory::ConsensusKey;

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

    pub fn to_output_script(
        store: &dyn Storage,
        dest: String,
        recovery_scripts: &Map<String, Adapter<Script>>,
    ) -> ContractResult<Option<Script>> {
        Ok(recovery_scripts
            .load(store, dest)
            .ok()
            .map(|script| script.clone().into_inner()))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Validator {
    pub pubkey: Vec<u8>,
    pub power: u64,
}
