use std::str::FromStr;
use std::time::Duration;

use super::utils::{
    get_wrapped_header_from_block_hash, populate_bitcoin_block, retry, test_bitcoin_client,
};
use crate::adapter::{Adapter, WrappedBinary};
use crate::checkpoint::{Checkpoint, CheckpointStatus};
use crate::constants::{BTC_NATIVE_TOKEN_DENOM, SIGSET_THRESHOLD};
use crate::header::WrappedHeader;
use crate::interface::{BitcoinConfig, CheckpointConfig, Dest, Xpub};
use crate::recovery::SignedRecoveryTx;
use crate::state::Ratio;
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
use cosmwasm_std::{Addr, Binary, Coin, Uint128};
use cosmwasm_testing_util::AppResponse;
use oraiswap::asset::AssetInfo;
use token_bindings::Metadata;
use tokio::time::sleep;

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

async fn relay_recovery(
    btc_client: &Client,
    app: &mut MockApp,
    wallet: &Address,
    sender: Addr,
    bitcoin_bridge_addr: Addr,
) -> () {
    let recovery_txs: Vec<SignedRecoveryTx> = app
        .as_querier()
        .query_wasm_smart(
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
            bitcoin_bridge_addr.clone(),
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
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                swap_router_contract: None,
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

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> Result<AppResponse, _> {
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
                    xpub: WrappedBinary(xpub),
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
     -> Result<AppResponse, _> {
        let secp = Secp256k1::signing_only();
        let to_signs: Vec<([u8; 32], u32)> = app
            .as_querier()
            .query_wasm_smart(
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
    init_bitcoin_config(&mut app, 45);
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
        ExtendedPrivKey::new_master(network, &[3]).unwrap(),
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
        ExtendedPubKey::from_priv(&secp, &xprivs[2]),
        ExtendedPubKey::from_priv(&secp, &xprivs[3]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32], [2; 32], [3; 32]];
    let validator_1 = Addr::unchecked("orai1Alice");
    let validator_2 = Addr::unchecked("orai1Bob");
    let validator_3 = Addr::unchecked("orai1Dave");
    let validator_4 = Addr::unchecked("orai1Jayce");
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
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction and try to submit tx with proof when not enough confirmations
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
        bitcoin_bridge_addr.clone(),
    )
    .await;
    let header_height: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
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
    assert_eq!(balance.amount.u128(), 119953074000000 as u128);
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

    // Make sure checkpoint one have 4 validators
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
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
    assert_eq!(balance.amount.u128(), 189894417000000 as u128);

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

    // [TESTCASE] test recovery
    println!("Waiting 10 seconds to make the deposit expired!");
    sleep(Duration::from_secs(10)).await;
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
    )
    .await;
    increase_block(&mut app, Binary::from([6; 32])).unwrap(); // should increase number of hash to be unique
    let current_header: u32 = app
        .as_querier()
        .query_wasm_smart(bitcoin_bridge_addr.clone(), &msg::QueryMsg::HeaderHeight {})
        .unwrap();
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
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
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.status, CheckpointStatus::Complete);
    assert_eq!(checkpoint.pending.len(), 0);

    // Check balance
    let balance = app
        .as_querier()
        .query_balance(&receiver, btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.amount.u128(), 309846837000000 as u128);

    relay_checkpoint(
        &btc_client,
        &mut app,
        &async_wallet_address,
        owner.clone(),
        bitcoin_bridge_addr.clone(),
        2,
    )
    .await;

    let confirmed_cp_index: u32 = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::ConfirmedIndex {},
        )
        .unwrap();
    assert_eq!(confirmed_cp_index, 2);

    // [TESTCASE] check validator 4 is punished, validate the changing in signatures length
    increase_block(&mut app, Binary::from([8; 32])).unwrap(); // should increase number of hash to be unique
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 3);
    assert_eq!(checkpoint.sigset.present_vp, 26);

    // Here validator 3 is added
    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 1 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 4);
    assert_eq!(checkpoint.sigset.present_vp, 51);

    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 2 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 4);
    assert_eq!(checkpoint.sigset.present_vp, 51);

    let checkpoint: Checkpoint = app
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 3 },
        )
        .unwrap();
    assert_eq!(checkpoint.sigset.signatories.len(), 3);
    assert_eq!(checkpoint.sigset.present_vp, 50);
    println!("[BRAVOOO] All testcases passed!");
}

#[tokio::test]
async fn test_deposit_with_token_fee() {
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
                relayer_fee: Uint128::from(0 as u16),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                relayer_fee_token: AssetInfo::NativeToken {
                    denom: "orai".to_string(),
                },
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                swap_router_contract: None,
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

    let init_bitcoin_config = |app: &mut MockApp, max_deposit_age: u32| -> () {
        let mut bitcoin_config = BitcoinConfig::default();
        bitcoin_config.min_withdrawal_checkpoints = 1;
        bitcoin_config.max_deposit_age = max_deposit_age as u64;
        bitcoin_config.max_offline_checkpoints = 1;
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
                          voting_powers: Vec<u64>,
                          consensus_keys: Vec<[u8; 32]>|
     -> Result<AppResponse, _> {
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

    let set_signatory_key =
        |app: &mut MockApp, sender: Addr, xpub: Xpub| -> Result<AppResponse, _> {
            app.execute(
                sender,
                bitcoin_bridge_addr.clone(),
                &msg::ExecuteMsg::SetSignatoryKey {
                    xpub: WrappedBinary(xpub),
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
            token_fee_receiver: Some(Addr::unchecked("fee_receiver")),
            relayer_fee_receiver: None,
            relayer_fee: None,
            swap_router_contract: None,
            token_fee: Some(Ratio {
                nominator: 1,
                denominator: 100,
            }),
            token_factory_addr: None,
            owner: None,
        },
        &[],
    )
    .unwrap();
    init_bitcoin_config(&mut app, 45);
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
    ];
    let xpubs = vec![
        ExtendedPubKey::from_priv(&secp, &xprivs[0]),
        ExtendedPubKey::from_priv(&secp, &xprivs[1]),
    ];
    let consensus_keys = vec![[0; 32], [1; 32]];
    let validator_1 = Addr::unchecked("orai1Alice");
    let validator_2 = Addr::unchecked("orai1Bob");
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
        .as_querier()
        .query_wasm_smart(
            bitcoin_bridge_addr.clone(),
            &msg::QueryMsg::CheckpointByIndex { index: 0 },
        )
        .unwrap();
    assert_eq!(checkpoint.pending.len(), 0);
    assert_eq!(checkpoint.status, CheckpointStatus::Building);
    let sigset = checkpoint.sigset;

    // [TESTCASE] Bridge one transaction
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
    // Check fee receiver balance
    assert_eq!(balance.amount.u128(), 118765157940000 as u128);
    let balance = app
        .as_querier()
        .query_balance(&Addr::unchecked("fee_receiver"), btc_bridge_denom.clone())
        .unwrap();
    assert_eq!(balance.amount.u128(), 1199648060000 as u128);
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
}
