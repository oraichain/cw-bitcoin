use super::utils::{
    get_wrapped_header_from_block_hash, populate_bitcoin_block, retry, test_bitcoin_client,
};
use crate::checkpoint::{Checkpoint, CheckpointStatus};
use crate::constants::{BTC_NATIVE_TOKEN_DENOM, SIGSET_THRESHOLD};
use crate::interface::{BitcoinConfig, CheckpointConfig, Dest};
use crate::msg;
use crate::recovery::SignedRecoveryTx;
use crate::state::Ratio;
use crate::tests::helper::{sign, MockApp};
use bitcoin::consensus::Decodable;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::util::bip32::{ExtendedPrivKey, ExtendedPubKey};
use bitcoin::util::merkleblock::PartialMerkleTree;
use bitcoin::{Address, BlockHeader, Transaction};
use bitcoincore_rpc_async::{Client, RawTx, RpcApi as AsyncRpcApi};
use bitcoind::bitcoincore_rpc::RpcApi;
use bitcoind::{BitcoinD, Conf};
use common_bitcoin::adapter::Adapter;
use common_bitcoin::adapter::WrappedBinary;
use common_bitcoin::xpub::Xpub;
use cosmwasm_std::coins;
use cosmwasm_std::{Addr, Binary, Coin, Uint128};
use cosmwasm_testing_util::MockResult;
use light_client_bitcoin::header::WrappedHeader;
use light_client_bitcoin::interface::HeaderConfig;
use light_client_bitcoin::msg as lc_msg;
use std::str::FromStr;

use oraiswap::asset::AssetInfo;
use token_bindings::Metadata;

async fn mine_and_relay_headers(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    block_num: u32,
    sender: Addr,
    light_client_addr: Addr,
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
    let res = app
        .execute(
            sender,
            light_client_addr,
            &lc_msg::ExecuteMsg::RelayHeaders {
                headers: headers.clone(),
            },
            &[],
        )
        .unwrap();

    println!("gas used {}", res.gas_info.gas_used);

    headers
}

async fn relay_checkpoint(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    sender: Addr,
    bitcoin_bridge_addr: Addr,
    light_client_addr: Addr,
    checkpoint_index: u32,
) -> () {
    let completed_cps: Vec<Adapter<Transaction>> = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CompletedCheckpointTxs { limit: 10 },
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
                    light_client_addr.clone(),
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
            Err(_e) => {
                println!("Error: {:?}", _e);
            }
        }
    }
}

async fn relay_recovery(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    sender: Addr,
    bitcoin_bridge_addr: Addr,
    light_client_addr: Addr,
) -> () {
    let recovery_txs: Vec<SignedRecoveryTx> = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::SignedRecoveryTxs {},
        )
        .unwrap();

    for recovery_tx in recovery_txs {
        let transaction = recovery_tx.tx.clone();
        let txid = btc_client
            .send_raw_transaction(&transaction.into_inner())
            .await;
        let headers = mine_and_relay_headers(
            btc_client,
            app,
            wallet,
            2,
            sender.clone(),
            light_client_addr.clone(),
        )
        .await;
        let header = &headers[0];
        let tx_proof = btc_client
            .get_tx_out_proof(&[txid.unwrap()], Some(&header.block_hash()))
            .await
            .unwrap();
        let proof =
            bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut tx_proof.as_slice())
                .unwrap()
                .txn;
        app.execute(
            sender.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::RelayDeposit {
                btc_tx: recovery_tx.tx.clone(),
                btc_height: header.height(),
                btc_proof: Adapter::from(proof),
                btc_vout: 0, // always is zero for sure
                sigset_index: recovery_tx.sigset_index,
                dest: recovery_tx.dest,
            },
            &[],
        )
        .unwrap();
    }
}

#[cfg(all(feature = "mainnet", not(feature = "native-validator")))]
#[tokio::test]
async fn test_full_flow_happy_case_bitcoin() {
    // Set up app
    let threshold = SIGSET_THRESHOLD;
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("dave", &coins(100_000_000_000, "orai")),
        ("jayce", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let validator_3 = Addr::unchecked(&accounts[3]);
    let validator_4 = Addr::unchecked(&accounts[4]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[5]);
    let token_fee_receiver = Addr::unchecked(&accounts[6]);
    let receiver = Addr::unchecked(&accounts[7]);

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
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

    let register_denom =
        |app: &mut MockApp, subdenom: String, metadata: Option<Metadata>| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
                &coins(10_000_000, "orai"),
            )
        };

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
        app.execute(
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

        app.execute(
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
            app.execute(
                owner.clone(),
                light_client_addr.clone(),
                &lc_msg::ExecuteMsg::UpdateHeaderConfig {
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
     -> MockResult<_> {
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> MockResult<_> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::AddValidators {
                addrs,
                voting_powers,
                consensus_keys,
            },
            &[],
        )
    };

    let withdraw_to_bitcoin =
        |app: &mut MockApp, sender: Addr, btc_address: Address, coin: Coin| -> MockResult<_> {
            app.execute(
                sender,
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::WithdrawToBitcoin {
                    btc_address: btc_address.to_string(),
                    fee: None,
                },
                &[coin],
            )
        };

    let set_signatory_key = |app: &mut MockApp, sender: Addr, xpub: Xpub| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SetSignatoryKey {
                xpub: WrappedBinary(xpub),
            },
            &[],
        )
    };

    let increase_block = |app: &mut MockApp, hash: Binary| -> MockResult<_> {
        app.sudo(
            bitcoin_bridge_addr.clone(),
            &msg::SudoMsg::ClockEndBlock { hash },
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    let sign_recovery = |app: &mut MockApp,
                         sender: Addr,
                         xpriv: &ExtendedPrivKey,
                         xpub: ExtendedPubKey|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningRecoveryTxs {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitRecoverySignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
            },
            &[],
        )
    };

    // Start testing
    init_bitcoin_config(&mut app, 180);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1020);

    // Set up 2 validators here
    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
        ExtendedPrivKey::new_master(network, &[2]).unwrap(),
        ExtendedPrivKey::new_master(network, &[3]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
        ExtendedPubKey::from_priv(&secp, &xprivs[2]),
        ExtendedPubKey::from_priv(&secp, &xprivs[3]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32], [2; 32], [3; 32]];

    add_validators(
        &mut app,
        vec![
            validator_1.clone().to_string(),
            validator_2.clone().to_string(),
        ],
        vec![15, 10],
        vec![consensus_keys[0], consensus_keys[1]],
    )
    .unwrap();
    // add validator 4
    add_validators(
        &mut app,
        vec![validator_4.clone().to_string()],
        vec![1],
        vec![consensus_keys[3]],
    )
    .unwrap();
    set_signatory_key(&mut app, validator_4.clone(), Xpub::new(xpubs[3])).unwrap();
    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    add_validators(
        &mut app,
        vec![validator_3.clone().to_string()],
        vec![25],
        vec![consensus_keys[2]],
    )
    .unwrap();
    set_signatory_key(&mut app, validator_3.clone(), Xpub::new(xpubs[2])).unwrap();

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction and try to submit tx with proof when not enough confirmations
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

    let expired_btc_txid = wallet
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
    let expired_btc_tx = btc_client
        .get_raw_transaction(&expired_btc_txid, None)
        .await
        .unwrap();
    let expired_vout = expired_btc_tx
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
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1022);

    // this proof is for current depositing
    let tx_proof = btc_client
        .get_tx_out_proof(&[btc_tx.txid()], Some(&headers[0].block_hash()))
        .await
        .unwrap();
    let proof = bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut tx_proof.as_slice())
        .unwrap()
        .txn;

    // this proof is for expired tx
    let expired_tx_proof = btc_client
        .get_tx_out_proof(&[expired_btc_tx.txid()], Some(&headers[0].block_hash()))
        .await
        .unwrap();
    let expired_proof =
        bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut expired_tx_proof.as_slice())
            .unwrap()
            .txn;

    let deposit_fee: u64 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::DepositFees { index: None },
        )
        .unwrap();
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
        .query(
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(
        balance.u128(),
        (deposit_amount.to_sat() * 1000000 - deposit_fee) as u128
    );
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
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
        light_client_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);

    // Make sure checkpoint one have 4 validators
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 4);

    // [TESTCASE] Test deposit + withdraw, for covering more cases here I will add an another validator
    let withdraw_address = wallet.get_new_address(None, None).unwrap();
    let script = checkpoint
        .sigset
        .output_script(&dest.commitment_bytes().unwrap(), threshold)
        .unwrap();
    withdraw_to_bitcoin(
        &mut app,
        receiver.clone(),
        withdraw_address,
        Coin {
            denom: btc_bridge_denom.clone(),
            amount: (bitcoin::Amount::from_btc(0.5).unwrap().to_sat() * 1000000).into(),
        },
    )
    .unwrap();

    // deposit
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

    let headers = mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        2,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);

    sign_cp(&mut app, validator_1.clone(), &xprivs[0], xpubs[0], 1, 1025).unwrap();
    sign_cp(&mut app, validator_2.clone(), &xprivs[1], xpubs[1], 1, 1025).unwrap();
    sign_cp(&mut app, validator_3.clone(), &xprivs[2], xpubs[2], 1, 1025).unwrap();

    // Increase block and current Signing checkpoint changed to Complete
    increase_block(&mut app, Binary::from([5; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Check balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.u128(), 189894417000000 as u128);

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
        light_client_addr.clone(),
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 1);

    // [TESTCASE] test recovery
    init_bitcoin_config(&mut app, 45);

    println!("Waiting 10 seconds to make the deposit expired!",);
    app.increase_time(10);

    relay_deposit(
        &mut app,
        Adapter::from(expired_btc_tx),
        1021,
        Adapter::from(expired_proof),
        expired_vout as u32, // vout
        0,                   // sigset_index
        dest.clone(),
    )
    .unwrap();
    sign_recovery(&mut app, validator_1.clone(), &xprivs[0], xpubs[0]).unwrap();
    sign_recovery(&mut app, validator_2.clone(), &xprivs[1], xpubs[1]).unwrap();

    init_bitcoin_config(&mut app, 300); // update to max age of current tx
    relay_recovery(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        light_client_addr.clone(),
    )
    .await;
    increase_block(&mut app, Binary::from([6; 32])).unwrap(); // should increase number of hash to be unique
    let current_header: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);
    sign_cp(
        &mut app,
        validator_1.clone(),
        &xprivs[0],
        xpubs[0],
        2,
        current_header,
    )
    .unwrap();
    sign_cp(
        &mut app,
        validator_2.clone(),
        &xprivs[1],
        xpubs[1],
        2,
        current_header,
    )
    .unwrap();
    sign_cp(
        &mut app,
        validator_3.clone(),
        &xprivs[2],
        xpubs[2],
        2,
        current_header,
    )
    .unwrap();
    // Increase block and current Signing checkpoint changed to Complete
    increase_block(&mut app, Binary::from([7; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Check balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.u128(), 309835521000000 as u128);

    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        light_client_addr.clone(),
        2,
    )
    .await;

    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 2);

    // [TESTCASE] check validator 4 is punished, validate the changing in signatures length
    increase_block(&mut app, Binary::from([8; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 3);
    assert_eq!(checkpoint.sigset.present_vp, 26);

    // Here validator 3 is added
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 4);
    assert_eq!(checkpoint.sigset.present_vp, 51);

    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 4);
    assert_eq!(checkpoint.sigset.present_vp, 51);

    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 3 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 3);
    assert_eq!(checkpoint.sigset.present_vp, 50);
    println!("[BRAVOOO] All testcases passed!");
}

#[cfg(all(feature = "mainnet", not(feature = "native-validator")))]
#[tokio::test]
async fn test_full_flow_with_foundation_validators() {
    // Set up app

    use bitcoin::{secp256k1::Message, util::bip32::ChildNumber, EcdsaSighashType, Sequence};

    use crate::threshold_sig::Pubkey;

    let threshold = SIGSET_THRESHOLD;
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("dave", &coins(100_000_000_000, "orai")),
        ("jayce", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let validator_3 = Addr::unchecked(&accounts[3]);
    let validator_4 = Addr::unchecked(&accounts[4]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[5]);
    let token_fee_receiver = Addr::unchecked(&accounts[6]);
    let receiver = Addr::unchecked(&accounts[7]);

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
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
    let receive_fund_address = wallet.get_new_address(None, None).unwrap();

    let async_wallet_address =
        bitcoincore_rpc_async::bitcoin::Address::from_str(&wallet_address.to_string()).unwrap();
    btc_client
        .generate_to_address(1000, &async_wallet_address)
        .await
        .unwrap();
    let block_data = populate_bitcoin_block(&btc_client).await;
    let trusted_header = block_data.block_header;

    let register_denom =
        |app: &mut MockApp, subdenom: String, metadata: Option<Metadata>| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
                &coins(10_000_000, "orai"),
            )
        };

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
        app.execute(
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

        app.execute(
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
            app.execute(
                owner.clone(),
                light_client_addr.clone(),
                &lc_msg::ExecuteMsg::UpdateHeaderConfig {
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
     -> MockResult<_> {
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> MockResult<_> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::AddValidators {
                addrs,
                voting_powers,
                consensus_keys,
            },
            &[],
        )
    };

    let set_signatory_key = |app: &mut MockApp, sender: Addr, xpub: Xpub| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SetSignatoryKey {
                xpub: WrappedBinary(xpub),
            },
            &[],
        )
    };

    let update_foundation_keys = |app: &mut MockApp, xpubs: Vec<ExtendedPubKey>| -> MockResult<_> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::UpdateFoundationKeys {
                xpubs: xpubs
                    .iter()
                    .map(|x| WrappedBinary(Xpub::new(x.clone())))
                    .collect(),
            },
            &[],
        )
    };

    let increase_block = |app: &mut MockApp, hash: Binary| -> MockResult<_> {
        app.sudo(
            bitcoin_bridge_addr.clone(),
            &msg::SudoMsg::ClockEndBlock { hash },
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    // Start testing
    init_bitcoin_config(&mut app, 180);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1020);

    // Set up 2 validators here
    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
        ExtendedPrivKey::new_master(network, &[2]).unwrap(),
        ExtendedPrivKey::new_master(network, &[3]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
        ExtendedPubKey::from_priv(&secp, &xprivs[2]),
        ExtendedPubKey::from_priv(&secp, &xprivs[3]),
    ];
    let foundation_xprivs = vec![
        ExtendedPrivKey::new_master(network, &[4]).unwrap(),
        ExtendedPrivKey::new_master(network, &[5]).unwrap(),
        ExtendedPrivKey::new_master(network, &[6]).unwrap(),
    ];
    let foundation_xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &foundation_xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &foundation_xprivs[1]),
        ExtendedPubKey::from_priv(&secp, &foundation_xprivs[2]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32], [2; 32], [3; 32]];

    let _ = update_foundation_keys(&mut app, foundation_xpubs.clone()).unwrap();
    add_validators(
        &mut app,
        vec![
            validator_1.clone().to_string(),
            validator_2.clone().to_string(),
        ],
        vec![15, 10],
        vec![consensus_keys[0], consensus_keys[1]],
    )
    .unwrap();
    // add validator 4
    add_validators(
        &mut app,
        vec![validator_4.clone().to_string()],
        vec![1],
        vec![consensus_keys[3]],
    )
    .unwrap();
    set_signatory_key(&mut app, validator_4.clone(), Xpub::new(xpubs[3])).unwrap();
    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction and try to submit tx with proof when not enough confirmations
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
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1022);

    // this proof is for current depositing
    let tx_proof = btc_client
        .get_tx_out_proof(&[btc_tx.txid()], Some(&headers[0].block_hash()))
        .await
        .unwrap();
    let proof = bitcoin::util::merkleblock::MerkleBlock::consensus_decode(&mut tx_proof.as_slice())
        .unwrap()
        .txn;

    let deposit_fee: u64 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::DepositFees { index: None },
        )
        .unwrap();
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
        .query(
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(
        balance.u128(),
        (deposit_amount.to_sat() * 1000000 - deposit_fee) as u128
    );
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
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
        light_client_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);

    // Notice: fetch checkpoint and try to withdraw it using foundation keys
    let checkpoint_tx: Adapter<Transaction> = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointTx { index: Some(0) },
        )
        .unwrap();
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();

    let sigset = checkpoint.sigset.clone();
    let redeem_script = sigset.redeem_script(&[0], threshold).unwrap();
    let bitcoin_txin = bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: checkpoint_tx.txid(),
            vout: 0,
        },
        script_sig: bitcoin::Script::default(),
        sequence: Sequence(u32::MAX),
        witness: bitcoin::Witness::new(),
    };
    let input_amount = if checkpoint_tx.output[0].value > 0 {
        checkpoint_tx.output[0].value
    } else {
        checkpoint_tx.output[1].value
    };
    let bitcoin_txout = bitcoin::TxOut {
        value: 1000,
        script_pubkey: receive_fund_address.script_pubkey(),
    };
    let bitcoin_transaction = bitcoin::Transaction {
        version: 1,
        lock_time: bitcoin::PackedLockTime(0),
        input: vec![bitcoin_txin],
        output: vec![bitcoin_txout],
    };
    let mut sc = bitcoin::util::sighash::SighashCache::new(&bitcoin_transaction);
    let sighash: bitcoin::Sighash = sc
        .segwit_signature_hash(0, &redeem_script, input_amount, EcdsaSighashType::All)
        .unwrap();
    // sign with foundation priv keys
    let mut sigs = vec![];
    let secp = Secp256k1::new();
    for i in 0..3 {
        let privkey = foundation_xprivs[i]
            .derive_priv(&secp, &[ChildNumber::from_normal_idx(1).unwrap()])
            .unwrap()
            .private_key;
        let message = Message::from_slice(&sighash.as_ref()).unwrap();
        let sig = secp.sign_ecdsa(&message, &privkey);
        sigs.push(sig);
    }

    // append signatures to witness based on the order of foundation keys
    let mut witness = bitcoin::Witness::new();
    let foundation_signatories = checkpoint.sigset.foundation_signatories;
    for i in 0..3 {
        for j in 0..3 {
            let pubkey: Pubkey = Xpub::new(foundation_xpubs[j])
                .derive_pubkey(1)
                .unwrap()
                .into();
            if foundation_signatories[2 - i].pubkey == pubkey {
                witness.push(sigs[j].serialize_der().to_vec());
            }
        }
    }
    witness.push(vec![1]);
    witness.push(redeem_script.into_bytes());
    let bitcoin_txin = bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: checkpoint_tx.txid(),
            vout: 0,
        },
        script_sig: bitcoin::Script::default(),
        sequence: Sequence(u32::MAX),
        witness,
    };
    let bitcoin_txout = bitcoin::TxOut {
        value: 1000,
        script_pubkey: receive_fund_address.script_pubkey(),
    };
    let bitcoin_transaction = bitcoin::Transaction {
        version: 1,
        lock_time: bitcoin::PackedLockTime(0),
        input: vec![bitcoin_txin],
        output: vec![bitcoin_txout],
    };
    let wrapped_txid = btc_client.send_raw_transaction(&bitcoin_transaction).await;
    println!("tx hex: {:?}", bitcoin_transaction.raw_hex());
    println!("txin hex: {:?}", checkpoint_tx.raw_hex());
    match wrapped_txid {
        Ok(txid) => {
            println!("Withdraw txid: {}", txid);
        }
        Err(err) => {
            println!("Failed to withdraw {:?}", err);
        }
    }
    println!("[BRAVOOO] All testcases passed!");
}

#[cfg(all(feature = "mainnet", feature = "native-validator"))]
#[tokio::test]
#[serial_test::serial]
async fn test_full_flow_native_validators() {
    let threshold = SIGSET_THRESHOLD;
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[3]);
    let token_fee_receiver = Addr::unchecked(&accounts[4]);
    let receiver = Addr::unchecked(&accounts[5]);
    let _ = app
        .inner()
        .setup_validator_with_secret(&coins(13_000_000_000, "orai"), "alice")
        .unwrap();
    let _ = app
        .inner()
        .setup_validator_with_secret(&coins(15_000_000_000, "orai"), "bob")
        .unwrap();

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();

    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );

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

    // common functions
    let register_denom =
        |app: &mut MockApp, subdenom: String, metadata: Option<Metadata>| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
                &coins(10_000_000, "orai"),
            )
        };

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
        app.execute(
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

        app.execute(
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
            app.execute(
                owner.clone(),
                light_client_addr.clone(),
                &lc_msg::ExecuteMsg::UpdateHeaderConfig {
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
     -> MockResult<_> {
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

    let set_whitelist_validator =
        |app: &mut MockApp, val_addr: Addr, permission: bool| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::SetWhitelistValidator {
                    val_addr,
                    permission,
                },
                &[],
            )
        };

    let register_validator = |app: &mut MockApp, sender: Addr| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::RegisterValidator {},
            &[],
        )
    };

    let set_signatory_key = |app: &mut MockApp, sender: Addr, xpub: Xpub| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SetSignatoryKey {
                xpub: WrappedBinary(xpub),
            },
            &[],
        )
    };

    let increase_block = |app: &mut MockApp, hash: Binary| -> MockResult<_> {
        app.sudo(
            bitcoin_bridge_addr.clone(),
            &msg::SudoMsg::ClockEndBlock { hash },
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    // Start testing
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateConfig {
            relayer_fee_token: None,
            token_fee_receiver: Some(token_fee_receiver.clone()),
            relayer_fee_receiver: None,
            relayer_fee: None,
            swap_router_contract: None,
            token_fee: Some(Ratio {
                nominator: 1,
                denominator: 100,
            }),
            token_factory_contract: None,
            light_client_contract: None,
            owner: None,
            osor_entry_point_contract: None,
        },
        &[],
    )
    .unwrap();
    init_bitcoin_config(&mut app, 45);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1020);

    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
    ];

    set_whitelist_validator(&mut app, validator_1.clone(), true).unwrap();
    set_whitelist_validator(&mut app, validator_2.clone(), true).unwrap();
    register_validator(&mut app, validator_1.clone()).unwrap();
    register_validator(&mut app, validator_2.clone()).unwrap();
    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction
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
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1022);

    // this proof is for current depositing
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
        .query(
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    // Check fee receiver balance
    assert_eq!(balance.u128(), 118765157940000 as u128);
    let balance = app
        .query_balance(token_fee_receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.u128(), 1199648060000 as u128);
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
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
        light_client_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);
}

#[cfg(all(feature = "mainnet", feature = "native-validator"))]
#[tokio::test]
#[serial_test::serial]
async fn test_check_eligible_validator() {
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[3]);
    let token_fee_receiver = Addr::unchecked(&accounts[4]);
    let _ = app
        .inner()
        .setup_validator_with_secret(&coins(13_000_000_000, "orai"), "alice")
        .unwrap();
    let _ = app
        .inner()
        .setup_validator_with_secret(&coins(15_000_000_000, "orai"), "bob")
        .unwrap();

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();

    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
            },
        )
        .unwrap();

    let set_whitelist_validator =
        |app: &mut MockApp, val_addr: Addr, permission: bool| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::SetWhitelistValidator {
                    val_addr,
                    permission,
                },
                &[],
            )
        };

    set_whitelist_validator(&mut app, validator_1.clone(), true).unwrap();
    let is_eligible: bool = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckEligibleValidator {
                val_addr: validator_1.clone(),
            },
        )
        .unwrap();
    assert_eq!(is_eligible, true);

    let is_eligible: bool = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckEligibleValidator {
                val_addr: validator_2.clone(),
            },
        )
        .unwrap();
    assert_eq!(is_eligible, false);
}

#[cfg(all(feature = "mainnet", not(feature = "native-validator")))]
#[tokio::test]
#[serial_test::serial]
async fn test_deposit_with_token_fee() {
    // Set up app
    let threshold = SIGSET_THRESHOLD;
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[3]);
    let token_fee_receiver = Addr::unchecked(&accounts[4]);
    let receiver = Addr::unchecked(&accounts[5]);

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();
    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
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

    let register_denom =
        |app: &mut MockApp, subdenom: String, metadata: Option<Metadata>| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
                &coins(10_000_000, "orai"),
            )
        };

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
        app.execute(
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

        app.execute(
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
            app.execute(
                owner.clone(),
                light_client_addr.clone(),
                &lc_msg::ExecuteMsg::UpdateHeaderConfig {
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
     -> MockResult<_> {
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> MockResult<_> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::AddValidators {
                addrs,
                voting_powers,
                consensus_keys,
            },
            &[],
        )
    };

    let set_signatory_key = |app: &mut MockApp, sender: Addr, xpub: Xpub| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SetSignatoryKey {
                xpub: WrappedBinary(xpub),
            },
            &[],
        )
    };

    let increase_block = |app: &mut MockApp, hash: Binary| -> MockResult<_> {
        app.sudo(
            bitcoin_bridge_addr.clone(),
            &msg::SudoMsg::ClockEndBlock { hash },
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    // Start testing
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateConfig {
            relayer_fee_token: None,
            token_fee_receiver: Some(token_fee_receiver.clone()),
            relayer_fee_receiver: None,
            relayer_fee: None,
            swap_router_contract: None,
            token_fee: Some(Ratio {
                nominator: 1,
                denominator: 100,
            }),
            token_factory_contract: None,
            light_client_contract: None,
            owner: None,
            osor_entry_point_contract: None,
        },
        &[],
    )
    .unwrap();
    init_bitcoin_config(&mut app, 45);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1020);

    // Set up 2 validators here
    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32]];

    add_validators(
        &mut app,
        vec![
            validator_1.clone().to_string(),
            validator_2.clone().to_string(),
        ],
        vec![15, 10],
        vec![consensus_keys[0], consensus_keys[1]],
    )
    .unwrap();

    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction
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
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1022);

    // this proof is for current depositing
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
        .query(
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    // Check fee receiver balance
    assert_eq!(balance.u128(), 118765157940000 as u128);
    let balance = app
        .query_balance(token_fee_receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.u128(), 1199648060000 as u128);
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
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
        light_client_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);
}

#[cfg(all(feature = "mainnet", not(feature = "native-validator")))]
#[tokio::test]
#[serial_test::serial]
async fn test_withdraw_with_dynamic_fee() {
    // Set up app
    let threshold = SIGSET_THRESHOLD;
    let (mut app, accounts) = MockApp::new(&[
        ("perfogic", &coins(100_000_000_000, "orai")),
        ("alice", &coins(100_000_000_000, "orai")),
        ("bob", &coins(100_000_000_000, "orai")),
        ("relayer_fee_receiver", &coins(100_000_000_000, "orai")),
        ("token_fee_receiver", &coins(100_000_000_000, "orai")),
        ("receiver", &coins(100_000_000_000, "orai")),
    ]);
    let owner = Addr::unchecked(&accounts[0]);
    let validator_1 = Addr::unchecked(&accounts[1]);
    let validator_2 = Addr::unchecked(&accounts[2]);
    let relayer_fee_receiver = Addr::unchecked(&accounts[3]);
    let token_fee_receiver = Addr::unchecked(&accounts[4]);
    let receiver = Addr::unchecked(&accounts[5]);

    let token_factory_addr = app.create_tokenfactory(owner.clone()).unwrap();
    let light_client_addr = app
        .create_light_client(owner.clone(), &lc_msg::InstantiateMsg {})
        .unwrap();
    let btc_bridge_denom = format!(
        "factory/{}/{}",
        token_factory_addr.clone().to_string(),
        BTC_NATIVE_TOKEN_DENOM
    );
    let bitcoin_bridge_addr = app
        .create_bridge(
            owner.clone(),
            &msg::InstantiateMsg {
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: relayer_fee_receiver.clone(),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: token_fee_receiver.clone(),
                token_factory_contract: token_factory_addr.clone(),
                light_client_contract: light_client_addr.clone(),
                swap_router_contract: None,
                osor_entry_point_contract: None,
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

    let register_denom =
        |app: &mut MockApp, subdenom: String, metadata: Option<Metadata>| -> MockResult<_> {
            app.execute(
                owner.clone(),
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::RegisterDenom { subdenom, metadata },
                &coins(10_000_000, "orai"),
            )
        };

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
        app.execute(
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

        app.execute(
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
            app.execute(
                owner.clone(),
                light_client_addr.clone(),
                &lc_msg::ExecuteMsg::UpdateHeaderConfig {
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
     -> MockResult<_> {
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> MockResult<_> {
        app.execute(
            owner.clone(),
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::AddValidators {
                addrs,
                voting_powers,
                consensus_keys,
            },
            &[],
        )
    };

    let set_signatory_key = |app: &mut MockApp, sender: Addr, xpub: Xpub| -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SetSignatoryKey {
                xpub: WrappedBinary(xpub),
            },
            &[],
        )
    };

    let increase_block = |app: &mut MockApp, hash: Binary| -> MockResult<_> {
        app.sudo(
            bitcoin_bridge_addr.clone(),
            &msg::SudoMsg::ClockEndBlock { hash },
        )
    };

    let sign_cp = |app: &mut MockApp,
                   sender: Addr,
                   xpriv: &ExtendedPrivKey,
                   xpub: ExtendedPubKey,
                   cp_index: u32,
                   btc_height: u32|
     -> MockResult<_> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .query(
                bitcoin_bridge_addr.clone(),
                &msg::QueryMsg::SigningTxsAtCheckpointIndex {
                    xpub: WrappedBinary(Xpub::new(xpub)),
                    checkpoint_index: cp_index,
                },
            )
            .unwrap();
        let sigs = sign(&secp, &xpriv, &to_signs).unwrap();
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::SubmitCheckpointSignature {
                xpub: WrappedBinary(Xpub::new(xpub)),
                sigs,
                checkpoint_index: cp_index,
                btc_height,
            },
            &[],
        )
    };

    let withdraw_to_bitcoin = |app: &mut MockApp,
                               sender: Addr,
                               btc_address: Address,
                               coin: Coin,
                               fee: Option<u64>|
     -> MockResult<_> {
        app.execute(
            sender,
            bitcoin_bridge_addr.clone(),
            &msg::ExecuteMsg::WithdrawToBitcoin {
                btc_address: btc_address.to_string(),
                fee,
            },
            &[coin],
        )
    };

    // Start testing
    app.execute(
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        &msg::ExecuteMsg::UpdateConfig {
            relayer_fee_token: None,
            token_fee_receiver: Some(token_fee_receiver.clone()),
            relayer_fee_receiver: None,
            relayer_fee: None,
            swap_router_contract: None,
            token_fee: Some(Ratio {
                nominator: 1,
                denominator: 100,
            }),
            token_factory_contract: None,
            light_client_contract: None,
            owner: None,
            osor_entry_point_contract: None,
        },
        &[],
    )
    .unwrap();
    init_bitcoin_config(&mut app, 45);
    init_checkpoint_config(&mut app);
    init_headers(&mut app, 1000, trusted_header);
    register_denom(&mut app, BTC_NATIVE_TOKEN_DENOM.to_string(), None).unwrap();

    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1000);

    // Mine more 20 blocks
    mine_and_relay_headers(
        &btc_client,
        &mut app,
        &async_wallet_address,
        20,
        owner.clone(),
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1020);

    // Set up 2 validators here
    let network = bitcoin::Network::Bitcoin; // This is actually hard-coded
    let secp = Secp256k1::new();
    let xprivs = vec![
        ExtendedPrivKey::new_master(network, &[0]).unwrap(),
        ExtendedPrivKey::new_master(network, &[1]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32]];

    add_validators(
        &mut app,
        vec![
            validator_1.clone().to_string(),
            validator_2.clone().to_string(),
        ],
        vec![15, 10],
        vec![consensus_keys[0], consensus_keys[1]],
    )
    .unwrap();

    set_signatory_key(&mut app, validator_1.clone(), Xpub::new(xpubs[0])).unwrap();
    set_signatory_key(&mut app, validator_2.clone(), Xpub::new(xpubs[1])).unwrap();
    increase_block(&mut app, Binary::from([0; 32])).unwrap(); // should increase number of hash to be unique

    // Fetching checkpoint and creating deposit address
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction
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
        light_client_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1022);

    // this proof is for current depositing
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
        .query(
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
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 1);
    increase_block(&mut app, Binary::from([2; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Validate balance
    let balance = app
        .query_balance(receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    // Check fee receiver balance
    assert_eq!(balance.u128(), 118765157940000 as u128);
    let balance = app
        .query_balance(token_fee_receiver.clone(), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.u128(), 1199648060000 as u128);
    increase_block(&mut app, Binary::from([3; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
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
        light_client_addr.clone(),
        0,
    )
    .await;
    let header_height: u32 = app
        .query(
            light_client_addr.clone(),
            &lc_msg::QueryMsg::HeaderHeight {},
        )
        .unwrap();
    assert_eq!(header_height, 1023);
    let confirmed_cp_index: u32 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 0);

    // Only one withdraw transaction => pass checkpoint
    let deposit_fee: u64 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::DepositFees { index: None },
        )
        .unwrap();
    let withdraw_address = wallet.get_new_address(None, None).unwrap();
    withdraw_to_bitcoin(
        &mut app,
        receiver.clone(),
        withdraw_address.clone(),
        Coin {
            denom: btc_bridge_denom.clone(),
            amount: (bitcoin::Amount::from_btc(0.8).unwrap().to_sat() * 1000000).into(),
        },
        Some(deposit_fee),
    )
    .unwrap();
    increase_block(&mut app, Binary::from([4; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);
    sign_cp(&mut app, validator_1.clone(), &xprivs[0], xpubs[0], 1, 1023).unwrap();
    sign_cp(&mut app, validator_2.clone(), &xprivs[1], xpubs[1], 1, 1023).unwrap();
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        light_client_addr.clone(),
        1,
    )
    .await;
    let amount = wallet
        .get_received_by_address(&withdraw_address, Some(0))
        .unwrap();
    assert_eq!(
        amount.to_sat(),
        80_000_000 - (deposit_fee / 1_000_000) - 800_000
    );

    // Two withdraw transaction, one with slow mode and one with fast mode
    let deposit_fee: u64 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::DepositFees { index: None },
        )
        .unwrap();
    let withdraw_fee: u64 = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::WithdrawalFees {
                address: withdraw_address.to_string(),
                index: None,
            },
        )
        .unwrap();
    let withdraw_address_1 = wallet.get_new_address(None, None).unwrap();
    withdraw_to_bitcoin(
        &mut app,
        receiver.clone(),
        withdraw_address_1.clone(),
        Coin {
            denom: btc_bridge_denom.clone(),
            amount: (bitcoin::Amount::from_btc(0.1).unwrap().to_sat() * 1000000).into(),
        },
        None,
    )
    .unwrap();
    let withdraw_address_2 = wallet.get_new_address(None, None).unwrap();
    withdraw_to_bitcoin(
        &mut app,
        receiver.clone(),
        withdraw_address_2.clone(),
        Coin {
            denom: btc_bridge_denom.clone(),
            amount: (bitcoin::Amount::from_btc(0.1).unwrap().to_sat() * 1000000).into(),
        },
        Some(deposit_fee),
    )
    .unwrap();
    increase_block(&mut app, Binary::from([5; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Signing);
    sign_cp(&mut app, validator_1.clone(), &xprivs[0], xpubs[0], 2, 1023).unwrap();
    sign_cp(&mut app, validator_2.clone(), &xprivs[1], xpubs[1], 2, 1023).unwrap();
    let checkpoint: Checkpoint = app
        .query(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        light_client_addr.clone(),
        2,
    )
    .await;
    let amount = wallet
        .get_received_by_address(&withdraw_address_1, Some(0))
        .unwrap();
    assert_eq!(
        amount.to_sat(),
        10_000_000 - 100_000 - (withdraw_fee / 1_000_000)
    );
    let amount = wallet
        .get_received_by_address(&withdraw_address_2, Some(0))
        .unwrap();
    assert_eq!(
        amount.to_sat(),
        10_000_000 - (deposit_fee / 1_000_000) - 100_000
    );
}
