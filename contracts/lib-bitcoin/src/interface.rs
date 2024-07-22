use bitcoin::util::bip32::ExtendedPubKey;
use cosmwasm_schema::serde::{de, ser, Deserialize, Serialize};
use cosmwasm_std::Binary;
use derive_more::Deref;

/// A Bitcoin extended public key, used to derive Bitcoin public keys which
/// signatories sign transactions with.
#[derive(Copy, Clone, PartialEq, Deref, Eq, Debug, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", serde(crate = "actual_serde"))]
pub struct Xpub {
    pub key: ExtendedPubKey,
}

impl Xpub {
    /// Creates a new `Xpub` from an `ExtendedPubKey`.
    pub fn new(key: ExtendedPubKey) -> Self {
        Xpub { key }
    }

    /// Gets the `ExtendedPubKey` from the `Xpub`.
    pub fn inner(&self) -> &ExtendedPubKey {
        &self.key
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
