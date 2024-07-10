use derive_more::{Deref, DerefMut};
use serde::{Deserialize, Serialize};
use tsify::Tsify;

/// A wrapper that adds core `orga` traits to types from the `bitcoin` crate.
#[derive(Clone, Debug, PartialEq, Deref, Serialize, Deserialize, DerefMut, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
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

impl<T: Copy> Copy for Adapter<T> {}
