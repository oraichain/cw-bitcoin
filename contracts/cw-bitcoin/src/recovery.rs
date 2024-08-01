use super::{
    checkpoint::{BitcoinTx, Input},
    signatory::SignatorySet,
    threshold_sig::Signature,
};
use crate::{
    adapter::Adapter,
    error::{ContractError, ContractResult},
    interface::{Dest, Xpub},
    state::RECOVERY_TXS,
};
use bitcoin::{OutPoint, Transaction, TxOut};
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::{QuerierWrapper, Storage};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct RecoveryTx {
    tx: BitcoinTx,
    old_sigset_index: u32,
    new_sigset_index: u32,
    dest: Dest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct SignedRecoveryTx {
    pub tx: Adapter<Transaction>,
    pub sigset_index: u32,
    pub dest: Dest,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct RecoveryTxs {}

pub struct RecoveryTxInput<'a> {
    pub expired_tx: Transaction,
    pub vout: u32,
    pub old_sigset: &'a SignatorySet,
    pub new_sigset: &'a SignatorySet,
    pub threshold: (u64, u64),
    pub fee_rate: u64,
    pub dest: Dest,
}

impl RecoveryTxs {
    pub fn create_recovery_tx(
        &mut self,
        store: &mut dyn Storage,
        args: RecoveryTxInput,
    ) -> ContractResult<()> {
        let expired_output = args
            .expired_tx
            .output
            .get(args.vout as usize)
            .ok_or_else(|| ContractError::Signer("Invalid recovery tx vout".to_string()))?;

        let input = Input::new(
            OutPoint::new(args.expired_tx.txid(), args.vout),
            args.old_sigset,
            &args.dest.commitment_bytes()?,
            expired_output.value,
            args.threshold,
        )?;
        let script_pubkey = args
            .new_sigset
            .output_script(args.dest.commitment_bytes()?.as_slice(), args.threshold)?;
        let output = TxOut {
            value: expired_output.value,
            script_pubkey,
        };

        let mut tx = BitcoinTx::default();
        tx.input.push(input);
        tx.output.push(Adapter::new(output));

        tx.deduct_fee(args.fee_rate * tx.vsize()?)?;

        tx.populate_input_sig_message(0)?;

        RECOVERY_TXS.push_back(
            store,
            &RecoveryTx {
                tx,
                old_sigset_index: args.old_sigset.index,
                new_sigset_index: args.new_sigset.index,
                dest: args.dest,
            },
        )?;

        Ok(())
    }

    pub fn to_sign(
        &self,
        store: &dyn Storage,
        xpub: &Xpub,
    ) -> ContractResult<Vec<([u8; 32], u32)>> {
        let mut msgs = vec![];

        for tx in RECOVERY_TXS.iter(store)? {
            let tx = tx?;
            for input in &tx.tx.input {
                let pubkey = xpub.derive_pubkey(input.sigset_index)?;
                if input.signatures.needs_sig(pubkey.into()) {
                    msgs.push((input.signatures.message(), input.sigset_index));
                }
            }
        }

        Ok(msgs)
    }

    pub fn sign(
        &mut self,
        store: &mut dyn Storage,
        xpub: &Xpub,
        sigs: Vec<Signature>,
    ) -> ContractResult<()> {
        let mut sig_index = 0;

        if sigs.is_empty() {
            return Err(ContractError::Signer(
                "No signatures supplied for recovery transaction".to_string(),
            ));
        }

        for i in 0..RECOVERY_TXS.len(store)? {
            let mut tx = RECOVERY_TXS.get(store, i)?.ok_or_else(|| {
                ContractError::Signer("Error getting recovery transaction".to_string())
            })?;

            for k in 0..tx.tx.input.len() {
                let input = tx.tx.input.get_mut(k).unwrap();
                let pubkey = xpub.derive_pubkey(input.sigset_index)?;

                if !input.signatures.needs_sig(pubkey.into()) {
                    continue;
                }

                if sig_index >= sigs.len() {
                    return Err(ContractError::Signer(
                        "Not enough signatures supplied for recovery transaction".to_string(),
                    ));
                }
                let sig = &sigs[sig_index];
                sig_index += 1;

                let input_was_signed = input.signatures.signed();
                input.signatures.sign(pubkey.into(), sig)?;

                if !input_was_signed && input.signatures.signed() {
                    tx.tx.signed_inputs += 1;
                }
            }

            // update tx
            RECOVERY_TXS.set(store, i, &tx)?;
        }

        if sig_index != sigs.len() {
            return Err(ContractError::Signer(
                "Excess signatures supplied for recovery transaction".to_string(),
            ));
        }

        Ok(())
    }

    pub fn signed(&self, store: &dyn Storage) -> ContractResult<Vec<SignedRecoveryTx>> {
        let mut txs = vec![];

        for tx in RECOVERY_TXS.iter(store)? {
            let tx = tx?;
            if tx.tx.signed() {
                txs.push(SignedRecoveryTx {
                    tx: Adapter::new(tx.tx.to_bitcoin_tx()?),
                    sigset_index: tx.new_sigset_index,
                    dest: tx.dest.clone(),
                });
            }
        }

        Ok(txs)
    }
}
