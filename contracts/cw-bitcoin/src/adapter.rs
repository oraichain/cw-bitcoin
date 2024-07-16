use bitcoin::consensus::{Decodable, Encodable};
use cosmwasm_std::{Binary, HexBinary};
use derive_more::{Deref, DerefMut};
use serde::{de, ser, Deserialize, Serialize};

macro_rules! forward_schema_impl {
    ($impl:tt => $target:ty) => {
        impl<T> schemars::JsonSchema for $impl<T> {
            fn schema_name() -> String {
                <$target>::schema_name()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                <$target>::schema_id()
            }

            fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
                <$target>::json_schema(gen)
            }
        }
    };
}

/// A wrapper that adds core `orga` traits to types from the `bitcoin` crate.
#[derive(Clone, Debug, PartialEq, Deref, DerefMut)]
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

/// these methods as for storage only, for passing to contract, use Decodable is much cleaner

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
        Binary::from(dest).serialize(serializer)
    }
}

/// Deserializes as string
impl<'de, T: Decodable> Deserialize<'de> for Adapter<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = Binary::deserialize(deserializer)?;
        let inner: T = Decodable::consensus_decode(&mut v.as_slice()).map_err(de::Error::custom)?;
        Ok(inner.into())
    }
}

impl<T: Copy> Copy for Adapter<T> {}

/// A wrapper that adds core `orga` traits to types from the `bitcoin` crate.
#[derive(Clone, Debug, PartialEq, Deref, DerefMut, Serialize, Deserialize)]
pub struct HashBinary<T>(pub T);

forward_schema_impl!(Adapter => Binary);
forward_schema_impl!(HashBinary => HexBinary);
