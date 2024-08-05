use super::helper::sign;
use adapter::Adapter;
use app::Bitcoin;
use bitcoin::hashes::Hash;
use bitcoin::util::bip32::ExtendedPubKey;
use bitcoin::util::merkleblock::PartialMerkleTree;
use bitcoin::util::uint::{self};
use bitcoin::{
    secp256k1::Secp256k1, util::bip32::ExtendedPrivKey, BlockHash, BlockHeader, OutPoint,
    TxMerkleNode, Txid,
};
use bitcoin::{Script, Transaction};
use checkpoint::{BatchType, Input};
use constants::BTC_NATIVE_TOKEN_DENOM;
use cosmwasm_std::testing::{mock_dependencies, mock_env};
use cosmwasm_std::{Addr, Coin, Env, Storage, Uint128};
use error::ContractResult;
use interface::{BitcoinConfig, CheckpointConfig, Dest, HeaderConfig, Xpub};
use state::{
    BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, CONFIRMED_INDEX, FEE_POOL,
    FIRST_UNHANDLED_CONFIRMED_INDEX, HEADERS, HEADER_CONFIG, SIGNERS, VALIDATORS,
};
use std::cell::RefCell;
use tests::helper::set_time;

use crate::interface::IbcDest;

use crate::{
    header::{WorkHeader, WrappedHeader},
    *,
};

#[test]
fn relay_height_validity() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    let header_config = HeaderConfig::mainnet()?;
    let header = header_config.work_header();

    HEADER_CONFIG.save(deps.as_mut().storage, &header_config)?;
    HEADERS.push_back(deps.as_mut().storage, &header)?;

    let mut btc = Bitcoin::default();
    BITCOIN_CONFIG.save(deps.as_mut().storage, &btc.config)?;

    for _ in 0..10 {
        let btc_height = btc.headers.height(deps.as_ref().storage)?;
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

        HEADERS.push_back(deps.as_mut().storage, &header)?;
    }

    let h = btc.headers.height(deps.as_ref().storage)?;

    let mut try_relay = |height| {
        // TODO: make test cases not fail at irrelevant steps in relay_deposit
        // (either by passing in valid input, or by handling other error paths)

        let btc_tx = Transaction {
            input: vec![],
            lock_time: bitcoin::PackedLockTime(0),
            output: vec![],
            version: 0,
        };
        let btc_proof = PartialMerkleTree::from_txids(&[Txid::all_zeros()], &[true]);

        let env = mock_env();

        btc.relay_deposit(
            env,
            deps.as_mut().storage,
            Adapter::new(btc_tx),
            height,
            Adapter::new(btc_proof),
            0,
            0,
            Dest::Address(Addr::unchecked("")),
        )
    };

    assert_eq!(
        try_relay(h + 100).unwrap_err().to_string(),
        "App Error: Invalid bitcoin block height",
    );
    assert_eq!(
            try_relay(h - 100).unwrap_err().to_string(),
            "Passed index is greater than initial height. Referenced header does not exist on the Header Queue",
        );

    Ok(())
}

#[serial_test::serial]
#[test]
fn check_change_rates() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    let header_config = HeaderConfig::mainnet()?;
    HEADER_CONFIG.save(deps.as_mut().storage, &header_config)?;
    HEADERS.push_back(deps.as_mut().storage, &header_config.work_header())?;

    let bitcoin_config = BitcoinConfig::default();
    BITCOIN_CONFIG.save(deps.as_mut().storage, &bitcoin_config)?;
    FEE_POOL.save(deps.as_mut().storage, &0)?;
    CHECKPOINT_CONFIG.save(deps.as_mut().storage, &CheckpointConfig::default())?;

    BUILDING_INDEX.save(deps.as_mut().storage, &0)?;

    let consensus_key1 = [0; 32];
    let consensus_key2 = [1; 32];

    let addr = ["validator1", "validator2"];

    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key1,
        &(100, addr[0].to_string()),
    )?;
    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key2,
        &(10, addr[1].to_string()),
    )?;

    SIGNERS.save(deps.as_mut().storage, addr[0], &consensus_key1)?;
    SIGNERS.save(deps.as_mut().storage, addr[1], &consensus_key2)?;

    let btc = RefCell::new(Bitcoin::default());
    let secp = Secp256k1::new();
    let network = btc.borrow().network();
    let xpriv = vec![
        ExtendedPrivKey::new_master(network, &[0])?,
        ExtendedPrivKey::new_master(network, &[1])?,
    ];
    let xpub = vec![
        ExtendedPubKey::from_priv(&secp, &xpriv[0]),
        ExtendedPubKey::from_priv(&secp, &xpriv[1]),
    ];

    let push_deposit = |store: &mut dyn Storage| -> ContractResult<()> {
        let btc = btc.borrow();
        let sigset = &btc.checkpoints.building(store)?.sigset;
        let input = Input::new(
            OutPoint {
                txid: Txid::from_slice(&[0; 32])?,
                vout: 0,
            },
            sigset,
            &[0u8],
            100_000_000,
            (9, 10),
        )?;

        let mut building_mut = btc.checkpoints.building(store)?;
        building_mut.fees_collected = 100_000_000;
        let building_checkpoint_batch = &mut building_mut.batches[BatchType::Checkpoint];
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.input.push(input);
        let index = btc.checkpoints.index(store);
        btc.checkpoints.set(store, index, &building_mut)?;
        Ok(())
    };

    let push_withdrawal = |store: &mut dyn Storage| -> ContractResult<()> {
        let mut btc = btc.borrow_mut();
        btc.add_withdrawal(
            store,
            Adapter::new(Script::new()),
            459_459_927_000_000u128.into(),
        )?;

        let mut building_mut = btc.checkpoints.building(store)?;
        building_mut.fees_collected = 100_000_000;
        let index = btc.checkpoints.index(store);
        btc.checkpoints.set(store, index, &building_mut)?;
        Ok(())
    };

    let sign_batch = |store: &mut dyn Storage, btc_height| -> ContractResult<()> {
        let mut btc = btc.borrow_mut();
        let cp = btc.checkpoints.signing(store)?.unwrap();
        let sigset_index = cp.sigset.index;
        for i in 0..2 {
            let Some(cp) = btc.checkpoints.signing(store)? else {
                break;
            };
            let to_sign = cp.to_sign(&Xpub::new(xpub[i]))?;
            let secp2 = Secp256k1::signing_only();
            let sigs = sign(&secp2, &xpriv[i], &to_sign)?;
            btc.checkpoints
                .sign(store, &Xpub::new(xpub[i]), sigs, sigset_index, btc_height)?;
        }
        Ok(())
    };
    let sign_cp = |store: &mut dyn Storage, btc_height| -> ContractResult<()> {
        sign_batch(store, btc_height)?;
        sign_batch(store, btc_height)?;
        if btc.borrow().checkpoints.signing(store).unwrap().is_some() {
            sign_batch(store, btc_height)?;
        }
        Ok(())
    };
    let maybe_step = |env: Env, store: &mut dyn Storage| -> ContractResult<()> {
        let mut btc = btc.borrow_mut();
        btc.begin_block_step(env, store, vec![1, 2, 3])?;
        Ok(())
    };

    let env = set_time(0);
    for i in 0..2 {
        btc.borrow_mut().set_signatory_key(
            deps.as_mut().storage,
            Addr::unchecked(addr[i]),
            Xpub::new(xpub[i]),
        )?;
    }

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 0);
    maybe_step(env, deps.as_mut().storage)?;
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 1);

    let env = set_time(1000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    sign_cp(deps.as_mut().storage, 10)?;

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 2);

    let env = set_time(2000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 2000, 2100, 0)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);
    sign_cp(deps.as_mut().storage, 10)?;

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 3);

    // Change the sigset
    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key2,
        &(100, addr[1].to_string()),
    )?;

    let env = set_time(3000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 3100, 0)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);
    sign_cp(deps.as_mut().storage, 10)?;

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 4);

    let env = set_time(4000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 4100, 0)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 4090);
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 5);

    sign_cp(deps.as_mut().storage, 10)?;

    let env = set_time(5000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 5100, 0)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 4090);
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 6);
    sign_cp(deps.as_mut().storage, 10)?;

    let env = set_time(6000);
    push_withdrawal(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 5100, 0)?;
    assert_eq!(change_rates.withdrawal, 8664);
    assert_eq!(change_rates.sigset_change, 4090);
    assert_eq!(
        btc.borrow()
            .checkpoints
            .signing(deps.as_ref().storage)?
            .unwrap()
            .sigset
            .index,
        5
    );
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 5100, 5)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);

    Ok(())
}

#[serial_test::serial]
#[test]
fn test_take_pending() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    let header_config = HeaderConfig::mainnet()?;
    HEADER_CONFIG.save(deps.as_mut().storage, &header_config)?;
    HEADERS.push_back(deps.as_mut().storage, &header_config.work_header())?;

    let bitcoin_config = BitcoinConfig::default();
    BITCOIN_CONFIG.save(deps.as_mut().storage, &bitcoin_config)?;
    FEE_POOL.save(deps.as_mut().storage, &0)?;
    CHECKPOINT_CONFIG.save(deps.as_mut().storage, &CheckpointConfig::default())?;
    FIRST_UNHANDLED_CONFIRMED_INDEX.save(deps.as_mut().storage, &0)?;

    BUILDING_INDEX.save(deps.as_mut().storage, &0)?;

    let consensus_key1 = [0; 32];
    let consensus_key2 = [1; 32];

    let addr = ["validator1", "validator2"];

    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key1,
        &(100, addr[0].to_string()),
    )?;
    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key2,
        &(10, addr[1].to_string()),
    )?;

    SIGNERS.save(deps.as_mut().storage, addr[0], &consensus_key1)?;
    SIGNERS.save(deps.as_mut().storage, addr[1], &consensus_key2)?;

    let btc = RefCell::new(Bitcoin::default());
    let secp = Secp256k1::new();
    let network = btc.borrow().network();
    let xpriv = vec![
        ExtendedPrivKey::new_master(network, &[0])?,
        ExtendedPrivKey::new_master(network, &[1])?,
    ];
    let xpub = vec![
        ExtendedPubKey::from_priv(&secp, &xpriv[0]),
        ExtendedPubKey::from_priv(&secp, &xpriv[1]),
    ];

    let push_deposit = |store: &mut dyn Storage, dest: Dest, coin: Coin| -> ContractResult<()> {
        let fixed_amount: u64 = 100_000_000;
        assert_eq!(coin.amount.le(&Uint128::new(fixed_amount.into())), true);
        let input = Input::new(
            OutPoint {
                txid: Txid::from_slice(&[0; 32])?,
                vout: 0,
            },
            &btc.borrow().checkpoints.building(store)?.sigset,
            &[0u8],
            fixed_amount,
            (9, 10),
        )?;
        let btc = btc.borrow_mut();
        let mut building_mut = btc.checkpoints.building(store)?;
        building_mut.fees_collected += 100_000_000u64 - (coin.amount.u128() as u64);
        building_mut.pending.push((dest, coin));
        let building_checkpoint_batch = &mut building_mut.batches[BatchType::Checkpoint];
        let checkpoint_tx = building_checkpoint_batch.get_mut(0).unwrap();
        checkpoint_tx.input.push(input);
        let index = btc.checkpoints.index(store);
        btc.checkpoints.set(store, index, &building_mut)?;
        Ok(())
    };

    let sign_batch = |store: &mut dyn Storage, btc_height| -> ContractResult<()> {
        let mut btc = btc.borrow_mut();
        let queue = &mut btc.checkpoints;
        let cp = queue.signing(store)?.unwrap();
        let sigset_index = cp.sigset.index;
        for i in 0..2 {
            let Some(cp) = queue.signing(store)? else {
                break;
            };

            let to_sign = cp.to_sign(&Xpub::new(xpub[i]))?;
            let secp2 = Secp256k1::signing_only();
            let sigs = sign(&secp2, &xpriv[i], &to_sign)?;
            queue.sign(store, &Xpub::new(xpub[i]), sigs, sigset_index, btc_height)?;
        }

        Ok(())
    };
    let sign_cp = |store: &mut dyn Storage, btc_height| -> ContractResult<()> {
        sign_batch(store, btc_height)?;
        sign_batch(store, btc_height)?;
        if btc.borrow().checkpoints.signing(store)?.is_some() {
            sign_batch(store, btc_height)?;
        }

        Ok(())
    };

    let confirm_cp = |store: &mut dyn Storage, confirmed_index: u32| {
        let btc = btc.borrow_mut();
        let current_checkpoint_index = btc.checkpoints.index(store);
        assert_eq!(current_checkpoint_index, confirmed_index + 1);
        let confirmed_checkpoint_index = btc.checkpoints.last_completed_index(store).unwrap();
        assert_eq!(confirmed_checkpoint_index, confirmed_index);
        CONFIRMED_INDEX.save(store, &confirmed_index).unwrap();
    };

    let take_pending = |store: &mut dyn Storage| -> ContractResult<_> {
        let mut btc = btc.borrow_mut();
        let pending = btc.take_pending(store)?;
        Ok(pending)
    };

    let maybe_step = |env: Env, store: &mut dyn Storage| -> ContractResult<()> {
        let mut btc = btc.borrow_mut();

        btc.begin_block_step(env, store, vec![1, 2, 3])?;

        Ok(())
    };

    let env = set_time(0);
    for i in 0..2 {
        btc.borrow_mut().set_signatory_key(
            deps.as_mut().storage,
            Addr::unchecked(addr[i]),
            Xpub::new(xpub[i]),
        )?;
    }

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 0);
    maybe_step(env, deps.as_mut().storage)?;
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 1);
    let env = set_time(1000);

    let mut dest = IbcDest {
        source_port: "transfer".to_string(),
        source_channel: "channel-0".to_string(),
        sender: "sender1".to_string(),
        receiver: "receiver".to_string(),
        timeout_timestamp: 10u64,
        memo: "".to_string(),
    };

    // initially, there should not be any confirmed checkpoints -> return empty array for pending dests
    assert_eq!(take_pending(deps.as_mut().storage)?.len(), 0);
    // fixture: create 2 confirmed checkpoints having deposits so we can validate later
    push_deposit(
        deps.as_mut().storage,
        Dest::Ibc(dest.clone()),
        Coin {
            denom: BTC_NATIVE_TOKEN_DENOM.to_string(),
            amount: 95_000_000u128.into(),
        },
    )?;
    dest.sender = "sender2".to_string();
    push_deposit(
        deps.as_mut().storage,
        Dest::Ibc(dest.clone()),
        Coin {
            denom: BTC_NATIVE_TOKEN_DENOM.to_string(),
            amount: 95_000_000u128.into(),
        },
    )?;
    maybe_step(env, deps.as_mut().storage)?;

    // validate current checkpoint is on signing state
    let checkpoint_signing = btc.borrow().checkpoints.signing(deps.as_ref().storage)?;

    match checkpoint_signing {
        Some(checkpoint_data) => {
            println!("Checkpoint Data: {:?}", checkpoint_data.fees_collected);
            println!("{:?}", checkpoint_data.pending);
            assert_eq!(checkpoint_data.fees_collected > 0, true);
        }
        None => {
            panic!("Checkpoint not found");
        }
    }
    sign_cp(deps.as_mut().storage, 10)?;
    confirm_cp(deps.as_mut().storage, 0);

    let env = set_time(2000);
    push_deposit(
        deps.as_mut().storage,
        Dest::Ibc(dest.clone()),
        Coin {
            denom: BTC_NATIVE_TOKEN_DENOM.to_string(),
            amount: 98_000_000u128.into(),
        },
    )?;
    maybe_step(env, deps.as_mut().storage)?;
    sign_cp(deps.as_mut().storage, 10)?;
    confirm_cp(deps.as_mut().storage, 1);

    let first_unhandled_confirmed_cp_index = FIRST_UNHANDLED_CONFIRMED_INDEX
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(first_unhandled_confirmed_cp_index, 0);

    let confirmed_index = CONFIRMED_INDEX.load(deps.as_ref().storage)?;
    assert_eq!(confirmed_index, 1);
    // before take pending, the confirmed checkpoints should have some pending deposits
    assert_eq!(
        btc.borrow()
            .checkpoints
            .get(deps.as_ref().storage, 0)?
            .pending
            .iter()
            .count(),
        2
    );
    assert_eq!(
        btc.borrow()
            .checkpoints
            .get(deps.as_ref().storage, 1)?
            .pending
            .iter()
            .count(),
        1
    );

    // action. After take pending, the unhandled confirmed index should increase to 2 since we handled 2 confirmed checkpoints
    let cp_dests = take_pending(deps.as_mut().storage)?;

    let first_unhandled_confirmed_cp_index =
        FIRST_UNHANDLED_CONFIRMED_INDEX.load(deps.as_ref().storage)?;
    assert_eq!(first_unhandled_confirmed_cp_index, 2);
    assert_eq!(cp_dests.len(), 2);
    assert_eq!(cp_dests[0].len(), 2);
    assert_eq!(cp_dests[1].len(), 1);
    assert_eq!(
        cp_dests[0][0].0,
        Dest::Ibc(IbcDest {
            sender: "sender1".to_string(),
            ..dest.clone()
        })
    );
    assert_eq!(cp_dests[0][0].1.amount.u128(), 95_000_000u128);

    assert_eq!(
        cp_dests[0][1].0,
        Dest::Ibc(IbcDest {
            sender: "sender2".to_string(),
            ..dest.clone()
        })
    );
    assert_eq!(cp_dests[0][1].1.amount.u128(), 95_000_000u128);

    assert_eq!(
        cp_dests[1][0].0,
        Dest::Ibc(IbcDest {
            sender: "sender2".to_string(),
            ..dest.clone()
        })
    );
    assert_eq!(cp_dests[1][0].1.amount.u128(), 98_000_000u128);

    // // assert confirmed checkpoints pending. Should not have anything because we have removed them already in take_pending()
    let checkpoints = &btc.borrow().checkpoints;
    let first_cp = checkpoints.get(deps.as_ref().storage, 0).unwrap();
    assert_eq!(first_cp.pending.iter().count(), 0);
    let second_cp = checkpoints.get(deps.as_ref().storage, 1).unwrap();
    assert_eq!(second_cp.pending.iter().count(), 0);
    Ok(())
}
