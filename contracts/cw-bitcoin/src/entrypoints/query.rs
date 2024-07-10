use cosmwasm_std::Storage;

use crate::{adapter::Adapter, app::Bitcoin, error::ContractResult};

pub fn deposit_fees(store: &dyn Storage, index: Option<u32>) -> ContractResult<u64> {
    let btc = Bitcoin::new(store)?;
    let checkpoint = match index {
        Some(index) => self.bitcoin.checkpoints.get(index)?,
        None => self
            .bitcoin
            .checkpoints
            .get(self.bitcoin.checkpoints.index)?, // get current checkpoint being built
    };
    let input_vsize = checkpoint.sigset.est_witness_vsize() + 40;
    let deposit_fees = self
        .bitcoin
        .calc_minimum_deposit_fees(input_vsize, checkpoint.fee_rate);
    Ok(deposit_fees)
}

pub fn withdrawal_fees(
    store: &dyn Storage,
    address: Adapter<String>,
    index: Option<u32>,
) -> ContractResult<u64> {
    let checkpoint = match index {
        Some(index) => self.bitcoin.checkpoints.get(index)?,
        None => self
            .bitcoin
            .checkpoints
            .get(self.bitcoin.checkpoints.index)?, // get current checkpoint being built
    };
    let btc_address: BitcoinAddress = bitcoin::Address::from_str(address.into_inner().as_str())
        .map_err(|err| orga::Error::App(err.to_string()))?;
    let script = btc_address.script_pubkey();
    let withdrawal_fees = self
        .bitcoin
        .calc_minimum_withdrawal_fees(script.len() as u64, checkpoint.fee_rate);
    Ok(withdrawal_fees)
}
