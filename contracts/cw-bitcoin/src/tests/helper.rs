use crate::checkpoint::{BitcoinTx, Output};
use crate::msg::{self};
use cosmwasm_std::{testing::mock_env, Env, Timestamp};
use cosmwasm_std::{Addr, Coin};
use cosmwasm_testing_util::{MockApp as TestingMockApp, MockResult};

use crate::{error::ContractResult, threshold_sig::Signature};
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::util::bip32::{ChildNumber, ExtendedPrivKey};

use derive_more::{Deref, DerefMut};

/// Sign the given messages with the given extended private key, deriving the
/// correct private keys for each signature.
pub fn sign(
    secp: &Secp256k1<bitcoin::secp256k1::SignOnly>,
    xpriv: &ExtendedPrivKey,
    to_sign: &[([u8; 32], u32)],
) -> ContractResult<Vec<Signature>> {
    Ok(to_sign
        .iter()
        .map(|(msg, index)| {
            let privkey = xpriv
                .derive_priv(secp, &[ChildNumber::from_normal_idx(*index)?])?
                .private_key;

            let signature = secp
                .sign_ecdsa(&Message::from_slice(&msg[..])?, &privkey)
                .serialize_compact()
                .to_vec();
            Ok(Signature(signature))
        })
        .collect::<ContractResult<Vec<_>>>()?)
}

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

#[cfg(feature = "test-tube")]
pub type TestMockApp = cosmwasm_testing_util::TestTubeMockApp;
#[cfg(not(feature = "test-tube"))]
pub type TestMockApp = cosmwasm_testing_util::MultiTestMockApp;

#[derive(Deref, DerefMut)]
pub struct MockApp {
    #[deref]
    #[deref_mut]
    app: TestMockApp,
    bridge_id: u64,
}

#[allow(dead_code)]
impl MockApp {
    pub fn new(init_balances: &[(&str, &[Coin])]) -> (Self, Vec<String>) {
        let (mut app, accounts) = TestMockApp::new(init_balances);
        let bridge_id;
        #[cfg(feature = "test-tube")]
        {
            static CW_BYTES: &[u8] = include_bytes!("./testdata/cw-bitcoin.wasm");
            bridge_id = app.upload(CW_BYTES);
        }
        #[cfg(not(feature = "test-tube"))]
        {
            bridge_id = app.upload(Box::new(
                cosmwasm_testing_util::ContractWrapper::new_with_empty(
                    crate::contract::execute,
                    crate::contract::instantiate,
                    crate::contract::query,
                )
                .with_sudo_empty(crate::contract::sudo),
            ));
        }

        (Self { app, bridge_id }, accounts)
    }

    /// external method
    pub fn create_bridge(
        &mut self,
        sender: Addr,
        init_msg: &msg::InstantiateMsg,
    ) -> MockResult<Addr> {
        let code_id = self.bridge_id;
        let addr = self.instantiate(code_id, sender, init_msg, &[], "cw-bitcoin-bridge")?;
        Ok(addr)
    }
}
