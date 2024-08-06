use super::constants::SIGSET_THRESHOLD;
use super::signatory::SignatorySet;
use crate::error::{ContractError, ContractResult};
use bitcoin::blockdata::transaction::EcdsaSighashType;
use bitcoin::secp256k1::{
    self,
    constants::{MESSAGE_SIZE, PUBLIC_KEY_SIZE},
    ecdsa, PublicKey,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::Api;

// TODO: update for taproot-based design (musig rounds, fallback path)

/// A sighash to be signed by a set of signers.
pub type Message = [u8; MESSAGE_SIZE];

/// A compact secp256k1 ECDSA signature.
#[cw_serde]
pub struct Signature(#[serde(serialize_with = "<[_]>::serialize")] pub Vec<u8>);

/// A compressed secp256k1 public key.
#[derive(Clone, Debug, PartialOrd, PartialEq, Eq, Ord, Deserialize, Serialize)]
#[serde(crate = "cosmwasm_schema::serde")]
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
    pub fn new(pubkey: [u8; PUBLIC_KEY_SIZE]) -> ContractResult<Self> {
        // Verify bytes are a valid compressed secp256k1 public key
        secp256k1::PublicKey::from_slice(pubkey.as_slice()).map_err(|err| {
            ContractError::App(format!(
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
    pub fn try_from_slice(bytes: &[u8]) -> ContractResult<Self> {
        if bytes.len() != PUBLIC_KEY_SIZE {
            return Err(ContractError::App("Incorrect length".into()));
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
#[serde(crate = "cosmwasm_schema::serde")]
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

    /// A map of entries containing the pubkey and voting power of each signer,
    /// and the signature if they have signed.
    pub sigs: Vec<(Pubkey, Share)>,
}

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
    pub fn clear_sigs(&mut self) {
        self.signed = 0;
        for (_, sig) in &mut self.sigs {
            sig.sig = None;
        }
    }

    /// Returns the message to be signed.
    pub fn message(&self) -> Message {
        self.message
    }

    /// Populates the set of signers based on the public keys and voting power
    /// in the given `SignatorySet`.
    pub fn from_sigset(signatories: &SignatorySet) -> Self {
        let mut ts = ThresholdSig::default();
        let mut total_vp = 0;

        for signatory in signatories.iter() {
            ts.sigs.push((
                signatory.pubkey.clone(),
                Share {
                    power: signatory.voting_power,
                    sig: None,
                },
            ));

            ts.len += 1;
            total_vp += signatory.voting_power;
        }

        // TODO: get threshold ratio from somewhere else
        ts.threshold =
            ((total_vp as u128) * SIGSET_THRESHOLD.0 as u128 / SIGSET_THRESHOLD.1 as u128) as u64;

        ts
    }

    /// Populates the set of signers based on the given list of entries of
    /// public keys and voting power.
    ///
    /// This function expects shares to be unsigned, and will panic if any of
    /// them already include a signature.
    pub fn from_shares(shares: Vec<(Pubkey, Share)>) -> Self {
        let mut ts = ThresholdSig::default();
        let mut total_vp = 0;
        let mut len = 0;

        for (pubkey, share) in shares {
            assert!(share.sig.is_none());
            total_vp += share.power;
            len += 1;
            ts.sigs.push((pubkey, share));
        }

        // TODO: get threshold ratio from somewhere else
        ts.threshold =
            ((total_vp as u128) * SIGSET_THRESHOLD.0 as u128 / SIGSET_THRESHOLD.1 as u128) as u64;
        ts.len = len;

        ts
    }

    /// Returns `true` if the more than the threshold of voting power has signed
    /// the message.    
    pub fn signed(&self) -> bool {
        self.signed > self.threshold
    }

    /// Returns a vector of `(pubkey, signature)` tuples for each signer who has
    /// signed the message.    
    pub fn sigs(&self) -> Vec<(Pubkey, Signature)> {
        self.sigs
            .iter()
            .filter_map(|(pubkey, share)| share.sig.clone().map(|sig| (pubkey.clone(), sig)))
            .collect()
    }

    /// Returns a vector of `(pubkey, share)` tuples for each signer, even if
    /// they have not yet signed.
    // TODO: should be iterator?
    pub fn shares(&self) -> Vec<(Pubkey, Share)> {
        self.sigs.clone()
    }

    /// Returns `true` if the given pubkey is part of the set of signers.
    /// Returns `false` otherwise.
    pub fn contains_key(&self, pubkey: Pubkey) -> bool {
        self.sigs.iter().any(|(key, _)| pubkey.eq(key))
    }

    /// Returns `true` if the given pubkey is part of the set of signers and has
    /// not yet signed. Returns `false` if the pubkey is not part of the set of
    /// signers or has already signed.
    pub fn needs_sig(&self, pubkey: Pubkey) -> bool {
        self.sigs
            .iter()
            .find(|(key, _)| pubkey.eq(key))
            .map(|(_, share)| share.sig.is_none())
            .unwrap_or(false)
    }

    /// Verifies and adds the given signature to the state for the given signer.
    ///
    /// Returns an error if the pubkey is not part of the set of signers, if the
    /// signature is invalid, or if the signer has already signed.
    // TODO: exempt from fee
    pub fn sign(&mut self, api: &dyn Api, pubkey: Pubkey, sig: &Signature) -> ContractResult<()> {
        let share = &mut self
            .sigs
            .iter_mut()
            .find(|(key, _)| pubkey.eq(key))
            .unwrap()
            .1;

        if share.sig.is_some() {
            return Err(ContractError::App("Pubkey already signed".into()))?;
        }

        Self::secp_verify(api, self.message.as_slice(), &pubkey, sig)?;

        share.sig = Some(sig.clone());
        self.signed += share.power;

        Ok(())
    }

    /// Verifies the given signature for the message, using the given signer's
    /// pubkey.
    /// Verifies the given signature for the message, using the given signer's
    /// pubkey.
    pub fn secp_verify(
        api: &dyn Api,
        msg: &[u8],
        pubkey: &Pubkey,
        sig: &Signature,
    ) -> ContractResult<()> {
        // TODO: re-use secp context
        // let secp = Secp256k1::verification_only();
        // let pubkey = PublicKey::from_slice(&pubkey.bytes)?;
        // let sig = ecdsa::Signature::from_compact(&sig.0)?;
        // secp.verify_ecdsa(msg, &sig, &pubkey)?;

        let verified = api.secp256k1_verify(msg, &sig.0, pubkey.as_slice())?;

        if !verified {
            return Err(ContractError::App("Can not verify signature".to_string()));
        }

        Ok(())
    }

    pub fn verify(&self, api: &dyn Api, pubkey: &Pubkey, sig: &Signature) -> ContractResult<()> {
        Self::secp_verify(api, self.message.as_slice(), pubkey, sig)
    }

    /// Returns a vector of signatures (or empty bytes for unsigned entries) in
    /// the order they should be added to the witness (ascending by voting
    /// power).
    ///
    /// This can be used to generate a valid spend of the associated Bitcoin
    /// script.
    // TODO: this shouldn't know so much about bitcoin-specific structure,
    // decouple by exposing a power-ordered iterator of Option<Signature>
    pub fn to_witness(&self) -> ContractResult<Vec<Vec<u8>>> {
        if !self.signed() {
            return Ok(vec![]);
        }

        let mut entries: Vec<_> = self.sigs.clone();
        // Sort ascending by voting power, opposite order of public keys in the
        // script
        entries.sort_by(|a, b| {
            if a.1.power == b.1.power {
                a.0.bytes.cmp(&b.0.bytes)
            } else {
                a.1.power.cmp(&b.1.power)
            }
        });

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
