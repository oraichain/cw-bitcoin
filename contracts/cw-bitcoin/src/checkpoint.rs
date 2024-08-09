use super::{
    signatory::SignatorySet,
    threshold_sig::{Signature, ThresholdSig},
};
use crate::{adapter::Adapter, interface::Xpub, state::BUILDING_INDEX};
use crate::{
    constants::DEFAULT_FEE_RATE,
    error::{ContractError, ContractResult},
    state::{CHECKPOINT_CONFIG, CONFIRMED_INDEX, FEE_POOL, FIRST_UNHANDLED_CONFIRMED_INDEX},
};
use crate::{
    interface::{BitcoinConfig, CheckpointConfig, Dest},
    state::CHECKPOINTS,
};
use bitcoin::hashes::Hash;
use bitcoin::{blockdata::transaction::EcdsaSighashType, Sequence, Transaction, TxIn, TxOut};
use cosmwasm_schema::cw_serde;
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::{Api, Coin, Env, QuerierWrapper, Storage};
use derive_more::{Deref, DerefMut};

/// The status of a checkpoint. Checkpoints start as `Building`, and eventually
/// advance through the three states.
#[cw_serde]
#[derive(Default)]
pub enum CheckpointStatus {
    #[default]
    /// The checkpoint is being constructed. It can still be mutated by adding
    /// bitcoin inputs and outputs, pending actions, etc.    
    Building,

    /// The inputs in the checkpoint are being signed. The checkpoint's
    /// structure is frozen in this stage, and it is no longer valid to add or
    /// remove inputs or outputs.
    Signing,

    /// All inputs in the the checkpoint are fully signed and the contained
    /// checkpoint transaction is valid and ready to be broadcast on the bitcoin
    /// network.
    Complete,
}

/// An input to a Bitcoin transaction - possibly in an unsigned state.
///
/// This structure contains the necessary data for signing an input, and once
/// signed can be turned into a `bitcoin::TxIn` for inclusion in a Bitcoin
/// transaction.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct Input {
    /// The outpoint being spent by this input.
    pub prevout: Adapter<bitcoin::OutPoint>,

    /// The script of the output being spent by this input. In practice, this
    /// will be a pay-to-witness-script-hash (P2WSH) script, containing the hash
    /// of the script in the `redeem_script` field.
    pub script_pubkey: Adapter<bitcoin::Script>,

    /// The redeem script which `script_pubkey` contains the hash of, supplied
    /// in the witness of the input when spending. In practice, this will
    /// represent a multisig tied to the associated signatory set.
    pub redeem_script: Adapter<bitcoin::Script>,

    /// The index of the signatory set which this input is associated with.
    pub sigset_index: u32,

    /// Bytes representing a commitment to a destination (e.g. a native nomic
    /// account address, an IBC transfer destination, or a 0-byte for the
    /// reserve output owned by the network). These bytes are included in the
    /// redeem script to tie the funds to the destination.
    pub dest: Vec<u8>,

    /// The amount of the input being spent, in satoshis.
    pub amount: u64,

    /// An estimate of the size of the witness for this input, in virtual bytes.
    /// This size is used for fee calculations.
    pub est_witness_vsize: u64,

    /// The signatures for this input. This structure is where the signatories
    /// coordinate to submit their signatures, and starts out with no
    /// signatures.
    pub signatures: ThresholdSig,
}

impl Input {
    /// Converts the `Input` to a `bitcoin::TxIn`, useful when constructing an
    /// actual Bitcoin transaction to be broadcast.
    pub fn to_txin(&self) -> ContractResult<TxIn> {
        let mut witness = self.signatures.to_witness()?;
        if self.signatures.signed() {
            witness.push(self.redeem_script.to_bytes());
        }

        Ok(bitcoin::TxIn {
            previous_output: *self.prevout,
            script_sig: bitcoin::Script::new(),
            sequence: Sequence(u32::MAX),
            witness: bitcoin::Witness::from_vec(witness),
        })
    }

    /// Creates an `Input` which spends the given Bitcoin outpoint, populating
    /// it with an empty signing state to be signed by the given signatory set.
    pub fn new(
        prevout: bitcoin::OutPoint,
        sigset: &SignatorySet,
        dest: &[u8],
        amount: u64,
        threshold: (u64, u64),
    ) -> ContractResult<Self> {
        let script_pubkey = sigset.output_script(dest, threshold)?;
        let redeem_script = sigset.redeem_script(dest, threshold)?;

        Ok(Input {
            prevout: Adapter::new(prevout),
            script_pubkey: Adapter::new(script_pubkey),
            redeem_script: Adapter::new(redeem_script),
            sigset_index: sigset.index(),
            dest: dest.to_vec(),
            amount,
            est_witness_vsize: sigset.est_witness_vsize(),
            signatures: ThresholdSig::from_sigset(sigset),
        })
    }

    /// The estimated size of the input, including the worst-case size of the
    /// witness once fully signed, in virtual bytes.
    pub fn est_vsize(&self) -> u64 {
        self.est_witness_vsize + 40
    }
}

/// A bitcoin transaction output, wrapped to implement the core `orga` traits.
pub type Output = Adapter<bitcoin::TxOut>;

/// A bitcoin transaction, as a native `orga` data structure.
#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct BitcoinTx {
    /// The locktime field included in the bitcoin transaction, representing
    /// either a block height or timestamp.
    pub lock_time: u32,

    /// A counter representing how many inputs have been fully-signed so far.
    /// The transaction is valid and ready to be broadcast to the bitcoin
    /// network once all inputs have been signed.
    pub signed_inputs: u16,

    /// The inputs to the transaction.
    pub input: Vec<Input>,

    /// The outputs to the transaction.
    pub output: Vec<Output>,
}

impl BitcoinTx {
    /// Converts the `BitcoinTx` to a `bitcoin::Transaction`.
    pub fn to_bitcoin_tx(&self) -> ContractResult<Transaction> {
        Ok(bitcoin::Transaction {
            version: 1,
            lock_time: bitcoin::PackedLockTime(self.lock_time),
            input: self
                .input
                .iter()
                .map(|input| input.to_txin())
                .collect::<ContractResult<_>>()?,
            output: self
                .output
                .iter()
                .map(|output| output.clone().into_inner())
                .collect(),
        })
    }

    /// Creates a new `BitcoinTx` with the given locktime, and no inputs or
    /// outputs.
    pub fn with_lock_time(lock_time: u32) -> Self {
        BitcoinTx {
            lock_time,
            ..Default::default()
        }
    }

    /// Returns `true` if all inputs in the transaction are fully signed,
    /// otherwise returns `false`.
    pub fn signed(&self) -> bool {
        self.signed_inputs as usize == self.input.len()
    }

    /// The estimated size of the transaction, including the worst-case sizes of
    /// all input witnesses once fully signed, in virtual bytes.
    pub fn vsize(&self) -> ContractResult<u64> {
        Ok(self.to_bitcoin_tx()?.vsize().try_into()?)
    }

    /// The hash of the transaction. Note that this will change if any inputs or
    /// outputs are added, removed, or modified, so should only be used once the
    /// transaction is known to be final.
    pub fn txid(&self) -> ContractResult<bitcoin::Txid> {
        let bitcoin_tx = self.to_bitcoin_tx()?;
        Ok(bitcoin_tx.txid())
    }

    /// The total value of the outputs in the transaction, in satoshis.
    pub fn value(&self) -> ContractResult<u64> {
        self.output
            .iter()
            .try_fold(0, |sum: u64, out| Ok(sum + out.value))
    }

    /// Calculates the sighash to be signed for the given input index, and
    /// populates the input's signing state with it. This should be used when a
    /// transaction is finalized and its structure will not change, and
    /// coordination of signing will begin.
    pub fn populate_input_sig_message(&mut self, input_index: usize) -> ContractResult<()> {
        let bitcoin_tx = self.to_bitcoin_tx()?;
        let mut sc = bitcoin::util::sighash::SighashCache::new(&bitcoin_tx);
        let input = self
            .input
            .get_mut(input_index)
            .ok_or(ContractError::InputIndexOutOfBounds(input_index))?;

        let sighash = sc.segwit_signature_hash(
            input_index,
            &input.redeem_script,
            input.amount,
            EcdsaSighashType::All,
        )?;

        input.signatures.set_message(sighash.into_inner());

        Ok(())
    }

    /// Deducts the given amount of satoshis evenly from all outputs in the
    /// transaction, leaving the difference as the amount to be paid to miners
    /// as a fee.
    ///
    /// This function will fail if the fee is greater than the value of the
    /// outputs in the transaction. Any inputs which are not large enough to pay
    /// their share of the fee will be removed.
    pub fn deduct_fee(&mut self, fee: u64) -> ContractResult<()> {
        if fee == 0 {
            return Ok(());
        }

        if self.output.is_empty() {
            // TODO: Bitcoin module error
            return Err(ContractError::BitcoinFee(fee));
        }

        let mut output_len = self.output.len() as u64;

        // This algorithm calculates the amount to attempt to deduct from each
        // output (`threshold`), and then removes any outputs which are too
        // small to pay this. Since removing outputs changes the threshold,
        // additional iterations will be required until all remaining outputs
        // are large enough.
        let threshold = loop {
            // The threshold is the fee divided by the number of outputs (each
            // output pays an equal share of the fee).
            let threshold = fee / output_len;

            // Remove any outputs which are too small to pay the threshold.
            let mut min_output = u64::MAX;
            self.output.retain(|output| {
                let dust_value = output.script_pubkey.dust_value().to_sat();
                let adjusted_output = output.value.saturating_sub(dust_value);
                if adjusted_output < min_output {
                    min_output = adjusted_output;
                }
                adjusted_output > threshold
            });

            output_len = self.output.len() as u64;

            // Handle the case where no outputs remain.
            if output_len == 0 {
                break threshold;
            }

            // If the threshold is less than the smallest output, we can stop
            // here.
            let threshold = fee / output_len;
            if min_output >= threshold {
                break threshold;
            }
        };

        // Deduct the final fee share from each remaining output.
        for output in self.output.iter_mut() {
            output.value -= threshold;
        }

        Ok(())
    }
}

/// `BatchType` represents one of the three types of transaction batches in a
/// checkpoint.
#[derive(Debug)]
pub enum BatchType {
    /// The batch containing the checkpoint transaction. This batch will always
    /// contain exactly one transaction, the "checkpoint transaction".
    ///
    /// This transaction spends the reserve output of the previous checkpoint
    /// transaction and the outputs of any incoming deposits. It pays out to the
    /// the latest signatory set (in the "reserve output") and to destinations
    /// of any requested withdrawals.
    Checkpoint,
}

impl<T> std::ops::Index<BatchType> for Vec<T> {
    type Output = T;
    fn index(&self, idx: BatchType) -> &Self::Output {
        &self[idx as usize]
    }
}

impl<T> std::ops::IndexMut<BatchType> for Vec<T> {
    fn index_mut(&mut self, idx: BatchType) -> &mut Self::Output {
        &mut self[idx as usize]
    }
}

/// A batch of transactions in a checkpoint.
///
/// A batch is a collection of transactions which are atomically signed
/// together. Signatories submit signatures for all inputs in all transactions
/// in the batch at once. Once the batch is fully signed, the checkpoint can
/// advance to signing of the next batch, if any.
#[derive(Default, Debug, Serialize, Deserialize, Deref, DerefMut, Clone, PartialEq)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct Batch {
    signed_txs: u16,
    #[deref]
    #[deref_mut]
    batch: Vec<BitcoinTx>,
}

impl Batch {
    fn signed(&self) -> bool {
        self.signed_txs as usize == self.batch.len()
    }
}

/// `Checkpoint` is the main structure which coordinates the network's
/// management of funds on the Bitcoin blockchain.
///
/// The network periodically creates checkpoints, which are Bitcoin transactions
/// that move the funds held in reserve. There is a singular sequential chain of
/// checkpoints, and each checkpoint has an associated signatory set. The
/// signatory set is a list of public keys of the signers performing the
/// decentralized custody of the funds held in reserve.
///
/// Checkpoints are each associated with a main transaction, the "checkpoint
/// transaction", which spends the reserve output of the previous checkpoint
/// transaction and the outputs of any incoming deposits. It pays out to the the
/// latest signatory set (in the "reserve output") and to destinations of any
/// requested withdrawals. This transaction is included in the third batch of
/// the `batches` deque.
///
/// Checkpoints are also associated with a set of transactions which pay out to
/// the recipients of the emergency disbursal (e.g. recovery wallets of nBTC
/// holders), if the checkpoint transaction is not spent after a given amount of
/// time (e.g. two weeks). These transactions are broken up into a single
/// "intermediate emergency disbursal transaction" (in the second batch of the
/// `batches` deque), and one or more "final emergency disbursal transactions"
/// (in the first batch of the `batches` deque).
#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct Checkpoint {
    /// The status of the checkpoint, either `Building`, `Signing`, or
    /// `Complete`.
    pub status: CheckpointStatus,

    /// Pending transfers of nBTC to be processed once the checkpoint is fully
    /// signed. These transfers are processed in lockstep with the checkpointing
    /// process in order to keep nBTC balances in sync with the emergency
    /// disbursal.
    ///
    /// These transfers can be initiated by a simple nBTC send or by a deposit.    
    pub pending: Vec<(Dest, Coin)>,

    /// The batches of transactions in the checkpoint, to each be signed
    /// atomically, in order. Currently we have only one batch which is
    /// "checkpoint transaction".
    pub batches: Vec<Batch>,

    /// The fee rate to use when calculating the miner fee for the transactions
    /// in the checkpoint, in satoshis per virtual byte.
    ///
    /// This rate is automatically adjusted per-checkpoint, being increased when
    /// completed checkpoints are not being confirmed on the Bitcoin network
    /// faster than the target confirmation speed (implying the network is
    /// paying too low of a fee), and being decreased if checkpoints are
    /// confirmed faster than the target confirmation speed.    
    pub fee_rate: u64,

    /// The height of the Bitcoin block at which the checkpoint was fully signed
    /// and ready to be broadcast to the Bitcoin network, used by the fee
    /// adjustment algorithm to determine if the checkpoint was confirmed too
    /// fast or too slow.    
    pub signed_at_btc_height: Option<u32>,

    /// Whether or not to honor relayed deposits made against this signatory
    /// set. This can be used, for example, to enforce a cap on deposits into
    /// the system.    
    pub deposits_enabled: bool,

    pub fees_collected: u64,

    /// The signatory set associated with the checkpoint. Note that deposits to
    /// slightly older signatory sets can still be processed in this checkpoint,
    /// but the reserve output will be paid to the latest signatory set.
    pub sigset: SignatorySet,
}

impl Checkpoint {
    /// Creates a new checkpoint with the given signatory set.
    ///
    /// The checkpoint will be initialized with a single empty checkpoint
    /// transaction, a single empty intermediate emergency disbursal
    /// transaction, and an empty batch of final emergency disbursal
    /// transactions.
    pub fn new(sigset: SignatorySet) -> ContractResult<Self> {
        let mut checkpoint = Checkpoint {
            status: CheckpointStatus::default(),
            fee_rate: DEFAULT_FEE_RATE,
            signed_at_btc_height: None,
            deposits_enabled: true,
            sigset,
            fees_collected: 0,
            pending: vec![],
            batches: vec![],
        };

        let checkpoint_tx = BitcoinTx::default();
        let mut checkpoint_batch = Batch::default();
        checkpoint_batch.push(checkpoint_tx);
        checkpoint.batches.push(checkpoint_batch);

        Ok(checkpoint)
    }

    /// Changes the status of the checkpoint to `Complete`.
    pub fn advance(&mut self) {
        self.status = CheckpointStatus::Complete;
    }

    /// Processes a batch of signatures from a signatory, applying them to the
    /// inputs of transaction batches which are ready to be signed.
    ///
    /// Transaction batches are ready to be signed if they are either already
    /// signed (all inputs of all transactions in the batch are above the
    /// signing threshold), in which case any newly-submitted signatures will
    /// "over-sign" the inputs, or if the batch is the first non-signed batch
    /// (the "active" batch). This prevents signatories from submitting
    /// signatures to a batch beyond the active batch, so that batches are
    /// always finished signing serially, in order.
    ///
    /// A signatory must submit all signatures for all inputs in which they are
    /// present in the signatory set, for all transactions of all batches ready
    /// to be signed. If the signatory provides more or less signatures than
    /// expected, `sign()` will return an error.
    fn sign(
        &mut self,
        api: &dyn Api,
        xpub: &Xpub,
        sigs: Vec<Signature>,
        btc_height: u32,
    ) -> ContractResult<()> {
        let cp_was_signed = self.signed();
        let mut sig_index = 0;

        // Iterate over all batches in the checkpoint, breaking once iterating
        // to a batch which is not ready to be signed.
        for batch in &mut self.batches {
            let batch_was_signed = batch.signed();

            // Iterate over all transactions in the batch.
            for tx in &mut batch.batch {
                let tx_was_signed = tx.signed();

                // Iterate over all inputs in the transaction.
                for k in 0..tx.input.len() {
                    let input = tx.input.get_mut(k).unwrap();
                    let pubkey = xpub.derive_pubkey(input.sigset_index)?;

                    // Skip input if either the signatory is not part of this
                    // input's signatory set, or the signatory has already
                    // submitted a signature for this input.
                    if !input.signatures.needs_sig(pubkey.into()) {
                        continue;
                    }

                    // Error if there are no remaining supplied signatures - the
                    // signatory supplied less signatures than we require from
                    // them.
                    if sig_index >= sigs.len() {
                        return Err(ContractError::Checkpoint(
                            "Not enough signatures supplied".into(),
                        ));
                    }
                    let sig = &sigs[sig_index];
                    sig_index += 1;

                    // Apply the signature.
                    let input_was_signed = input.signatures.signed();
                    input.signatures.sign(api, pubkey.into(), sig)?;

                    // If this signature made the input fully signed, increase
                    // the counter of fully-signed inputs in the containing
                    // transaction.
                    if !input_was_signed && input.signatures.signed() {
                        tx.signed_inputs += 1;
                    }
                }

                // If these signatures made the transaction fully signed,
                // increase the counter of fully-signed transactions in the
                // containing batch.
                if !tx_was_signed && tx.signed() {
                    batch.signed_txs += 1;
                }
            }

            // If this was the last batch ready to be signed, stop here.
            if !batch_was_signed {
                break;
            }
        }

        // Error if there are remaining supplied signatures - the signatory
        // supplied more signatures than we require from them.
        if sig_index != sigs.len() {
            return Err(ContractError::Checkpoint(
                "Excess signatures supplied".into(),
            ));
        }

        // If these signatures made the checkpoint fully signed, record the
        // height at which it was signed.
        if self.signed() && !cp_was_signed {
            self.signed_at_btc_height = Some(btc_height);
        }

        Ok(())
    }

    /// Gets the checkpoint transaction as a `bitcoin::Transaction`.    
    pub fn checkpoint_tx(&self) -> ContractResult<Adapter<bitcoin::Transaction>> {
        Ok(Adapter::new(
            self.batches[BatchType::Checkpoint]
                .last()
                .unwrap()
                .to_bitcoin_tx()?,
        ))
    }

    /// Gets the output containing the reserve funds for the checkpoint, the
    /// "reserve output". This output is owned by the latest signatory set, and
    /// is spent by the suceeding checkpoint transaction.
    ///
    /// This output is not created until the checkpoint advances to `Signing`
    /// status.
    pub fn reserve_output(&self) -> ContractResult<Option<TxOut>> {
        // TODO: should return None for Building checkpoints? otherwise this
        // might return a withdrawal
        let checkpoint_tx = self.checkpoint_tx()?;
        if let Some(output) = checkpoint_tx.output.first() {
            Ok(Some(output.clone()))
        } else {
            Ok(None)
        }
    }

    /// Returns a list of all inputs in the checkpoint which the signatory with
    /// the given extended public key should sign.
    ///
    /// The return value is a list of tuples, each containing `(sighash,
    /// sigset_index)` - the sighash to be signed and the index of the signatory
    /// set associated with the input.    
    pub fn to_sign(&self, xpub: &Xpub) -> ContractResult<Vec<([u8; 32], u32)>> {
        let mut msgs = vec![];

        for batch in &self.batches {
            for tx in &batch.batch {
                for input in &tx.input {
                    let pubkey = xpub.derive_pubkey(input.sigset_index)?;
                    if input.signatures.needs_sig(pubkey.into()) {
                        msgs.push((input.signatures.message(), input.sigset_index));
                    }
                }
            }
            if !batch.signed() {
                break;
            }
        }

        Ok(msgs)
    }

    /// Returns the number of fully-signed batches in the checkpoint.
    fn signed_batches(&self) -> usize {
        let mut signed_batches = 0;
        for batch in &self.batches {
            if batch.signed() {
                signed_batches += 1;
            } else {
                break;
            }
        }

        signed_batches
    }

    /// Returns the current batch being signed, or `None` if all batches are
    /// signed.
    pub fn current_batch(&self) -> Option<Batch> {
        if self.signed() {
            return None;
        }
        let pos = self.signed_batches();
        self.batches.get(pos).cloned()
    }

    /// Returns the timestamp at which the checkpoint was created (when it was
    /// first constructed in the `Building` status).
    pub fn create_time(&self) -> u64 {
        self.sigset.create_time()
    }

    /// Returns `true` if all batches in the checkpoint are fully signed,
    /// otherwise returns `false`.
    pub fn signed(&self) -> bool {
        self.signed_batches() == self.batches.len()
    }

    pub fn checkpoint_tx_miner_fees(&self) -> ContractResult<u64> {
        let mut fees = 0;

        let batch = &self.batches[BatchType::Checkpoint];
        let tx = &batch[0];

        for input in &tx.input {
            fees += input.amount;
        }

        for output in &tx.output {
            fees -= output.value;
        }

        Ok(fees)
    }

    pub fn base_fee(
        &self,
        config: &CheckpointConfig,
        timestamping_commitment: &[u8],
    ) -> ContractResult<u64> {
        let est_vsize = self.est_vsize(config, timestamping_commitment)?;
        Ok(est_vsize * self.fee_rate)
    }

    fn est_vsize(
        &self,
        config: &CheckpointConfig,
        timestamping_commitment: &[u8],
    ) -> ContractResult<u64> {
        let batch = &self.batches[BatchType::Checkpoint];
        let cp = &batch[0];
        let mut tx = cp.to_bitcoin_tx()?;

        tx.output = self
            .additional_outputs(config, timestamping_commitment)?
            .into_iter()
            .chain(tx.output)
            .take(config.max_outputs as usize)
            .collect();
        tx.input.truncate(config.max_inputs as usize);

        let vsize = tx.vsize() as u64
            + cp.input
                .iter()
                .take(config.max_inputs as usize)
                .try_fold(0, |sum, input| {
                    Ok::<_, ContractError>(sum + input.est_witness_vsize)
                })?;

        Ok(vsize)
    }

    // This function will return total input amount and output amount in checkpoint transaction
    pub fn calc_total_input_and_output(
        &self,
        config: &CheckpointConfig,
    ) -> ContractResult<(u64, u64)> {
        let mut in_amount = 0;
        let checkpoint_batch =
            self.batches
                .get(BatchType::Checkpoint as usize)
                .ok_or(ContractError::Checkpoint(
                    "Cannot get batch checkpoint".into(),
                ))?;
        let checkpoint_tx = checkpoint_batch
            .get(0)
            .ok_or(ContractError::Checkpoint("Cannot get checkpoint tx".into()))?;
        for i in 0..(config.max_inputs as usize).min(checkpoint_tx.input.len()) {
            let input = checkpoint_tx.input.get(i).ok_or(ContractError::Checkpoint(
                "Cannot get checkpoint tx input".into(),
            ))?;
            in_amount += input.amount;
        }
        let mut out_amount = 0;
        for i in 0..(config.max_outputs as usize).min(checkpoint_tx.output.len()) {
            let output = checkpoint_tx
                .output
                .get(i)
                .ok_or(ContractError::Checkpoint(
                    "Cannot get checkpoint tx output".into(),
                ))?;
            out_amount += output.value;
        }
        Ok((in_amount, out_amount))
    }

    fn additional_outputs(
        &self,
        config: &CheckpointConfig,
        timestamping_commitment: &[u8],
    ) -> ContractResult<Vec<bitcoin::TxOut>> {
        // The reserve output is the first output of the checkpoint tx, and
        // contains all funds held in reserve by the network.
        let reserve_out = bitcoin::TxOut {
            value: 0, // will be updated after counting ins/outs and fees
            script_pubkey: self.sigset.output_script(&[0u8], config.sigset_threshold)?,
        };

        // The timestamping commitment output is the second output of the
        // checkpoint tx, and contains a commitment to some given data, which
        // will be included on the Bitcoin blockchain as `OP_RETURN` data, now
        // timestamped by Bitcoin's proof-of-work security.
        let timestamping_commitment_out = bitcoin::TxOut {
            value: 0,
            script_pubkey: bitcoin::Script::new_op_return(timestamping_commitment),
        };

        Ok(vec![reserve_out, timestamping_commitment_out])
    }
}

/// `CheckpointQueue` is the main collection for the checkpointing process,
/// containing a sequential chain of checkpoints.
///
/// Once the network has processed its first deposit, the checkpoint queue will
/// always contain at least one checkpoint, in the `Building` state, at the
/// highest index in the queue.
///
/// The queue will only contain at most one checkpoint in the `Signing` state,
/// at the second-highest index in the queue if it exists. When this checkpoint
/// is stil being signed, progress will block and no new checkpoints will be
/// created since the checkpoints are in a sequential chain.
///
/// The queue may contain any number of checkpoints in the `Complete` state,
/// which are the checkpoints which have been fully signed and are ready to be
/// broadcast to the Bitcoin network. The queue also maintains a counter
/// (`confirmed_index`) to track which of these completed checkpoints have been
/// confirmed in a Bitcoin block.
#[cw_serde]
#[derive(Default)]
pub struct CheckpointQueue {}

/// A wrapper around  an immutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Complete` state.
#[derive(Deref)]
pub struct CompletedCheckpoint(Checkpoint);

/// A wrapper around a mutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Complete` state.
#[derive(Deref, DerefMut)]
pub struct SigningCheckpoint(Checkpoint);

impl SigningCheckpoint {
    /// Adds a batch of signatures to the checkpoint for the signatory with the
    /// given extended public key (`xpub`).
    ///
    /// The signatures must be provided in the same order as the inputs in the
    /// checkpoint transaction, and must be provided for all inputs in which the
    /// signatory is present in the signatory set.
    pub fn sign(
        &mut self,
        api: &dyn Api,
        querier: QuerierWrapper,
        store: &mut dyn Storage,
        xpub: Xpub,
        sigs: Vec<Signature>,
        btc_height: u32,
    ) -> ContractResult<()> {
        self.0.sign(api, &xpub, sigs, btc_height)?;
        Ok(())
    }
}

/// A wrapper around a mutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Building` state.
#[derive(Deref, DerefMut)]
pub struct BuildingCheckpoint(Checkpoint);

/// The data returned by the `advance()` method of `BuildingCheckpointMut`.
type BuildingAdvanceRes = (
    bitcoin::OutPoint, // reserve outpoint
    u64,               // reserve size (sats)
    u64,               // fees paid (sats)
    Vec<Input>,        // excess inputs
    Vec<Output>,       // excess outputs
);

impl BuildingCheckpoint {
    /// Advances the checkpoint to the `Signing` state.
    ///
    /// This will generate the emergency disbursal transactions representing the
    /// ownership of nBTC at this point in time. It will also prepare all inputs
    /// to be signed, across the three transaction batches.
    ///
    /// This step freezes the checkpoint, and no further changes can be made to
    /// it other than adding signatures. This means at this point all
    /// transactions contained within have a known transaction id which will not
    /// change.    
    pub fn advance(
        &mut self,
        timestamping_commitment: Vec<u8>,
        cp_fees: u64,
        config: &CheckpointConfig,
    ) -> ContractResult<BuildingAdvanceRes> {
        self.0.status = CheckpointStatus::Signing;

        let outs = self.additional_outputs(config, &timestamping_commitment)?;
        let checkpoint_batch = &mut self.batches[BatchType::Checkpoint];
        let checkpoint_tx = checkpoint_batch.get_mut(0).unwrap();
        for out in outs.iter().rev() {
            checkpoint_tx.output.insert(0, Adapter::new(out.clone()));
        }

        // Remove excess inputs and outputs from the checkpoint tx, to be pushed
        // onto the suceeding checkpoint while in its `Building` state.
        let mut excess_inputs = vec![];
        while checkpoint_tx.input.len() as u64 > config.max_inputs {
            let removed_input = checkpoint_tx.input.pop().unwrap();
            excess_inputs.push(removed_input);
        }
        let mut excess_outputs = vec![];
        while checkpoint_tx.output.len() as u64 > config.max_outputs {
            let removed_output = checkpoint_tx.output.pop().unwrap();
            excess_outputs.push(removed_output);
        }

        // Sum the total input and output amounts.
        // TODO: Input/Output sum functions
        let mut in_amount = 0;
        for i in 0..checkpoint_tx.input.len() {
            let input = checkpoint_tx.input.get(i).unwrap();
            in_amount += input.amount;
        }
        let mut out_amount = 0;
        for i in 0..checkpoint_tx.output.len() {
            let output = checkpoint_tx.output.get(i).unwrap();
            out_amount += output.value;
        }

        // Deduct the outgoing amount and calculated fee amount from the reserve
        // input amount, to set the resulting reserve output value.
        let reserve_value = in_amount.checked_sub(out_amount + cp_fees).ok_or_else(|| {
            ContractError::Checkpoint("Insufficient reserve value to cover miner fees".into())
        })?;
        let reserve_out = checkpoint_tx.output.get_mut(0).unwrap();
        reserve_out.value = reserve_value;

        // Prepare the checkpoint tx's inputs to be signed by calculating their
        // sighashes.
        let bitcoin_tx = checkpoint_tx.to_bitcoin_tx()?;
        let mut sc = bitcoin::util::sighash::SighashCache::new(&bitcoin_tx);
        for i in 0..checkpoint_tx.input.len() {
            let input = checkpoint_tx.input.get_mut(i).unwrap();
            let sighash = sc.segwit_signature_hash(
                i,
                &input.redeem_script,
                input.amount,
                EcdsaSighashType::All,
            )?;
            input.signatures.set_message(sighash.into_inner());
        }

        let reserve_outpoint = bitcoin::OutPoint {
            txid: checkpoint_tx.txid()?,
            vout: 0,
        };

        Ok((
            reserve_outpoint,
            reserve_value,
            cp_fees,
            excess_inputs,
            excess_outputs,
        ))
    }

    /// Insert a transfer to the pending transfer queue.
    ///
    /// Transfers will be processed once the containing checkpoint is finished
    /// being signed, but will be represented in this checkpoint's emergency
    /// disbursal before they are processed.
    pub fn insert_pending(&mut self, dest: Dest, coin: Coin) -> ContractResult<()> {
        self.pending.push((dest, coin));
        Ok(())
    }
}

impl CheckpointQueue {
    /// The queue's current configuration parameters.
    pub fn config(&self, store: &dyn Storage) -> CheckpointConfig {
        let checkpoint_config = CHECKPOINT_CONFIG.load(store).unwrap();
        checkpoint_config
    }

    pub fn index(&self, store: &dyn Storage) -> u32 {
        let building_index = BUILDING_INDEX.load(store).unwrap();
        building_index
    }

    pub fn first_unhandled_confirmed_index(&self, store: &dyn Storage) -> u32 {
        let index = FIRST_UNHANDLED_CONFIRMED_INDEX.load(store).unwrap();
        index
    }

    pub fn confirmed_index(&self, store: &dyn Storage) -> Option<u32> {
        let index = CONFIRMED_INDEX.may_load(store).unwrap();
        index
    }

    /// Removes all checkpoints from the queue and resets the index to zero.
    pub fn reset(&mut self, store: &mut dyn Storage) -> ContractResult<()> {
        let _ = BUILDING_INDEX.save(store, &0).unwrap();
        FIRST_UNHANDLED_CONFIRMED_INDEX.remove(store);
        CONFIRMED_INDEX.remove(store);
        CHECKPOINTS.clear(store)
    }

    /// Gets a reference to the checkpoint at the given index.
    ///
    /// If the index is out of bounds or was pruned, an error is returned.
    pub fn get(&self, store: &dyn Storage, index: u32) -> ContractResult<Checkpoint> {
        let queue_len = CHECKPOINTS.len(store)?;
        let index = self.get_deque_index(store, index, queue_len)?;
        let checkpoint = CHECKPOINTS.get(store, index)?.unwrap();
        Ok(checkpoint)
    }

    pub fn set(
        &self,
        store: &mut dyn Storage,
        index: u32,
        checkpoint: &Checkpoint,
    ) -> ContractResult<()> {
        let queue_len = CHECKPOINTS.len(store)?;
        let index = self.get_deque_index(store, index, queue_len)?;
        CHECKPOINTS.set(store, index, checkpoint)?;
        Ok(())
    }

    /// Calculates the index within the deque based on the given checkpoint
    /// index.
    ///
    /// This is necessary because the values can differ for queues which have
    /// been pruned. For example, a queue may contain 5 checkpoints,
    /// representing indexes 30 to 34. Checkpoint index 30 is at deque index 0,
    /// checkpoint 34 is at deque index 4, and checkpoint index 29 is now
    /// out-of-bounds.
    fn get_deque_index(
        &self,
        store: &dyn Storage,
        index: u32,
        queue_len: u32,
    ) -> ContractResult<u32> {
        let start = self.index(store) + 1 - queue_len;
        if index > self.index(store) || index < start {
            Err(ContractError::App("Index out of bounds".into()))
        } else {
            Ok(index - start)
        }
    }

    /// The number of checkpoints in the queue.
    ///
    /// This will likely be different from `index` since checkpoints can be
    /// pruned. After receiving the first deposit, the network will always have
    /// at least one checkpoint in the queue.
    // TODO: remove this attribute, not sure why clippy is complaining when
    // is_empty is defined
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self, store: &dyn Storage) -> ContractResult<u32> {
        let queue_len = CHECKPOINTS.len(store)?;
        Ok(queue_len)
    }

    /// Returns `true` if there are no checkpoints in the queue.
    ///
    /// This will only be `true` before the first deposit has been processed.
    pub fn is_empty(&self, store: &dyn Storage) -> ContractResult<bool> {
        Ok(self.len(store)? == 0)
    }

    /// All checkpoints in the queue, in order from oldest to newest.
    ///
    /// The return value is a vector of tuples, where the first element is the
    /// checkpoint's index, and the second element is a reference to the
    /// checkpoint.
    pub fn all(&self, store: &dyn Storage) -> ContractResult<Vec<(u32, Checkpoint)>> {
        // TODO: return iterator
        // TODO: use DequeExtension iterator
        let queue_len = CHECKPOINTS.len(store)?;
        let mut out = Vec::with_capacity(queue_len as usize);

        for i in 0..queue_len {
            let checkpoint = CHECKPOINTS.get(store, i)?.unwrap();
            out.push(((self.index(store) + 1 - (queue_len - i)), checkpoint));
        }

        Ok(out)
    }

    /// All checkpoints in the queue which are in the `Complete` state, in order
    /// from oldest to newest.
    pub fn completed(
        &self,
        store: &dyn Storage,
        limit: u32,
    ) -> ContractResult<Vec<CompletedCheckpoint>> {
        // TODO: return iterator
        // TODO: use DequeExtension iterator

        let mut out = vec![];

        let length = self.len(store)?;
        if length == 0 {
            return Ok(out);
        }

        let skip = if self.signing(store)?.is_some() { 2 } else { 1 };
        let end = self.index(store).saturating_sub(skip - 1);

        let start = end - limit.min(length - skip);

        for i in start..end {
            let checkpoint = self.get(store, i)?;
            out.push(CompletedCheckpoint(checkpoint));
        }

        Ok(out)
    }

    /// The index of the last completed checkpoint.
    pub fn last_completed_index(&self, store: &dyn Storage) -> ContractResult<u32> {
        if self.signing(store)?.is_some() {
            self.index(store).checked_sub(2)
        } else {
            self.index(store).checked_sub(1)
        }
        .ok_or_else(|| ContractError::App("No completed checkpoints yet".to_string()))
    }

    pub fn first_index(&self, store: &dyn Storage) -> ContractResult<u32> {
        Ok(self.index(store) + 1 - self.len(store)?)
    }

    /// A reference to the last completed checkpoint.
    pub fn last_completed(&self, store: &dyn Storage) -> ContractResult<Checkpoint> {
        let index = self.last_completed_index(store)?;
        self.get(store, index)
    }

    /// The last completed checkpoint, converted to a Bitcoin transaction.
    pub fn last_completed_tx(
        &self,
        store: &dyn Storage,
    ) -> ContractResult<Adapter<bitcoin::Transaction>> {
        self.last_completed(store)?.checkpoint_tx()
    }

    /// All completed checkpoints, converted to Bitcoin transactions.
    pub fn completed_txs(
        &self,
        store: &dyn Storage,
        limit: u32,
    ) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        self.completed(store, limit)?
            .into_iter()
            .map(|c| c.checkpoint_tx())
            .collect()
    }

    /// The last complete builiding checkpoint transaction, which have the type BatchType::Checkpoint
    /// Here we have only one element in the vector, and I use vector because I don't want to throw
    /// any error if vec! is empty
    pub fn last_completed_checkpoint_tx(
        &self,
        store: &dyn Storage,
    ) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        let mut txs = vec![];
        if let Some(completed) = self.completed(store, 1)?.last() {
            txs.push(completed.checkpoint_tx()?);
            Ok(txs)
        } else {
            Ok(vec![])
        }
    }

    /// A reference to the checkpoint in the `Signing` state, if there is one.
    pub fn signing(&self, store: &dyn Storage) -> ContractResult<Option<SigningCheckpoint>> {
        if self.len(store)? < 2 {
            return Ok(None);
        }

        let second = self.get(store, self.index(store) - 1)?;
        if !matches!(second.status, CheckpointStatus::Signing) {
            return Ok(None);
        }

        Ok(Some(SigningCheckpoint(second)))
    }

    /// A reference to the checkpoint in the `Building` state.
    ///
    /// This is the checkpoint which is currently being built, and is not yet
    /// being signed. Other than at the start of the network, before the first
    /// deposit has been received, there will always be a checkpoint in this
    /// state.
    pub fn building(&self, store: &dyn Storage) -> ContractResult<BuildingCheckpoint> {
        let last = self.get(store, self.index(store))?;
        Ok(BuildingCheckpoint(last))
    }

    /// Advances the checkpoint queue state machine.
    ///
    /// This method is called once per sidechain block, and will handle adding
    /// new checkpoints to the queue, advancing the `Building` checkpoint to
    /// `Signing`, and adjusting the checkpoint fee rates.
    ///
    /// If the `Building` checkpoint was advanced to `Signing` and a new
    /// `Building` checkpoint was created, this method will return `Ok(true)`.
    /// Otherwise, it will return `Ok(false)`.
    ///
    /// **Parameters:**
    ///
    /// - `sig_keys`: a map of consensus keys to their corresponding xpubs. This
    /// is used to determine which keys should be used in the signatory set,
    /// getting the set participation from the current validator set.
    /// - `nbtc_accounts`: a map of nBTC accounts to their corresponding
    /// balances. This is used along with to create outputs for the emergency
    /// disbursal transactions by getting the recovery script for each account
    /// from the `recovery_scripts` parameter.
    /// - `recovery_scripts`: a map of nBTC account addresses to their
    /// corresponding recovery scripts (account holders' desired destinations
    /// for the emergency disbursal).
    /// - `external_outputs`: an iterator of Bitcoin transaction outputs which
    /// should be included in the emergency disbursal transactions. This allows
    /// higher level modules the ability to create outputs for their own
    /// purposes.
    /// - `btc_height`: the current Bitcoin block height.
    /// - `should_allow_deposits`: whether or not deposits should be allowed in
    ///   any newly-created checkpoints.
    /// - `timestamping_commitment`: the data to be timestamped by the
    ///  checkpoint's timestamping commitment output (included as `OP_RETURN`
    ///  data in the checkpoint transaction to timestamp on the Bitcoin
    ///  blockchain for proof-of-work security).    
    #[allow(clippy::too_many_arguments)]
    pub fn maybe_step(
        &mut self,
        env: Env,
        store: &mut dyn Storage,
        btc_height: u32,
        should_allow_deposits: bool,
        timestamping_commitment: Vec<u8>,
        // fee_pool: &mut i64,
        parent_config: &BitcoinConfig,
    ) -> ContractResult<bool> {
        let is_should_push =
            self.should_push(env.clone(), store, &timestamping_commitment, btc_height)?;
        if !is_should_push {
            return Ok(false);
        }

        let is_not_maybe_push = self
            .maybe_push(env.clone(), store, should_allow_deposits)?
            .is_none();
        if is_not_maybe_push {
            return Ok(false);
        }

        self.prune(store)?;

        if self.index(store) > 0 {
            let prev_index = self.index(store) - 1;
            let cp_fees = self.calc_fee_checkpoint(store, prev_index, &timestamping_commitment)?;

            let config = self.config(store);
            let prev = self.get(store, prev_index)?;
            let sigset = prev.sigset.clone();
            let prev_fee_rate = prev.fee_rate;
            let mut building_checkpoint = BuildingCheckpoint(prev);
            let (reserve_outpoint, reserve_value, fees_paid, excess_inputs, excess_outputs) =
                building_checkpoint.advance(timestamping_commitment, cp_fees, &config)?;
            // update checkpoint
            self.set(store, prev_index, &building_checkpoint)?;

            let mut fee_pool = FEE_POOL.load(store)?;
            fee_pool -= (fees_paid * parent_config.units_per_sat) as i64;
            FEE_POOL.save(store, &fee_pool)?;

            // Adjust the fee rate for the next checkpoint based on whether past
            // checkpoints have been confirmed in greater or less than the
            // target number of Bitcoin blocks.
            let fee_rate = if let Some(first_unconf_index) = self.first_unconfirmed_index(store)? {
                // There are unconfirmed checkpoints.

                let first_unconf = self.get(store, first_unconf_index)?;
                let btc_blocks_since_first =
                    btc_height - first_unconf.signed_at_btc_height.unwrap_or(0);
                let miners_excluded_cps =
                    btc_blocks_since_first >= config.target_checkpoint_inclusion;

                let last_unconf_index = self.last_completed_index(store)?;
                let last_unconf = self.get(store, last_unconf_index)?;
                let btc_blocks_since_last =
                    btc_height - last_unconf.signed_at_btc_height.unwrap_or(0);
                let block_was_mined = btc_blocks_since_last > 0;

                if miners_excluded_cps && block_was_mined {
                    // Blocks were mined since a signed checkpoint, but it was
                    // not included.
                    adjust_fee_rate(prev_fee_rate, true, &config)
                } else {
                    prev_fee_rate
                }
            } else {
                let has_completed = self.last_completed_index(store).is_ok();
                if has_completed {
                    // No unconfirmed checkpoints.
                    adjust_fee_rate(prev_fee_rate, false, &config)
                } else {
                    // This case only happens at start of chain - having no
                    // unconfs doesn't mean anything.
                    prev_fee_rate
                }
            };

            let mut building = self.building(store)?;
            building.fee_rate = fee_rate;
            let building_checkpoint_batch = &mut building.batches[BatchType::Checkpoint];
            let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();

            // The new checkpoint tx's first input is the reserve output from
            // the previous checkpoint.
            let input = Input::new(
                reserve_outpoint,
                &sigset,
                &[0u8], // TODO: double-check safety
                reserve_value,
                config.sigset_threshold,
            )?;
            checkpoint_tx.input.push(input);

            // Add any excess inputs and outputs from the previous checkpoint to
            // the new checkpoint.
            for input in excess_inputs {
                let shares = input.signatures.shares();
                let mut data = input.clone();
                data.signatures = ThresholdSig::from_shares(shares);
                checkpoint_tx.input.push(data);
            }
            for output in excess_outputs {
                checkpoint_tx.output.push(output);
            }

            let index = self.index(store);
            self.set(store, index, &building)?;
        }

        Ok(true)
    }

    /// Prunes old checkpoints from the queue.
    pub fn prune(&mut self, store: &mut dyn Storage) -> ContractResult<()> {
        let latest = self.building(store)?.create_time();
        let mut queue_len = CHECKPOINTS.len(store)?;
        while let Some(oldest) = CHECKPOINTS.front(store)? {
            // TODO: move to min_checkpoints field in config
            if queue_len <= 10 {
                break;
            }

            if latest - oldest.create_time() <= self.config(store).max_age {
                break;
            }

            CHECKPOINTS.pop_front(store)?;
            queue_len -= 1;
        }

        Ok(())
    }

    pub fn should_push(
        &mut self,
        env: Env,
        store: &dyn Storage,
        timestamping_commitment: &[u8],
        btc_height: u32,
    ) -> ContractResult<bool> {
        // Do not push if there is a checkpoint in the `Signing` state. There
        // should only ever be at most one checkpoint in this state.
        if self.signing(store)?.is_some() {
            return Ok(false);
        }

        if !CHECKPOINTS.is_empty(store)? {
            let now = env.block.time.seconds();
            let elapsed = now - self.building(store)?.create_time();

            // Do not push if the minimum checkpoint interval has not elapsed
            // since creating the current `Building` checkpoint.
            if elapsed < self.config(store).min_checkpoint_interval {
                return Ok(false);
            }

            // Do not push if Bitcoin headers are being backfilled (e.g. the
            // current latest height is less than the height at which the last
            // confirmed checkpoint was signed).
            if let Ok(last_completed_index) = self.last_completed_index(store) {
                let last_completed = self.get(store, last_completed_index)?;
                let last_signed_height = last_completed.signed_at_btc_height.unwrap_or(0);
                if btc_height < last_signed_height {
                    return Ok(false);
                }
            }
            let cp_miner_fees =
                self.calc_fee_checkpoint(store, self.index(store), timestamping_commitment)?;
            let building = self.building(store)?;

            // Don't push if there are no pending deposits, withdrawals, or
            // transfers, or if not enough has been collected to pay for the
            // miner fee, unless the maximum checkpoint interval has elapsed
            // since creating the current `Building` checkpoint.
            if elapsed < self.config(store).max_checkpoint_interval || self.index(store) == 0 {
                let checkpoint_tx = building.checkpoint_tx()?;
                let has_pending_deposit = if self.index(store) == 0 {
                    !checkpoint_tx.input.is_empty()
                } else {
                    checkpoint_tx.input.len() > 1
                };

                let has_pending_withdrawal = !checkpoint_tx.output.is_empty();
                let has_pending_transfers = building.pending.first().is_some();

                if !has_pending_deposit && !has_pending_withdrawal && !has_pending_transfers {
                    return Ok(false);
                }

                if building.fees_collected < cp_miner_fees {
                    #[cfg(debug_assertions)]
                    println!(
                        "Not enough collected to pay miner fee: {} < {}",
                        building.fees_collected, cp_miner_fees,
                    );
                    return Ok(false);
                }
            }

            // Do not push if the reserve value is not enough to spend the output & miner fees
            let (input_amount, output_amount) =
                building.calc_total_input_and_output(&self.config(store))?;
            if input_amount < output_amount + cp_miner_fees {
                #[cfg(debug_assertions)]
                println!(
                    "Total reserve value is not enough to spend the output + miner fee: {} < {}. Output amount: {}; cp_miner_fees: {}",
                    input_amount,
                    output_amount + cp_miner_fees,
                    output_amount,
                    cp_miner_fees
                );
                return Ok(false);
            }
        }

        // Do not push if there are too many unconfirmed checkpoints.
        //
        // If there is a long chain of unconfirmed checkpoints, there is possibly an
        // issue causing the transactions to not be included on Bitcoin (e.g. an
        // invalid transaction was created, the fee rate is too low even after
        // adjustments, Bitcoin miners are censoring the transactions, etc.), in
        // which case the network should evaluate and fix the issue before creating
        // more checkpoints.
        //
        // This will also stop the fee rate from being adjusted too high if the
        // issue is simply with relayers failing to report the confirmation of the
        // checkpoint transactions.
        let unconfs = self.num_unconfirmed(store)?;
        if unconfs >= self.config(store).max_unconfirmed_checkpoints {
            return Ok(false);
        }

        // Increment the index. For the first checkpoint, leave the index at
        // zero.
        let mut index = self.index(store);
        if !CHECKPOINTS.is_empty(store)? {
            index += 1;
        }

        // Build the signatory set for the new checkpoint based on the current
        // validator set.
        let sigset = SignatorySet::from_validator_ctx(store, env.block.time.seconds(), index)?;
        // Do not push if there are no validators in the signatory set.
        if sigset.possible_vp() == 0 {
            return Ok(false);
        }

        // Do not push if the signatory set does not have a quorum.
        if !sigset.has_quorum() {
            return Ok(false);
        }

        // Otherwise, push a new checkpoint.
        Ok(true)
    }

    pub fn calc_fee_checkpoint(
        &self,
        store: &dyn Storage,
        cp_index: u32,
        timestamping_commitment: &[u8],
    ) -> ContractResult<u64> {
        let cp = self.get(store, cp_index)?;
        let additional_fees = self.fee_adjustment(store, cp.fee_rate, &self.config(store))?;
        let base_fee = cp.base_fee(&self.config(store), timestamping_commitment)?;
        let total_fee = base_fee + additional_fees;

        Ok(total_fee)
    }

    pub fn maybe_push(
        &mut self,
        env: Env,
        store: &mut dyn Storage,
        deposits_enabled: bool,
    ) -> ContractResult<Option<BuildingCheckpoint>> {
        // Increment the index. For the first checkpoint, leave the index at
        // zero.
        let mut index = self.index(store);
        if !CHECKPOINTS.is_empty(store)? {
            index += 1;
        }

        // Build the signatory set for the new checkpoint based on the current
        // validator set.
        let sigset = SignatorySet::from_validator_ctx(store, env.block.time.seconds(), index)?;

        // Do not push if there are no validators in the signatory set.
        if sigset.possible_vp() == 0 {
            return Ok(None);
        }

        // Do not push if the signatory set does not have a quorum.
        if !sigset.has_quorum() {
            return Ok(None);
        }

        let _ = BUILDING_INDEX.save(store, &index);
        CHECKPOINTS.push_back(store, &Checkpoint::new(sigset)?)?;

        let mut building = self.building(store)?;
        building.deposits_enabled = deposits_enabled;

        let index = self.index(store);
        self.set(store, index, &building)?;

        Ok(Some(building))
    }

    /// The active signatory set, which is the signatory set for the `Building`
    /// checkpoint.
    pub fn active_sigset(&self, store: &dyn Storage) -> ContractResult<SignatorySet> {
        Ok(self.building(store)?.sigset.clone())
    }

    /// Process a batch of signatures, applying them to the checkpoint with the
    /// given index.
    ///
    /// Note that signatures can be sumitted to checkpoints which are already
    /// complete, causing them to be over-signed (which does not affect their
    /// validity). This is useful for letting all signers submit, regardless of
    /// whether they are faster or slower than the other signers. This is
    /// useful, for example, in being able to check if a signer is offline.
    ///
    /// If the batch of signatures causes the checkpoint to be fully signed, it
    /// will be advanced to the `Complete` state.
    ///
    /// This method is exempt from paying transaction fees since the amount of
    /// signatures that can be submitted is capped and this type of transaction
    /// cannot be used to DoS the network.
    pub fn sign(
        &mut self,
        api: &dyn Api,
        store: &mut dyn Storage,
        xpub: &Xpub,
        sigs: Vec<Signature>,
        index: u32,
        btc_height: u32,
    ) -> ContractResult<()> {
        let mut checkpoint = self.get(store, index)?;
        let status = checkpoint.status.clone();
        if matches!(status, CheckpointStatus::Building) {
            return Err(ContractError::Checkpoint(
                "Checkpoint is still building".into(),
            ));
        }

        checkpoint.sign(api, xpub, sigs, btc_height)?;

        if matches!(status, CheckpointStatus::Signing) && checkpoint.signed() {
            let checkpoint_tx = checkpoint.checkpoint_tx()?;
            #[cfg(debug_assertions)]
            println!("Checkpoint signing complete {:?}", checkpoint_tx);
            checkpoint.advance();
            checkpoint.status = CheckpointStatus::Complete
        }

        self.set(store, index, &checkpoint)?;

        Ok(())
    }

    /// The signatory set for the checkpoint with the given index.
    pub fn sigset(&self, store: &dyn Storage, index: u32) -> ContractResult<SignatorySet> {
        Ok(self.get(store, index)?.sigset.clone())
    }

    /// Query building miner fee for checking with fee_collected
    pub fn query_building_miner_fee(
        &self,
        store: &dyn Storage,
        cp_index: u32,
        timestamping_commitment: [u8; 32],
    ) -> ContractResult<u64> {
        self.calc_fee_checkpoint(store, cp_index, &timestamping_commitment)
    }

    /// The number of completed checkpoints which have not yet been confirmed on
    /// the Bitcoin network.
    pub fn num_unconfirmed(&self, store: &dyn Storage) -> ContractResult<u32> {
        let has_signing = self.signing(store)?.is_some();
        let signing_offset = has_signing as u32;

        let last_completed_index = self.index(store).checked_sub(1 + signing_offset);
        let last_completed_index = match last_completed_index {
            None => return Ok(0),
            Some(index) => index,
        };

        let confirmed_index = match self.confirmed_index(store) {
            None => return Ok(self.len(store)? - 1 - signing_offset),
            Some(index) => index,
        };

        Ok(last_completed_index - confirmed_index)
    }

    pub fn first_unconfirmed_index(&self, store: &dyn Storage) -> ContractResult<Option<u32>> {
        let num_unconf = self.num_unconfirmed(store)?;
        if num_unconf == 0 {
            return Ok(None);
        }

        let has_signing = self.signing(store)?.is_some();
        let signing_offset = has_signing as u32;

        Ok(Some(self.index(store) - num_unconf - signing_offset))
    }

    pub fn unconfirmed(&self, store: &dyn Storage) -> ContractResult<Vec<Checkpoint>> {
        let first_unconf_index = self.first_unconfirmed_index(store)?;
        if let Some(index) = first_unconf_index {
            let mut out = vec![];
            for i in index..=self.index(store) {
                let cp = self.get(store, i)?;
                if !matches!(cp.status, CheckpointStatus::Complete) {
                    break;
                }
                out.push(cp);
            }
            Ok(out)
        } else {
            Ok(vec![])
        }
    }

    pub fn unhandled_confirmed(&self, store: &dyn Storage) -> ContractResult<Vec<u32>> {
        if self.confirmed_index(store).is_none() {
            return Ok(vec![]);
        }

        let mut out = vec![];
        for i in self.first_unhandled_confirmed_index(store)..=self.confirmed_index(store).unwrap()
        {
            let cp = self.get(store, i)?;
            if !matches!(cp.status, CheckpointStatus::Complete) {
                #[cfg(debug_assertions)]
                println!("Existing confirmed checkpoint without 'complete' status.");
                break;
            }
            out.push(i);
        }
        Ok(out)
    }

    pub fn unconfirmed_fees_paid(&self, store: &dyn Storage) -> ContractResult<u64> {
        self.unconfirmed(store)?
            .iter()
            .map(|cp| cp.checkpoint_tx_miner_fees())
            .try_fold(0, |fees, result: ContractResult<_>| {
                let fee = result?;
                Ok::<_, ContractError>(fees + fee)
            })
    }

    pub fn unconfirmed_vbytes(
        &self,
        store: &dyn Storage,
        config: &CheckpointConfig,
    ) -> ContractResult<u64> {
        self.unconfirmed(store)?
            .iter()
            .map(|cp| cp.est_vsize(config, &[0; 32])) // TODO: shouldn't need to pass fixed length commitment to est_vsize
            .try_fold(0, |sum, result: ContractResult<_>| {
                let vbytes = result?;
                Ok::<_, ContractError>(sum + vbytes)
            })
    }

    fn fee_adjustment(
        &self,
        store: &dyn Storage,
        fee_rate: u64,
        config: &CheckpointConfig,
    ) -> ContractResult<u64> {
        let unconf_fees_paid = self.unconfirmed_fees_paid(store)?;
        let unconf_vbytes = self.unconfirmed_vbytes(store, config)?;
        Ok((unconf_vbytes * fee_rate).saturating_sub(unconf_fees_paid))
    }
}

/// Takes a previous fee rate and returns a new fee rate, adjusted up or down by
/// 25%. The new fee rate is capped at the maximum and minimum fee rates
/// specified in the given config.
pub fn adjust_fee_rate(prev_fee_rate: u64, up: bool, config: &CheckpointConfig) -> u64 {
    if up {
        (prev_fee_rate * 5 / 4).max(prev_fee_rate + 1)
    } else {
        (prev_fee_rate * 3 / 4).min(prev_fee_rate - 1)
    }
    .clamp(config.min_fee_rate, config.max_fee_rate)
}
