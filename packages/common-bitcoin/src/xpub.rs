use bitcoin::secp256k1;
use bitcoin::util::bip32::ExtendedPubKey;
use cosmwasm_schema::serde::{de, ser, Deserialize, Serialize};
use cosmwasm_std::{Binary, StdError};
use derive_more::Deref;
use libsecp256k1_core::curve::{Affine, ECMultContext, Field, Scalar};
use libsecp256k1_core::util::{TAG_PUBKEY_EVEN, TAG_PUBKEY_ODD};

use crate::error::ContractResult;

/// A Bitcoin extended public key, used to derive Bitcoin public keys which
/// signatories sign transactions with.
#[derive(Copy, Clone, PartialEq, Deref, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Xpub {
    pub key: ExtendedPubKey,
}

impl Xpub {
    /// Creates a new `Xpub` from an `ExtendedPubKey`.
    pub fn new(key: ExtendedPubKey) -> Self {
        Xpub { key }
    }

    fn parse_pubkey(&self) -> ContractResult<Affine> {
        let bytes = self.public_key.serialize();
        let mut x = Field::default();
        if !x.set_b32(arrayref::array_ref!(&bytes, 1, 32)) {
            return Err(StdError::generic_err("invalid pubkey").into());
        }
        let mut elem = libsecp256k1_core::curve::Affine::default();
        elem.set_xo_var(&x, bytes[0] == TAG_PUBKEY_ODD);
        Ok(elem)
    }

    fn add_exp_tweak(&self, secret: &secp256k1::SecretKey) -> ContractResult<secp256k1::PublicKey> {
        let tweak = secret.secret_bytes();
        let mut elem = self.parse_pubkey()?;
        let mut scala = Scalar::default();
        if bool::from(scala.set_b32(&tweak)) {
            return Err(StdError::generic_err("invalid secret").into());
        }

        let ctx = ECMultContext::new_boxed();
        let mut r = libsecp256k1_core::curve::Jacobian::default();
        let a = libsecp256k1_core::curve::Jacobian::from_ge(&elem);
        let one = libsecp256k1_core::curve::Scalar::from_int(1);
        ctx.ecmult(&mut r, &a, &one, &scala);

        elem.set_gej(&r);

        let mut ret = [0u8; 33];

        elem.x.normalize_var();
        elem.y.normalize_var();
        elem.x.fill_b32(arrayref::array_mut_ref!(ret, 1, 32));
        ret[0] = if elem.y.is_odd() {
            TAG_PUBKEY_ODD
        } else {
            TAG_PUBKEY_EVEN
        };
        let pubkey = secp256k1::PublicKey::from_slice(&ret)?;
        Ok(pubkey)
    }

    /// Deterministically derive the public key for a signatory in a signatory set,
    /// based on the current signatory set index.
    pub fn derive_pubkey(&self, sigset_index: u32) -> ContractResult<secp256k1::PublicKey> {
        let child_number = bitcoin::util::bip32::ChildNumber::from_normal_idx(sigset_index)?;
        let (sk, _) = self.ckd_pub_tweak(child_number)?;
        self.add_exp_tweak(&sk)
    }
}

impl From<ExtendedPubKey> for Xpub {
    fn from(key: ExtendedPubKey) -> Self {
        Xpub { key }
    }
}

impl From<&ExtendedPubKey> for Xpub {
    fn from(key: &ExtendedPubKey) -> Self {
        Xpub { key: *key }
    }
}

/// Serializes as a string
impl Serialize for Xpub {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let dest = self.key.encode();
        Binary::from(dest).serialize(serializer)
    }
}

/// Deserializes as string
impl<'de> Deserialize<'de> for Xpub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = Binary::deserialize(deserializer)?;
        let inner = ExtendedPubKey::decode(v.as_slice()).map_err(de::Error::custom)?;
        Ok(inner.into())
    }
}
