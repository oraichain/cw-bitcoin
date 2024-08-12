use bitcoin::consensus::{Decodable, Encodable};
use derive_more::{Deref, DerefMut};
use serde::{de, ser, Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// A wrapper that adds core `orga` traits to types from the `bitcoin` crate.
#[derive(Clone, Debug, PartialEq, Deref, DerefMut, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(transparent)]
pub struct Adapter<T> {
    #[tsify(type = "string")]
    inner: T,
}

impl<T> Adapter<T> {
    /// Creates a new `Adapter` from a value.
    pub fn new(inner: T) -> Self {
        Self { inner }
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

/// Serializes as a string
impl<T: Encodable> Serialize for Adapter<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut dest: Vec<u8> = Vec::new();
        self.inner
            .consensus_encode(&mut dest)
            .map_err(ser::Error::custom)?;
        base64::encode(dest).serialize(serializer)
    }
}

/// Deserializes as string
impl<'de, T: Decodable> Deserialize<'de> for Adapter<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = String::deserialize(deserializer)?;
        let bytes = base64::decode(v).map_err(de::Error::custom)?;
        let inner: T =
            Decodable::consensus_decode(&mut bytes.as_slice()).map_err(de::Error::custom)?;
        Ok(inner.into())
    }
}

impl<T: Copy> Copy for Adapter<T> {}
