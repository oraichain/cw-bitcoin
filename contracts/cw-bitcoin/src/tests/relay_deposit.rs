use super::helper::MockApp;
use crate::{error::ContractResult, msg};

// #[test]

// fn test_relay_deposit() -> ContractResult<()> {
//     let mut app = MockApp::new(&[]);
//     let bridge_addr = app.create_bridge(&msg::InstantiateMsg {})?;
//     app.execute(
//         Addr::unchecked("alice"),
//         bridge_addr,
//         msg::ExecuteMsg::RelayDeposit {
//             btc_tx: (),
//             btc_height: (),
//             btc_proof: (),
//             btc_vout: (),
//             sigset_index: (),
//             dest: (),
//         },
//         &[],
//     )?;

//     Ok(())
// }
