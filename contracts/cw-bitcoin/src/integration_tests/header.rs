use crate::{
    adapter::Adapter, header::WrappedHeader, interface::HeaderConfig, msg, tests::helper::MockApp,
};
use bitcoincore_rpc_async::jsonrpc::error::RpcError;
use bitcoind::bitcoincore_rpc::RpcApi;
use bitcoind::{BitcoinD, Conf, P2P};
use cosmwasm_std::{coins, Addr, Uint128};
use oraiswap::asset::AssetInfo;

fn into_json<T>(val: T) -> Result<bitcoind::bitcoincore_rpc::jsonrpc::serde_json::Value, RpcError>
where
    T: serde::ser::Serialize,
{
    Ok(serde_json::to_value(val).unwrap())
}

#[cfg(feature = "mainnet")]
#[tokio::test]
#[serial_test::serial]
async fn test_relay_bulk_headers() {
    use std::str::FromStr;

    use bitcoin::{BlockHash, BlockHeader, TxMerkleNode};
    // Set up app

    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[3]);
    let token_fee_receiver = Addr::unchecked(&accounts[4]);

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                token_factory_addr: token_factory_addr.clone(),
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let trusted_height = 2872800;
    let header_config = HeaderConfig {
        max_length: 24192,
        max_time_increase: 2 * 60 * 60,
        trusted_height,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: true,
        trusted_header: Adapter::from(BlockHeader {
            bits: 420466436,
            nonce: 732839121,
            time: 1723142223,
            merkle_root: TxMerkleNode::from_str(
                "7f718cbfe30ad75f698ac6fdacb0a5c53aae8dfab2c5642a657fb8e03c766880",
            )
            .unwrap(),
            prev_blockhash: BlockHash::from_str(
                "000000000000000591b541ed7088c4ce52fd10a0b99a4b5db377a3c1ab198756",
            )
            .unwrap(),
            version: 654221312,
        }),
    };
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateHeaderConfig {
            config: header_config,
        },
        &[],
    )
    .unwrap();

    // write me a code that load headers from json
    static JSON: &[u8] = include_bytes!("testdata/headers.json");

    let headers: Vec<WrappedHeader> = serde_json::from_slice(JSON).unwrap();

    let num = 12;
    for i in 0..num {
        let res = app
            .execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RelayHeaders {
                    headers: headers[250 * i..250 * (i + 1)].to_vec(),
                },
                &[],
            )
            .unwrap();
        println!("gas used: {:?}", res.gas_info.gas_used);
    }
}

#[test]
fn reorg() {
    // Set up app

    let (mut app, accounts) = MockApp::new(&[("perfogic", &coins(100_000_000_000, "orai"))]);
    let owner = Addr::unchecked(&accounts[0]);
    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                token_factory_addr: token_factory_addr.clone(),
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let mut conf = Conf::default();
    conf.p2p = P2P::Yes;
    let node_1 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();
    let mut conf = Conf::default();
    conf.p2p = node_1.p2p_connect(true).unwrap();
    let node_2 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();
    let alice_address = node_1.client.get_new_address(Some("alice"), None).unwrap();
    let bob_address = node_2.client.get_new_address(Some("bob"), None).unwrap();

    node_1
        .client
        .generate_to_address(1, &alice_address)
        .unwrap();

    let tip_hash = node_1.client.get_best_block_hash().unwrap();
    let tip_height = node_1
        .client
        .get_block_header_info(&tip_hash)
        .unwrap()
        .height;

    let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();

    let header_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: tip_height as u32,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: Adapter::from(tip_header),
    };
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateHeaderConfig {
            config: header_config,
        },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(11);
    for _ in 0..10 {
        node_1
            .client
            .generate_to_address(1, &alice_address)
            .unwrap();

        let tip_hash = node_1.client.get_best_block_hash().unwrap();
        let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
        let tip_height_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_height_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders {
            headers: headers.clone(),
        },
        &[],
    )
    .unwrap();

    node_2
        .client
        .call::<bitcoind::bitcoincore_rpc::jsonrpc::serde_json::Value>(
            "disconnectnode",
            &[into_json(node_1.params.p2p_socket.unwrap()).unwrap()],
        )
        .unwrap();

    node_1
        .client
        .generate_to_address(1, &alice_address)
        .unwrap();

    let tip_hash = node_1.client.get_best_block_hash().unwrap();
    let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
    let tip_header_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
    let tip_height = tip_header_info.height;

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders {
            headers: vec![WrappedHeader::from_header(&tip_header, tip_height as u32)],
        },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(5);
    for _ in 0..5 {
        node_2.client.generate_to_address(1, &bob_address).unwrap();

        let tip_hash = node_2.client.get_best_block_hash().unwrap();
        let tip_header = node_2.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_2.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    let header_height: u32 = app
        .query(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 16);
}

#[test]
fn reorg_competing_chain_similar() {
    // Set up app
    let (mut app, accounts) = MockApp::new(&[("perfogic", &coins(100_000_000_000, "orai"))]);
    let owner = Addr::unchecked(&accounts[0]);
    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                token_factory_addr: token_factory_addr.clone(),
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let mut conf = Conf::default();
    conf.p2p = P2P::Yes;
    let node_1 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();

    let mut conf = Conf::default();
    conf.p2p = node_1.p2p_connect(true).unwrap();
    let node_2 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();
    let alice_address = node_1.client.get_new_address(Some("alice"), None).unwrap();
    let bob_address = node_2.client.get_new_address(Some("bob"), None).unwrap();

    node_1
        .client
        .generate_to_address(1, &alice_address)
        .unwrap();

    let tip_hash = node_1.client.get_best_block_hash().unwrap();
    let tip_height = node_1
        .client
        .get_block_header_info(&tip_hash)
        .unwrap()
        .height;

    let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();

    let header_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: tip_height as u32,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: Adapter::from(tip_header),
    };
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateHeaderConfig {
            config: header_config,
        },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(11);
    for _ in 0..10 {
        node_1
            .client
            .generate_to_address(1, &alice_address)
            .unwrap();

        let tip_hash = node_1.client.get_best_block_hash().unwrap();
        let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    node_2
        .client
        .call::<bitcoind::bitcoincore_rpc::jsonrpc::serde_json::Value>(
            "disconnectnode",
            &[into_json(node_1.params.p2p_socket.unwrap()).unwrap()],
        )
        .unwrap();

    let mut headers = Vec::with_capacity(5);
    for _ in 0..1 {
        node_1.client.generate_to_address(1, &bob_address).unwrap();

        let tip_hash = node_1.client.get_best_block_hash().unwrap();
        let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(5);
    for _ in 0..2 {
        node_2
            .client
            .generate_to_address(1, &alice_address)
            .unwrap();

        let tip_hash = node_2.client.get_best_block_hash().unwrap();
        let tip_header = node_2.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_2.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    let header_height: u32 = app
        .query(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 13);
}

#[test]
fn reorg_deep() {
    // Set up app
    let (mut app, accounts) = MockApp::new(&[("perfogic", &coins(100_000_000_000, "orai"))]);
    let owner = Addr::unchecked(&accounts[0]);
    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                token_factory_addr: token_factory_addr.clone(),
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let mut conf = Conf::default();
    conf.p2p = P2P::Yes;
    let node_1 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();

    let mut conf = Conf::default();
    conf.p2p = node_1.p2p_connect(true).unwrap();
    let node_2 = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();
    let alice_address = node_1.client.get_new_address(Some("alice"), None).unwrap();
    let bob_address = node_2.client.get_new_address(Some("bob"), None).unwrap();

    node_1
        .client
        .generate_to_address(1, &alice_address)
        .unwrap();

    let tip_hash = node_1.client.get_best_block_hash().unwrap();
    let tip_height = node_1
        .client
        .get_block_header_info(&tip_hash)
        .unwrap()
        .height;

    let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();

    let header_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: tip_height as u32,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: Adapter::from(tip_header),
    };
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateHeaderConfig {
            config: header_config,
        },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(10);
    for _ in 0..10 {
        node_1
            .client
            .generate_to_address(1, &alice_address)
            .unwrap();

        let tip_hash = node_1.client.get_best_block_hash().unwrap();
        let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    node_2
        .client
        .call::<bitcoind::bitcoincore_rpc::jsonrpc::serde_json::Value>(
            "disconnectnode",
            &[into_json(node_1.params.p2p_socket.unwrap()).unwrap()],
        )
        .unwrap();

    let mut headers = Vec::with_capacity(10);
    for _ in 0..10 {
        node_1
            .client
            .generate_to_address(1, &alice_address)
            .unwrap();

        let tip_hash = node_1.client.get_best_block_hash().unwrap();
        let tip_header = node_1.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_1.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    let mut headers = Vec::with_capacity(25);
    for _ in 0..25 {
        node_2.client.generate_to_address(1, &bob_address).unwrap();

        let tip_hash = node_2.client.get_best_block_hash().unwrap();
        let tip_header = node_2.client.get_block_header(&tip_hash).unwrap();
        let tip_header_info = node_2.client.get_block_header_info(&tip_hash).unwrap();
        let tip_height = tip_header_info.height;

        headers.push(WrappedHeader::from_header(&tip_header, tip_height as u32));
    }

    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::RelayHeaders { headers },
        &[],
    )
    .unwrap();

    let header_height: u32 = app
        .query(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 36);
}
