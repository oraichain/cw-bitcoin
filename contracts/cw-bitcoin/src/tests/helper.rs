use cosmwasm_std::{testing::mock_env, Env, Timestamp};

use crate::checkpoint::{BitcoinTx, Output};

pub fn push_bitcoin_tx_output(tx: &mut BitcoinTx, value: u64) {
    let tx_out = bitcoin::TxOut {
        value,
        script_pubkey: bitcoin::Script::new(),
    };
    tx.output.push(Output::new(tx_out));
}

pub fn set_time(seconds: u64) -> Env {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(seconds);
    env
}
