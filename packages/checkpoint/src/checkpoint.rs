use super::{
    signatory::SignatorySet,
    threshold_sig::{Signature, ThresholdSig},
};
use crate::{adapter::Adapter, msg::ConsensusKey};
use crate::{
    bitcoin::{signatory::derive_pubkey, Nbtc},
    constants::{
        MAX_CHECKPOINT_AGE, MAX_CHECKPOINT_INTERVAL, MAX_FEE_RATE, MIN_FEE_RATE, USER_FEE_FACTOR,
    },
};
use crate::{
    constants::DEFAULT_FEE_RATE,
    error::{ContractError, ContractResult},
};
use bitcoin::{blockdata::transaction::EcdsaSighashType, Sequence, Transaction, TxIn, TxOut};
use bitcoin::{hashes::Hash, Script};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, ContractResult, StdError, StdResult, Storage};
use derive_more::{Deref, DerefMut};

use crate::signatory::SIGSET_THRESHOLD;

// use std::convert::TryFrom;
use std::{
    default,
    ops::{Deref, DerefMut},
};

/// The status of a checkpoint. Checkpoints start as `Building`, and eventually
/// advance through the three states.
#[derive(Default)]
pub enum CheckpointStatus {
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
#[derive(Debug)]
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
    pub fn to_txin(&self, store: &mut dyn Storage) -> ContractResult<TxIn> {
        let mut witness = self.signatures.to_witness(store)?;
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
        store: &mut dyn Storage,
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
            dest: dest.encode()?.try_into()?,
            amount,
            est_witness_vsize: sigset.est_witness_vsize(),
            signatures: ThresholdSig::from_sigset(store, sigset)?,
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
#[derive(Debug, Default)]
pub struct BitcoinTx {
    /// The locktime field included in the bitcoin transaction, representing
    /// either a block height or timestamp.
    pub lock_time: u32,

    /// A counter representing how many inputs have been fully-signed so far.
    /// The transaction is valid and ready to be broadcast to the bitcoin
    /// network once all inputs have been signed.
    pub signed_inputs: u16,

    /// The inputs to the transaction.
    pub input: Deque<Input>,

    /// The outputs to the transaction.
    pub output: Deque<Output>,
}

impl BitcoinTx {
    /// Converts the `BitcoinTx` to a `bitcoin::Transaction`.
    pub fn to_bitcoin_tx(&self) -> ContractResult<Transaction> {
        Ok(bitcoin::Transaction {
            version: 1,
            lock_time: bitcoin::PackedLockTime(self.lock_time),
            input: self
                .input
                .iter()?
                .map(|input| input?.to_txin())
                .collect::<ContractResult<Vec<TxIn>>>()?,
            output: self
                .output
                .iter()?
                .map(|output| Ok((**output?).clone()))
                .collect::<ContractResult<Vec<TxOut>>>()?,
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
        self.signed_inputs as u64 == self.input.len()
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
        #[allow(clippy::manual_try_fold)]
        self.output
            .iter()?
            .fold(Ok(0), |sum: ContractResult<u64>, out| Ok(sum? + out?.value))
    }

    /// Calculates the sighash to be signed for the given input index, and
    /// populates the input's signing state with it. This should be used when a
    /// transaction is finalized and its structure will not change, and
    /// coordination of signing will begin.
    pub fn populate_input_sig_message(&mut self, input_index: usize) -> ContractResult<()> {
        let bitcoin_tx = self.to_bitcoin_tx()?;
        let mut sc = bitcoin::util::sighash::SighashCache::new(&bitcoin_tx);
        let mut input = self
            .input
            .get_mut(input_index as u64)?
            .ok_or(Error::InputIndexOutOfBounds(input_index))?;

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
            return Err(Error::BitcoinFee(fee));
        }

        // This algorithm calculates the amount to attempt to deduct from each
        // output (`threshold`), and then removes any outputs which are too
        // small to pay this. Since removing outputs changes the threshold,
        // additional iterations will be required until all remaining outputs
        // are large enough.
        let threshold = loop {
            // The threshold is the fee divided by the number of outputs (each
            // output pays an equal share of the fee).
            let threshold = fee / self.output.len();

            // Remove any outputs which are too small to pay the threshold.
            let mut min_output = u64::MAX;
            self.output.retain_unordered(|output| {
                let dust_value = output.script_pubkey.dust_value().to_sat();
                let adjusted_output = output.value.saturating_sub(dust_value);
                if adjusted_output < min_output {
                    min_output = adjusted_output;
                }
                Ok(adjusted_output > threshold)
            })?;

            // Handle the case where no outputs remain.
            if self.output.is_empty() {
                break threshold;
            }

            // If the threshold is less than the smallest output, we can stop
            // here.
            let threshold = fee / self.output.len();
            if min_output >= threshold {
                break threshold;
            }
        };

        // Deduct the final fee share from each remaining output.
        for i in 0..self.output.len() {
            let mut output = self.output.get_mut(i)?.unwrap();
            output.value -= threshold;
        }

        Ok(())
    }
}

/// `BatchType` represents one of the three types of transaction batches in a
/// checkpoint.
#[derive(Debug)]
pub enum BatchType {
    /// The batch containing the "final emergency disbursal transactions".
    ///
    /// This batch will contain at least one and potentially many transactions,
    /// paying out to the recipients of the emergency disbursal (e.g. recovery
    /// wallets of nBTC holders).
    Disbursal,

    /// The batch containing the intermediate transaction.
    ///
    /// This batch will always contain exactly one transaction, the
    /// "intermediate emergency disbursal transaction", which spends the reserve
    /// output of a stuck checkpoint transaction, and pays out to inputs which
    /// will be spent by the final emergency disbursal transactions.
    IntermediateTx,

    /// The batch containing the checkpoint transaction. This batch will always
    /// contain exactly one transaction, the "checkpoint transaction".
    ///
    /// This transaction spends the reserve output of the previous checkpoint
    /// transaction and the outputs of any incoming deposits. It pays out to the
    /// the latest signatory set (in the "reserve output") and to destinations
    /// of any requested withdrawals.
    Checkpoint,
}

/// A batch of transactions in a checkpoint.
///
/// A batch is a collection of transactions which are atomically signed
/// together. Signatories submit signatures for all inputs in all transactions
/// in the batch at once. Once the batch is fully signed, the checkpoint can
/// advance to signing of the next batch, if any.
#[derive(Default)]
pub struct Batch {
    batch: Deque<BitcoinTx>,
    signed_txs: u16,
}

impl Deref for Batch {
    type Target = Deque<BitcoinTx>;

    fn deref(&self) -> &Self::Target {
        &self.batch
    }
}

impl DerefMut for Batch {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.batch
    }
}

impl Batch {
    fn signed(&self) -> bool {
        self.signed_txs as u64 == self.batch.len()
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
#[derive(Debug, Default)]
pub struct Checkpoint {
    /// The status of the checkpoint, either `Building`, `Signing`, or
    /// `Complete`.
    pub status: CheckpointStatus,

    /// The batches of transactions in the checkpoint, to each be signed
    /// atomically, in order. The first batch contains the "final emergency
    /// disbursal transactions", the second batch contains the "intermediate
    /// emergency disbursal transaction", and the third batch contains the
    /// "checkpoint transaction".
    pub batches: Deque<Batch>,

    /// Pending transfers of nBTC to be processed once the checkpoint is fully
    /// signed. These transfers are processed in lockstep with the checkpointing
    /// process in order to keep nBTC balances in sync with the emergency
    /// disbursal.
    ///
    /// These transfers can be initiated by a simple nBTC send or by a deposit.    
    pub pending: Map<Dest, Coin<Nbtc>>,

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
            batches: Deque::default(),
            pending: Map::new(),
            fee_rate: DEFAULT_FEE_RATE,
            signed_at_btc_height: None,
            deposits_enabled: true,
            sigset,
            fees_collected: 0,
        };

        let disbursal_batch = Batch::default();
        checkpoint.batches.push_front(disbursal_batch)?;

        #[allow(unused_mut)]
        let mut intermediate_tx_batch = Batch::default();
        intermediate_tx_batch.push_back(BitcoinTx::default())?;
        checkpoint.batches.push_back(intermediate_tx_batch)?;

        let checkpoint_tx = BitcoinTx::default();
        let mut checkpoint_batch = Batch::default();
        checkpoint_batch.push_back(checkpoint_tx)?;
        checkpoint.batches.push_back(checkpoint_batch)?;

        Ok(checkpoint)
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
    fn sign(&mut self, xpub: Xpub, sigs: Vec<Signature>, btc_height: u32) -> ContractResult<()> {
        let secp = bitcoin::secp256k1::Secp256k1::verification_only();

        let cp_was_signed = self.signed()?;
        let mut sig_index = 0;

        // Iterate over all batches in the checkpoint, breaking once iterating
        // to a batch which is not ready to be signed.
        for i in 0..self.batches.len() {
            let mut batch = self.batches.get_mut(i)?.unwrap();
            let batch_was_signed = batch.signed();

            // Iterate over all transactions in the batch.
            for j in 0..batch.len() {
                let mut tx = batch.get_mut(j)?.unwrap();
                let tx_was_signed = tx.signed();

                // Iterate over all inputs in the transaction.
                for k in 0..tx.input.len() {
                    let mut input = tx.input.get_mut(k)?.unwrap();
                    let pubkey = derive_pubkey(&secp, xpub, input.sigset_index)?;

                    // Skip input if either the signatory is not part of this
                    // input's signatory set, or the signatory has already
                    // submitted a signature for this input.
                    if !input.signatures.needs_sig(pubkey.into())? {
                        continue;
                    }

                    // Error if there are no remaining supplied signatures - the
                    // signatory supplied less signatures than we require from
                    // them.
                    if sig_index >= sigs.len() {
                        return Err(StdError::generic_err("Not enough signatures supplied"));
                    }
                    let sig = sigs[sig_index];
                    sig_index += 1;

                    // Apply the signature.
                    let input_was_signed = input.signatures.signed();
                    input.signatures.sign(pubkey.into(), sig)?;

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
            return Err(StdError::generic_err("Excess signatures supplied"));
        }

        // If these signatures made the checkpoint fully signed, record the
        // height at which it was signed.
        if self.signed()? && !cp_was_signed {
            self.signed_at_btc_height = Some(btc_height);
        }

        Ok(())
    }

    /// Gets the checkpoint transaction as a `bitcoin::Transaction`.    
    pub fn checkpoint_tx(&self) -> ContractResult<Adapter<bitcoin::Transaction>> {
        Ok(Adapter::new(
            self.batches
                .get(BatchType::Checkpoint as u64)?
                .unwrap()
                .back()?
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
    pub fn to_sign(&self, xpub: Xpub) -> ContractResult<Vec<([u8; 32], u32)>> {
        // TODO: thread local secpk256k1 context
        let secp = bitcoin::secp256k1::Secp256k1::verification_only();

        let mut msgs = vec![];

        for batch in self.batches.iter()? {
            let batch = batch?;
            for tx in batch.iter()? {
                for input in tx?.input.iter()? {
                    let input = input?;

                    let pubkey = derive_pubkey(&secp, xpub, input.sigset_index)?;
                    if input.signatures.needs_sig(pubkey.into())? {
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
    fn signed_batches(&self) -> ContractResult<u64> {
        let mut signed_batches = 0;
        for batch in self.batches.iter()? {
            if batch?.signed() {
                signed_batches += 1;
            } else {
                break;
            }
        }

        Ok(signed_batches)
    }

    /// Returns the current batch being signed, or `None` if all batches are
    /// signed.
    pub fn current_batch(&self) -> ContractResult<Option<Ref<Batch>>> {
        if self.signed()? {
            return Ok(None);
        }

        Ok(Some(self.batches.get(self.signed_batches()?)?.unwrap()))
    }

    /// Returns the timestamp at which the checkpoint was created (when it was
    /// first constructed in the `Building` status).
    pub fn create_time(&self) -> u64 {
        self.sigset.create_time()
    }

    /// Returns `true` if all batches in the checkpoint are fully signed,
    /// otherwise returns `false`.
    pub fn signed(&self) -> ContractResult<bool> {
        Ok(self.signed_batches()? == self.batches.len())
    }

    /// The emergency disbursal transactions for checkpoint.
    ///
    /// The first element of the returned vector is the intermediate
    /// transaction, and the remaining elements are the final transactions.
    pub fn emergency_disbursal_txs(&self) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        let mut txs = vec![];

        let intermediate_tx_batch = self.batches.get(BatchType::IntermediateTx as u64)?.unwrap();
        let Some(intermediate_tx) = intermediate_tx_batch.get(0)? else {
            return Ok(txs);
        };
        txs.push(Adapter::new(intermediate_tx.to_bitcoin_tx()?));

        let disbursal_batch = self.batches.get(BatchType::Disbursal as u64)?.unwrap();
        for tx in disbursal_batch.iter()? {
            txs.push(Adapter::new(tx?.to_bitcoin_tx()?));
        }

        Ok(txs)
    }

    pub fn checkpoint_tx_miner_fees(&self) -> ContractResult<u64> {
        let mut fees = 0;

        let batch = self.batches.get(BatchType::Checkpoint as u64)?.unwrap();
        let tx = batch.get(0)?.unwrap();

        for input in tx.input.iter()? {
            let input = input?;
            fees += input.amount;
        }

        for output in tx.output.iter()? {
            let output = output?;
            fees -= output.value;
        }

        Ok(fees)
    }

    pub fn base_fee(&self, config: &Config, timestamping_commitment: &[u8]) -> ContractResult<u64> {
        let est_vsize = self.est_vsize(config, timestamping_commitment)?;
        Ok(est_vsize * self.fee_rate)
    }

    fn est_vsize(&self, config: &Config, timestamping_commitment: &[u8]) -> ContractResult<u64> {
        let batch = self.batches.get(BatchType::Checkpoint as u64)?.unwrap();
        let cp = batch.get(0)?.unwrap();
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
                .iter()?
                .take(config.max_inputs as usize)
                .try_fold(0, |sum, input| {
                    Ok::<_, Error>(sum + input?.est_witness_vsize)
                })?;

        Ok(vsize)
    }

    // This function will return total input amount and output amount in checkpoint transaction
    pub fn calc_total_input_and_output(&self, config: &Config) -> ContractResult<(u64, u64)> {
        let mut in_amount = 0;
        let checkpoint_batch = self
            .batches
            .get(BatchType::Checkpoint as u64)?
            .ok_or(StdError::generic_err("Cannot get batch checkpoint"))?;
        let checkpoint_tx = checkpoint_batch
            .get(0)?
            .ok_or(StdError::generic_err("Cannot get checkpoint tx"))?;
        for i in 0..config.max_inputs.min(checkpoint_tx.input.len()) {
            let input = checkpoint_tx
                .input
                .get(i)?
                .ok_or(StdError::generic_err("Cannot get checkpoint tx input"))?;
            in_amount += input.amount;
        }
        let mut out_amount = 0;
        for i in 0..config.max_outputs.min(checkpoint_tx.output.len()) {
            let output = checkpoint_tx
                .output
                .get(i)?
                .ok_or(StdError::generic_err("Cannot get checkpoint tx output"))?;
            out_amount += output.value;
        }
        Ok((in_amount, out_amount))
    }

    fn additional_outputs(
        &self,
        config: &Config,
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

/// Configuration parameters used in processing checkpoints.
#[derive(Clone, Default)]
pub struct Config {
    /// The minimum amount of time between the creation of checkpoints, in
    /// seconds.
    ///
    /// If a checkpoint is to be created, but less than this time has passed
    /// since the last checkpoint was created (in the `Building` state), the
    /// current `Building` checkpoint will be delayed in advancing to `Signing`.
    pub min_checkpoint_interval: u64,

    /// The maximum amount of time between the creation of checkpoints, in
    /// seconds.
    ///
    /// If a checkpoint would otherwise not be created, but this amount of time
    /// has passed since the last checkpoint was created (in the `Building`
    /// state), the current `Building` checkpoint will be advanced to `Signing`
    /// and a new `Building` checkpoint will be added.
    pub max_checkpoint_interval: u64,

    /// The maximum number of inputs allowed in a checkpoint transaction.
    ///
    /// This is used to prevent the checkpoint transaction from being too large
    /// to be accepted by the Bitcoin network.
    ///
    /// If a checkpoint has more inputs than this when advancing from `Building`
    /// to `Signing`, the excess inputs will be moved to the suceeding,
    /// newly-created `Building` checkpoint.
    pub max_inputs: u64,

    /// The maximum number of outputs allowed in a checkpoint transaction.
    ///
    /// This is used to prevent the checkpoint transaction from being too large
    /// to be accepted by the Bitcoin network.
    ///
    /// If a checkpoint has more outputs than this when advancing from `Building`
    /// to `Signing`, the excess outputs will be moved to the suceeding,
    /// newly-created `Building` checkpoint.∑
    pub max_outputs: u64,

    /// The default fee rate to use when creating the first checkpoint of the
    /// network, in satoshis per virtual byte.    
    pub fee_rate: u64,

    /// The maximum age of a checkpoint to retain, in seconds.
    ///
    /// Checkpoints older than this will be pruned from the state, down to a
    /// minimum of 10 checkpoints in the checkpoint queue.
    pub max_age: u64,

    /// The number of blocks to target for confirmation of the checkpoint
    /// transaction.
    ///
    /// This is used to adjust the fee rate of the checkpoint transaction, to
    /// ensure it is confirmed within the target number of blocks. The fee rate
    /// will be adjusted up if the checkpoint transaction is not confirmed
    /// within the target number of blocks, and will be adjusted down if the
    /// checkpoint transaction faster than the target.    
    pub target_checkpoint_inclusion: u32,

    /// The lower bound to use when adjusting the fee rate of the checkpoint
    /// transaction, in satoshis per virtual byte.    
    pub min_fee_rate: u64,

    /// The upper bound to use when adjusting the fee rate of the checkpoint
    /// transaction, in satoshis per virtual byte.    
    pub max_fee_rate: u64,

    /// The value (in basis points) to multiply by when calculating the miner
    /// fee to deduct from a user's deposit or withdrawal. This value should be
    /// at least 1 (10,000 basis points).
    ///
    /// The difference in the fee deducted and the fee paid in the checkpoint
    /// transaction is added to the fee pool, to help the network pay for
    /// its own miner fees.    
    pub user_fee_factor: u64,

    /// The threshold of signatures required to spend reserve scripts, as a
    /// ratio represented by a tuple, `(numerator, denominator)`.
    ///
    /// For example, `(9, 10)` means the threshold is 90% of the signatory set.    
    pub sigset_threshold: (u64, u64),

    /// The minimum amount of nBTC an account must hold to be eligible for an
    /// output in the emergency disbursal.    
    pub emergency_disbursal_min_tx_amt: u64,

    /// The amount of time between the creation of a checkpoint and when the
    /// associated emergency disbursal transactions can be spent, in seconds.    
    pub emergency_disbursal_lock_time_interval: u32,

    /// The maximum size of a final emergency disbursal transaction, in virtual
    /// bytes.
    ///
    /// The outputs to be included in final emergency disbursal transactions
    /// will be distributed across multiple transactions around this size.    
    pub emergency_disbursal_max_tx_size: u64,

    /// The maximum number of unconfirmed checkpoints before the network will
    /// stop creating new checkpoints.
    ///
    /// If there is a long chain of unconfirmed checkpoints, there is possibly
    /// an issue causing the transactions to not be included on Bitcoin (e.g. an
    /// invalid transaction was created, the fee rate is too low even after
    /// adjustments, Bitcoin miners are censoring the transactions, etc.), in
    /// which case the network should evaluate and fix the issue before creating
    /// more checkpoints.
    ///
    /// This will also stop the fee rate from being adjusted too high if the
    /// issue is simply with relayers failing to report the confirmation of the
    /// checkpoint transactions.    
    pub max_unconfirmed_checkpoints: u32,
}

impl Config {
    fn regtest() -> Self {
        Self {
            min_checkpoint_interval: 15,
            emergency_disbursal_lock_time_interval: 60,
            emergency_disbursal_max_tx_size: 11,
            user_fee_factor: 20_000,
            max_age: 60 * 60 * 24 * 7 * 3,
            ..Config::bitcoin()
        }
    }

    fn bitcoin() -> Self {
        Self {
            min_checkpoint_interval: 60 * 5,
            max_checkpoint_interval: MAX_CHECKPOINT_INTERVAL,
            max_inputs: 40,
            max_outputs: 200,
            max_age: MAX_CHECKPOINT_AGE,
            target_checkpoint_inclusion: 2,
            min_fee_rate: MIN_FEE_RATE, // relay threshold is 1 sat/vbyte
            max_fee_rate: MAX_FEE_RATE,
            user_fee_factor: USER_FEE_FACTOR, // 2.7x
            sigset_threshold: SIGSET_THRESHOLD,
            emergency_disbursal_min_tx_amt: 1000,
            emergency_disbursal_lock_time_interval: 60 * 60 * 24 * 7 * 8, // 8 weeks
            emergency_disbursal_max_tx_size: 50_000,
            max_unconfirmed_checkpoints: 15,
            fee_rate: 0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        match super::NETWORK {
            bitcoin::Network::Regtest => Config::regtest(),
            bitcoin::Network::Testnet | bitcoin::Network::Bitcoin => Config::bitcoin(),
            _ => unimplemented!(),
        }
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
#[derive(Default)]
pub struct CheckpointQueue {
    /// The checkpoints in the queue, in order from oldest to newest. The last
    /// checkpoint is the checkpoint currently being built, and has the index
    /// contained in the `index` field.
    pub queue: Deque<Checkpoint>,

    /// The index of the checkpoint currently being built.
    pub index: u32,

    /// The index of the last checkpoint which has been confirmed in a Bitcoin
    /// block. Since checkpoints are a sequential cahin, each spending an output
    /// from the previous, all checkpoints with an index lower than this must
    /// have also been confirmed.    
    pub confirmed_index: Option<u32>,

    // the first confirmed checkpoint that we have not handled its pending deposit
    pub first_unhandled_confirmed_cp_index: u32,

    /// Configuration parameters used in processing checkpoints.
    pub config: Config,
}

/// A wrapper around  an immutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Complete` state.
#[derive(Deref)]
pub struct CompletedCheckpoint<'a>(Ref<'a, Checkpoint>);

/// A wrapper around an immutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Signing` state.
#[derive(Deref, Debug)]
pub struct SigningCheckpoint<'a>(Ref<'a, Checkpoint>);

/// A wrapper around a mutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Complete` state.
#[derive(Deref, DerefMut)]
pub struct SigningCheckpointMut<'a>(ChildMut<'a, u64, Checkpoint>);

impl<'a> SigningCheckpointMut<'a> {
    /// Adds a batch of signatures to the checkpoint for the signatory with the
    /// given extended public key (`xpub`).
    ///
    /// The signatures must be provided in the same order as the inputs in the
    /// checkpoint transaction, and must be provided for all inputs in which the
    /// signatory is present in the signatory set.
    pub fn sign(
        &mut self,
        xpub: Xpub,
        sigs: Vec<Signature>,
        btc_height: u32,
    ) -> ContractResult<()> {
        self.0.sign(xpub, sigs, btc_height)
    }

    /// Changes the status of the checkpoint to `Complete`.
    pub fn advance(self) -> ContractResult<()> {
        let mut checkpoint = self.0;

        checkpoint.status = CheckpointStatus::Complete;

        Ok(())
    }
}

/// A wrapper around an immutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Building` state.
#[derive(Deref)]
pub struct BuildingCheckpoint<'a>(Ref<'a, Checkpoint>);

/// A wrapper around a mutable reference to a `Checkpoint` which adds type
/// information guaranteeing that the checkpoint is in the `Building` state.
#[derive(Deref, DerefMut)]
pub struct BuildingCheckpointMut<'a>(ChildMut<'a, u64, Checkpoint>);

/// The data returned by the `advance()` method of `BuildingCheckpointMut`.
type BuildingAdvanceRes = (
    bitcoin::OutPoint,     // reserve outpoint
    u64,                   // reserve size (sats)
    u64,                   // fees paid (sats)
    Vec<ReadOnly<Input>>,  // excess inputs
    Vec<ReadOnly<Output>>, // excess outputs
);

impl<'a> BuildingCheckpointMut<'a> {
    /// Adds an output to the intermediate emergency disbursal transaction of
    /// the checkpoint, to be spent by the given final emergency disbursal
    /// transaction. The corresponding input is also added to the final
    /// emergency disbursal transaction.
    fn link_intermediate_tx(
        &mut self,
        store: &mut dyn Storage,
        tx: &mut BitcoinTx,
        threshold: (u64, u64),
    ) -> ContractResult<()> {
        let sigset = self.sigset.clone();
        let output_script = sigset.output_script(&[0u8], threshold)?;
        let tx_value = tx.value()?;

        let mut intermediate_tx_batch = self
            .batches
            .get_mut(BatchType::IntermediateTx as u64)?
            .unwrap();
        let mut intermediate_tx = intermediate_tx_batch.get_mut(0)?.unwrap();
        let num_outputs = u32::try_from(intermediate_tx.output.len())?;

        let final_tx_input = Input::new(
            store,
            bitcoin::OutPoint::new(intermediate_tx.txid()?, num_outputs),
            &sigset,
            &[0u8],
            tx_value,
            threshold,
        )?;

        let intermediate_tx_output = bitcoin::TxOut {
            value: tx_value,
            script_pubkey: output_script,
        };

        intermediate_tx
            .output
            .push_back(intermediate_tx_output.into())?;

        tx.input.push_back(final_tx_input)?;

        Ok(())
    }

    /// Deducts satoshis from the outputs of all emergency disbursal
    /// transactions (the intermediate transaction and all final transactions)
    /// to make them pay the miner fee at the given fee rate.
    ///
    /// Any outputs which are too small to pay their share of the required fees
    /// will be removed.
    ///
    /// It is possible for this process to remove outputs from the intermediate
    /// transaction, leaving an orphaned final transaction which spends from a
    /// non-existent output. for simplicity the unconnected final transaction is
    /// left in the state (it can be skipped by relayers when broadcasting the
    /// remaining valid emergency disbursal transactions).
    fn deduct_emergency_disbursal_fees(&mut self, fee_rate: u64) -> ContractResult<()> {
        // TODO: Unit tests

        // Deduct fees from intermediate emergency disbursal transaction.
        // Let-binds the amount deducted so we can ensure to deduct the same
        // amount from the final emergency disbursal transactions since the
        // outputs they spend are now worth less than before.
        let intermediate_tx_fee = {
            let mut intermediate_tx_batch = self
                .batches
                .get_mut(BatchType::IntermediateTx as u64)?
                .unwrap();
            let mut intermediate_tx = intermediate_tx_batch.get_mut(0)?.unwrap();
            let fee = intermediate_tx.vsize()? * fee_rate;
            intermediate_tx.deduct_fee(fee)?;
            fee
        };

        let intermediate_tx_batch = self.batches.get(BatchType::IntermediateTx as u64)?.unwrap();
        let intermediate_tx = intermediate_tx_batch.get(0)?.unwrap();
        let intermediate_tx_id = intermediate_tx.txid()?;
        let intermediate_tx_len = intermediate_tx.output.len();

        if intermediate_tx_len == 0 {
            log::warn!("Generated empty emergency disbursal");
            return Ok(());
        }

        // Collect a list of the outputs of the intermediate emergency
        // disbursal, so later on we can ensure there is a 1-to-1 mapping
        // between final transactions and intermediate outputs, matched by
        // amount.
        let mut intermediate_tx_outputs: Vec<(usize, u64)> = intermediate_tx
            .output
            .iter()?
            .enumerate()
            .map(|(i, output)| Ok((i, output?.value)))
            .collect::<Result<_>>()?;

        // Deduct fees from final emergency disbursal transactions. Only retain
        // transactions which have enough value to pay the fee.
        let mut disbursal_batch = self.batches.get_mut(BatchType::Disbursal as u64)?.unwrap();
        disbursal_batch.retain_unordered(|mut tx| {
            // Do not retain transactions which were never linked to the
            // intermediate tx.
            // TODO: is this even possible?
            let mut input = match tx.input.get_mut(0)? {
                Some(input) => input,
                None => return Ok(false),
            };

            // Do not retain transactions which are smaller than the amount of
            // fee applied to the intermediate tx output which they spend. If
            // large enough, deduct the fee from the input to match what was
            // already deducted for the intermediate tx output.
            if input.amount < intermediate_tx_fee / intermediate_tx_len {
                return Ok(false);
            }
            input.amount -= intermediate_tx_fee / intermediate_tx_len;

            // Find the first remaining output of the intermediate tx which
            // matches the amount being spent by this final tx's input.
            for (i, (vout, output)) in intermediate_tx_outputs.iter().enumerate() {
                if output == &(input.amount) {
                    // Once found, link the final tx's input to the vout index
                    // of the the matching output from the intermediate tx, and
                    // remove it from the matching list.

                    input.prevout = Adapter::new(bitcoin::OutPoint {
                        txid: intermediate_tx_id,
                        vout: *vout as u32,
                    });
                    intermediate_tx_outputs.remove(i);
                    // Deduct the final tx's miner fee from its outputs,
                    // removing any outputs which are too small to pay their
                    // share of the fee.
                    let tx_size = tx
                        .vsize()
                        .map_err(|err| StdError::generic_err(err.to_string()))?;
                    let fee = intermediate_tx_fee / intermediate_tx_len + tx_size * fee_rate;
                    tx.deduct_fee(fee)
                        .map_err(|err| StdError::generic_err(err.to_string()))?;

                    return Ok(true);
                }
            }
            Ok(false)
        })?;

        Ok(())
    }

    /// Generates the emergency disbursal transactions for the checkpoint,
    /// populating the first and second transaction batches in the checkpoint.
    ///
    /// The emergency disbursal transactions are generated from a list of
    /// outputs representing the holders of nBTC: one for every nBTC account
    /// which has an associated recovery script, one for every pending transfer
    /// in the checkpoint, and one for every output passed in by the consumer
    /// via the `external_outputs` iterator.
    #[allow(clippy::too_many_arguments)]
    fn generate_emergency_disbursal_txs(
        &mut self,
        nbtc_accounts: &Accounts<Nbtc>,
        recovery_scripts: &Map<Addr, Adapter<bitcoin::Script>>,
        reserve_outpoint: bitcoin::OutPoint,
        external_outputs: impl Iterator<Item = ContractResult<bitcoin::TxOut>>,
        fee_rate: u64,
        reserve_value: u64,
        config: &Config,
    ) -> ContractResult<()> {
        // TODO: Use tree structure instead of single-intermediate, many-final,
        // since the intermediate tx may grow too large

        #[cfg(not(feature = "full"))]
        unimplemented!();

        // Deduct Bitcoin miner fees from the intermediate tx and all final txs.
        self.deduct_emergency_disbursal_fees(fee_rate)?;

        // Populate the sighashes to be signed for each final tx's input.
        let mut disbursal_batch = self.batches.get_mut(BatchType::Disbursal as u64)?.unwrap();
        for i in 0..disbursal_batch.len() {
            let mut tx = disbursal_batch.get_mut(i)?.unwrap();
            for j in 0..tx.input.len() {
                tx.populate_input_sig_message(j.try_into()?)?;
            }
        }

        // Populate the sighashes to be signed for the intermediate tx's input.
        let mut intermediate_tx_batch = self
            .batches
            .get_mut(BatchType::IntermediateTx as u64)?
            .unwrap();
        let mut intermediate_tx = intermediate_tx_batch.get_mut(0)?.unwrap();
        intermediate_tx.populate_input_sig_message(0)?;

        Ok(())
    }

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
    #[allow(unused_variables)]
    pub fn advance(
        mut self,
        nbtc_accounts: &Accounts<Nbtc>,
        recovery_scripts: &Map<Addr, Adapter<bitcoin::Script>>,
        external_outputs: impl Iterator<Item = ContractResult<bitcoin::TxOut>>,
        timestamping_commitment: Vec<u8>,
        cp_fees: u64,
        config: &Config,
    ) -> ContractResult<BuildingAdvanceRes> {
        self.0.status = CheckpointStatus::Signing;

        let outs = self.additional_outputs(config, &timestamping_commitment)?;
        let mut checkpoint_batch = self.batches.get_mut(BatchType::Checkpoint as u64)?.unwrap();
        let mut checkpoint_tx = checkpoint_batch.get_mut(0)?.unwrap();
        for out in outs.iter().rev() {
            checkpoint_tx.output.push_front(Adapter::new(out.clone()))?;
        }

        // Remove excess inputs and outputs from the checkpoint tx, to be pushed
        // onto the suceeding checkpoint while in its `Building` state.
        let mut excess_inputs = vec![];
        while checkpoint_tx.input.len() > config.max_inputs {
            let removed_input = checkpoint_tx.input.pop_back()?.unwrap();
            excess_inputs.push(removed_input);
        }
        let mut excess_outputs = vec![];
        while checkpoint_tx.output.len() > config.max_outputs {
            let removed_output = checkpoint_tx.output.pop_back()?.unwrap();
            excess_outputs.push(removed_output);
        }

        // Sum the total input and output amounts.
        // TODO: Input/Output sum functions
        let mut in_amount = 0;
        for i in 0..checkpoint_tx.input.len() {
            let input = checkpoint_tx.input.get(i)?.unwrap();
            in_amount += input.amount;
        }
        let mut out_amount = 0;
        for i in 0..checkpoint_tx.output.len() {
            let output = checkpoint_tx.output.get(i)?.unwrap();
            out_amount += output.value;
        }

        // Deduct the outgoing amount and calculated fee amount from the reserve
        // input amount, to set the resulting reserve output value.
        let reserve_value = in_amount.checked_sub(out_amount + cp_fees).ok_or_else(|| {
            StdError::generic_err("Insufficient reserve value to cover miner fees")
        })?;
        let mut reserve_out = checkpoint_tx.output.get_mut(0)?.unwrap();
        reserve_out.value = reserve_value;

        // Prepare the checkpoint tx's inputs to be signed by calculating their
        // sighashes.
        let bitcoin_tx = checkpoint_tx.to_bitcoin_tx()?;
        let mut sc = bitcoin::util::sighash::SighashCache::new(&bitcoin_tx);
        for i in 0..checkpoint_tx.input.len() {
            let mut input = checkpoint_tx.input.get_mut(i)?.unwrap();
            let sighash = sc.segwit_signature_hash(
                i as usize,
                &input.redeem_script,
                input.amount,
                EcdsaSighashType::All,
            )?;
            input.signatures.set_message(sighash.into_inner());
        }

        // Generate the emergency disbursal transactions, spending from the
        // reserve output.
        let reserve_outpoint = bitcoin::OutPoint {
            txid: checkpoint_tx.txid()?,
            vout: 0,
        };
        self.generate_emergency_disbursal_txs(
            nbtc_accounts,
            recovery_scripts,
            reserve_outpoint,
            external_outputs,
            self.fee_rate,
            reserve_value,
            config,
        )?;

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
    pub fn insert_pending(&mut self, dest: Dest, coins: Coin<Nbtc>) -> ContractResult<()> {
        let mut amount = self
            .pending
            .remove(dest.clone())?
            .map_or(0.into(), |c| c.amount);
        amount = (amount + coins.amount).result()?;
        self.pending.insert(dest, Coin::mint(amount))?;
        Ok(())
    }
}

impl CheckpointQueue {
    /// Set the queue's configuration parameters.
    pub fn configure(&mut self, config: Config) {
        self.config = config;
    }

    /// The queue's current configuration parameters.
    pub fn config(&self) -> Config {
        self.config.clone()
    }

    /// Removes all checkpoints from the queue and resets the index to zero.
    pub fn reset(&mut self) -> ContractResult<()> {
        self.index = 0;
        super::clear_deque(&mut self.queue)?;

        Ok(())
    }

    /// Gets a reference to the checkpoint at the given index.
    ///
    /// If the index is out of bounds or was pruned, an error is returned.    
    pub fn get(&self, index: u32) -> ContractResult<Ref<'_, Checkpoint>> {
        let index = self.get_deque_index(index)?;
        Ok(self.queue.get(index as u64)?.unwrap())
    }

    /// Gets a mutable reference to the checkpoint at the given index.
    ///
    /// If the index is out of bounds or was pruned, an error is returned.
    pub fn get_mut(&mut self, index: u32) -> ContractResult<ChildMut<'_, u64, Checkpoint>> {
        let index = self.get_deque_index(index)?;
        Ok(self.queue.get_mut(index as u64)?.unwrap())
    }

    /// Calculates the index within the deque based on the given checkpoint
    /// index.
    ///
    /// This is necessary because the values can differ for queues which have
    /// been pruned. For example, a queue may contain 5 checkpoints,
    /// representing indexes 30 to 34. Checkpoint index 30 is at deque index 0,
    /// checkpoint 34 is at deque index 4, and checkpoint index 29 is now
    /// out-of-bounds.
    fn get_deque_index(&self, index: u32) -> ContractResult<u32> {
        let start = self.index + 1 - (self.queue.len() as u32);
        if index > self.index || index < start {
            Err(StdError::generic_err("Index out of bounds"))
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
    pub fn len(&self) -> ContractResult<u32> {
        Ok(u32::try_from(self.queue.len())?)
    }

    /// Returns `true` if there are no checkpoints in the queue.
    ///
    /// This will only be `true` before the first deposit has been processed.
    pub fn is_empty(&self) -> ContractResult<bool> {
        Ok(self.len()? == 0)
    }

    /// The index of the last checkpoint in the queue (aka the `Building`
    /// checkpoint).    
    pub fn index(&self) -> u32 {
        self.index
    }

    /// All checkpoints in the queue, in order from oldest to newest.
    ///
    /// The return value is a vector of tuples, where the first element is the
    /// checkpoint's index, and the second element is a reference to the
    /// checkpoint.    
    pub fn all(&self) -> ContractResult<Vec<(u32, Ref<'_, Checkpoint>)>> {
        // TODO: return iterator
        // TODO: use Deque iterator

        let mut out = Vec::with_capacity(self.queue.len() as usize);

        for i in 0..self.queue.len() {
            let checkpoint = self.queue.get(i)?.unwrap();
            out.push((
                (self.index + 1 - (self.queue.len() as u32 - i as u32)),
                checkpoint,
            ));
        }

        Ok(out)
    }

    /// All checkpoints in the queue which are in the `Complete` state, in order
    /// from oldest to newest.    
    pub fn completed(&self, limit: u32) -> ContractResult<Vec<CompletedCheckpoint<'_>>> {
        // TODO: return iterator
        // TODO: use Deque iterator

        let mut out = vec![];

        let length = self.len()?;
        if length == 0 {
            return Ok(out);
        }

        let skip = if self.signing()?.is_some() { 2 } else { 1 };
        let end = self.index.saturating_sub(skip - 1);

        let start = end - limit.min(length - skip);

        for i in start..end {
            let checkpoint = self.get(i)?;
            out.push(CompletedCheckpoint(checkpoint));
        }

        Ok(out)
    }

    /// The index of the last completed checkpoint.    
    pub fn last_completed_index(&self) -> ContractResult<u32> {
        if self.signing()?.is_some() {
            self.index.checked_sub(2)
        } else {
            self.index.checked_sub(1)
        }
        .ok_or_else(|| Error::Orga(StdError::generic_err("No completed checkpoints yet")))
    }

    pub fn first_index(&self) -> ContractResult<u32> {
        Ok(self.index + 1 - self.len()?)
    }

    /// A reference to the last completed checkpoint.    
    pub fn last_completed(&self) -> ContractResult<Ref<Checkpoint>> {
        self.get(self.last_completed_index()?)
    }

    /// A mutable reference to the last completed checkpoint.
    pub fn last_completed_mut(&mut self) -> ContractResult<ChildMut<u64, Checkpoint>> {
        self.get_mut(self.last_completed_index()?)
    }

    /// The last completed checkpoint, converted to a Bitcoin transaction.    
    pub fn last_completed_tx(&self) -> ContractResult<Adapter<bitcoin::Transaction>> {
        self.last_completed()?.checkpoint_tx()
    }

    /// All completed checkpoints, converted to Bitcoin transactions.    
    pub fn completed_txs(&self, limit: u32) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        self.completed(limit)?
            .into_iter()
            .map(|c| c.checkpoint_tx())
            .collect()
    }

    /// The emergency disbursal transactions for the last completed checkpoint.
    ///
    /// The first element of the returned vector is the intermediate
    /// transaction, and the remaining elements are the final transactions.    
    pub fn emergency_disbursal_txs(&self) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        if let Some(completed) = self.completed(1)?.last() {
            completed.emergency_disbursal_txs()
        } else {
            Ok(vec![])
        }
    }

    /// The last complete builiding checkpoint transaction, which have the type BatchType::Checkpoint
    /// Here we have only one element in the vector, and I use vector because I don't want to throw
    /// any error if vec! is empty    
    pub fn last_completed_checkpoint_tx(
        &self,
    ) -> ContractResult<Vec<Adapter<bitcoin::Transaction>>> {
        let mut txs = vec![];
        if let Some(completed) = self.completed(1)?.last() {
            txs.push(completed.checkpoint_tx()?);
            Ok(txs)
        } else {
            Ok(vec![])
        }
    }

    /// A reference to the checkpoint in the `Signing` state, if there is one.    
    pub fn signing(&self) -> ContractResult<Option<SigningCheckpoint<'_>>> {
        if self.queue.len() < 2 {
            return Ok(None);
        }

        let second = self.get(self.index - 1)?;
        if !matches!(second.status, CheckpointStatus::Signing) {
            return Ok(None);
        }

        Ok(Some(SigningCheckpoint(second)))
    }

    /// A mutable reference to the checkpoint in the `Signing` state, if there
    /// is one.
    pub fn signing_mut(&mut self) -> ContractResult<Option<SigningCheckpointMut>> {
        if self.queue.len() < 2 {
            return Ok(None);
        }

        let second = self.get_mut(self.index - 1)?;
        if !matches!(second.status, CheckpointStatus::Signing) {
            return Ok(None);
        }

        Ok(Some(SigningCheckpointMut(second)))
    }

    /// A reference to the checkpoint in the `Building` state.
    ///
    /// This is the checkpoint which is currently being built, and is not yet
    /// being signed. Other than at the start of the network, before the first
    /// deposit has been received, there will always be a checkpoint in this
    /// state.
    pub fn building(&self) -> ContractResult<BuildingCheckpoint> {
        let last = self.get(self.index)?;
        Ok(BuildingCheckpoint(last))
    }

    /// A mutable reference to the checkpoint in the `Building` state.
    ///
    /// This is the checkpoint which is currently being built, and is not yet
    /// being signed. Other than at the start of the network, before the first
    /// deposit has been received, there will always be a checkpoint in this
    /// state.
    pub fn building_mut(&mut self) -> ContractResult<BuildingCheckpointMut> {
        let last = self.get_mut(self.index)?;
        Ok(BuildingCheckpointMut(last))
    }

    /// Prunes old checkpoints from the queue.
    pub fn prune(&mut self) -> ContractResult<()> {
        let latest = self.building()?.create_time();

        while let Some(oldest) = self.queue.front()? {
            // TODO: move to min_checkpoints field in config
            if self.queue.len() <= 10 {
                break;
            }

            if latest - oldest.create_time() <= self.config.max_age {
                break;
            }

            self.queue.pop_front()?;
        }

        Ok(())
    }

    pub fn calc_fee_checkpoint(
        &self,
        cp_index: u32,
        timestamping_commitment: &[u8],
    ) -> ContractResult<u64> {
        let cp = self.get(cp_index)?;
        let additional_fees = self.fee_adjustment(cp.fee_rate, &self.config)?;
        let base_fee = cp.base_fee(&self.config, timestamping_commitment)?;
        let total_fee = base_fee + additional_fees;

        Ok(total_fee)
    }

    /// The active signatory set, which is the signatory set for the `Building`
    /// checkpoint.    
    pub fn active_sigset(&self) -> ContractResult<SignatorySet> {
        Ok(self.building()?.sigset.clone())
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
        xpub: Xpub,
        sigs: Vec<Signature>,
        index: u32,
        btc_height: u32,
    ) -> ContractResult<()> {
        super::exempt_from_fee()?;

        let mut checkpoint = self.get_mut(index)?;
        let status = checkpoint.status;
        if matches!(status, CheckpointStatus::Building) {
            return Err(StdError::generic_err("Checkpoint is still building"));
        }

        checkpoint.sign(xpub, sigs, btc_height)?;

        if matches!(status, CheckpointStatus::Signing) && checkpoint.signed()? {
            let checkpoint_tx = checkpoint.checkpoint_tx()?;
            info!("Checkpoint signing complete {:?}", checkpoint_tx);
            SigningCheckpointMut(checkpoint).advance()?;
        }

        Ok(())
    }

    /// The signatory set for the checkpoint with the given index.    
    pub fn sigset(&self, index: u32) -> ContractResult<SignatorySet> {
        Ok(self.get(index)?.sigset.clone())
    }

    /// Query building miner fee for checking with fee_collected    
    pub fn query_building_miner_fee(
        &self,
        cp_index: u32,
        timestamping_commitment: [u8; 32],
    ) -> ContractResult<u64> {
        self.calc_fee_checkpoint(cp_index, &timestamping_commitment)
    }

    /// The number of completed checkpoints which have not yet been confirmed on
    /// the Bitcoin network.
    pub fn num_unconfirmed(&self) -> ContractResult<u32> {
        let has_signing = self.signing()?.is_some();
        let signing_offset = has_signing as u32;

        let last_completed_index = self.index.checked_sub(1 + signing_offset);
        let last_completed_index = match last_completed_index {
            None => return Ok(0),
            Some(index) => index,
        };

        let confirmed_index = match self.confirmed_index {
            None => return Ok(self.len()? - 1 - signing_offset),
            Some(index) => index,
        };

        Ok(last_completed_index - confirmed_index)
    }

    pub fn first_unconfirmed_index(&self) -> ContractResult<Option<u32>> {
        let num_unconf = self.num_unconfirmed()?;
        if num_unconf == 0 {
            return Ok(None);
        }

        let has_signing = self.signing()?.is_some();
        let signing_offset = has_signing as u32;

        Ok(Some(self.index - num_unconf - signing_offset))
    }

    pub fn unconfirmed(&self) -> ContractResult<Vec<Ref<'_, Checkpoint>>> {
        let first_unconf_index = self.first_unconfirmed_index()?;
        if let Some(index) = first_unconf_index {
            let mut out = vec![];
            for i in index..=self.index {
                let cp = self.get(i)?;
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

    pub fn unhandled_confirmed(&self) -> ContractResult<Vec<u32>> {
        if self.confirmed_index.is_none() {
            return Ok(vec![]);
        }

        let mut out = vec![];
        for i in self.first_unhandled_confirmed_cp_index..=self.confirmed_index.unwrap() {
            let cp = self.get(i)?;
            if !matches!(cp.status, CheckpointStatus::Complete) {
                log::warn!("Existing confirmed checkpoint without 'complete' status.");
                break;
            }
            out.push(i);
        }
        Ok(out)
    }

    pub fn unconfirmed_fees_paid(&self) -> ContractResult<u64> {
        self.unconfirmed()?
            .iter()
            .map(|cp| cp.checkpoint_tx_miner_fees())
            .try_fold(0, |fees, result: ContractResult<_>| {
                let fee = result?;
                Ok::<_, ContractError>(fees + fee)
            })
    }

    pub fn unconfirmed_vbytes(&self, config: &Config) -> ContractResult<u64> {
        self.unconfirmed()?
            .iter()
            .map(|cp| cp.est_vsize(config, &[0; 32])) // TODO: shouldn't need to pass fixed length commitment to est_vsize
            .try_fold(0, |sum, result: ContractResult<_>| {
                let vbytes = result?;
                Ok::<_, ContractError>(sum + vbytes)
            })
    }

    fn fee_adjustment(&self, fee_rate: u64, config: &Config) -> ContractResult<u64> {
        let unconf_fees_paid = self.unconfirmed_fees_paid()?;
        let unconf_vbytes = self.unconfirmed_vbytes(config)?;
        Ok((unconf_vbytes * fee_rate).saturating_sub(unconf_fees_paid))
    }

    pub fn backfill(
        &mut self,
        first_index: u32,
        redeem_scripts: impl Iterator<Item = Script>,
        threshold_ratio: (u64, u64),
    ) -> ContractResult<()> {
        let mut index = first_index + 1;

        let create_time = self.queue.get(0)?.unwrap().create_time();

        for script in redeem_scripts {
            index -= 1;

            if index >= self.first_index()? {
                continue;
            }

            let (mut sigset, _) = SignatorySet::from_script(&script, threshold_ratio)?;
            sigset.index = index;
            sigset.create_time = create_time;
            let mut cp = Checkpoint::new(sigset)?;
            cp.status = CheckpointStatus::Complete;

            self.queue.push_front(cp)?;
        }

        Ok(())
    }
}

/// Takes a previous fee rate and returns a new fee rate, adjusted up or down by
/// 25%. The new fee rate is capped at the maximum and minimum fee rates
/// specified in the given config.
pub fn adjust_fee_rate(prev_fee_rate: u64, up: bool, config: &Config) -> u64 {
    if up {
        (prev_fee_rate * 5 / 4).max(prev_fee_rate + 1)
    } else {
        (prev_fee_rate * 3 / 4).min(prev_fee_rate - 1)
    }
    .min(config.max_fee_rate)
    .max(config.min_fee_rate)
}

#[cfg(test)]
mod test {
    use crate::bitcoin::{signatory::Signatory, threshold_sig::Pubkey};

    use std::{cell::RefCell, rc::Rc};

    use super::*;

    fn push_bitcoin_tx_output(tx: &mut BitcoinTx, value: u64) {
        let tx_out = bitcoin::TxOut {
            value,
            script_pubkey: bitcoin::Script::new(),
        };
        tx.output.push_back(Output::new(tx_out)).unwrap();
    }

    #[test]
    fn deduct_fee() {
        let mut bitcoin_tx = BitcoinTx::default();
        push_bitcoin_tx_output(&mut bitcoin_tx, 0);
        push_bitcoin_tx_output(&mut bitcoin_tx, 10000);

        bitcoin_tx.deduct_fee(100).unwrap();

        assert_eq!(bitcoin_tx.output.len(), 1);
        assert_eq!(bitcoin_tx.output.get(0).unwrap().unwrap().value, 9900);
    }

    #[test]
    fn deduct_fee_multi_pass() {
        let mut bitcoin_tx = BitcoinTx::default();
        push_bitcoin_tx_output(&mut bitcoin_tx, 502);
        push_bitcoin_tx_output(&mut bitcoin_tx, 482);
        push_bitcoin_tx_output(&mut bitcoin_tx, 300);

        bitcoin_tx.deduct_fee(30).unwrap();

        assert_eq!(bitcoin_tx.output.len(), 1);
        assert_eq!(bitcoin_tx.output.get(0).unwrap().unwrap().value, 472);
    }

    #[test]
    fn deduct_fee_multi_pass_empty_result() {
        let mut bitcoin_tx = BitcoinTx::default();
        push_bitcoin_tx_output(&mut bitcoin_tx, 60);
        push_bitcoin_tx_output(&mut bitcoin_tx, 70);
        push_bitcoin_tx_output(&mut bitcoin_tx, 100);

        bitcoin_tx.deduct_fee(200).unwrap();
    }

    //TODO: More fee deduction tests

    fn create_queue_with_statuses(complete: u32, signing: bool) -> CheckpointQueue {
        let mut queue = CheckpointQueue::default();
        let mut push = |status| {
            let mut cp = Checkpoint {
                status,
                batches: Deque::new(),
                pending: Map::new(),
                fee_rate: DEFAULT_FEE_RATE,
                signed_at_btc_height: None,
                deposits_enabled: true,
                sigset: SignatorySet::default(),
                fees_collected: 0,
            };
            cp.status = status;
            queue.queue.push_back(cp).unwrap();
        };

        queue.index = complete;

        for _ in 0..complete {
            push(CheckpointStatus::Complete);
        }
        if signing {
            push(CheckpointStatus::Signing);
            queue.index += 1;
        }
        push(CheckpointStatus::Building);

        queue
    }

    #[test]
    fn completed_with_signing() {
        let queue = create_queue_with_statuses(10, true);
        let cp = queue.completed(1).unwrap();
        assert_eq!(cp.len(), 1);
        assert_eq!(cp[0].status, CheckpointStatus::Complete);
    }

    #[test]
    fn completed_without_signing() {
        let queue = create_queue_with_statuses(10, false);
        let cp = queue.completed(1).unwrap();
        assert_eq!(cp.len(), 1);
        assert_eq!(cp[0].status, CheckpointStatus::Complete);
    }

    #[test]
    fn completed_no_complete() {
        let queue = create_queue_with_statuses(0, false);
        let cp = queue.completed(10).unwrap();
        assert_eq!(cp.len(), 0);
    }

    #[test]
    fn completed_zero_limit() {
        let queue = create_queue_with_statuses(10, false);
        let cp = queue.completed(0).unwrap();
        assert_eq!(cp.len(), 0);
    }

    #[test]
    fn completed_oversized_limit() {
        let queue = create_queue_with_statuses(10, false);
        let cp = queue.completed(100).unwrap();
        assert_eq!(cp.len(), 10);
    }

    #[test]
    fn completed_pruned() {
        let mut queue = create_queue_with_statuses(10, false);
        queue.index += 10;
        let cp = queue.completed(2).unwrap();
        assert_eq!(cp.len(), 2);
        assert_eq!(cp[1].status, CheckpointStatus::Complete);
    }

    #[test]
    fn num_unconfirmed() {
        let mut queue = create_queue_with_statuses(10, false);
        queue.confirmed_index = Some(5);
        assert_eq!(queue.num_unconfirmed().unwrap(), 4);

        let mut queue = create_queue_with_statuses(10, true);
        queue.confirmed_index = Some(5);
        assert_eq!(queue.num_unconfirmed().unwrap(), 4);

        let mut queue = create_queue_with_statuses(0, false);
        queue.confirmed_index = None;
        assert_eq!(queue.num_unconfirmed().unwrap(), 0);

        let mut queue = create_queue_with_statuses(0, true);
        queue.confirmed_index = None;
        assert_eq!(queue.num_unconfirmed().unwrap(), 0);

        let mut queue = create_queue_with_statuses(10, false);
        queue.confirmed_index = None;
        assert_eq!(queue.num_unconfirmed().unwrap(), 10);

        let mut queue = create_queue_with_statuses(10, true);
        queue.confirmed_index = None;
        assert_eq!(queue.num_unconfirmed().unwrap(), 10);
    }

    #[test]
    fn first_unconfirmed_index() {
        let mut queue = create_queue_with_statuses(10, false);
        queue.confirmed_index = Some(5);
        assert_eq!(queue.first_unconfirmed_index().unwrap(), Some(6));

        let mut queue = create_queue_with_statuses(10, true);
        queue.confirmed_index = Some(5);
        assert_eq!(queue.first_unconfirmed_index().unwrap(), Some(6));

        let mut queue = create_queue_with_statuses(0, false);
        queue.confirmed_index = None;
        assert_eq!(queue.first_unconfirmed_index().unwrap(), None);

        let mut queue = create_queue_with_statuses(0, true);
        queue.confirmed_index = None;
        assert_eq!(queue.first_unconfirmed_index().unwrap(), None);

        let mut queue = create_queue_with_statuses(10, false);
        queue.confirmed_index = None;
        assert_eq!(queue.first_unconfirmed_index().unwrap(), Some(0));

        let mut queue = create_queue_with_statuses(10, true);
        queue.confirmed_index = None;
        assert_eq!(queue.first_unconfirmed_index().unwrap(), Some(0));
    }

    #[test]
    fn adjust_fee_rate() {
        let config = Config::default();
        assert_eq!(super::adjust_fee_rate(100, true, &config), 125);
        assert_eq!(super::adjust_fee_rate(100, false, &config), 75);
        assert_eq!(super::adjust_fee_rate(2, true, &config), 40);
        assert_eq!(super::adjust_fee_rate(0, true, &config), 40);
        assert_eq!(super::adjust_fee_rate(2, false, &config), 40);
        assert_eq!(super::adjust_fee_rate(200, true, &config), 250);
        assert_eq!(super::adjust_fee_rate(300, true, &config), 375);
    }

    fn sigset(n: u32) -> SignatorySet {
        let mut sigset = SignatorySet::default();
        sigset.index = n;
        sigset.create_time = n as u64;

        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[(n + 1) as u8; 32]).unwrap();
        let pubkey: Pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(
            &bitcoin::secp256k1::Secp256k1::new(),
            &secret,
        )
        .into();

        sigset.signatories.push(Signatory {
            pubkey: pubkey.into(),
            voting_power: 100,
        });

        sigset.possible_vp = 100;
        sigset.present_vp = 100;

        sigset
    }

    #[test]
    fn backfill_basic() {
        let mut queue = CheckpointQueue::default();
        queue.index = 10;
        queue
            .queue
            .push_back(Checkpoint::new(sigset(7)).unwrap())
            .unwrap();
        queue
            .queue
            .push_back(Checkpoint::new(sigset(8)).unwrap())
            .unwrap();
        queue
            .queue
            .push_back(Checkpoint::new(sigset(9)).unwrap())
            .unwrap();
        queue
            .queue
            .push_back(Checkpoint::new(sigset(10)).unwrap())
            .unwrap();

        let backfill_data = vec![
            sigset(8).redeem_script(&[0], (2, 3)).unwrap(),
            sigset(7).redeem_script(&[0], (2, 3)).unwrap(),
            sigset(6).redeem_script(&[0], (2, 3)).unwrap(),
            sigset(5).redeem_script(&[0], (2, 3)).unwrap(),
            sigset(4).redeem_script(&[0], (2, 3)).unwrap(),
            sigset(3).redeem_script(&[0], (2, 3)).unwrap(),
        ];
        queue
            .backfill(8, backfill_data.into_iter(), (2, 3))
            .unwrap();

        assert_eq!(queue.len().unwrap(), 8);
        assert_eq!(queue.index, 10);
        assert_eq!(
            queue
                .get(3)
                .unwrap()
                .sigset
                .redeem_script(&[0], (2, 3))
                .unwrap(),
            sigset(3).redeem_script(&[0], (2, 3)).unwrap(),
        );
        assert_eq!(
            queue
                .get(10)
                .unwrap()
                .sigset
                .redeem_script(&[0], (2, 3))
                .unwrap(),
            sigset(10).redeem_script(&[0], (2, 3)).unwrap(),
        );
    }

    #[test]
    fn backfill_with_zeroth() {
        let mut queue = CheckpointQueue::default();
        queue.index = 1;
        queue
            .queue
            .push_back(Checkpoint::new(sigset(1)).unwrap())
            .unwrap();

        let backfill_data = vec![sigset(0).redeem_script(&[0], (2, 3)).unwrap()];
        queue
            .backfill(0, backfill_data.into_iter(), (2, 3))
            .unwrap();

        assert_eq!(queue.len().unwrap(), 2);
        assert_eq!(queue.index, 1);
        assert_eq!(
            queue
                .get(0)
                .unwrap()
                .sigset
                .redeem_script(&[0], (2, 3))
                .unwrap(),
            sigset(0).redeem_script(&[0], (2, 3)).unwrap(),
        );
        assert_eq!(
            queue
                .get(1)
                .unwrap()
                .sigset
                .redeem_script(&[0], (2, 3))
                .unwrap(),
            sigset(1).redeem_script(&[0], (2, 3)).unwrap(),
        );
    }
}
