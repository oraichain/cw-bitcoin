use cosmwasm_schema::serde::{de, Serialize};
use cosmwasm_std::{to_json_vec, Storage};
use cw_storage_plus::Deque;
use derive_more::{Deref, DerefMut};

use crate::error::ContractResult;

#[derive(Deref, DerefMut)]
pub struct DequeExtension<'a, T> {
    // prefix of the deque items
    key_prefix: [u8; 2],
    namespace: &'a [u8],
    // see https://doc.rust-lang.org/std/marker/struct.PhantomData.html#unused-type-parameters for why this is needed
    #[deref]
    #[deref_mut]
    queue: Deque<'a, T>,
}

impl<'a, T: Serialize + de::DeserializeOwned> DequeExtension<'a, T> {
    pub const fn new(prefix: &'a str) -> Self {
        Self {
            namespace: prefix.as_bytes(),
            key_prefix: (prefix.len() as u16).to_be_bytes(),
            queue: Deque::new(prefix),
        }
    }

    pub fn clear(&self, store: &mut dyn Storage) -> ContractResult<()> {
        let mut queue_len = self.len(store)?;

        while queue_len > 0 {
            self.pop_back(store)?;
            queue_len -= 1;
        }
        Ok(())
    }

    pub fn get_key(&self, pos: u32) -> Vec<u8> {
        let key = &pos.to_be_bytes();
        let size = self.namespace.len() + 2 + key.len();
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&self.key_prefix);
        out.extend_from_slice(self.namespace);
        out.extend_from_slice(key);
        out
    }

    /// Sets the value at the given position in the queue. Returns [`StdError::NotFound`] if index is out of bounds
    pub fn set(&self, storage: &mut dyn Storage, pos: u32, value: &T) -> ContractResult<()> {
        let prefixed_key = self.get_key(pos);
        storage.set(&prefixed_key, &to_json_vec(value)?);
        Ok(())
    }
}
