use crate::interface::{Accounts, BitcoinConfig, ChangeRates, Dest, Validator, Xpub};
use crate::signatory::SignatoryKeys;
use crate::state::{get_validators, RECOVERY_SCRIPTS, SIGNERS, SIG_KEYS};
use crate::threshold_sig;

use super::checkpoint::Input;
use super::recovery::{RecoveryTxInput, RecoveryTxs};
use super::threshold_sig::Signature;

use super::adapter::Adapter;
use super::checkpoint::BatchType;
use super::checkpoint::CheckpointQueue;
use super::error::{ContractError, ContractResult};
use super::header::HeaderQueue;
use bitcoin::util::bip32::ChildNumber;
use bitcoin::Script;
use bitcoin::{util::merkleblock::PartialMerkleTree, Transaction};
use cosmwasm_std::{Addr, Coin, Env, Order, Storage, Uint128};

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
pub struct Bitcoin {
    /// A light client of the Bitcoin blockchain, keeping track of the headers
    /// of the highest-work chain.    
    pub headers: HeaderQueue,

    /// The set of outpoints which have been relayed to the bridge. This is used
    /// to prevent replay attacks of deposits.
    pub processed_outpoints: OutpointSet,

    /// The checkpoint queue, which manages the checkpointing process,
    /// periodically moving the reserve of BTC on the Bitcoin blockchain to
    /// collect incoming deposits, move the funds to the latest signatory set,
    /// and pay out requested withdrawals.    
    pub checkpoints: CheckpointQueue,

    /// The map of nBTC accounts, which hold the nBTC balances of users.
    pub accounts: Accounts,

    /// The public keys declared by signatories, which are used to sign Bitcoin
    /// transactions.
    // TODO: store recovery script data in account struct
    pub signatory_keys: SignatoryKeys,

    /// A pool of BTC where bridge fees are collected.
    pub(crate) reward_pool: Coin,

    // TODO: turn into Coin<Nbtc>
    pub(crate) fee_pool: i64,

    /// The configuration parameters for the Bitcoin module.
    pub config: BitcoinConfig,

    pub recovery_txs: RecoveryTxs,
}

/// A Tendermint/CometBFT public key.
pub type ConsensusKey = [u8; 32];

impl Bitcoin {
    /// Sets the configuration parameters to the given values.
    pub fn configure(&mut self, config: BitcoinConfig) {
        self.config = config;
    }

    /// Gets the configuration parameters.
    pub fn config() -> BitcoinConfig {
        BitcoinConfig::default()
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
        let consensus_key = SIGNERS.load(store, signer.as_str()).or_else(|_| {
            Err(ContractError::App(
                "Signer does not have a consensus key".to_string(),
            ))
        })?;

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
        if signatory_script.len() as u64 > self.config.max_withdrawal_script_length {
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

    pub fn calc_minimum_deposit_fees(&self, input_vsize: u64, fee_rate: u64) -> u64 {
        input_vsize * fee_rate * self.checkpoints.config.user_fee_factor / 10_000
            * self.config.units_per_sat
    }

    pub fn calc_minimum_withdrawal_fees(&self, script_pubkey_length: u64, fee_rate: u64) -> u64 {
        (9 + script_pubkey_length) * fee_rate * self.checkpoints.config.user_fee_factor / 10_000
            * self.config.units_per_sat
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
        let now = env.block.time.seconds();

        let btc_header = self
            .headers
            .get_by_height(store, btc_height)?
            .ok_or_else(|| ContractError::App("Invalid bitcoin block height".to_string()))?;

        if self.headers.height(store)? - btc_height < self.config.min_confirmations {
            return Err(
                ContractError::App("Block is not sufficiently confirmed".to_string()).into(),
            );
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
            sigset.output_script(&dest_bytes, self.checkpoints.config.sigset_threshold)?;
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
        let deposit_timeout = sigset.create_time() + self.config.max_deposit_age;
        self.processed_outpoints
            .insert(store, outpoint, deposit_timeout)?;

        if !checkpoint.deposits_enabled {
            return Err(ContractError::App(
                "Deposits are disabled for the given checkpoint".to_string(),
            ))?;
        }

        if now > deposit_timeout {
            let checkpoint = self.checkpoints.building(store)?;
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
                    threshold: self.checkpoints.config.sigset_threshold,
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
            self.checkpoints.config.sigset_threshold,
        )?;
        let input_size = input.est_vsize();
        // mint token_factory here
        // let mut nbtc = Nbtc::mint(output.value * self.config.units_per_sat);
        let mut nbtc = Coin {
            denom: "oraibtc".to_string(),
            amount: (output.value * self.config.units_per_sat).into(),
        };
        let fee_amount = self.calc_minimum_deposit_fees(input_size, checkpoint.fee_rate);
        let deposit_fees = calc_deposit_fee(nbtc.amount);
        let fee = (fee_amount + deposit_fees).into();
        nbtc.amount = nbtc.amount.checked_sub(fee).map_err(|_| {
            ContractError::App("Deposit amount is too small to pay its spending fee".to_string())
        })?;
        println!(
            "Relay deposit with output value: {}, input size: {}, checkpoint fee rate: {}",
            output.value, input_size, checkpoint.fee_rate
        );

        self.give_miner_fee(store, fee)?;
        // TODO: record as excess collected if inputs are full

        let mut building_mut = self.checkpoints.building(store)?;
        let building_checkpoint_batch = building_mut
            .batches
            .get_mut(BatchType::Checkpoint as usize)
            .unwrap();
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.input.push(input);
        // TODO: keep in excess queue if full

        // let deposit_fee = nbtc.take(calc_deposit_fee(nbtc.amount.into()))?;
        // self.give_rewards(deposit_fee)?;

        self.checkpoints
            .building(store)?
            .insert_pending(dest, nbtc)?;

        self.checkpoints.set(store, &**building_mut)?;

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
        if let Some(conf_index) = self.checkpoints.confirmed_index {
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

        if self.headers.height(store)? - btc_height < self.config.min_checkpoint_confirmations {
            return Err(
                ContractError::App("Block is not sufficiently confirmed".to_string()).into(),
            );
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

        self.checkpoints.confirmed_index = Some(cp_index);
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
        let coins = self.accounts.withdraw(signer, amount)?;

        self.add_withdrawal(store, script_pubkey, coins)
    }

    /// Adds an output to the current `Building` checkpoint to be paid out once
    /// the checkpoint is fully signed.
    pub fn add_withdrawal(
        &mut self,
        store: &mut dyn Storage,
        script_pubkey: Adapter<Script>,
        mut coins: Coin,
    ) -> ContractResult<()> {
        if script_pubkey.len() as u64 > self.config.max_withdrawal_script_length {
            return Err(ContractError::App("Script exceeds maximum length".to_string()).into());
        }

        if self.checkpoints.len(store)? < self.config.min_withdrawal_checkpoints {
            return Err(ContractError::App(format!(
                "Withdrawals are disabled until the network has produced at least {} checkpoints",
                self.config.min_withdrawal_checkpoints
            ))
            .into());
        }

        let fee_amount = self.calc_minimum_withdrawal_fees(
            script_pubkey.len() as u64,
            self.checkpoints.building(store)?.fee_rate,
        );
        let fee = fee_amount.into();
        coins.amount = coins.amount.checked_sub(fee).map_err(|_| {
            ContractError::App("Withdrawal is too small to pay its miner fee".to_string())
        })?;

        self.give_miner_fee(store, fee)?;
        // TODO: record as collected for excess if full

        let value = (coins.amount.u128() as u64) / self.config.units_per_sat;
        // if value < self.config.min_withdrawal_amount {
        //     return Err(ContractError::App(
        //         "Withdrawal is smaller than than minimum amount".to_string(),
        //     )
        //     .into());
        // }
        if bitcoin::Amount::from_sat(value) <= script_pubkey.dust_value() {
            return Err(ContractError::App(
                "Withdrawal is too small to pay its dust limit".to_string(),
            )
            .into());
        }

        let output = bitcoin::TxOut {
            script_pubkey: script_pubkey.into_inner(),
            value,
        };

        let mut checkpoint = self.checkpoints.building(store)?;
        let building_checkpoint_batch = checkpoint
            .batches
            .get_mut(BatchType::Checkpoint as usize)
            .unwrap();
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.output.push(Adapter::new(output));

        self.checkpoints.set(store, &checkpoint)?;
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
        let coins = self.accounts.withdraw(signer, amount)?;
        let mut checkpoint = self.checkpoints.building(store)?;

        checkpoint.insert_pending(dest, coins)?;

        self.checkpoints.set(store, &checkpoint)?;

        Ok(())
    }

    /// Called by signatories to submit their signatures for the current
    /// `Signing` checkpoint.    
    pub fn sign(
        &mut self,
        store: &mut dyn Storage,
        xpub: &Xpub,
        sigs: Vec<Signature>,
        cp_index: u32,
    ) -> ContractResult<()> {
        let btc_height = self.headers.height(store)?;
        self.checkpoints
            .sign(store, xpub, sigs, cp_index, btc_height)
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
            (interval / self.checkpoints.config.min_checkpoint_interval) as u32 + 1,
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
            let secp = bitcoin::secp256k1::Secp256k1::verification_only();
            let sigset_index = sigset.index();
            let total_vp = sigset.present_vp() as f64;
            let sigset_fractions: HashMap<_, _> = sigset
                .iter()
                .map(|v| (v.pubkey.as_slice(), v.voting_power as f64 / total_vp))
                .collect();
            let mut sigset: HashMap<_, _> = Default::default();
            for entry in SIG_KEYS.range_raw(store, None, None, Order::Ascending) {
                let (_, xpub) = entry?;
                let derive_path = [ChildNumber::from_normal_idx(sigset_index)?];
                let pubkey: threshold_sig::Pubkey =
                    xpub.derive_pub(&secp, &derive_path)?.public_key.into();
                sigset.insert(
                    xpub.inner().encode(),
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
    pub fn begin_block_step(
        &mut self,
        env: Env,
        store: &mut dyn Storage,
        external_outputs: impl Iterator<Item = ContractResult<bitcoin::TxOut>>,
        timestamping_commitment: Vec<u8>,
    ) -> ContractResult<Vec<ConsensusKey>> {
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
            self.value_locked(store)? >= self.config.capacity_limit
        } else {
            false
        };

        let btc_height = self.headers.height(store)?;
        let pushed = self
            .checkpoints
            .maybe_step(
                env,
                store,
                &self.accounts,
                external_outputs,
                btc_height,
                !reached_capacity_limit,
                timestamping_commitment,
                &mut self.fee_pool,
                &self.config,
            )
            .map_err(|err| ContractError::App(err.to_string()))?;

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
        let mut validators = get_validators(store)?;
        validators.sort_by(|a, b| b.power.cmp(&a.power));

        let offline_threshold = self.config.max_offline_checkpoints;
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
    ) -> ContractResult<Vec<Vec<(String, Coin)>>> {
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
            self.checkpoints.set(store, &checkpoint)?;
        }
        if let Some(last_index) = unhandled_confirmed_cps.last() {
            self.checkpoints.first_unhandled_confirmed_cp_index = *last_index + 1;
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
    ) -> ContractResult<Vec<Vec<(String, Coin)>>> {
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

        let confirmed_index = self.checkpoints.confirmed_index.unwrap_or_default();
        let mut completed_dests = vec![];
        for checkpoint_index in confirmed_index..=last_completed_index {
            let mut checkpoint = self.checkpoints.get(store, checkpoint_index)?;
            completed_dests.push(checkpoint.pending);
            checkpoint.pending = vec![]; // clear pointer
            self.checkpoints.set(store, &checkpoint)?;
        }
        Ok(completed_dests)
    }

    pub fn give_miner_fee(
        &mut self,
        store: &mut dyn Storage,
        amount: Uint128,
    ) -> ContractResult<()> {
        let amount: u64 = amount.u128() as u64;
        // TODO: burn via token factory
        // coin.burn();

        self.fee_pool += amount as i64;
        let mut checkpoint = self.checkpoints.building(store)?;
        checkpoint.fees_collected += amount / self.config.units_per_sat;
        self.checkpoints.set(store, &checkpoint)?;

        Ok(())
    }

    pub fn give_rewards(&mut self, store: &mut dyn Storage, amount: Uint128) -> ContractResult<()> {
        if self.fee_pool < (self.config.fee_pool_target_balance * self.config.units_per_sat) as i64
        {
            let amount: u64 = amount.u128() as u64;
            // TODO:// tokenfactory coin.burn();

            let reward_amount = (amount as u128 * self.config.fee_pool_reward_split.0 as u128
                / self.config.fee_pool_reward_split.1 as u128)
                as u64;
            let fee_amount = amount - reward_amount;

            // self.reward_pool.give(Coin::mint(reward_amount))?;
            self.reward_pool.amount = self.reward_pool.amount.checked_sub(reward_amount.into())?;
            self.give_miner_fee(store, fee_amount.into())?;

            assert_eq!(reward_amount + fee_amount, amount);
        } else {
            // self.reward_pool.give(coin)?;
            self.reward_pool.amount = self.reward_pool.amount.checked_sub(amount)?;
        }

        Ok(())
    }

    pub fn give_funding_to_fee_pool(
        &mut self,
        store: &mut dyn Storage,
        amount: Uint128,
    ) -> ContractResult<()> {
        // TODO: update total paid?
        self.give_miner_fee(store, amount)
    }

    pub fn transfer_to_fee_pool(
        &mut self,
        store: &mut dyn Storage,
        signer: Addr,
        amount: Uint128,
    ) -> ContractResult<()> {
        if amount < (100 * self.config.units_per_sat).into() {
            return Err(ContractError::App(
                "Minimum transfer to fee pool is 100 sat".into(),
            ));
        }

        let coins = self.accounts.withdraw(signer, amount)?;
        self.give_miner_fee(store, coins.amount)
    }
}

// #[cfg(test)]
// mod tests {
//     use std::{cell::RefCell, rc::Rc};

//     use bitcoin::{
//         secp256k1::Secp256k1, util::bip32::ExtendedPrivKey, BlockHash, BlockHeader, OutPoint,
//         TxMerkleNode, Txid,
//     };
//     use orga::{
//         collections::EntryMap,
//         ibc::ibc_rs::core::ics24_host::identifier::{ChannelId, PortId},
//     };

//     use crate::app::IbcDest;

//     use super::{
//         header_queue::{WorkHeader, WrappedHeader},
//         *,
//     };

//     #[serial_test::serial]
//     #[test]
//     fn relay_height_validity() {
//         Context::add(Paid::default());
//         Context::add(Time::from_seconds(0));

//         let mut btc = Bitcoin::default();

//         for _ in 0..10 {
//             btc.headers
//                 .deque
//                 .push_back(WorkHeader::new(
//                     WrappedHeader::new(
//                         Adapter::new(BlockHeader {
//                             bits: 0,
//                             merkle_root: TxMerkleNode::all_zeros(),
//                             nonce: 0,
//                             prev_blockhash: BlockHash::all_zeros(),
//                             time: 0,
//                             version: 0,
//                         }),
//                         btc.headers.height().unwrap() + 1,
//                     ),
//                     bitcoin::util::uint::Uint256([0, 0, 0, 0]),
//                 ))
//                 .unwrap();
//         }

//         let h = btc.headers.height().unwrap();
//         let mut try_relay = |height| {
//             // TODO: make test cases not fail at irrelevant steps in relay_deposit
//             // (either by passing in valid input, or by handling other error paths)
//             btc.relay_deposit(
//                 Adapter::new(Transaction {
//                     input: vec![],
//                     lock_time: bitcoin::PackedLockTime(0),
//                     output: vec![],
//                     version: 0,
//                 }),
//                 height,
//                 Adapter::new(PartialMerkleTree::from_txids(&[Txid::all_zeros()], &[true])),
//                 0,
//                 0,
//                 Dest::Address(Address::NULL),
//             )
//         };

//         assert_eq!(
//             try_relay(h + 100).unwrap_err().to_string(),
//             "App Error: Invalid bitcoin block height",
//         );
//         assert_eq!(
//             try_relay(h - 100).unwrap_err().to_string(),
//             "Passed index is greater than initial height. Referenced header does not exist on the Header Queue",
//         );

//         Context::remove::<Paid>();
//     }

//     #[test]
//     #[serial_test::serial]
//     fn check_change_rates() -> ContractResult<()> {
//         // use checkpoint::*;
//         let paid = orga::plugins::Paid::default();
//         Context::add(paid);

//         let mut vals = orga::plugins::Validators::new(
//             Rc::new(RefCell::new(Some(EntryMap::new()))),
//             Rc::new(RefCell::new(Some(Map::new()))),
//         );
//         let addr = vec![Address::from_pubkey([0; 33]), Address::from_pubkey([1; 33])];
//         vals.set_voting_power([0; 32], 100);
//         vals.set_operator([0; 32], addr[0])?;
//         vals.set_voting_power([1; 32], 10);
//         vals.set_operator([1; 32], addr[1])?;
//         Context::add(vals);

//         let set_signer = |addr| {
//             Context::add(Signer { signer: Some(addr) });
//         };
//         let set_time = |time| {
//             let time = orga::plugins::Time::from_seconds(time);
//             Context::add(time);
//         };

//         let btc = Rc::new(RefCell::new(Bitcoin::default()));
//         let secp = Secp256k1::new();
//         let network = btc.borrow().network();
//         let xpriv = vec![
//             ExtendedPrivKey::new_master(network, &[0]).unwrap(),
//             ExtendedPrivKey::new_master(network, &[1]).unwrap(),
//         ];
//         let xpub = vec![
//             ExtendedPubKey::from_priv(&secp, &xpriv[0]),
//             ExtendedPubKey::from_priv(&secp, &xpriv[1]),
//         ];

//         let push_deposit = || {
//             let input = Input::new(
//                 OutPoint {
//                     txid: Txid::from_slice(&[0; 32]).unwrap(),
//                     vout: 0,
//                 },
//                 &btc.borrow().checkpoints.building().unwrap().sigset,
//                 &[0u8],
//                 100_000_000,
//                 (9, 10),
//             )
//             .unwrap();
//             let mut btc = btc.borrow_mut();
//             let mut building_mut = btc.checkpoints.building_mut().unwrap();
//             building_mut.fees_collected = 100_000_000;
//             let mut building_checkpoint_batch = building_mut
//                 .batches
//                 .get_mut(BatchType::Checkpoint as u64)
//                 .unwrap()
//                 .unwrap();
//             let mut checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap().unwrap();
//             checkpoint_tx.input.push_back(input).unwrap();
//         };

//         let push_withdrawal = || {
//             let mut btc = btc.borrow_mut();

//             btc.add_withdrawal(Adapter::new(Script::new()), 459_459_927_000_000.into())
//                 .unwrap();

//             let mut building_mut = btc.checkpoints.building_mut().unwrap();
//             building_mut.fees_collected = 100_000_000;
//         };

//         let sign_batch = |btc_height| {
//             let mut btc = btc.borrow_mut();
//             let queue = &mut btc.checkpoints;
//             let cp = queue.signing().unwrap().unwrap();
//             let sigset_index = cp.sigset.index;
//             for i in 0..2 {
//                 if queue.signing().unwrap().is_none() {
//                     break;
//                 }
//                 let cp = queue.signing().unwrap().unwrap();
//                 let to_sign = cp.to_sign(Xpub::new(xpub[i])).unwrap();
//                 let secp2 = Secp256k1::signing_only();
//                 let sigs = crate::bitcoin::signer::sign(&secp2, &xpriv[i], &to_sign).unwrap();
//                 queue
//                     .sign(Xpub::new(xpub[i]), sigs, sigset_index, btc_height)
//                     .unwrap();
//             }
//         };
//         let sign_cp = |btc_height| {
//             sign_batch(btc_height);
//             sign_batch(btc_height);
//             if btc.borrow().checkpoints.signing().unwrap().is_some() {
//                 sign_batch(btc_height);
//             }
//         };
//         let maybe_step = || {
//             let mut btc = btc.borrow_mut();

//             btc.begin_block_step(vec![].into_iter(), vec![1, 2, 3])
//                 .unwrap();
//         };

//         set_time(0);
//         for i in 0..2 {
//             set_signer(addr[i]);
//             btc.borrow_mut().set_signatory_key(Xpub::new(xpub[i]))?;
//         }

//         assert_eq!(btc.borrow().checkpoints.len()?, 0);
//         maybe_step();
//         assert_eq!(btc.borrow().checkpoints.len()?, 1);

//         set_time(1000);
//         push_deposit();
//         maybe_step();
//         sign_cp(10);

//         assert_eq!(btc.borrow().checkpoints.len()?, 2);

//         set_time(2000);
//         push_deposit();
//         maybe_step();
//         let change_rates = btc.borrow().change_rates(2000, 2100, 0)?;
//         assert_eq!(change_rates.withdrawal, 0);
//         assert_eq!(change_rates.sigset_change, 0);
//         sign_cp(10);

//         assert_eq!(btc.borrow().checkpoints.len()?, 3);

//         // Change the sigset
//         let vals = Context::resolve::<Validators>().unwrap();
//         vals.set_voting_power([1; 32], 100);

//         set_time(3000);
//         push_deposit();
//         maybe_step();
//         let change_rates = btc.borrow().change_rates(3000, 3100, 0)?;
//         assert_eq!(change_rates.withdrawal, 0);
//         assert_eq!(change_rates.sigset_change, 0);
//         sign_cp(10);

//         assert_eq!(btc.borrow().checkpoints.len()?, 4);

//         set_time(4000);
//         push_deposit();
//         maybe_step();
//         let change_rates = btc.borrow().change_rates(3000, 4100, 0)?;
//         assert_eq!(change_rates.withdrawal, 0);
//         assert_eq!(change_rates.sigset_change, 4090);
//         assert_eq!(btc.borrow().checkpoints.len()?, 5);

//         sign_cp(10);

//         set_time(5000);
//         push_deposit();
//         maybe_step();
//         let change_rates = btc.borrow().change_rates(3000, 5100, 0)?;
//         assert_eq!(change_rates.withdrawal, 0);
//         assert_eq!(change_rates.sigset_change, 4090);
//         assert_eq!(btc.borrow().checkpoints.len()?, 6);
//         sign_cp(10);

//         set_time(6000);
//         push_withdrawal();
//         maybe_step();
//         let change_rates = btc.borrow().change_rates(3000, 5100, 0)?;
//         assert_eq!(change_rates.withdrawal, 8664);
//         assert_eq!(change_rates.sigset_change, 4090);
//         assert_eq!(btc.borrow().checkpoints.signing()?.unwrap().sigset.index, 5);
//         let change_rates = btc.borrow().change_rates(3000, 5100, 5)?;
//         assert_eq!(change_rates.withdrawal, 0);
//         assert_eq!(change_rates.sigset_change, 0);

//         Ok(())
//     }

//     #[test]
//     #[serial_test::serial]
//     fn test_take_pending() -> ContractResult<()> {
//         // use checkpoint::*;
//         let paid = orga::plugins::Paid::default();
//         Context::add(paid);

//         let mut vals = orga::plugins::Validators::new(
//             Rc::new(RefCell::new(Some(EntryMap::new()))),
//             Rc::new(RefCell::new(Some(Map::new()))),
//         );
//         let addr = vec![Address::from_pubkey([0; 33]), Address::from_pubkey([1; 33])];
//         vals.set_voting_power([0; 32], 100);
//         vals.set_operator([0; 32], addr[0])?;
//         vals.set_voting_power([1; 32], 10);
//         vals.set_operator([1; 32], addr[1])?;
//         Context::add(vals);

//         let set_signer = |addr| {
//             Context::add(Signer { signer: Some(addr) });
//         };
//         let set_time = |time| {
//             let time = orga::plugins::Time::from_seconds(time);
//             Context::add(time);
//         };

//         let btc = Rc::new(RefCell::new(Bitcoin::default()));
//         let secp = Secp256k1::new();
//         let network = btc.borrow().network();
//         let xpriv = vec![
//             ExtendedPrivKey::new_master(network, &[0]).unwrap(),
//             ExtendedPrivKey::new_master(network, &[1]).unwrap(),
//         ];
//         let xpub = vec![
//             ExtendedPubKey::from_priv(&secp, &xpriv[0]),
//             ExtendedPubKey::from_priv(&secp, &xpriv[1]),
//         ];

//         let push_deposit = |dest: Dest, coin: Coin<Nbtc>| {
//             let input = Input::new(
//                 OutPoint {
//                     txid: Txid::from_slice(&[0; 32]).unwrap(),
//                     vout: 0,
//                 },
//                 &btc.borrow().checkpoints.building().unwrap().sigset,
//                 &[0u8],
//                 100_000_000,
//                 (9, 10),
//             )
//             .unwrap();
//             let mut btc = btc.borrow_mut();
//             let mut building_mut = btc.checkpoints.building_mut().unwrap();
//             building_mut.fees_collected = 100_000_000;
//             building_mut.pending.insert(dest, coin).unwrap();
//             let mut building_checkpoint_batch = building_mut
//                 .batches
//                 .get_mut(BatchType::Checkpoint as u64)
//                 .unwrap()
//                 .unwrap();
//             let mut checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap().unwrap();
//             checkpoint_tx.input.push_back(input).unwrap();
//         };

//         let push_withdrawal = || {
//             let mut btc = btc.borrow_mut();

//             btc.add_withdrawal(Adapter::new(Script::new()), 459_459_927_000_000.into())
//                 .unwrap();

//             let mut building_mut = btc.checkpoints.building_mut().unwrap();
//             building_mut.fees_collected = 100_000_000;
//         };

//         let sign_batch = |btc_height| {
//             let mut btc = btc.borrow_mut();
//             let queue = &mut btc.checkpoints;
//             let cp = queue.signing().unwrap().unwrap();
//             let sigset_index = cp.sigset.index;
//             for i in 0..2 {
//                 if queue.signing().unwrap().is_none() {
//                     break;
//                 }
//                 let cp = queue.signing().unwrap().unwrap();
//                 let to_sign = cp.to_sign(Xpub::new(xpub[i])).unwrap();
//                 let secp2 = Secp256k1::signing_only();
//                 let sigs = crate::bitcoin::signer::sign(&secp2, &xpriv[i], &to_sign).unwrap();
//                 queue
//                     .sign(Xpub::new(xpub[i]), sigs, sigset_index, btc_height)
//                     .unwrap();
//             }
//         };
//         let sign_cp = |btc_height| {
//             sign_batch(btc_height);
//             sign_batch(btc_height);
//             if btc.borrow().checkpoints.signing().unwrap().is_some() {
//                 sign_batch(btc_height);
//             }
//         };

//         let confirm_cp = |confirmed_index| {
//             let mut btc = btc.borrow_mut();
//             btc.checkpoints.confirmed_index = Some(confirmed_index);
//         };

//         let take_pending = || {
//             let mut btc = btc.borrow_mut();
//             btc.take_pending().unwrap()
//         };

//         let maybe_step = || {
//             let mut btc = btc.borrow_mut();

//             btc.begin_block_step(vec![].into_iter(), vec![1, 2, 3])
//                 .unwrap();
//         };

//         set_time(0);
//         for i in 0..2 {
//             set_signer(addr[i]);
//             btc.borrow_mut().set_signatory_key(Xpub::new(xpub[i]))?;
//         }

//         assert_eq!(btc.borrow().checkpoints.len()?, 0);
//         maybe_step();
//         assert_eq!(btc.borrow().checkpoints.len()?, 1);
//         set_time(1000);
//         let channel_id = "channel-0"
//             .parse::<ChannelId>()
//             .map_err(|_| Error::Ibc("Invalid channel id".into()))?;

//         let port_id = "transfer"
//             .parse::<PortId>()
//             .map_err(|_| Error::Ibc("Invalid port".into()))?;
//         let mut dest = IbcDest {
//             source_port: port_id.to_string().try_into()?,
//             source_channel: channel_id.to_string().try_into()?,
//             sender: orga::encoding::Adapter("sender1".to_string().into()),
//             receiver: orga::encoding::Adapter("receiver".to_owned().into()),
//             timeout_timestamp: 10u64,
//             memo: "".try_into()?,
//         };

//         // initially, there should not be any confirmed checkpoints -> return empty array for pending dests
//         assert_eq!(take_pending().len(), 0);
//         // fixture: create 2 confirmed checkpoints having deposits so we can validate later
//         push_deposit(Dest::Ibc(dest.clone()), Coin::<Nbtc>::mint(Amount::new(1)));
//         dest.sender = orga::encoding::Adapter("sender2".to_string().into());
//         push_deposit(Dest::Ibc(dest.clone()), Coin::<Nbtc>::mint(Amount::new(1)));
//         maybe_step();
//         sign_cp(10);
//         confirm_cp(0);
//         set_time(2000);
//         push_deposit(Dest::Ibc(dest.clone()), Coin::<Nbtc>::mint(Amount::new(5)));
//         maybe_step();
//         sign_cp(10);
//         confirm_cp(1);
//         assert_eq!(
//             btc.borrow().checkpoints.first_unhandled_confirmed_cp_index,
//             0
//         );
//         assert_eq!(btc.borrow().checkpoints.confirmed_index, Some(1));
//         // before take pending, the confirmed checkpoints should have some pending deposits
//         assert_eq!(
//             btc.borrow()
//                 .checkpoints
//                 .get(0)
//                 .unwrap()
//                 .pending
//                 .iter()
//                 .unwrap()
//                 .count(),
//             2
//         );
//         assert_eq!(
//             btc.borrow()
//                 .checkpoints
//                 .get(1)
//                 .unwrap()
//                 .pending
//                 .iter()
//                 .unwrap()
//                 .count(),
//             1
//         );

//         // action. After take pending, the unhandled confirmed index should increase to 2 since we handled 2 confirmed checkpoints
//         let cp_dests = take_pending();
//         let checkpoints = &btc.borrow().checkpoints;
//         assert_eq!(checkpoints.first_unhandled_confirmed_cp_index, 2);
//         assert_eq!(cp_dests.len(), 2);
//         assert_eq!(cp_dests[0].len(), 2);
//         assert_eq!(cp_dests[1].len(), 1);
//         assert_eq!(
//             cp_dests[0][0].0.to_base64().unwrap(),
//             Dest::Ibc(IbcDest {
//                 sender: orga::encoding::Adapter("sender1".to_string().into()),
//                 ..dest.clone()
//             })
//             .to_base64()
//             .unwrap(),
//         );
//         assert_eq!(cp_dests[0][0].1.amount, Amount::new(1));

//         assert_eq!(
//             cp_dests[0][1].0.to_base64().unwrap(),
//             Dest::Ibc(IbcDest {
//                 sender: orga::encoding::Adapter("sender2".to_string().into()),
//                 ..dest.clone()
//             })
//             .to_base64()
//             .unwrap(),
//         );
//         assert_eq!(cp_dests[0][1].1.amount, Amount::new(1));

//         assert_eq!(
//             cp_dests[1][0].0.to_base64().unwrap(),
//             Dest::Ibc(IbcDest {
//                 sender: orga::encoding::Adapter("sender2".to_string().into()),
//                 ..dest.clone()
//             })
//             .to_base64()
//             .unwrap(),
//         );
//         assert_eq!(cp_dests[1][0].1.amount, Amount::new(5));

//         // assert confirmed checkpoints pending. Should not have anything because we have removed them already in take_pending()
//         let checkpoints = &btc.borrow().checkpoints;
//         let first_cp = checkpoints.get(0).unwrap();
//         assert_eq!(first_cp.pending.iter().unwrap().count(), 0);
//         let second_cp = checkpoints.get(1).unwrap();
//         assert_eq!(second_cp.pending.iter().unwrap().count(), 0);
//         Ok(())
//     }
// }
