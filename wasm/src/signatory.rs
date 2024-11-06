use std::cmp::Ordering;

use crate::{
    error::{ContractError, ContractResult},
    threshold_sig::Pubkey,
    MAX_SIGNATORIES,
};
use bitcoin::{
    blockdata::{
        opcodes::{
            self,
            all::{
                OP_ADD, OP_CHECKSIG, OP_DROP, OP_ELSE, OP_ENDIF, OP_GREATERTHAN, OP_IF, OP_SWAP,
            },
            OP_FALSE,
        },
        script::{read_scriptint, Instruction},
    },
    hashes::hex::FromHex,
    Script,
};
use bitcoin_script::bitcoin_script as script;
use derive_more::Deref;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// A signatory in a signatory set, consisting of a public key and voting power.
#[derive(Clone, Debug, PartialOrd, PartialEq, Eq, Ord, Deserialize, Serialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Signatory {
    pub voting_power: u64,
    pub pubkey: Pubkey,
}

#[derive(
    Clone, Debug, Default, PartialOrd, Deref, PartialEq, Eq, Ord, Deserialize, Serialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SignatorySet {
    /// The time at which this signatory set was created, in seconds.
    ///
    /// This is used to enforce that deposits can not be relayed against old
    /// signatory sets (see [`MAX_DEPOSIT_AGE`]).
    pub create_time: u64,

    /// The total voting power of the validators participating in this set. If a
    /// validator has not submitted their signatory xpub, they will not be
    /// included.
    pub present_vp: u64,

    /// The total voting power of the validator set at the time this signatory
    /// set was created. This is used to ensure a sufficient quorum of
    /// validators have submitted a signatory xpub.
    pub possible_vp: u64,

    /// The index of this signatory set.
    pub index: u32,

    /// The signatories in this set, sorted by voting power.
    #[deref]
    pub signatories: Vec<Signatory>,
}

type IterItem<'a> = std::result::Result<Instruction<'a>, bitcoin::blockdata::script::Error>;

impl SignatorySet {
    pub fn from_script(
        script: &bitcoin::Script,
        threshold_ratio: (u64, u64),
    ) -> ContractResult<(Self, Vec<u8>)> {
        fn take_instruction<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
        ) -> ContractResult<Instruction<'a>> {
            ins.next()
                .ok_or_else(|| ContractError::App("Unexpected end of script".into()))?
                .map_err(|_| ContractError::App("Failed to read script".into()))
        }

        fn take_bytes<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
        ) -> ContractResult<&'a [u8]> {
            let instruction = take_instruction(ins)?;

            let Instruction::PushBytes(bytes) = instruction else {
                return Err(ContractError::App("Expected OP_PUSHBYTES".into()));
            };

            Ok(bytes)
        }

        fn take_key<'a>(ins: &mut impl Iterator<Item = IterItem<'a>>) -> ContractResult<Pubkey> {
            let bytes = take_bytes(ins)?;

            if bytes.len() != 33 {
                return Err(ContractError::App("Expected 33 bytes".into()));
            }

            Pubkey::try_from_slice(bytes)
        }

        fn take_number<'a>(ins: &mut impl Iterator<Item = IterItem<'a>>) -> ContractResult<i64> {
            let bytes = take_bytes(ins)?;
            read_scriptint(bytes).map_err(|_| ContractError::App("Failed to read scriptint".into()))
        }

        fn take_op<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
            expected_op: opcodes::All,
        ) -> ContractResult<opcodes::All> {
            let instruction = take_instruction(ins)?;

            let op = match instruction {
                Instruction::Op(op) => op,
                Instruction::PushBytes(&[]) => OP_FALSE,
                _ => return Err(ContractError::App(format!("Expected {:?}", expected_op))),
            };

            if op != expected_op {
                return Err(ContractError::App(format!("Expected {:?}", expected_op)));
            }

            Ok(op)
        }

        fn take_first_signatory<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
        ) -> ContractResult<Signatory> {
            let pubkey = take_key(ins)?;
            take_op(ins, OP_CHECKSIG)?;
            take_op(ins, OP_IF)?;
            let voting_power = take_number(ins)?;
            take_op(ins, OP_ELSE)?;
            take_op(ins, OP_FALSE)?;
            take_op(ins, OP_ENDIF)?;

            Ok::<_, ContractError>(Signatory {
                pubkey,
                voting_power: voting_power as u64,
            })
        }

        fn take_nth_signatory<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
        ) -> ContractResult<Signatory> {
            take_op(ins, OP_SWAP)?;
            let pubkey = take_key(ins)?;
            take_op(ins, OP_CHECKSIG)?;
            take_op(ins, OP_IF)?;
            let voting_power = take_number(ins)?;
            take_op(ins, OP_ADD)?;
            take_op(ins, OP_ENDIF)?;

            Ok::<_, ContractError>(Signatory {
                pubkey,
                voting_power: voting_power as u64,
            })
        }

        fn take_threshold<'a>(ins: &mut impl Iterator<Item = IterItem<'a>>) -> ContractResult<u64> {
            let threshold = take_number(ins)?;
            take_op(ins, OP_GREATERTHAN)?;
            Ok(threshold as u64)
        }

        fn take_commitment<'a>(
            ins: &mut impl Iterator<Item = IterItem<'a>>,
        ) -> ContractResult<&'a [u8]> {
            let bytes = take_bytes(ins)?;
            take_op(ins, OP_DROP)?;
            Ok(bytes)
        }

        let mut ins = script.instructions().peekable();
        let mut sigs = vec![take_first_signatory(&mut ins)?];
        loop {
            let next = ins
                .peek()
                .ok_or_else(|| ContractError::App("Unexpected end of script".into()))?
                .clone()
                .map_err(|_| ContractError::App("Failed to read script".into()))?;

            if let Instruction::Op(opcodes::all::OP_SWAP) = next {
                sigs.push(take_nth_signatory(&mut ins)?);
            } else {
                break;
            }
        }

        let expected_threshold = take_threshold(&mut ins)?;
        let commitment = take_commitment(&mut ins)?;

        assert!(ins.next().is_none());

        let total_vp: u64 = sigs.iter().map(|s| s.voting_power).sum();
        let mut sigset = Self {
            signatories: sigs,
            present_vp: total_vp,
            possible_vp: total_vp,
            create_time: 0,
            index: 0,
        };

        for _ in 0..100 {
            let actual_threshold = sigset.signature_threshold(threshold_ratio);
            match actual_threshold.cmp(&expected_threshold) {
                Ordering::Equal => break,
                Ordering::Less => {
                    sigset.present_vp += 1;
                    sigset.possible_vp += 1;
                }
                Ordering::Greater => {
                    sigset.present_vp -= 1;
                    sigset.possible_vp -= 1;
                }
            }
        }

        assert_eq!(
            sigset.signature_threshold(threshold_ratio),
            expected_threshold,
        );
        assert_eq!(&sigset.redeem_script(commitment, threshold_ratio)?, script);

        Ok((sigset, commitment.to_vec()))
    }

    pub fn insert(&mut self, signatory: Signatory) {
        self.present_vp += signatory.voting_power;
        self.signatories.push(signatory);
    }

    pub fn sort_and_truncate(&mut self) {
        self.signatories.sort_by(|a, b| b.cmp(a));

        if self.signatories.len() as u64 > MAX_SIGNATORIES {
            for removed in self.signatories.drain(MAX_SIGNATORIES as usize..) {
                self.present_vp -= removed.voting_power;
            }
        }
    }

    /// The voting power threshold required to spend outputs secured by this
    /// signatory set.
    pub fn signature_threshold(&self, (numerator, denominator): (u64, u64)) -> u64 {
        ((self.present_vp as u128) * numerator as u128 / denominator as u128) as u64
    }

    /// The quorum threshold required for the signatory set to be valid.
    pub fn quorum_threshold(&self) -> u64 {
        self.possible_vp / 2
    }

    /// The total amount of voting power of validators participating in the set.
    /// Validators who have not submitted a signatory xpub are not included.
    pub fn present_vp(&self) -> u64 {
        self.present_vp
    }

    /// The total amount of voting power of the validator set at the time this
    /// signatory set was created. This is used to ensure a sufficient quorum of
    /// validators have submitted a signatory xpub.
    pub fn possible_vp(&self) -> u64 {
        self.possible_vp
    }

    /// Whether the signatory set has a sufficient quorum of validators who have
    /// submitted a signatory xpub.
    ///
    /// If this returns `false`, this signatory set should not be used to secure
    /// a UTXO.
    pub fn has_quorum(&self) -> bool {
        self.present_vp >= self.quorum_threshold()
    }

    /// The number of signatories in the set.
    // TODO: remove this attribute, not sure why clippy is complaining when is_empty is defined
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.signatories.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Builds a Bitcoin script which can be used to spend a UTXO secured by
    /// this signatory set.
    ///
    /// This script is essentially a weighted multisig script, where each
    /// signatory has a weight equal to their voting power. It is specified in
    /// the input witness when the UTXO is spent. The output contains a hash of
    /// this script, since it is a pay-to-witness-script-hash (P2WSH) output.
    pub fn redeem_script(&self, dest: &[u8], threshold: (u64, u64)) -> ContractResult<Script> {
        // We will truncate voting power values to 23 bits, to reduce the amount
        // of bytes used in the resulting encoded script. In practice, this
        // should be enough precision for effective voting power threshold
        // checking. We use 23 bits since Bitcoin script reserves one bit as the
        // sign bit, making our resulting integer value use 3 bytes. The value
        // returned here is the number of bits of precision to remove from our
        // 64-bit voting power values.
        let truncation = self.get_truncation(23);

        let mut iter = self.signatories.iter();

        // First signatory
        let signatory = iter.next().ok_or_else(|| {
            ContractError::App("Cannot create redeem script for empty signatory set".to_string())
        })?;
        let truncated_voting_power = signatory.voting_power >> truncation;
        // Push the pubkey onto the stack, check the signature against it, and
        // leave the voting power on the stack if the signature was valid,
        // otherwise leave 0 (this number will be an accumulator of voting power
        // which had valid signatures, and will be added to as we check the
        // remaining signatures).
        let script = script! {
            <signatory.pubkey.as_slice()> OP_CHECKSIG
            OP_IF
                <truncated_voting_power as i64>
            OP_ELSE
                0
            OP_ENDIF
        };
        let mut bytes = script.into_bytes();

        // All other signatories
        for signatory in iter {
            let truncated_voting_power = signatory.voting_power >> truncation;
            // Swap to move the current voting power accumulator down the stack
            // (leaving the next signature at the top of the stack), push the
            // pubkey onto the stack, check the signature against it, and add to
            // the voting power accumulator if the signature was valid.
            let script = script! {
                OP_SWAP
                <signatory.pubkey.as_slice()> OP_CHECKSIG
                OP_IF
                    <truncated_voting_power as i64> OP_ADD
                OP_ENDIF
            };
            bytes.extend(&script.into_bytes());
        }

        // Threshold check
        let truncated_threshold = self.signature_threshold(threshold) >> truncation;
        // Check that accumulator of voting power which had valid signatures
        // (now a final sum) is greater than the threshold.
        let script = script! {
            <truncated_threshold as i64> OP_GREATERTHAN
        };
        bytes.extend(&script.into_bytes());

        // Depositor data commitment
        // Add a commitment of arbitrary data so that deposits can be tied to a
        // specific destination, then remove it from the stack so that the final
        // value on the stack is the threshold check result.
        let script = script!(<dest> OP_DROP);
        bytes.extend(&script.into_bytes());

        Ok(bytes.into())
    }

    /// Hashes the weighted multisig redeem script to create a P2WSH output
    /// script, which is what is used as the script pubkey in deposit outputs
    /// and reserve outputs.
    pub fn output_script(&self, dest: &[u8], threshold: (u64, u64)) -> ContractResult<Script> {
        Ok(self.redeem_script(dest, threshold)?.to_p2sh())
    }

    /// Calculates the number of bits of precision to remove from voting power
    /// values in order to have a maximum of `target_precision` bits of
    /// precision.
    fn get_truncation(&self, target_precision: u32) -> u32 {
        let vp_bits = u64::BITS - self.present_vp.leading_zeros();
        vp_bits.saturating_sub(target_precision)
    }

    /// The time at which this signatory set was created, in seconds.
    pub fn create_time(&self) -> u64 {
        self.create_time
    }

    /// The index of this signatory set.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// An iterator over the signatories in this set.
    pub fn iter(&self) -> impl Iterator<Item = &Signatory> {
        self.signatories.iter()
    }

    /// The estimated size of a witness containing the redeem script and
    /// signatures for this signatory set, in virtual bytes.
    ///
    /// This represents the worst-case, where there is a signature for each
    /// signatory. In practice, we could trim this down by removing signatures
    /// for signatories beyond the threshold, but for fee estimation we err on
    /// the side of paying too much.
    pub fn est_witness_vsize(&self) -> u64 {
        self.signatories.len() as u64 * 79 + 39
    }
}

#[wasm_bindgen]
pub fn newSignatorySet(
    hex_script: &str,
    numerator: u64,
    denominator: u64,
) -> ContractResult<SignatorySet> {
    let script = bitcoin::Script::from_hex(hex_script)?;
    let (signatory_set, _) = SignatorySet::from_script(&script, (numerator, denominator))?;
    Ok(signatory_set)
}
