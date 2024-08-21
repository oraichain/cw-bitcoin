use bitcoin::{BlockHash, Transaction};
use cosmwasm_std::{Addr, Env, QuerierWrapper, Storage};
use std::str::FromStr;

use crate::{
    adapter::Adapter,
    app::{Bitcoin, ConsensusKey},
    checkpoint::{Checkpoint, CheckpointQueue, CheckpointStatus},
    error::{ContractError, ContractResult},
    header::HeaderQueue,
    interface::{BitcoinConfig, ChangeRates, CheckpointConfig, HeaderConfig, Xpub},
    recovery::{RecoveryTxs, SignedRecoveryTx},
    signatory::SignatorySet,
    state::{
        header_height, BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, HEADER_CONFIG, OUTPOINTS,
        SIGNERS, SIG_KEYS,
    },
};

pub fn query_bitcoin_config(store: &dyn Storage) -> ContractResult<BitcoinConfig> {
    let bitcoin_config = BITCOIN_CONFIG.load(store)?;
    Ok(bitcoin_config)
}

pub fn query_checkpoint_config(store: &dyn Storage) -> ContractResult<CheckpointConfig> {
    let checkpoint_config = CHECKPOINT_CONFIG.load(store)?;
    Ok(checkpoint_config)
}

pub fn query_header_config(store: &dyn Storage) -> ContractResult<HeaderConfig> {
    let header_config = HEADER_CONFIG.load(store)?;
    Ok(header_config)
}

pub fn query_signatory_key(store: &dyn Storage, addr: Addr) -> ContractResult<Option<Xpub>> {
    let consensus_key = SIGNERS.load(store, addr.as_str())?;
    let sig_keys = SIG_KEYS.load(store, &consensus_key);
    let result = match sig_keys {
        Ok(xpub) => Some(xpub),
        Err(_) => None,
    };
    Ok(result)
}

pub fn query_header_height(store: &dyn Storage) -> ContractResult<u32> {
    header_height(store)
}

pub fn query_deposit_fees(store: &dyn Storage, index: Option<u32>) -> ContractResult<u64> {
    let btc = Bitcoin::default();

    let checkpoint = btc.get_checkpoint(store, index)?;
    let input_vsize = checkpoint.sigset.est_witness_vsize() + 40;
    let deposit_fees = btc.calc_minimum_deposit_fees(store, input_vsize, checkpoint.fee_rate)?;
    Ok(deposit_fees)
}

pub fn query_withdrawal_fees(
    store: &dyn Storage,
    address: String,
    index: Option<u32>,
) -> ContractResult<u64> {
    let btc = Bitcoin::default();
    let checkpoint = btc.get_checkpoint(store, index)?;
    let btc_address = bitcoin::Address::from_str(address.as_str())
        .map_err(|err| ContractError::App(err.to_string()))?;
    let script = btc_address.script_pubkey();
    let withdrawal_fees =
        btc.calc_minimum_withdrawal_fees(store, script.len() as u64, checkpoint.fee_rate)?;
    Ok(withdrawal_fees)
}

pub fn query_checkpoint_fees(store: &dyn Storage, index: Option<u32>) -> ContractResult<u64> {
    let btc = Bitcoin::default();
    let building_index = BUILDING_INDEX.load(store)?;
    let checkpoint_fees = btc
        .checkpoints
        .calc_fee_checkpoint(store, index.unwrap_or(building_index), &[0])
        .unwrap();
    Ok(checkpoint_fees)
}

pub fn query_sidechain_block_hash(store: &dyn Storage) -> ContractResult<Adapter<BlockHash>> {
    // let header_config = HEADER_CONFIG.load(store)?;
    let headers = HeaderQueue::default();
    let hash = Adapter::new(headers.hash(store)?);
    Ok(hash)
}

pub fn query_checkpoint_by_index(store: &dyn Storage, index: u32) -> ContractResult<Checkpoint> {
    let checkpoints = CheckpointQueue::default();
    let checkpoint = checkpoints.get(store, index)?;
    Ok(checkpoint)
}

pub fn query_building_checkpoint(store: &dyn Storage) -> ContractResult<Checkpoint> {
    let building_index = query_building_index(store)?;
    let checkpoints = CheckpointQueue::default();
    let checkpoint = checkpoints.get(store, building_index)?;
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

pub fn query_checkpoint_tx(
    store: &dyn Storage,
    index: Option<u32>,
) -> ContractResult<Adapter<Transaction>> {
    let checkpoints = CheckpointQueue::default();
    let checkpoint = match index {
        Some(index) => checkpoints.get(store, index)?,
        None => checkpoints.get(store, checkpoints.index(store))?,
    };
    checkpoint.checkpoint_tx()
}

pub fn query_last_complete_tx(store: &dyn Storage) -> ContractResult<Adapter<Transaction>> {
    let checkpoints = CheckpointQueue::default();
    let last_complete_tx = checkpoints.last_completed_tx(store)?;
    Ok(last_complete_tx)
}

pub fn query_complete_checkpoint_txs(
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

pub fn query_signing_recovery_txs(
    _querier: QuerierWrapper,
    store: &dyn Storage,
    xpub: Xpub,
) -> ContractResult<Vec<([u8; 32], u32)>> {
    let recovery_txs = RecoveryTxs::default();
    recovery_txs.to_sign(store, &xpub)
}

pub fn query_comfirmed_index(store: &dyn Storage) -> ContractResult<Option<u32>> {
    let checkpoints = CheckpointQueue::default();
    let confirmed_index = checkpoints.confirmed_index(store);
    Ok(confirmed_index)
}

pub fn query_first_unconfirmed_index(store: &dyn Storage) -> ContractResult<Option<u32>> {
    let checkpoints: CheckpointQueue = CheckpointQueue::default();
    let first_unconfirmed_index = checkpoints.first_unconfirmed_index(store)?;
    Ok(first_unconfirmed_index)
}

pub fn query_building_index(store: &dyn Storage) -> ContractResult<u32> {
    let checkpoints = CheckpointQueue::default();
    let building_index = checkpoints.index(store);
    Ok(building_index)
}

pub fn query_completed_index(store: &dyn Storage) -> ContractResult<u32> {
    let checkpoints = CheckpointQueue::default();
    let completed_index = checkpoints.last_completed_index(store)?;
    Ok(completed_index)
}

pub fn query_process_outpoints(store: &dyn Storage, key: String) -> ContractResult<bool> {
    // get all key of oupoints map
    let process_outpoints = OUTPOINTS.has(store, &key);
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

pub fn query_signing_txs_at_checkpoint_index(
    store: &dyn Storage,
    xpub: Xpub,
    cp_index: u32,
) -> ContractResult<Vec<([u8; 32], u32)>> {
    let checkpoints = CheckpointQueue::default();
    let checkpoint = checkpoints.get(store, cp_index)?;
    if checkpoint.status != CheckpointStatus::Signing {
        return Err(ContractError::App("checkpoint is not signing".to_string()));
    }
    checkpoint.to_sign(&xpub)
}

pub fn query_change_rates(
    store: &dyn Storage,
    env: Env,
    interval: u64,
) -> ContractResult<ChangeRates> {
    let now = env.block.time;
    let btc = Bitcoin::default();
    let change_rates = btc.change_rates(store, interval, now.seconds())?;
    Ok(change_rates)
}
