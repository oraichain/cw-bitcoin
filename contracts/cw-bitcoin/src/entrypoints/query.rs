use bitcoin::{BlockHash, Transaction};
use cosmwasm_std::{Order, Storage};
use std::str::FromStr;

use crate::{
    adapter::{Adapter, HashBinary},
    app::{Bitcoin, ConsensusKey},
    checkpoint::{BuildingCheckpoint, Checkpoint, CheckpointQueue},
    error::{ContractError, ContractResult},
    header::HeaderQueue,
    recovery::{RecoveryTxs, SignedRecoveryTx},
    signatory::SignatorySet,
    state::{header_height, HEADER_CONFIG, OUTPOINTS, SIG_KEYS},
};
use common::interface::Xpub;

pub fn query_header_height(store: &dyn Storage) -> ContractResult<u32> {
    header_height(store)
}

pub fn query_deposit_fees(store: &dyn Storage, index: Option<u32>) -> ContractResult<u64> {
    let header_config = HEADER_CONFIG.load(store)?;
    let btc = Bitcoin::new(header_config);
    let checkpoint = btc.get_checkpoint(store, index)?;
    let input_vsize = checkpoint.sigset.est_witness_vsize() + 40;
    let deposit_fees = btc.calc_minimum_deposit_fees(input_vsize, checkpoint.fee_rate);
    Ok(deposit_fees)
}

pub fn query_withdrawal_fees(
    store: &dyn Storage,
    address: String,
    index: Option<u32>,
) -> ContractResult<u64> {
    let header_config = HEADER_CONFIG.load(store)?;
    let btc = Bitcoin::new(header_config);
    let checkpoint = btc.get_checkpoint(store, index)?;
    let btc_address = bitcoin::Address::from_str(address.as_str())
        .map_err(|err| ContractError::App(err.to_string()))?;
    let script = btc_address.script_pubkey();
    let withdrawal_fees =
        btc.calc_minimum_withdrawal_fees(script.len() as u64, checkpoint.fee_rate);
    Ok(withdrawal_fees)
}

pub fn query_sidechain_block_hash(store: &dyn Storage) -> ContractResult<HashBinary<BlockHash>> {
    let header_config = HEADER_CONFIG.load(store)?;
    let headers = HeaderQueue::new(header_config);
    let hash = HashBinary(headers.hash(store)?);
    Ok(hash)
}

pub fn query_checkpoint_by_index(store: &dyn Storage, index: u32) -> ContractResult<Checkpoint> {
    let checkpoints = CheckpointQueue::default();
    let checkpoint = checkpoints.get(store, index)?;
    Ok(checkpoint)
}

pub fn query_building_checkpoint(store: &dyn Storage) -> ContractResult<BuildingCheckpoint> {
    let checkpoints = CheckpointQueue::default();
    let checkpoint = checkpoints.building(store)?;
    Ok(checkpoint)
}

pub fn query_est_witness_vsize(store: &dyn Storage) -> ContractResult<u64> {
    let checkpoints = CheckpointQueue::default();
    let est_witness_vsize = checkpoints.active_sigset(store)?.est_witness_vsize();
    Ok(est_witness_vsize)
}

pub fn query_active_sigset(store: &dyn Storage) -> ContractResult<SignatorySet> {
    let checkpoints = CheckpointQueue::default();
    let active_sigset = checkpoints.active_sigset(store)?;
    Ok(active_sigset)
}

pub fn query_last_complete_tx(store: &dyn Storage) -> ContractResult<Adapter<Transaction>> {
    let checkpoints = CheckpointQueue::default();
    let last_complete_tx = checkpoints.last_completed_tx(store)?;
    Ok(last_complete_tx)
}

pub fn query_complete_txs(
    store: &dyn Storage,
    limit: u32,
) -> ContractResult<Vec<Adapter<Transaction>>> {
    let checkpoints = CheckpointQueue::default();
    let complete_txs = checkpoints.completed_txs(store, limit)?;
    Ok(complete_txs)
}

pub fn query_signed_recovery_txs(store: &dyn Storage) -> ContractResult<Vec<SignedRecoveryTx>> {
    let recovery_txs = RecoveryTxs::default();
    let signed_recovery_txs = recovery_txs.signed(store)?;
    Ok(signed_recovery_txs)
}

pub fn query_last_complete_index(store: &dyn Storage) -> ContractResult<u32> {
    let checkpoints = CheckpointQueue::default();
    let last_complete_index = checkpoints.last_completed_index(store)?;
    Ok(last_complete_index)
}

pub fn query_comfirmed_index(store: &dyn Storage) -> ContractResult<u32> {
    let checkpoints = CheckpointQueue::default();
    let has_signing = checkpoints.signing(store)?.is_some();
    let signing_offset = has_signing as u32;
    let confirmed_index = match checkpoints.confirmed_index {
        None => return Ok(checkpoints.len(store)? - 1 - signing_offset),
        Some(index) => index,
    };
    Ok(confirmed_index)
}

pub fn query_first_unconfirmed_index(store: &dyn Storage) -> ContractResult<Option<u32>> {
    let checkpoints = CheckpointQueue::default();
    let first_unconfirmed_index = checkpoints.first_unconfirmed_index(store)?;
    Ok(first_unconfirmed_index)
}

pub fn query_process_outpoints(store: &dyn Storage) -> ContractResult<Vec<String>> {
    // get all key of oupoints map
    let process_outpoints = OUTPOINTS
        .range(store, None, None, Order::Ascending)
        .map(|item| {
            let (k, _) = item?;
            Ok(k.to_string())
        })
        .collect::<ContractResult<Vec<String>>>()?;
    Ok(process_outpoints)
}

pub fn query_signatory_keys(
    store: &dyn Storage,
    cons_key: ConsensusKey,
) -> ContractResult<Option<Xpub>> {
    let signatory_keys = SIG_KEYS.may_load(store, &cons_key)?;
    Ok(signatory_keys)
}

pub fn query_checkpoint_len(store: &dyn Storage) -> ContractResult<u32> {
    let checkpoints = CheckpointQueue::default();
    let len = checkpoints.len(store)?;
    Ok(len)
}
