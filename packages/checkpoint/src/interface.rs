use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Deque;
use serde::{de::DeserializeOwned, Serialize};

pub trait DequeExtension<'a, T: Serialize + DeserializeOwned> {
    fn retain_unordered<F>(&self, store: &mut dyn Storage, f: F) -> StdResult<u64>
    where
        F: FnMut(&T) -> bool;
}

impl<'a, T: Serialize + DeserializeOwned> DequeExtension<'a, T> for Deque<'a, T> {
    fn retain_unordered<F>(&self, store: &mut dyn Storage, mut f: F) -> StdResult<u64>
    where
        F: FnMut(&T) -> bool,
    {
        let mut temp = vec![];
        while let Some(item) = self.pop_front(store)? {
            temp.push(item);
        }
        let mut size = 0;
        for item in temp {
            if f(&item) {
                self.push_back(store, &item)?;
                size += 1;
            }
        }

        Ok(size)
    }
}
