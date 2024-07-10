#![allow(non_snake_case)]

mod error;
use deposit_index::DepositIndex;
use traceable_result::TrackableResult;
use wasm_bindgen::prelude::*;

mod deposit_index;

#[wasm_bindgen(js_name = createDeposit)]
pub fn create_deposit() -> TrackableResult<DepositIndex> {
    let deposit = DepositIndex::new();
    Ok(deposit)
}
