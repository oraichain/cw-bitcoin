use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_schema::cw_serde;
use serde::{Deserialize, Serialize};

use crate::{adapter::Adapter, interface::Dest};

#[cw_serde]
pub struct InstantiateMsg {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecuteMsg {
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
pub struct QueryMsg {}

#[cw_serde]
pub struct MigrateMsg {}
