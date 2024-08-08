use cosmwasm_std::{testing::mock_dependencies, Storage};

use crate::{
    checkpoint::{adjust_fee_rate, BitcoinTx, Checkpoint, CheckpointQueue, CheckpointStatus},
    constants::DEFAULT_FEE_RATE,
    error::ContractResult,
    interface::CheckpointConfig,
    signatory::{Signatory, SignatorySet},
    state::{BUILDING_INDEX, CHECKPOINTS, CONFIRMED_INDEX},
    tests::helper::push_bitcoin_tx_output,
    threshold_sig::Pubkey,
};

#[test]
fn deduct_fee() {
    let mut bitcoin_tx = BitcoinTx::default();
    push_bitcoin_tx_output(&mut bitcoin_tx, 0);
    push_bitcoin_tx_output(&mut bitcoin_tx, 10000);

    bitcoin_tx.deduct_fee(100).unwrap();

    assert_eq!(bitcoin_tx.output.len(), 1);
    assert_eq!(bitcoin_tx.output.get(0).unwrap().value, 9900);
}

#[test]
fn deduct_fee_multi_pass() {
    let mut bitcoin_tx = BitcoinTx::default();
    push_bitcoin_tx_output(&mut bitcoin_tx, 502);
    push_bitcoin_tx_output(&mut bitcoin_tx, 482);
    push_bitcoin_tx_output(&mut bitcoin_tx, 300);

    bitcoin_tx.deduct_fee(30).unwrap();

    assert_eq!(bitcoin_tx.output.len(), 1);
    assert_eq!(bitcoin_tx.output.get(0).unwrap().value, 472);
}

#[test]
fn deduct_fee_multi_pass_empty_result() {
    let mut bitcoin_tx = BitcoinTx::default();
    push_bitcoin_tx_output(&mut bitcoin_tx, 60);
    push_bitcoin_tx_output(&mut bitcoin_tx, 70);
    push_bitcoin_tx_output(&mut bitcoin_tx, 100);

    bitcoin_tx.deduct_fee(200).unwrap();
}

//TODO: More fee deduction tests

fn create_queue_with_status(
    store: &mut dyn Storage,
    complete: u32,
    signing: bool,
) -> ContractResult<CheckpointQueue> {
    let mut checkpoint_queue = CheckpointQueue::default();
    checkpoint_queue.reset(store)?;

    let push = |store: &mut dyn Storage, status| {
        let cp = Checkpoint {
            status,
            fee_rate: DEFAULT_FEE_RATE,
            signed_at_btc_height: None,
            deposits_enabled: true,
            sigset: SignatorySet::default(),
            fees_collected: 0,
            pending: vec![],
            batches: vec![],
        };

        CHECKPOINTS.push_back(store, &cp).unwrap();
    };

    BUILDING_INDEX.save(store, &complete).unwrap();

    for _ in 0..complete {
        push(store, CheckpointStatus::Complete);
    }
    if signing {
        push(store, CheckpointStatus::Signing);
        BUILDING_INDEX.save(store, &(complete + 1))?;
    }
    push(store, CheckpointStatus::Building);

    Ok(checkpoint_queue)
}

#[test]
fn completed_with_signing() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    let cp = queue.completed(deps.as_mut().storage, 1).unwrap();
    assert_eq!(cp.len(), 1);
    assert_eq!(cp[0].status, CheckpointStatus::Complete);
}

#[test]
fn completed_without_signing() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let cp = queue.completed(deps.as_mut().storage, 1).unwrap();
    assert_eq!(cp.len(), 1);
    assert_eq!(cp[0].status, CheckpointStatus::Complete);
}

#[test]
fn completed_no_complete() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    let cp = queue.completed(deps.as_mut().storage, 10).unwrap();
    assert_eq!(cp.len(), 0);
}

#[test]
fn completed_zero_limit() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let cp = queue.completed(deps.as_mut().storage, 0).unwrap();
    assert_eq!(cp.len(), 0);
}

#[test]
fn completed_oversized_limit() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let cp = queue.completed(deps.as_mut().storage, 100).unwrap();
    assert_eq!(cp.len(), 10);
}

#[test]
fn completed_pruned() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let index = BUILDING_INDEX.load(deps.as_mut().storage).unwrap();
    let _ = BUILDING_INDEX
        .save(deps.as_mut().storage, &(index + 10))
        .unwrap();

    let cp = queue.completed(deps.as_mut().storage, 2).unwrap();
    assert_eq!(cp.len(), 2);
    assert_eq!(cp[1].status, CheckpointStatus::Complete);
}

#[test]
fn num_unconfirmed() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let _ = CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    let _ = CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let queue = create_queue_with_status(deps.as_mut().storage, 0, true).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);

    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);

    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 10);

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 10);
}

#[test]
fn first_unconfirmed_index() {
    let mut deps = mock_dependencies();
    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    let _ = CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(6)
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    let _ = CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(6)
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        None
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 0, true).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        None
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(0)
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(0)
    );
}

#[test]
fn test_adjust_fee_rate() {
    let config = CheckpointConfig::default();
    assert_eq!(adjust_fee_rate(100, true, &config), 125);
    assert_eq!(adjust_fee_rate(100, false, &config), 75);
    assert_eq!(adjust_fee_rate(2, true, &config), 40);
    assert_eq!(adjust_fee_rate(0, true, &config), 40);
    assert_eq!(adjust_fee_rate(2, false, &config), 40);
    assert_eq!(adjust_fee_rate(200, true, &config), 250);
    assert_eq!(adjust_fee_rate(300, true, &config), 375);
}

fn sigset(n: u32) -> SignatorySet {
    let mut sigset = SignatorySet::default();
    sigset.index = n;
    sigset.create_time = n as u64;

    let secret = bitcoin::secp256k1::SecretKey::from_slice(&[(n + 1) as u8; 32]).unwrap();
    let pubkey: Pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(
        &bitcoin::secp256k1::Secp256k1::new(),
        &secret,
    )
    .into();

    sigset.signatories.push(Signatory {
        pubkey: pubkey.into(),
        voting_power: 100,
    });

    sigset.possible_vp = 100;
    sigset.present_vp = 100;

    sigset
}
