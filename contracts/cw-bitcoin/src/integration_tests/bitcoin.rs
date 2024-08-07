use std::borrow::Borrow;
use std::str::FromStr;

use super::utils::{
    get_wrapped_header_from_block_hash, populate_bitcoin_block, retry, test_bitcoin_client,
};
use crate::adapter::{Adapter, HashBinary};
use crate::app::ConsensusKey;
use crate::checkpoint::{self, BatchType, Checkpoint, CheckpointStatus};
use crate::constants::{BTC_NATIVE_TOKEN_DENOM, SIGSET_THRESHOLD};
use crate::header::WrappedHeader;
use crate::interface::{BitcoinConfig, CheckpointConfig, Dest, Xpub};
use crate::tests::helper::{sign, MockApp};
use crate::{interface::HeaderConfig, msg};
use bitcoin::consensus::Decodable;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::util::bip32::{ExtendedPrivKey, ExtendedPubKey};
use bitcoin::util::merkleblock::PartialMerkleTree;
use bitcoin::{Address, BlockHeader, Script, Transaction};
use bitcoincore_rpc_async::{Client, RpcApi as AsyncRpcApi};
use bitcoind::bitcoincore_rpc::RpcApi;
use bitcoind::{BitcoinD, Conf};
use cosmwasm_std::{Addr, Binary, Coin};
use cosmwasm_testing_util::AppResponse;
use token_bindings::Metadata;

async fn mine_and_relay_headers(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    block_num: u32,
    sender: Addr,
    bitcoin_bridge_addr: Addr,
) -> Vec<WrappedHeader> {
    let blocks = btc_client
        .generate_to_address(block_num.into(), wallet)
        .await
        .unwrap();

    let mut headers = Vec::new();
    for h in blocks.iter() {
        let result = get_wrapped_header_from_block_hash(&btc_client, h).await;
        headers.push(result);
    }
    app.execute(
        sender,
        bitcoin_bridge_addr,
        &msg::ExecuteMsg::RelayHeaders {
            headers: headers.clone(),
        },
        &[],
    )
    .unwrap();

    headers
}

async fn relay_checkpoint(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    sender: Addr,
    bitcoin_bridge_addr: Addr,
    checkpoint_index: u32,
) -> () {
    let completed_cps: Vec<Adapter<Transaction>> = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CompletedTxs { limit: 10 },
        )
        .unwrap();

    for cp in completed_cps {
        let tx = cp.into_inner();
        let wrapped_txid = btc_client.send_raw_transaction(&tx).await;
        match wrapped_txid {
            Ok(txid) => {
                let btc_tx = btc_client.get_raw_transaction(&txid, None).await.unwrap();

                let headers = mine_and_relay_headers(
                    btc_client,
                    app,
                    wallet,
                    1,
                    sender.clone(),
                    bitcoin_bridge_addr.clone(),
                )
                .await;
                let block = headers[0].block_hash();
                let block_height = headers[0].height();
                let tx_proof = btc_client
                    .get_tx_out_proof(&[btc_tx.txid()], Some(&block))
                    .await
                    .unwrap();
                let proof = bitcoin::util::merkleblock::MerkleBlock::consensus_decode(
                    &mut tx_proof.as_slice(),
                )
                .unwrap()
                .txn;

                app.execute(
                    sender.clone(),
                    bitcoin_bridge_addr.clone(),
                    &msg::ExecuteMsg::RelayCheckpoint {
                        btc_height: block_height,
                        btc_proof: Adapter::from(proof),
                        cp_index: checkpoint_index,
                    },
                    &[],
                )
                .unwrap();
            }
            Err(_e) => {}
        }
    }
}

#[tokio::test]
async fn test_full_flow_happy_case_bitcoin() {
    // Set up app
    let owner = Addr::unchecked("perfogic");
    let threshold = SIGSET_THRESHOLD;
    let mut app = MockApp::new(&[]);
    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                token_factory_addr: token_factory_addr.clone(),
                bridge_wasm_addr: None,
            },
        )
        .unwrap();

    // Set up bitcoin
    let mut conf = Conf::default();
    conf.args.push("-txindex");
    let bitcoind = BitcoinD::with_conf(bitcoind::downloaded_exe_path().unwrap(), &conf).unwrap();
    let rpc_url = bitcoind.rpc_url();
    let cookie_file = bitcoind.params.cookie_file.clone();
    let btc_client = test_bitcoin_client(rpc_url.clone(), cookie_file.clone()).await;
    let wallet = retry(|| bitcoind.create_wallet("bridger"), 10).unwrap();
    let wallet_address = wallet.get_new_address(None, None).unwrap();

    let async_wallet_address =
        bitcoincore_rpc_async::bitcoin::Address::from_str(&wallet_address.to_string()).unwrap();
    btc_client
        .generate_to_address(1000, &async_wallet_address)
        .await
        .unwrap();
    let block_data = populate_bitcoin_block(&btc_client).await;
    let trusted_header = block_data.block_header;

    let register_denom = |app: &mut MockApp,
                          subdenom: String,
                          metadata: Option<Metadata>|
     -> Result<AppResponse, _> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
            &[],
        )
    };

    let init_bitcoin_config = |app: &mut MockApp| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        let _ = app
            .execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::UpdateBitcoinConfig {
                    config: bitcoin_config,
                },
                &[],
            )
            .unwrap();
    };

    let init_checkpoint_config = |app: &mut MockApp| -> () {
        // Set up header config based on the header of block data
        let mut checkpoint_config = CheckpointConfig::default();
        checkpoint_config.min_checkpoint_interval = 1; // 1 seconds

        let _ = app
            .execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::UpdateCheckpointConfig {
                    config: checkpoint_config,
                },
                &[],
            )
            .unwrap();
    };

    let init_headers =
        |app: &mut MockApp, trusted_height: u32, trusted_header: BlockHeader| -> () {
            // Set up header config based on the header of block data
            let header_config = HeaderConfig {
                max_length: 2000,
                max_time_increase: 8 * 60 * 60,
                trusted_height,
                retarget_interval: 2016,
                target_spacing: 10 * 60,
                target_timespan: 2016 * (10 * 60),
                max_target: 0x1d00ffff,
                retargeting: true,
                min_difficulty_blocks: false,
                trusted_header: Adapter::from(trusted_header),
            };
            let _ = app
                .execute(
                    owner.clone(),
                    bitcoin_bridge_addr.clone(),
                    &msg::ExecuteMsg::UpdateHeaderConfig {
                        config: header_config,
                    },
                    &[],
                )
                .unwrap();
        };

    let relay_deposit = |app: &mut MockApp,
                         btc_tx: Adapter<Transaction>,
                         btc_height: u32,
                         btc_proof: Adapter<PartialMerkleTree>,
                         btc_vout: u32,
                         sigset_index: u32,
                         dest: Dest|
     -> Result<AppResponse, _> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::RelayDeposit {
                btc_tx,
                btc_height,
                btc_proof,
                btc_vout,
                sigset_index,
                dest,
            },
            &[],
        )
    };

    let add_validators = |app: &mut MockApp,
                          addrs: Vec<String>,
                          infos: Vec<(u64, ConsensusKey)>|
     -> Result<AppResponse, _> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::AddValidators { addrs, infos },
            &[],
        )
    };

    let withdraw_to_bitcoin = |app: &mut MockApp,
                               sender: Addr,
                               script_pubkey: Adapter<Script>,
                               coin: Coin|
     -> Result<AppResponse, _> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::WithdrawToBitcoin { script_pubkey },
            &[coin],
        )
    };

    let set_signatory_key =
        |app: &mut MockApp, sender: Addr, xpub: Xpub| -> Result<AppResponse, _> {
            app.execute(
                sender,
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::SetSignatoryKey {
                    xpub: HashBinary(xpub),
                },
                &[],
            )
        };

    let increase_block = |app: &mut MockApp, hash: Binary| -> Result<AppResponse, _> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::TriggerBeginBlock { hash },
            &[],
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> Result<AppResponse, _> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .as_querier()
            .query_wasm_smart(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: HashBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: HashBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    // Start testing
    init_bitcoin_config(&mut app);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 1020);

    // Set up 2 validators here
    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
        ExtendedPrivKey::new_master(network, &[2]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
        ExtendedPubKey::from_priv(&secp, &xprivs[2]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32], [2; 32]];
    let validator_1 = Addr::unchecked("orai1Alice");
    let validator_2 = Addr::unchecked("orai1Bob");
    let validator_3 = Addr::unchecked("orai1Dave");
    add_validators(
        &mut app,
        vec![
            validator_1.clone().to_string(),
            validator_2.clone().to_string(),
        ],
        vec![(15, consensus_keys[0]), (10, consensus_keys[1])],
    )
    .unwrap();
    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    add_validators(
        &mut app,
        vec![validator_3.clone().to_string()],
        vec![(25, [3; 32])],
    )
    .unwrap();
    set_signatory_key(&mut app, validator_3.clone(), Xpub::new(xpubs[2])).unwrap();

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // Bridge one transaction and try to submit tx with proof when not enough confirmations
    let receiver = Addr::unchecked("receiver");
    let dest = Dest::Address(receiver.clone());
    let script = sigset
        .output_script(&dest.commitment_bytes().unwrap(), threshold)
        .unwrap();
    let deposit_addr = bitcoin::Address::from_script(&script, bitcoin::Network::Regtest).unwrap();
    let deposit_amount = bitcoin::Amount::from_btc(1.2).unwrap();

    let btc_txid = wallet
        .send_to_address(
            &deposit_addr,
            deposit_amount,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    let btc_tx = btc_client
        .get_raw_transaction(&btc_txid, None)
        .await
        .unwrap();
    let vout = btc_tx
        .output
        .iter()
        .position(|o| o.value == deposit_amount.to_sat())
        .unwrap();

    // mine one block to get proof
    let headers = mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        2,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 1022);

    let tx_proof = btc_client
        .get_tx_out_proof(&[btc_tx.txid()], Some(&headers[0].block_hash()))
        .await
        .unwrap();
    let proof = bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut tx_proof.as_slice())
        .unwrap()
        .txn;
    relay_deposit(
        &mut app,
        Adapter::from(btc_tx),
        1021,
        Adapter::from(proof),
        vout as u32, // vout
        0,           // sigset_index
        dest.clone(),
    )
    .unwrap();

    // Increase block and current Building checkpoint changed to Signing
    increase_block(&mut app, Binary::from([1; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);

    // Validators submit signature
    sign_cp(&mut app, validator_1.clone(), &xprivs[0], xpubs[0], 0, 1021).unwrap();
    sign_cp(&mut app, validator_2.clone(), &xprivs[1], xpubs[1], 0, 1021).unwrap();

    // Increase block and current Signing checkpoint changed to Complete
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .as_querier()
        .query_balance(&receiver, btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.amount.u128(), 119964806000000 as u128);
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);

    // Relay checkpoint
    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);

    // Make sure checkpoint one have 3 validators
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 3);

    // Test deposit + withdraw, for covering more cases here I will add an another validator
    let withdraw_address = wallet.get_new_address(None, None).unwrap();
    let script = checkpoint
        .sigset
        .output_script(&dest.commitment_bytes().unwrap(), threshold)
        .unwrap();
    let deposit_addr = bitcoin::Address::from_script(&script, bitcoin::Network::Regtest).unwrap();
    let btc_txid = wallet
        .send_to_address(
            &deposit_addr,
            deposit_amount,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    let btc_tx = btc_client
        .get_raw_transaction(&btc_txid, None)
        .await
        .unwrap();
    let vout = btc_tx
        .output
        .iter()
        .position(|o| o.value == deposit_amount.to_sat())
        .unwrap();
    withdraw_to_bitcoin(
        &mut app,
        receiver.clone(),
        Adapter::from(withdraw_address.script_pubkey()),
        Coin {
            denom: btc_bridge_denom.clone(),
            amount: (bitcoin::Amount::from_btc(0.5).unwrap().to_sat() * 1000000).into(),
        },
    )
    .unwrap();

    let headers = mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        2,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    assert_eq!(header_height, 1025);
    let tx_proof = btc_client
        .get_tx_out_proof(&[btc_tx.txid()], Some(&headers[0].block_hash()))
        .await
        .unwrap();
    let proof = bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut tx_proof.as_slice())
        .unwrap()
        .txn;
    relay_deposit(
        &mut app,
        Adapter::from(btc_tx),
        1024,
        Adapter::from(proof),
        vout as u32, // vout
        1,           // sigset_index
        dest.clone(),
    )
    .unwrap();

    // Increase block and current Building checkpoint changed to Signing
    increase_block(&mut app, Binary::from([4; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);
    println!("Checkpoint: {:?}", checkpoint.sigset);

    sign_cp(&mut app, validator_1.clone(), &xprivs[0], xpubs[0], 1, 1025).unwrap();
    sign_cp(&mut app, validator_2.clone(), &xprivs[1], xpubs[1], 1, 1025).unwrap();
    sign_cp(&mut app, validator_3.clone(), &xprivs[2], xpubs[2], 1, 1025).unwrap();

    // Increase block and current Signing checkpoint changed to Complete
    increase_block(&mut app, Binary::from([5; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Check balance
    let balance = app
        .as_querier()
        .query_balance(&receiver, btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.amount.u128(), 189917880000000 as u128);

    let mut trusted_balance = 0;
    match wallet.get_balances() {
        Ok(data) => {
            trusted_balance = data.mine.trusted.to_sat();
        }
        Err(_e) => {}
    }
    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        1,
    )
    .await;

    match wallet.get_balances() {
        Ok(data) => {
            let magic_number = 78000000; // this is number which may be from mined block
            let gap = data.mine.trusted.to_sat() - trusted_balance;
            assert!(gap - magic_number > 50000000);
        }
        Err(_e) => {}
    }

    let confirmed_cp_index: u32 = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 1);
}
