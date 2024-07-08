use std::ops::Deref;

use bitcoin::util::bip32::ExtendedPubKey;
use serde::{Deserialize, Serialize};

/// A Bitcoin extended public key, used to derive Bitcoin public keys which
/// signatories sign transactions with.
// #[derive(Call, Query, Clone, Debug, Client, PartialEq, Serialize)]
#[derive(Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Xpub {
    key: ExtendedPubKey,
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

impl Deref for Xpub {
    type Target = ExtendedPubKey;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}
