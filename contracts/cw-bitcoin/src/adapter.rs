use bitcoin::consensus::{Decodable, Encodable};

use cosmwasm_schema::schemars::{gen, schema, JsonSchema};
use cosmwasm_schema::serde::{de, ser, Deserialize, Serialize};
use cosmwasm_std::Binary;
use derive_more::{Deref, DerefMut};

use crate::interface::Xpub;

macro_rules! forward_schema_impl {
    (($($impl:tt)+) => $target:ty) => {
        impl $($impl)+ {
            fn schema_name() -> String {
                <$target>::schema_name()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                <$target>::schema_id()
            }

            fn json_schema(gen: &mut gen::SchemaGenerator) -> schema::Schema {
                <$target>::json_schema(gen)
            }
        }
    };
    ($ty:ty => $target:ty) => {
        forward_schema_impl!((JsonSchema for $ty) => $target);
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
        let mut dest = Binary::default();
        self.inner
            .consensus_encode(&mut dest.0)
            .map_err(ser::Error::custom)?;
        dest.serialize(serializer)
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

forward_schema_impl!((<T> JsonSchema for Adapter<T>) => Binary);
forward_schema_impl!(Xpub => String);
