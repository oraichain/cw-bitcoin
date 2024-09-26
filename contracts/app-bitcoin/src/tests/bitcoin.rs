use super::helper::sign;
use crate::app::Bitcoin;
use crate::checkpoint::{BatchType, Input};
use crate::constants::BTC_NATIVE_TOKEN_DENOM;
use crate::interface::{BitcoinConfig, CheckpointConfig, Dest};
use crate::msg::Config;
use crate::state::{
    BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, CONFIG, CONFIRMED_INDEX, FEE_POOL,
    FIRST_UNHANDLED_CONFIRMED_INDEX, SIGNERS, VALIDATORS,
};
use crate::tests::helper::set_time;
use bitcoin::hashes::Hash;
use bitcoin::util::bip32::ExtendedPubKey;
use bitcoin::Script;
use bitcoin::{secp256k1::Secp256k1, util::bip32::ExtendedPrivKey, OutPoint, Txid};
use common_bitcoin::adapter::Adapter;
use common_bitcoin::error::ContractResult;
use common_bitcoin::xpub::Xpub;
use cosmwasm_std::testing::{mock_dependencies, MockApi, MockQuerier};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Api, Coin, DepsMut, Empty, Env, QuerierResult, QuerierWrapper,
    Storage, SystemError, SystemResult, Uint128, WasmQuery,
};
use light_client_bitcoin::msg::QueryMsg::{HeaderHeight, Network};
use oraiswap::asset::AssetInfo;
use std::cell::RefCell;

use crate::interface::IbcDest;

fn handle_wasm_query(height: u32) -> Box<dyn Fn(&WasmQuery) -> QuerierResult> {
    // Return a boxed closure that captures the `height` variable
    Box::new(move |wasm_query: &WasmQuery| -> QuerierResult {
        match wasm_query {
            WasmQuery::Smart {
                contract_addr: _,
                msg,
            } => {
                let query_msg = from_json::<light_client_bitcoin::msg::QueryMsg>(msg).unwrap();
                match query_msg {
                    Network {} => SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                        to_json_binary(&(bitcoin::Network::Bitcoin).to_string()).unwrap(),
                    )),
                    HeaderHeight {} => SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                        to_json_binary(&height).unwrap(),
                    )),
                    _ => SystemResult::Err(SystemError::UnsupportedRequest {
                        kind: "QueryMsg".to_string(),
                    }),
                }
            }
            _ => unreachable!(),
        }
    })
}

#[test]
fn check_change_rates() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    // mock querier
    let mut mock_query = MockQuerier::<Empty>::new(&[]);
    mock_query.update_wasm(handle_wasm_query(0));
    let mock_querier = QuerierWrapper::new(&mock_query);
    // let block height
    let mut block_height = 10;

    let bitcoin_config = BitcoinConfig::default();
    CONFIG.save(
        deps.as_mut().storage,
        &Config {
            owner: Addr::unchecked("owner"),
            relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
            token_fee_receiver: Addr::unchecked("token_fee_receiver"),
            relayer_fee_token: AssetInfo::NativeToken {
                denom: "orai".to_string(),
            },
            relayer_fee: Uint128::from(0u128),
            token_factory_contract: Addr::unchecked("token_factory_contract"),
            light_client_contract: Addr::unchecked("light_client_contract"),
            swap_router_contract: None,
            osor_entry_point_contract: None,
        },
    )?;
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
    let network = bitcoin::Network::Bitcoin;
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
            None,
        )?;

        let mut building_mut = btc.checkpoints.building(store)?;
        building_mut.fees_collected = 100_000_000;
        let index = btc.checkpoints.index(store);
        btc.checkpoints.set(store, index, &building_mut)?;
        Ok(())
    };

    let sign_batch = |api: &dyn Api, store: &mut dyn Storage, btc_height| -> ContractResult<()> {
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
            btc.checkpoints.sign(
                api,
                store,
                &Xpub::new(xpub[i]),
                sigs,
                sigset_index,
                btc_height,
            )?;
        }
        Ok(())
    };
    let sign_cp = |deps: DepsMut, btc_height| -> ContractResult<()> {
        if btc
            .borrow()
            .checkpoints
            .signing(deps.storage)
            .unwrap()
            .is_some()
        {
            sign_batch(deps.api, deps.storage, btc_height)?;
        }
        Ok(())
    };
    let maybe_step =
        |env: Env, store: &mut dyn Storage, block_height: &mut u32| -> ContractResult<()> {
            let mut mock_query = MockQuerier::<Empty>::new(&[]);
            mock_query.update_wasm(handle_wasm_query(block_height.clone()));
            let mock_querier = QuerierWrapper::new(&mock_query);
            *block_height += 1;
            let mut btc = btc.borrow_mut();
            btc.begin_block_step(&env, &mock_querier, store, vec![1, 2, 3])?;
            Ok(())
        };

    let env = set_time(0);
    for i in 0..2 {
        btc.borrow_mut().set_signatory_key(
            &mock_querier,
            deps.as_mut().storage,
            Addr::unchecked(addr[i]),
            Xpub::new(xpub[i]),
        )?;
    }

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 0);
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 1);

    let env = set_time(1000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    sign_cp(deps.as_mut(), 10)?;
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 2);

    let env = set_time(2000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 2000, 2100)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);
    sign_cp(deps.as_mut(), 10)?;

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 3);

    // Change the sigset
    VALIDATORS.save(
        deps.as_mut().storage,
        &consensus_key2,
        &(100, addr[1].to_string()),
    )?;

    let env = set_time(3000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 3100)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);
    sign_cp(deps.as_mut(), 10)?;

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 4);

    let env = set_time(4000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 4100)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 4090);
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 5);
    sign_cp(deps.as_mut(), 10)?;

    let env = set_time(5000);
    push_deposit(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 5100)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 4090);
    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 6);
    sign_cp(deps.as_mut(), 10)?;

    let env = set_time(6000);
    push_withdrawal(deps.as_mut().storage)?;
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    let change_rates = btc
        .borrow()
        .change_rates(deps.as_mut().storage, 3000, 5100)?;
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
    let change_rates = btc.borrow().change_rates(deps.as_mut().storage, 0, 5100)?;
    assert_eq!(change_rates.withdrawal, 0);
    assert_eq!(change_rates.sigset_change, 0);

    Ok(())
}

#[test]
fn test_take_pending() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    let mut block_height = 10;
    // mock querier
    let mut mock_query = MockQuerier::<Empty>::new(&[]);
    mock_query.update_wasm(handle_wasm_query(block_height));
    let mock_querier = QuerierWrapper::new(&mock_query);

    let bitcoin_config = BitcoinConfig::default();
    CONFIG.save(
        deps.as_mut().storage,
        &Config {
            owner: Addr::unchecked("owner"),
            relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
            token_fee_receiver: Addr::unchecked("token_fee_receiver"),
            relayer_fee_token: AssetInfo::NativeToken {
                denom: "orai".to_string(),
            },
            relayer_fee: Uint128::from(0u128),
            token_factory_contract: Addr::unchecked("token_factory_contract"),
            light_client_contract: Addr::unchecked("light_client_contract"),
            swap_router_contract: None,
            osor_entry_point_contract: None,
        },
    )?;
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
    let network = bitcoin::Network::Bitcoin;
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

    let sign_batch = |api: &dyn Api, store: &mut dyn Storage, btc_height| -> ContractResult<()> {
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
            queue.sign(
                api,
                store,
                &Xpub::new(xpub[i]),
                sigs,
                sigset_index,
                btc_height,
            )?;
        }

        Ok(())
    };
    let sign_cp = |deps: DepsMut, btc_height| -> ContractResult<()> {
        if btc.borrow().checkpoints.signing(deps.storage)?.is_some() {
            sign_batch(deps.api, deps.storage, btc_height)?;
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
        let pending = btc.take_pending_completed(store)?;
        Ok(pending)
    };

    let maybe_step =
        |env: Env, store: &mut dyn Storage, block_height: &mut u32| -> ContractResult<()> {
            let mut mock_query = MockQuerier::<Empty>::new(&[]);
            mock_query.update_wasm(handle_wasm_query(block_height.clone()));
            *block_height += 1;
            let mut btc = btc.borrow_mut();
            btc.begin_block_step(&env, &mock_querier, store, vec![1, 2, 3])?;

            Ok(())
        };

    let env = set_time(0);
    for i in 0..2 {
        btc.borrow_mut().set_signatory_key(
            &mock_querier,
            deps.as_mut().storage,
            Addr::unchecked(addr[i]),
            Xpub::new(xpub[i]),
        )?;
    }

    assert_eq!(btc.borrow().checkpoints.len(deps.as_ref().storage)?, 0);
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
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
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;

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
    sign_cp(deps.as_mut(), 10)?;
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
    maybe_step(env, deps.as_mut().storage, &mut block_height)?;
    sign_cp(deps.as_mut(), 10)?;
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
    assert_eq!(cp_dests.len(), 3);
    assert_eq!(cp_dests[0].len(), 2); // cp_dest confirmed
    assert_eq!(cp_dests[1].len(), 1); // cp_dest confirmed
    assert_eq!(cp_dests[2].len(), 0); // cp_dest completed
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
