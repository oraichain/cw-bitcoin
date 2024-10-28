use cosmwasm_schema::serde::{de, Serialize};
use cosmwasm_std::{storage_keys::namespace_with_key, to_json_vec, StdError, StdResult, Storage};
use cw_storage_plus::Deque;
use derive_more::{Deref, DerefMut};

use crate::error::ContractResult;

const TAIL_KEY: &[u8] = b"t";
const HEAD_KEY: &[u8] = b"h";

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
        self.set_head(store, 0);
        self.set_tail(store, 0);
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
        let head_value = self.head(storage)?;
        let prefixed_key = self.get_key(pos + head_value);
        storage.set(&prefixed_key, &to_json_vec(value)?);
        Ok(())
    }

    // Setters
    fn set_head(&self, storage: &mut dyn Storage, value: u32) {
        self.set_meta_key(storage, HEAD_KEY, value);
    }

    #[inline]
    fn set_tail(&self, storage: &mut dyn Storage, value: u32) {
        self.set_meta_key(storage, TAIL_KEY, value);
    }

    fn set_meta_key(&self, storage: &mut dyn Storage, key: &[u8], value: u32) {
        let full_key = namespace_with_key(&[self.namespace], key);
        storage.set(&full_key, &value.to_be_bytes());
    }

    // Getters
    pub fn head(&self, storage: &dyn Storage) -> StdResult<u32> {
        self.read_meta_key(storage, HEAD_KEY)
    }

    pub fn tail(&self, storage: &dyn Storage) -> StdResult<u32> {
        self.read_meta_key(storage, TAIL_KEY)
    }

    fn read_meta_key(&self, storage: &dyn Storage, key: &[u8]) -> StdResult<u32> {
        let full_key = namespace_with_key(&[self.namespace], key);
        storage
            .get(&full_key)
            .map(|vec| {
                Ok(u32::from_be_bytes(
                    vec.as_slice()
                        .try_into()
                        .map_err(|e| StdError::parse_err("u32", e))?,
                ))
            })
            .unwrap_or(Ok(0))
    }
}
