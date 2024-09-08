use bitcoin::util::uint::Uint256;
use common_bitcoin::{adapter::Adapter, deque::DequeExtension, error::ContractResult};
use cosmwasm_std::Storage;
use cw_storage_plus::Item;

use crate::{header::WorkHeader, interface::HeaderConfig, msg::Config};

pub const CONFIG: Item<Config> = Item::new("config");
pub const HEADER_CONFIG: Item<HeaderConfig> = Item::new("header");
/// A queue of Bitcoin block headers, along with the total estimated amount of
/// work (measured in hashes) done in the headers included in the queue.
///
/// The header queue is used to validate headers as they are received from the
/// Bitcoin network, ensuring each header is associated with a valid
/// proof-of-work and that the chain of headers is valid.
///
/// The queue is able to reorg if a new chain of headers is received that
/// contains more work than the current chain, however it can not process reorgs
/// that are deeper than the length of the queue (the length will be at the
/// configured pruning level based on the `max_length` config parameter).
pub const HEADERS: DequeExtension<WorkHeader> = DequeExtension::new("headers");
/// Header current work
pub const CURRENT_WORK: Item<Adapter<Uint256>> = Item::new("current_work");

/// The height of the last header in the header queue.    
pub fn header_height(store: &dyn Storage) -> ContractResult<u32> {
    match HEADERS.back(store)? {
        Some(inner) => Ok(inner.height()),
        None => Ok(0),
    }
}
