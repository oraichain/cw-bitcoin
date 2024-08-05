use crate::adapter::Adapter;
use crate::checkpoint::Checkpoint;
use crate::constants::BTC_NATIVE_TOKEN_DENOM;
use crate::interface::{Accounts, BitcoinConfig, ChangeRates, Dest, Validator, Xpub};
use crate::signatory::SignatoryKeys;
use crate::state::{
    get_validators, BITCOIN_CONFIG, CONFIRMED_INDEX, FEE_POOL, FIRST_UNHANDLED_CONFIRMED_INDEX,
    RECOVERY_SCRIPTS, SIGNERS, SIG_KEYS,
};
use crate::threshold_sig;

use super::checkpoint::Input;
use super::recovery::{RecoveryTxInput, RecoveryTxs};
use super::threshold_sig::Signature;

use super::checkpoint::BatchType;
use super::checkpoint::CheckpointQueue;
use super::error::{ContractError, ContractResult};
use super::header::HeaderQueue;
use bitcoin::Script;
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::{Addr, Api, Coin, Env, Order, Storage, Uint128};

use super::outpoint_set::OutpointSet;
use super::signatory::SignatorySet;
use std::collections::HashMap;

pub const NETWORK: ::bitcoin::Network = ::bitcoin::Network::Bitcoin;

/// Calculates the bridge fee for a deposit of the given amount of BTC, in
/// satoshis.
pub fn calc_deposit_fee(_: Uint128) -> u64 {
    0
}

/// The main structure where Bitcoin bridge state is held.
///
/// This structure is the main entry point for interacting with the Bitcoin
/// bridge. It contains all of the state necessary to keep track of the Bitcoin
/// blockchain headers, relay deposit transactions, maintain nBTC accounts, and
/// coordinate the checkpointing process to manage the BTC reserve on the
/// Bitcoin blockchain.
#[derive(Debug, Deserialize, Serialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct Bitcoin {
    /// A light client of the Bitcoin blockchain, keeping track of the headers
    /// of the highest-work chain.    
    pub headers: HeaderQueue, // HEADERS

    /// The set of outpoints which have been relayed to the bridge. This is used
    /// to prevent replay attacks of deposits.
    pub processed_outpoints: OutpointSet, // OUTPOINT_SET

    /// The checkpoint queue, which manages the checkpointing process,
    /// periodically moving the reserve of BTC on the Bitcoin blockchain to
    /// collect incoming deposits, move the funds to the latest signatory set,
    /// and pay out requested withdrawals.    
    pub checkpoints: CheckpointQueue, // CHECKPOINTS

    /// TODO: remove this, for flow withdrawing money we don't need to use this when using factory module
    /// The map of nBTC accounts, which hold the nBTC balances of users.
    pub accounts: Accounts, // ?

    /// The public keys declared by signatories, which are used to sign Bitcoin
    /// transactions.
    // TODO: store recovery script data in account struct
    pub signatory_keys: SignatoryKeys, // ?

    /// A pool of BTC where bridge fees are collected.
    pub(crate) reward_pool: Coin, // ?

    // TODO: turn into Coin<Nbtc>
    // pub(crate) fee_pool: i64, // FEE_POOL
    /// The configuration parameters for the Bitcoin module.
    pub config: BitcoinConfig, // BITCOIN_CONFIG

    pub recovery_txs: RecoveryTxs, // ?
}

/// A Tendermint/CometBFT public key.
pub type ConsensusKey = [u8; 32];

impl Bitcoin {
    pub fn default() -> Self {
        Self {
            headers: HeaderQueue::default(),
            checkpoints: CheckpointQueue::default(),
            processed_outpoints: OutpointSet::default(),
            accounts: Accounts::default(),
            signatory_keys: SignatoryKeys::default(),
            reward_pool: Coin::default(),
            // fee_pool: 0,
            config: BitcoinConfig::default(),
            recovery_txs: RecoveryTxs::default(),
        }
    }

    pub fn fee_pool(&self, store: &dyn Storage) -> ContractResult<i64> {
        Ok(FEE_POOL.load(store).unwrap_or_default())
    }

    pub fn get_checkpoint(
        &self,
        store: &dyn Storage,
        index: Option<u32>,
    ) -> ContractResult<Checkpoint> {
        let checkpoint = match index {
            Some(index) => self.checkpoints.get(store, index)?,
            None => self.checkpoints.get(store, self.checkpoints.index(store))?, // get current checkpoint being built
        };

        Ok(checkpoint)
    }

    /// Sets the configuration parameters to the given values.
    pub fn configure(
        &mut self,
        store: &mut dyn Storage,
        config: BitcoinConfig,
    ) -> ContractResult<()> {
        BITCOIN_CONFIG.save(store, &config)?;
        Ok(())
    }

    /// Gets the configuration parameters.
    pub fn config(&self, store: &dyn Storage) -> ContractResult<BitcoinConfig> {
        let config = BITCOIN_CONFIG.load(store)?;
        Ok(config)
    }

    /// Called by validators to store their signatory public key, which will be
    /// used for their signing of Bitcoin transactions.
    ///
    /// This call must be signed by an operator key associated with an account
    /// which has declared a validator.    
    pub fn set_signatory_key(
        &mut self,
        store: &mut dyn Storage,
        signer: Addr,
        signatory_key: Xpub,
    ) -> ContractResult<()> {
        let consensus_key = SIGNERS
            .load(store, signer.as_str())
            .map_err(|_| ContractError::App("Signer does not have a consensus key".to_string()))?;

        if signatory_key.network != self.network() {
            return Err(ContractError::App(
                "Signatory key network does not match network".to_string(),
            ));
        }

        self.signatory_keys
            .insert(store, consensus_key, signatory_key)?;

        Ok(())
    }

    /// Called by users to set their recovery script, which is their desired
    /// destination paid out to in the emergency disbursal process if the the
    /// account has sufficient balance.    
    pub fn set_recovery_script(
        &mut self,
        store: &mut dyn Storage,
        signer: Addr,
        signatory_script: Adapter<Script>,
    ) -> ContractResult<()> {
        let config = self.config(store)?;
        if signatory_script.len() as u64 > config.max_withdrawal_script_length {
            return Err(ContractError::App(
                "Script exceeds maximum length".to_string(),
            ));
        }

        RECOVERY_SCRIPTS.save(store, signer.as_str(), &signatory_script)?;

        Ok(())
    }

    /// Returns `true` if the next call to `self.checkpoints.maybe_step()` will
    /// push a new checkpoint (along with advancing the current `Building`
    /// checkpoint to `Signing`). Returns `false` otherwise.    
    pub fn should_push_checkpoint(
        &mut self,
        env: Env,
        store: &dyn Storage,
    ) -> ContractResult<bool> {
        self.checkpoints
            .should_push(env, store, &[0; 32], self.headers.height(store)?)
        // TODO: we shouldn't need this slice, commitment should be fixed-length
    }

    pub fn calc_minimum_deposit_fees(
        &self,
        store: &dyn Storage,
        input_vsize: u64,
        fee_rate: u64,
    ) -> ContractResult<u64> {
        let config = self.config(store)?;
        Ok(
            input_vsize * fee_rate * self.checkpoints.config(store).user_fee_factor / 10_000
                * config.units_per_sat,
        )
    }

    pub fn calc_minimum_withdrawal_fees(
        &self,
        store: &dyn Storage,
        script_pubkey_length: u64,
        fee_rate: u64,
    ) -> ContractResult<u64> {
        let config = self.config(store)?;
        Ok(
            (9 + script_pubkey_length) * fee_rate * self.checkpoints.config(store).user_fee_factor
                / 10_000
                * config.units_per_sat,
        )
    }

    /// Verifies and processes a deposit of BTC into the reserve.   
    ///
    /// This will check that the Bitcoin transaction has been sufficiently
    /// confirmed on the Bitcoin blockchain, then will add the deposit to the
    /// current `Building` checkpoint to be spent as an input. The deposit's
    /// committed destination will be credited once the checkpoint is fully
    /// signed.
    pub fn relay_deposit(
        &mut self,
        env: Env,
        store: &mut dyn Storage,
        btc_tx: Adapter<Transaction>,
        btc_height: u32,
        btc_proof: Adapter<PartialMerkleTree>,
        btc_vout: u32,
        sigset_index: u32,
        dest: Dest,
    ) -> ContractResult<()> {
        let config = self.config(store)?;
        let now = env.block.time.seconds();

        let btc_header = self
            .headers
            .get_by_height(store, btc_height)?
            .ok_or_else(|| ContractError::App("Invalid bitcoin block height".to_string()))?;

        if self.headers.height(store)? - btc_height < config.min_confirmations {
            return Err(ContractError::App(
                "Block is not sufficiently confirmed".to_string(),
            ));
        }

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

        if btc_vout as usize >= btc_tx.output.len() {
            return Err(ContractError::App(
                "Output index is out of bounds".to_string(),
            ))?;
        }
        let output = &btc_tx.output[btc_vout as usize];

        // if output.value < self.config.min_deposit_amount {
        //     return Err(ContractError::App(
        //         "Deposit amount is below minimum".to_string(),
        //     ))?;
        // }

        let checkpoint = self.checkpoints.get(store, sigset_index)?;
        let sigset = checkpoint.sigset.clone();

        let dest_bytes = dest.commitment_bytes()?;
        let expected_script =
            sigset.output_script(&dest_bytes, self.checkpoints.config(store).sigset_threshold)?;
        if output.script_pubkey != expected_script {
            return Err(ContractError::App(
                "Output script does not match signature set".to_string(),
            ))?;
        }
        let outpoint = bitcoin::OutPoint::new(btc_tx.txid(), btc_vout);
        if self.processed_outpoints.contains(store, outpoint) {
            return Err(ContractError::App(
                "Output has already been relayed".to_string(),
            ))?;
        }
        let deposit_timeout = sigset.create_time() + config.max_deposit_age;
        self.processed_outpoints
            .insert(store, outpoint, deposit_timeout)?;

        if !checkpoint.deposits_enabled {
            return Err(ContractError::App(
                "Deposits are disabled for the given checkpoint".to_string(),
            ))?;
        }

        if now > deposit_timeout {
            let checkpoint = self.checkpoints.building(store)?;
            let checkpoint_config = self.checkpoints.config(store);
            self.recovery_txs.create_recovery_tx(
                store,
                RecoveryTxInput {
                    expired_tx: btc_tx.into_inner(),
                    vout: btc_vout,
                    old_sigset: &sigset,
                    new_sigset: &checkpoint.sigset,
                    dest,
                    fee_rate: checkpoint.fee_rate,
                    //TODO: Hold checkpoint config on state
                    threshold: checkpoint_config.sigset_threshold,
                },
            )?;

            return Ok(());
        }

        let prevout = bitcoin::OutPoint {
            txid: btc_tx.txid(),
            vout: btc_vout,
        };
        let input = Input::new(
            prevout,
            &sigset,
            &dest_bytes,
            output.value,
            self.checkpoints.config(store).sigset_threshold,
        )?;
        let input_size = input.est_vsize();

        // note: we only mint nbtc when it is send to destination
        let mint_amount = (output.value * config.units_per_sat).into();
        let mut nbtc = Coin {
            denom: BTC_NATIVE_TOKEN_DENOM.to_string(),
            amount: mint_amount,
        };
        let fee_amount = self.calc_minimum_deposit_fees(store, input_size, checkpoint.fee_rate)?;
        let deposit_fees = calc_deposit_fee(nbtc.amount);
        let fee = (fee_amount + deposit_fees).into();
        nbtc.amount = nbtc.amount.checked_sub(fee).map_err(|_| {
            ContractError::App("Deposit amount is too small to pay its spending fee".to_string())
        })?;
        #[cfg(debug_assertions)]
        println!(
            "Relay deposit with output value: {}, input size: {}, checkpoint fee rate: {}",
            output.value, input_size, checkpoint.fee_rate
        );

        self.give_miner_fee(store, fee)?;
        // TODO: record as excess collected if inputs are full

        let mut building_mut = self.checkpoints.building(store)?;
        let building_checkpoint_batch = &mut building_mut.batches[BatchType::Checkpoint];
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.input.push(input);
        // TODO: keep in excess queue if full

        // let deposit_fee = nbtc.take(calc_deposit_fee(nbtc.amount.into()))?;
        // self.give_rewards(deposit_fee)?;

        self.checkpoints
            .building(store)?
            .insert_pending(dest, nbtc)?;

        let index = self.checkpoints.index(store);
        self.checkpoints.set(store, index, &building_mut)?;

        Ok(())
    }

    /// Records proof that a checkpoint produced by the network has been
    /// confirmed into a Bitcoin block.    
    pub fn relay_checkpoint(
        &mut self,
        store: &mut dyn Storage,
        btc_height: u32,
        btc_proof: Adapter<PartialMerkleTree>,
        cp_index: u32,
    ) -> ContractResult<()> {
        let config = self.config(store)?;
        if let Some(conf_index) = self.checkpoints.confirmed_index(store) {
            if cp_index <= conf_index {
                return Err(ContractError::App(
                    "Checkpoint has already been relayed".to_string(),
                ))?;
            }
        }

        let btc_header = self
            .headers
            .get_by_height(store, btc_height)?
            .ok_or_else(|| ContractError::App("Invalid bitcoin block height".to_string()))?;

        if self.headers.height(store)? - btc_height < config.min_checkpoint_confirmations {
            return Err(ContractError::App(
                "Block is not sufficiently confirmed".to_string(),
            ));
        }

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

        let btc_tx = self.checkpoints.get(store, cp_index)?.checkpoint_tx()?;
        if txids[0] != btc_tx.txid() {
            return Err(ContractError::App(
                "Bitcoin merkle proof does not match transaction".to_string(),
            ))?;
        }

        CONFIRMED_INDEX.save(store, &cp_index)?;
        println!(
            "Checkpoint {} confirmed at Bitcoin height {}",
            cp_index, btc_height
        );

        Ok(())
    }

    /// Initiates a withdrawal, adding an output to the current `Building`
    /// checkpoint to be paid out once the checkpoint is fully signed.
    pub fn withdraw(
        &mut self,
        store: &mut dyn Storage,
        signer: Addr,
        script_pubkey: Adapter<Script>,
        amount: Uint128,
    ) -> ContractResult<()> {
        let coin = self.accounts.withdraw(signer, amount)?; // ?

        self.add_withdrawal(store, script_pubkey, coin.amount)
    }

    /// Adds an output to the current `Building` checkpoint to be paid out once
    /// the checkpoint is fully signed.
    pub fn add_withdrawal(
        &mut self,
        store: &mut dyn Storage,
        script_pubkey: Adapter<Script>,
        mut amount: Uint128,
    ) -> ContractResult<()> {
        let config = self.config(store)?;
        if script_pubkey.len() as u64 > config.max_withdrawal_script_length {
            return Err(ContractError::App(
                "Script exceeds maximum length".to_string(),
            ));
        }

        if self.checkpoints.len(store)? < config.min_withdrawal_checkpoints {
            return Err(ContractError::App(format!(
                "Withdrawals are disabled until the network has produced at least {} checkpoints",
                config.min_withdrawal_checkpoints
            )));
        }

        let fee_amount = self.calc_minimum_withdrawal_fees(
            store,
            script_pubkey.len() as u64,
            self.checkpoints.building(store)?.fee_rate,
        )?;
        let fee = fee_amount.into();
        amount = amount.checked_sub(fee).map_err(|_| {
            ContractError::App("Withdrawal is too small to pay its miner fee".to_string())
        })?;

        self.give_miner_fee(store, fee)?;
        // TODO: record as collected for excess if full

        let value = (amount.u128() as u64) / config.units_per_sat;
        // if value < self.config.min_withdrawal_amount {
        //     return Err(ContractError::App(
        //         "Withdrawal is smaller than than minimum amount".to_string(),
        //     ));
        // }
        if bitcoin::Amount::from_sat(value) <= script_pubkey.dust_value() {
            return Err(ContractError::App(
                "Withdrawal is too small to pay its dust limit".to_string(),
            ));
        }

        let output = bitcoin::TxOut {
            script_pubkey: script_pubkey.into_inner(),
            value,
        };

        let mut checkpoint = self.checkpoints.building(store)?;
        let building_checkpoint_batch = &mut checkpoint.batches[BatchType::Checkpoint];
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.output.push(Adapter::new(output));

        let index = self.checkpoints.index(store);
        self.checkpoints.set(store, index, &checkpoint)?;
        // TODO: push to excess if full

        Ok(())
    }

    /// Transfers nBTC to another account.    
    pub fn transfer(
        &mut self,
        store: &mut dyn Storage,
        signer: Addr,
        to: Addr,
        amount: Uint128,
    ) -> ContractResult<()> {
        // let transfer_fee = self
        //     .accounts
        //     .withdraw(signer, self.config.transfer_fee.into())?;
        // self.give_rewards(transfer_fee)?;

        let dest = Dest::Address(to);
        let coins = self.accounts.withdraw(signer, amount)?; // ?
        let mut checkpoint = self.checkpoints.building(store)?;

        checkpoint.insert_pending(dest, coins)?;

        let index = self.checkpoints.index(store);
        self.checkpoints.set(store, index, &checkpoint)?;

        Ok(())
    }

    /// Called by signatories to submit their signatures for the current
    /// `Signing` checkpoint.    
    pub fn sign(
        &mut self,
        api: &dyn Api,
        store: &mut dyn Storage,
        xpub: &Xpub,
        sigs: Vec<Signature>,
        cp_index: u32,
    ) -> ContractResult<()> {
        let btc_height = self.headers.height(store)?;
        self.checkpoints
            .sign(api, store, xpub, sigs, cp_index, btc_height)
    }

    /// The amount of BTC in the reserve output of the most recent fully-signed
    /// checkpoint.    
    pub fn value_locked(&self, store: &dyn Storage) -> ContractResult<u64> {
        let last_completed = self.checkpoints.last_completed(store)?;
        Ok(last_completed.reserve_output()?.unwrap().value)
    }

    /// The network (e.g. Bitcoin testnet vs mainnet) which is currently
    /// configured.
    pub fn network(&self) -> bitcoin::Network {
        self.headers.network()
    }

    /// Gets the rate of change of the reserve output and signatory set over the
    /// given interval, in basis points (1/100th of a percent).
    ///
    /// This is used by signers to implement a "circuit breaker" mechanism,
    /// temporarily halting signing if funds are leaving the reserve too quickly
    /// or if the signatory set is changing too quickly.    
    pub fn change_rates(
        &self,
        store: &dyn Storage,
        interval: u64,
        now: u64,
        reset_index: u32,
    ) -> ContractResult<ChangeRates> {
        let signing = self
            .checkpoints
            .signing(store)?
            .ok_or_else(|| ContractError::App("No checkpoint to be signed".to_string()))?;

        if now > interval && now - interval > signing.create_time()
            || reset_index >= signing.sigset.index
        {
            return Ok(ChangeRates::default());
        }
        let now = signing.create_time().max(now);

        let completed = self.checkpoints.completed(
            store,
            (interval / self.checkpoints.config(store).min_checkpoint_interval) as u32 + 1,
        )?;
        if completed.is_empty() {
            return Ok(ChangeRates::default());
        }

        let prev_index = completed
            .iter()
            .rposition(|c| (now - c.create_time()) > interval || c.sigset.index <= reset_index)
            .unwrap_or(0);

        let prev_checkpoint = completed.get(prev_index).unwrap();

        let amount_prev = prev_checkpoint.reserve_output()?.unwrap().value;
        let amount_now = signing.reserve_output()?.unwrap().value;

        let reserve_decrease = amount_prev.saturating_sub(amount_now);

        let vp_shares = |sigset: &SignatorySet| -> ContractResult<_> {
            let sigset_index = sigset.index();
            let total_vp = sigset.present_vp() as f64;
            let sigset_fractions: HashMap<_, _> = sigset
                .iter()
                .map(|v| (v.pubkey.as_slice(), v.voting_power as f64 / total_vp))
                .collect();
            let mut sigset: HashMap<_, _> = Default::default();
            for entry in SIG_KEYS.range_raw(store, None, None, Order::Ascending) {
                let (_, xpub) = entry?;
                let pubkey: threshold_sig::Pubkey = xpub.derive_pubkey(sigset_index)?.into();
                sigset.insert(
                    xpub.key.encode(),
                    *sigset_fractions.get(pubkey.as_slice()).unwrap_or(&0.0),
                );
            }

            Ok(sigset)
        };

        let now_sigset = vp_shares(&signing.sigset)?;
        let prev_sigset = vp_shares(&prev_checkpoint.sigset)?;
        let sigset_change = now_sigset.iter().fold(0.0, |acc, (k, v)| {
            let prev_share = prev_sigset.get(k).unwrap_or(&0.0);
            if v > prev_share {
                acc + (v - prev_share)
            } else {
                acc
            }
        });
        let sigset_change = (sigset_change * 10_000.0) as u16;

        Ok(ChangeRates {
            withdrawal: (reserve_decrease * 10_000 / amount_prev) as u16,
            sigset_change,
        })
    }

    /// Called once per sidechain block to advance the checkpointing process.        
    /// Can add to clock module
    pub fn begin_block_step(
        &mut self,
        env: Env,
        store: &mut dyn Storage,
        external_outputs: impl Iterator<Item = ContractResult<bitcoin::TxOut>>,
        timestamping_commitment: Vec<u8>,
    ) -> ContractResult<Vec<ConsensusKey>> {
        let config = self.config(store)?;
        let has_completed_cp =
            if let Err(ContractError::App(err)) = self.checkpoints.last_completed_index(store) {
                if err == "No completed checkpoints yet" {
                    false
                } else {
                    return Err(ContractError::App(err));
                }
            } else {
                true
            };

        let reached_capacity_limit = if has_completed_cp {
            self.value_locked(store)? >= config.capacity_limit
        } else {
            false
        };

        let btc_height = self.headers.height(store)?;

        let pushed = self.checkpoints.maybe_step(
            env,
            store,
            &self.accounts, // ?
            external_outputs,
            btc_height,
            !reached_capacity_limit,
            timestamping_commitment,
            &config,
        )?;

        // TODO: remove expired outpoints from processed_outpoints

        if pushed {
            self.offline_signers(store)
        } else {
            Ok(vec![])
        }
    }

    /// Returns the consensus keys of signers who have not submitted signatures
    /// for the last `max_offline_checkpoints` checkpoints.
    ///
    /// This should be used to punish offline signers, by e.g. removing them
    /// from the validator set and slashing their stake.    
    fn offline_signers(&mut self, store: &mut dyn Storage) -> ContractResult<Vec<ConsensusKey>> {
        let config = self.config(store)?;
        let mut validators = get_validators(store)?;
        validators.sort_by(|a, b| b.power.cmp(&a.power));

        let offline_threshold = config.max_offline_checkpoints;
        let sigset = self.checkpoints.active_sigset(store)?;
        let lowest_power = sigset.signatories.last().unwrap().voting_power;
        let completed = self.checkpoints.completed(store, offline_threshold)?;
        if completed.len() < offline_threshold as usize {
            return Ok(vec![]);
        }
        let mut offline_signers = vec![];
        for Validator {
            power,
            pubkey: cons_key,
        } in validators
        {
            if power < lowest_power {
                break;
            }

            let xpub = if let Some(xpub) = self.signatory_keys.get(store, cons_key)? {
                xpub
            } else {
                continue;
            };

            let mut offline = true;
            for checkpoint in completed.iter().rev() {
                if checkpoint.to_sign(&xpub)?.is_empty() {
                    offline = false;
                    break;
                }
            }

            if offline {
                offline_signers.push(cons_key);
            }
        }

        Ok(offline_signers)
    }

    /// Takes the pending nBTC transfers from the most recent fully-signed
    /// checkpoint, leaving the vector empty after calling.
    ///
    /// This should be used to process the pending transfers, crediting each of
    /// them now that the checkpoint has been fully signed.
    #[allow(clippy::type_complexity)]
    pub fn take_pending(
        &mut self,
        store: &mut dyn Storage,
    ) -> ContractResult<Vec<Vec<(Dest, Coin)>>> {
        let unhandled_confirmed_cps = match self.checkpoints.unhandled_confirmed(store) {
            Err(_) => return Ok(vec![]),
            Ok(val) => val,
        };
        let mut confirmed_dests = vec![];

        // TODO: drain iter
        for confirmed_index in &unhandled_confirmed_cps {
            let mut checkpoint = self.checkpoints.get(store, *confirmed_index)?;
            confirmed_dests.push(checkpoint.pending);
            // clear pending
            checkpoint.pending = vec![];
            self.checkpoints.set(store, *confirmed_index, &checkpoint)?;
        }
        if let Some(last_index) = unhandled_confirmed_cps.last() {
            let _ = FIRST_UNHANDLED_CONFIRMED_INDEX
                .save(store, &(*last_index + 1))
                .unwrap();
        }
        Ok(confirmed_dests)
    }

    /// Takes the pending nBTC transfers from the most recent fully-signed
    /// checkpoint, leaving the vector empty after calling.
    ///
    /// This should be used to process the pending transfers, crediting each of
    /// them now that the checkpoint has been fully signed.
    #[allow(clippy::type_complexity)]
    pub fn take_pending_completed(
        &mut self,
        store: &mut dyn Storage,
    ) -> ContractResult<Vec<Vec<(Dest, Coin)>>> {
        let last_completed_index = match self.checkpoints.last_completed_index(store) {
            Err(err) => {
                if let ContractError::App(err_str) = &err {
                    if err_str == "No completed checkpoints yet" {
                        return Ok(vec![]);
                    }
                }
                return Err(err);
            }
            Ok(val) => val,
        };

        let confirmed_index = self.checkpoints.confirmed_index(store).unwrap_or_default();
        let mut completed_dests = vec![];
        for checkpoint_index in confirmed_index..=last_completed_index {
            let mut checkpoint = self.checkpoints.get(store, checkpoint_index)?;
            completed_dests.push(checkpoint.pending);
            checkpoint.pending = vec![]; // clear pointer
            self.checkpoints.set(store, checkpoint_index, &checkpoint)?;
        }
        Ok(completed_dests)
    }

    pub fn give_miner_fee(
        &mut self,
        store: &mut dyn Storage,
        amount: Uint128,
    ) -> ContractResult<()> {
        let config = self.config(store)?;
        let amount: u64 = amount.u128() as u64;
        // note: we don't need to burn coin here
        // coin.burn();

        // self.fee_pool += amount as i64;
        let fee_pool = self.fee_pool(store)?;
        FEE_POOL.save(store, &fee_pool)?;

        let mut checkpoint = self.checkpoints.building(store)?;
        checkpoint.fees_collected += amount / config.units_per_sat;

        let index = self.checkpoints.index(store);
        self.checkpoints.set(store, index, &checkpoint)?;

        Ok(())
    }

    // TODO: reward pool ...
    // pub fn give_rewards(&mut self, store: &mut dyn Storage, amount: Uint128) -> ContractResult<()> {
    //     let config = self.config(store)?;
    //     let fee_pool = FEE_POOL.load(store)?;
    //     if fee_pool < (config.fee_pool_target_balance * config.units_per_sat) as i64 {
    //         let amount: u64 = amount.u128() as u64;
    //         // TODO:// tokenfactory coin.burn();

    //         let reward_amount = (amount as u128 * config.fee_pool_reward_split.0 as u128
    //             / config.fee_pool_reward_split.1 as u128) as u64;
    //         let fee_amount = amount - reward_amount;

    //         // self.reward_pool.give(Coin::mint(reward_amount))?;
    //         self.reward_pool.amount = self.reward_pool.amount.checked_sub(reward_amount.into())?;
    //         self.give_miner_fee(store, fee_amount.into())?;

    //         assert_eq!(reward_amount + fee_amount, amount);
    //     } else {
    //         // self.reward_pool.give(coin)?;
    //         self.reward_pool.amount = self.reward_pool.amount.checked_sub(amount)?;
    //     }

    //     Ok(())
    // }

    // pub fn give_funding_to_fee_pool(
    //     &mut self,
    //     store: &mut dyn Storage,
    //     amount: Uint128,
    // ) -> ContractResult<()> {
    //     // TODO: update total paid?
    //     self.give_miner_fee(store, amount)
    // }

    // pub fn transfer_to_fee_pool(
    //     &mut self,
    //     store: &mut dyn Storage,
    //     signer: Addr,
    //     amount: Uint128,
    // ) -> ContractResult<()> {
    //     let config = self.config(store)?;
    //     if amount < (100 * config.units_per_sat).into() {
    //         return Err(ContractError::App(
    //             "Minimum transfer to fee pool is 100 sat".into(),
    //         ));
    //     }

    //     let coins = self.accounts.withdraw(signer, amount)?;
    //     self.give_miner_fee(store, coins.amount)
    // }
}
