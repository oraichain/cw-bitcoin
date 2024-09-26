use bitcoin::{util::merkleblock::PartialMerkleTree, BlockHash, Transaction};
use common_bitcoin::{
    adapter::{Adapter, WrappedBinary},
    error::{ContractError, ContractResult},
};
use cosmwasm_std::Storage;

use crate::{
    header::HeaderQueue,
    state::{header_height, HEADER_CONFIG},
};
use light_client_bitcoin::interface::HeaderConfig;

pub fn query_header_config(store: &dyn Storage) -> ContractResult<HeaderConfig> {
    let header_config = HEADER_CONFIG.load(store)?;
    Ok(header_config)
}

pub fn query_network() -> ContractResult<String> {
    let header_queue = HeaderQueue::default();
    Ok(header_queue.network().to_string())
}

pub fn query_header_height(store: &dyn Storage) -> ContractResult<u32> {
    header_height(store)
}

pub fn query_sidechain_block_hash(store: &dyn Storage) -> ContractResult<WrappedBinary<BlockHash>> {
    let headers = HeaderQueue::default();
    let hash = WrappedBinary(headers.hash(store)?);
    Ok(hash)
}

pub fn query_verify_tx_with_proof(
    store: &dyn Storage,
    btc_tx: Adapter<Transaction>,
    btc_height: u32,
    btc_proof: Adapter<PartialMerkleTree>,
) -> ContractResult<()> {
    let header_queue = HeaderQueue::default();
    let btc_header = header_queue
        .get_by_height(store, btc_height, None)?
        .ok_or_else(|| ContractError::App("Invalid bitcoin block height".to_string()))?;
    let mut txids = vec![];
    let mut block_indexes = vec![];
    let proof_merkle_root = btc_proof
        .extract_matches(&mut txids, &mut block_indexes)
        .map_err(|_| ContractError::BitcoinMerkleBlockError)?;
    if proof_merkle_root != btc_header.merkle_root() {
        return Err(ContractError::App(
            "Bitcoin merkle proof does not match header".to_string(),
        ))?;
    }
    if txids.len() != 1 {
        return Err(ContractError::App(
            "Bitcoin merkle proof contains an invalid number of txids".to_string(),
        ))?;
    }
    if txids[0] != btc_tx.txid() {
        return Err(ContractError::App(
            "Bitcoin merkle proof does not match transaction".to_string(),
        ))?;
    }
    Ok(())
}
