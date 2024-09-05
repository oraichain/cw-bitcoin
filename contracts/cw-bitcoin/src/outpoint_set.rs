use std::str::FromStr;

use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::{Order, Storage};

use crate::state::{EXPIRATION_QUEUE, OUTPOINTS};
use common_bitcoin::error::{ContractError, ContractResult};

/// A collection to keep track of which deposit outpoints have already been
/// relayed, in order to ensure that we don't credit the same deposit more than
/// once.
///
/// Outpoints are stored in a set, and added to a queue with an expiration
/// timestamp so we can prune the set.
///
/// It is important for safety that outpoints can not expire from the set until
/// after they are no longer considered valid to relay, otherwise there is risk
/// of the network crediting a deposit twice. Care should be taken to configure
/// usage of this collection to set timestamps properly to ensure this does not
/// happen.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct OutpointSet {}

impl OutpointSet {
    /// Clear the set.
    pub fn reset(&mut self, store: &mut dyn Storage) {
        EXPIRATION_QUEUE.clear(store);
        OUTPOINTS.clear(store);
    }

    /// Check if the set contains an outpoint.
    pub fn contains(&self, store: &dyn Storage, outpoint: bitcoin::OutPoint) -> bool {
        OUTPOINTS.has(store, &outpoint.to_string())
    }

    /// Insert an outpoint into the set, to be pruned at the given expiration
    /// timestamp.
    pub fn insert(
        &mut self,
        store: &mut dyn Storage,
        outpoint: bitcoin::OutPoint,
        expiration: u64,
    ) -> ContractResult<()> {
        let outpoint_key = &outpoint.to_string();
        OUTPOINTS.save(store, outpoint_key, &())?;
        EXPIRATION_QUEUE.save(store, (expiration, outpoint_key), &())?;
        Ok(())
    }

    /// Remove expired outpoints from the set.
    pub fn remove_expired(&mut self, store: &mut dyn Storage, now: u64) -> ContractResult<()> {
        // TODO: use drain iterator to eliminate need to collect into vec
        let mut expired = vec![];
        for entry in EXPIRATION_QUEUE.keys(store, None, None, Order::Ascending) {
            let (expiration, outpoint_str) = entry?;
            if expiration > now {
                break;
            }
            let outpoint =
                bitcoin::OutPoint::from_str(&outpoint_str).map_err(ContractError::ParseOutPoint)?;
            expired.push((expiration, outpoint));
        }

        for (expiration, outpoint) in expired {
            let outpoint_key = &outpoint.to_string();
            OUTPOINTS.remove(store, outpoint_key);
            EXPIRATION_QUEUE.remove(store, (expiration, outpoint_key));
        }

        Ok(())
    }
}
