use super::helper::MockApp;
use crate::adapter::Adapter;
use crate::header::{WorkHeader, WrappedHeader};
use crate::interface::{Dest, HeaderConfig};
use crate::msg;
use bitcoin::hashes::Hash;
use bitcoin::util::merkleblock::PartialMerkleTree;
use bitcoin::util::uint;
use bitcoin::{BlockHash, BlockHeader, Transaction, TxMerkleNode, Txid};
use cosmwasm_std::Addr;

#[test]
fn test_relay_height_validity() {
    let mut app = MockApp::new(&[]);
    let bridge_addr = app
        .create_bridge(Addr::unchecked("alice"), &msg::InstantiateMsg {})
        .unwrap();

    let header_config = HeaderConfig::from_bytes(include_bytes!("checkpoint.json")).unwrap();
    let header = header_config.work_header();
    let _res = app
        .execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::UpdateHeaderConfig {
                config: header_config,
            },
            &[],
        )
        .unwrap();

    let _res = app
        .execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::AddWorkHeader { header },
            &[],
        )
        .unwrap();

    for _ in 0..10 {
        let btc_height: u32 = app
            .query(bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
            .unwrap();

        let header = WorkHeader::new(
            WrappedHeader::new(
                Adapter::new(BlockHeader {
                    bits: 0,
                    merkle_root: TxMerkleNode::all_zeros(),
                    nonce: 0,
                    prev_blockhash: BlockHash::all_zeros(),
                    time: 0,
                    version: 0,
                }),
                btc_height + 1,
            ),
            uint::Uint256([0, 0, 0, 0]),
        );
        app.execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::AddWorkHeader { header },
            &[],
        )
        .unwrap();
    }

    let h: u32 = app
        .query(bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();

    let mut try_relay = |height: u32| {
        // TODO: make test cases not fail at irrelevant steps in relay_deposit
        // (either by passing in valid input, or by handling other error paths)

        let btc_tx = Transaction {
            input: vec![],
            lock_time: bitcoin::PackedLockTime(0),
            output: vec![],
            version: 0,
        }
        .into();

        let btc_proof = PartialMerkleTree::from_txids(&[Txid::all_zeros()], &[true]).into();

        app.execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::RelayDeposit {
                btc_tx,
                btc_height: height,
                btc_proof,
                btc_vout: 0,
                sigset_index: 0,
                dest: Dest::Address(Addr::unchecked("bob")),
            },
            &[],
        )
    };

    assert!(try_relay(h + 100)
        .unwrap_err()
        .to_string()
        .contains("error executing WasmMsg"));
    assert!(try_relay(h - 100)
        .unwrap_err()
        .to_string()
        .contains("error executing WasmMsg"))
}
