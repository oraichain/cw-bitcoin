use cosmwasm_std::{Addr, Coin};
use cosmwasm_testing_util::MockResult;

use crate::msg;
use derive_more::{Deref, DerefMut};

#[cfg(not(feature = "test-tube"))]
pub type TestMockApp = cosmwasm_testing_util::MultiTestMockApp;
#[cfg(feature = "test-tube")]
pub type TestMockApp = cosmwasm_testing_util::TestTubeMockApp;
#[derive(Deref, DerefMut)]
pub struct MockApp {
    #[deref]
    #[deref_mut]
    app: TestMockApp,
    light_client_id: u64,
}

#[allow(dead_code)]
impl MockApp {
    pub fn new(init_balances: &[(&str, &[Coin])]) -> (Self, Vec<String>) {
        let (mut app, accounts) = TestMockApp::new(init_balances);
        let light_client_id;
        #[cfg(feature = "test-tube")]
        {
            static CW_BYTES: &[u8] = include_bytes!("./testdata/light-client-bitcoin.wasm");
            light_client_id = app.upload(CW_BYTES);
        }
        #[cfg(not(feature = "test-tube"))]
        {
            light_client_id = app.upload(Box::new(
                cosmwasm_testing_util::ContractWrapper::new_with_empty(
                    crate::contract::execute,
                    crate::contract::instantiate,
                    crate::contract::query,
                ),
            ));
        }

        (
            Self {
                app,
                light_client_id,
            },
            accounts,
        )
    }

    /// external method
    pub fn create_light_client(
        &mut self,
        sender: Addr,
        init_msg: &msg::InstantiateMsg,
    ) -> MockResult<Addr> {
        let code_id = self.light_client_id;
        let addr = self.instantiate(
            code_id,
            sender.clone(),
            init_msg,
            &[],
            "light-client-bitcoin-bridge",
        )?;
        Ok(addr)
    }
}
