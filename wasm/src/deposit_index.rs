use crate::error::ContractResult;
use bitcoin::{hashes::Hash, Address, Txid};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr};
use tsify::Tsify;

#[derive(Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Deposit {
    txid: Txid,
    vout: u32,
    amount: u64,
    height: Option<u64>,
}

impl Deposit {
    pub fn new(txid: Txid, vout: u32, amount: u64, height: Option<u64>) -> Self {
        Self {
            txid,
            vout,
            amount,
            height,
        }
    }

    pub fn key(txid: Txid, vout: u32) -> String {
        let mut key = txid.to_vec();
        key.extend_from_slice(&vout.to_be_bytes());
        base64::encode(key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct DepositInfo {
    pub deposit: Deposit,
    pub confirmations: u64,
}

type ReceiverIndex = HashMap<String, HashMap<String, HashMap<String, Deposit>>>;

#[derive(Debug, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct DepositIndex {
    #[tsify(type = "{ [key: string]: { [key: string]: { [key: string]: Deposit } } }")]
    pub receiver_index: ReceiverIndex,
}

impl DepositIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_deposit(
        &mut self,
        receiver: String,
        address: bitcoin::Address,
        deposit: Deposit,
    ) {
        self.receiver_index
            .entry(receiver)
            .or_default()
            .entry(address.to_string())
            .or_default()
            .insert(Deposit::key(deposit.txid, deposit.vout), deposit);
    }

    fn remove_address_index_deposit(
        &mut self,
        receiver: String,
        address: bitcoin::Address,
        txid: Txid,
        vout: u32,
    ) -> ContractResult<()> {
        self.receiver_index
            .get_mut(&receiver)
            .unwrap_or(&mut HashMap::new())
            .get_mut(&address.to_string())
            .unwrap_or(&mut HashMap::new())
            .remove(&Deposit::key(txid, vout));

        Ok(())
    }

    pub fn remove_deposit(
        &mut self,
        receiver: String,
        address: bitcoin::Address,
        txid: Txid,
        vout: u32,
    ) -> ContractResult<()> {
        self.remove_address_index_deposit(receiver, address, txid, vout)?;
        Ok(())
    }

    pub fn get_deposits_by_receiver(
        &self,
        receiver: String,
        current_btc_height: u64,
    ) -> ContractResult<Vec<DepositInfo>> {
        let mut deposits = Vec::new();
        if let Some(address_map) = self.receiver_index.get(&receiver) {
            for address in address_map.values() {
                for (_, deposit) in address.iter() {
                    let confirmations = match deposit.height {
                        Some(height) => current_btc_height.saturating_sub(height) + 1,
                        None => 0,
                    };

                    deposits.push(DepositInfo {
                        deposit: deposit.clone(),
                        confirmations,
                    });
                }
            }
        }

        Ok(deposits)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn newDepositIndex() -> DepositIndex {
    let mut deposit = DepositIndex::default();
    deposit.insert_deposit(
        "thanhtu".to_string(),
        Address::from_str("bc1q7vdh8ttns5k2u5acel8yddjw0shens4zmyun4n06gd5mjeq3y4kq9lhw09")
            .unwrap(),
        Deposit::new(Txid::from_slice(&[0; 32]).unwrap(), 1, 10, Some(1000)),
    );

    deposit
}
