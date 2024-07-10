use cosmwasm_std::Storage;
use std::str::FromStr;

use crate::{
    app::Bitcoin,
    error::{ContractError, ContractResult},
    state::HEADER_CONFIG,
};

pub fn query_deposit_fees(store: &dyn Storage, index: Option<u32>) -> ContractResult<u64> {
    let header_config = HEADER_CONFIG.load(store)?;
    let btc = Bitcoin::new(header_config);
    let checkpoint = btc.get_checkpoint(store, index)?;
    let input_vsize = checkpoint.sigset.est_witness_vsize() + 40;
    let deposit_fees = btc.calc_minimum_deposit_fees(input_vsize, checkpoint.fee_rate);
    Ok(deposit_fees)
}

pub fn query_withdrawal_fees(
    store: &dyn Storage,
    address: String,
    index: Option<u32>,
) -> ContractResult<u64> {
    let header_config = HEADER_CONFIG.load(store)?;
    let btc = Bitcoin::new(header_config);
    let checkpoint = btc.get_checkpoint(store, index)?;
    let btc_address = bitcoin::Address::from_str(address.as_str())
        .map_err(|err| ContractError::App(err.to_string()))?;
    let script = btc_address.script_pubkey();
    let withdrawal_fees =
        btc.calc_minimum_withdrawal_fees(script.len() as u64, checkpoint.fee_rate);
    Ok(withdrawal_fees)
}
