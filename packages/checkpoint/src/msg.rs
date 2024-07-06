use std::ops::Deref;

use bitcoin::util::bip32::ExtendedPubKey;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, IbcMsg};

/// A Tendermint/CometBFT public key.
pub type ConsensusKey = [u8; 32];

/// A Bitcoin extended public key, used to derive Bitcoin public keys which
/// signatories sign transactions with.
// #[derive(Call, Query, Clone, Debug, Client, PartialEq, Serialize)]
#[derive(Debug)]
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

#[cw_serde]
pub enum Dest {
    Address(Addr),
    Ibc(IbcDest),
}

impl Dest {
    pub fn to_receiver_addr(&self) -> String {
        match self {
            Dest::Address(addr) => addr.to_string(),
            Dest::Ibc(dest) => dest.receiver.to_string(),
        }
    }
}

#[cw_serde]
pub struct IbcDest {
    pub source_port: String,
    pub source_channel: String,
    #[serde(skip)]
    pub receiver: String,
    #[serde(skip)]
    pub sender: String,
    pub timeout_timestamp: u64,
    pub memo: String,
}
