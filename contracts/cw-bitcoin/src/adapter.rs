use bitcoin::consensus::{Decodable, Encodable};
use cosmwasm_std::Binary;
use serde::{ser, Deserialize, Deserializer, Serialize};
use std::ops::{Deref, DerefMut};

/// A wrapper that adds core `orga` traits to types from the `bitcoin` crate.
#[derive(Clone, Debug, PartialEq)]
pub struct Adapter<T> {
    inner: T,
}

impl<T> Adapter<T> {
    /// Creates a new `Adapter` from a value.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Consumes the `Adapter` and returns the inner value.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> From<T> for Adapter<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Default> Default for Adapter<T> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<T> Deref for Adapter<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Adapter<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Serializes as a string
impl<T: Encodable> Serialize for Adapter<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut dest: Vec<u8> = Vec::new();
        self.inner.consensus_encode(&mut dest).unwrap();
        serializer.serialize_str(&Binary::from(dest).to_base64())
    }
}

/// Deserializes as string
impl<'de, T: Decodable> Deserialize<'de> for Adapter<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // deserializer.deserialize_bytes(AdapterVisitor(T))
        let v = Binary::deserialize(deserializer)?.to_vec();
        let inner: T = Decodable::consensus_decode(&mut v.as_slice()).unwrap();
        Ok(inner.into())
    }
}

impl<T: Copy> Copy for Adapter<T> {}
