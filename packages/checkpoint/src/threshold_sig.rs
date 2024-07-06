use crate::error::ContractResult;

use super::signatory::{SignatorySet, SIGSET_THRESHOLD};
use bitcoin::blockdata::transaction::EcdsaSighashType;
use bitcoin::secp256k1::{
    self,
    constants::{MESSAGE_SIZE, PUBLIC_KEY_SIZE},
    ecdsa, PublicKey, Secp256k1,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Order, StdError, StdResult, Storage};
use cw_storage_plus::Map;
use serde::{Deserialize, Serialize};

// TODO: update for taproot-based design (musig rounds, fallback path)

/// A sighash to be signed by a set of signers.
pub type Message = [u8; MESSAGE_SIZE];

/// A compact secp256k1 ECDSA signature.
#[cw_serde]
pub struct Signature(#[serde(serialize_with = "<[_]>::serialize")] pub Vec<u8>);

/// A compressed secp256k1 public key.
#[cw_serde]
pub struct Pubkey {
    #[serde(serialize_with = "<[_]>::serialize")]
    bytes: Vec<u8>,
}

impl Default for Pubkey {
    fn default() -> Self {
        Pubkey {
            bytes: [0; PUBLIC_KEY_SIZE].to_vec(),
        }
    }
}

impl Pubkey {
    /// Create a new pubkey from compressed secp256k1 public key bytes.
    ///
    /// This will error if the bytes are not a valid compressed secp256k1 public
    /// key.
    pub fn new(pubkey: [u8; PUBLIC_KEY_SIZE]) -> StdResult<Self> {
        // Verify bytes are a valid compressed secp256k1 public key
        secp256k1::PublicKey::from_slice(pubkey.as_slice()).map_err(|err| {
            StdError::generic_err(format!(
                "Error deserializing public key from slice: {}",
                err
            ))
        })?;

        Ok(Pubkey {
            bytes: pubkey.to_vec(),
        })
    }

    /// Create a new pubkey from compressed secp256k1 public key bytes.
    ///
    /// This will error if the bytes are not a valid compressed secp256k1 public
    /// key.
    pub fn try_from_slice(bytes: &[u8]) -> StdResult<Self> {
        if bytes.len() != PUBLIC_KEY_SIZE {
            return Err(StdError::generic_err("Incorrect length"));
        }

        let mut buf = [0; PUBLIC_KEY_SIZE];
        buf.copy_from_slice(bytes);

        Self::new(buf)
    }

    /// Get the compressed secp256k1 public key bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

impl From<PublicKey> for Pubkey {
    fn from(pubkey: PublicKey) -> Self {
        Pubkey {
            bytes: pubkey.serialize().to_vec(),
        }
    }
}

/// `ThresholdSig` is a state type used to coordinate the signing of a message
/// by a set of signers.
///
/// It is populated based on a `SignatorySet` and a message to sign, and then
/// each signer signs the message and adds their signature to the state.]
#[derive(Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct ThresholdSig {
    /// The threshold of voting power required for a the signature to be
    /// considered "signed".
    pub threshold: u64,

    /// The total voting power of signers who have signed the message.
    pub signed: u64,

    /// The message to be signed (in practice, this will be a Bitcoin sighash).
    pub message: Message,

    /// The number of signers in the set.
    pub len: u16,
}

/// A map of entries containing the pubkey and voting power of each signer,
/// and the signature if they have signed.
pub const SIGS: Map<&[u8], Share> = Map::new("sigs");

impl ThresholdSig {
    /// Create a new empty `ThresholdSig` state. It will need to be populated
    /// with a `SignatorySet` and a message to sign.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of signers in the set.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u16 {
        self.len
    }

    /// Populates the message to be signed.
    pub fn set_message(&mut self, message: Message) {
        self.message = message;
    }

    /// Clears all signatures from the state.
    pub fn clear_sigs(&mut self, store: &mut dyn Storage) -> StdResult<()> {
        self.signed = 0;

        SIGS.clear(store);

        // let entries: Vec<_> = SIGS
        //     .iter()?
        //     .collect::<StdResult<Vec<_>>>()?
        //     .into_iter()
        //     .map(|(k, _)| *k)
        //     .collect();
        // for k in entries {
        //     let mut sig = SIGS.get_mut(k)?.unwrap();
        //     sig.sig = None;
        // }

        Ok(())
    }

    /// Returns the message to be signed.
    pub fn message(&self) -> Message {
        self.message
    }

    /// Populates the set of signers based on the public keys and voting power
    /// in the given `SignatorySet`.
    pub fn from_sigset(store: &mut dyn Storage, signatories: &SignatorySet) -> StdResult<Self> {
        let mut ts = ThresholdSig::default();
        let mut total_vp = 0;

        for signatory in signatories.iter() {
            SIGS.save(
                store,
                signatory.pubkey.as_slice(),
                &Share {
                    power: signatory.voting_power,
                    sig: None,
                },
            )?;

            ts.len += 1;
            total_vp += signatory.voting_power;
        }

        // TODO: get threshold ratio from somewhere else
        ts.threshold =
            ((total_vp as u128) * SIGSET_THRESHOLD.0 as u128 / SIGSET_THRESHOLD.1 as u128) as u64;

        Ok(ts)
    }

    /// Populates the set of signers based on the given list of entries of
    /// public keys and voting power.
    ///
    /// This function expects shares to be unsigned, and will panic if any of
    /// them already include a signature.
    pub fn from_shares(store: &mut dyn Storage, shares: Vec<(Pubkey, Share)>) -> StdResult<Self> {
        let mut ts = ThresholdSig::default();
        let mut total_vp = 0;
        let mut len = 0;

        for (pubkey, share) in shares.into_iter() {
            assert!(share.sig.is_none());
            total_vp += share.power;
            len += 1;
            SIGS.save(store, pubkey.as_slice(), &share)?;
        }

        // TODO: get threshold ratio from somewhere else
        ts.threshold =
            ((total_vp as u128) * SIGSET_THRESHOLD.0 as u128 / SIGSET_THRESHOLD.1 as u128) as u64;
        ts.len = len;

        Ok(ts)
    }

    /// Returns `true` if the more than the threshold of voting power has signed
    /// the message.    
    pub fn signed(&self) -> bool {
        self.signed > self.threshold
    }

    /// Returns a vector of `(pubkey, signature)` tuples for each signer who has
    /// signed the message.    
    pub fn sigs(&self, store: &dyn Storage) -> StdResult<Vec<(Pubkey, Signature)>> {
        SIGS.range_raw(store, None, None, Order::Ascending)
            .filter_map(|entry| {
                let (pubkey, share) = match entry {
                    Err(e) => return Some(Err(e)),
                    Ok(entry) => entry,
                };
                share
                    .sig
                    .map(|sig| Ok((Pubkey { bytes: pubkey }, sig.clone())))
            })
            .collect::<StdResult<_>>()
    }

    /// Returns a vector of `(pubkey, share)` tuples for each signer, even if
    /// they have not yet signed.
    // TODO: should be iterator?
    pub fn shares(&self, store: &dyn Storage) -> StdResult<Vec<(Pubkey, Share)>> {
        SIGS.range_raw(store, None, None, Order::Ascending)
            .map(|entry| entry.map(|(pubkey, share)| (Pubkey { bytes: pubkey }, share.clone())))
            .collect::<StdResult<_>>()
    }

    /// Returns `true` if the given pubkey is part of the set of signers.
    /// Returns `false` otherwise.
    pub fn contains_key(&self, store: &dyn Storage, pubkey: Pubkey) -> bool {
        SIGS.has(store, pubkey.as_slice())
    }

    /// Returns `true` if the given pubkey is part of the set of signers and has
    /// not yet signed. Returns `false` if the pubkey is not part of the set of
    /// signers or has already signed.
    pub fn needs_sig(&self, store: &dyn Storage, pubkey: Pubkey) -> bool {
        SIGS.load(store, pubkey.as_slice())
            .map(|share| share.sig.is_none())
            .unwrap_or(false)
    }

    /// Verifies and adds the given signature to the state for the given signer.
    ///
    /// Returns an error if the pubkey is not part of the set of signers, if the
    /// signature is invalid, or if the signer has already signed.
    // TODO: exempt from fee
    pub fn sign(
        &mut self,
        store: &mut dyn Storage,
        pubkey: Pubkey,
        sig: &Signature,
    ) -> ContractResult<()> {
        let mut share = SIGS.load(store, pubkey.as_slice())?;

        if share.sig.is_some() {
            return Err(StdError::generic_err("Pubkey already signed"))?;
        }

        self.verify(&pubkey, sig)?;

        share.sig = Some(sig.clone());
        self.signed += share.power;

        SIGS.save(store, pubkey.as_slice(), &share)?;

        Ok(())
    }

    /// Verifies the given signature for the message, using the given signer's
    /// pubkey.
    pub fn verify(&self, pubkey: &Pubkey, sig: &Signature) -> ContractResult<()> {
        // TODO: re-use secp context
        let secp = Secp256k1::verification_only();
        let pubkey = PublicKey::from_slice(&pubkey.bytes)?;
        let msg = secp256k1::Message::from_slice(self.message.as_slice())?;
        let sig = ecdsa::Signature::from_compact(&sig.0)?;

        secp.verify_ecdsa(&msg, &sig, &pubkey)?;

        Ok(())
    }

    /// Returns a vector of signatures (or empty bytes for unsigned entries) in
    /// the order they should be added to the witness (ascending by voting
    /// power).
    ///
    /// This can be used to generate a valid spend of the associated Bitcoin
    /// script.
    // TODO: this shouldn't know so much about bitcoin-specific structure,
    // decouple by exposing a power-ordered iterator of Option<Signature>
    pub fn to_witness(&self, store: &dyn Storage) -> ContractResult<Vec<Vec<u8>>> {
        if !self.signed() {
            return Ok(vec![]);
        }

        let mut entries: Vec<_> = SIGS
            .range(store, None, None, Order::Ascending)
            .collect::<StdResult<_>>()?;
        // Sort ascending by voting power, opposite order of public keys in the
        // script
        entries.sort_by(|a, b| (a.1.power, &a.0).cmp(&(b.1.power, &b.0)));

        entries
            .into_iter()
            .map(|(_, share)| {
                share.sig.map_or(Ok(vec![]), |sig| {
                    let sig = ecdsa::Signature::from_compact(&sig.0)?;
                    let mut v = sig.serialize_der().to_vec();
                    v.push(EcdsaSighashType::All.to_u32() as u8);
                    Ok(v)
                })
            })
            .collect()
    }
}

use std::fmt::Debug;
impl Debug for ThresholdSig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThresholdSig")
            .field("threshold", &self.threshold)
            .field("signed", &self.signed)
            .field("message", &self.message)
            .field("len", &self.len)
            .field("sigs", &"TODO")
            .finish()
    }
}

/// An entry containing a signer's voting power, and their signature if they
/// have signed.
#[cw_serde]
pub struct Share {
    pub power: u64,
    pub(super) sig: Option<Signature>,
}
