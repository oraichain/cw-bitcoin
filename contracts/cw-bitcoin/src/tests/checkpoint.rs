use cosmwasm_std::{testing::mock_dependencies, Storage};

use crate::{
    checkpoint::{adjust_fee_rate, BitcoinTx, Checkpoint, CheckpointQueue, CheckpointStatus},
    constants::DEFAULT_FEE_RATE,
    error::ContractResult,
    interface::CheckpointConfig,
    signatory::{Signatory, SignatorySet},
    state::CHECKPOINTS,
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

    let mut push = |status| {
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
    checkpoint_queue.index = complete;

    for _ in 0..complete {
        push(CheckpointStatus::Complete);
    }
    if signing {
        push(CheckpointStatus::Signing);
        checkpoint_queue.index += 1;
    }
    push(CheckpointStatus::Building);

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
    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    queue.index += 10;
    let cp = queue.completed(deps.as_mut().storage, 2).unwrap();
    assert_eq!(cp.len(), 2);
    assert_eq!(cp[1].status, CheckpointStatus::Complete);
}

#[test]
fn num_unconfirmed() {
    let mut deps = mock_dependencies();
    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    queue.confirmed_index = Some(5);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    queue.confirmed_index = Some(5);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let mut queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    queue.confirmed_index = None;
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let mut queue = create_queue_with_status(deps.as_mut().storage, 0, true).unwrap();
    queue.confirmed_index = None;
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    queue.confirmed_index = None;
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 10);

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    queue.confirmed_index = None;
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 10);
}

#[test]
fn first_unconfirmed_index() {
    let mut deps = mock_dependencies();
    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    queue.confirmed_index = Some(5);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(6)
    );

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    queue.confirmed_index = Some(5);
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(6)
    );

    let mut queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    queue.confirmed_index = None;
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        None
    );

    let mut queue = create_queue_with_status(deps.as_mut().storage, 0, true).unwrap();
    queue.confirmed_index = None;
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        None
    );

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, false).unwrap();
    queue.confirmed_index = None;
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(0)
    );

    let mut queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    queue.confirmed_index = None;
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

#[test]
fn backfill_basic() {
    let mut deps = mock_dependencies();
    let mut queue = CheckpointQueue::default();

    queue.index = 10;
    CHECKPOINTS
        .push_back(deps.as_mut().storage, &Checkpoint::new(sigset(7)).unwrap())
        .unwrap();
    CHECKPOINTS
        .push_back(deps.as_mut().storage, &Checkpoint::new(sigset(8)).unwrap())
        .unwrap();
    CHECKPOINTS
        .push_back(deps.as_mut().storage, &Checkpoint::new(sigset(9)).unwrap())
        .unwrap();
    CHECKPOINTS
        .push_back(deps.as_mut().storage, &Checkpoint::new(sigset(10)).unwrap())
        .unwrap();

    let backfill_data = vec![
        sigset(8).redeem_script(&[0], (2, 3)).unwrap(),
        sigset(7).redeem_script(&[0], (2, 3)).unwrap(),
        sigset(6).redeem_script(&[0], (2, 3)).unwrap(),
        sigset(5).redeem_script(&[0], (2, 3)).unwrap(),
        sigset(4).redeem_script(&[0], (2, 3)).unwrap(),
        sigset(3).redeem_script(&[0], (2, 3)).unwrap(),
    ];
    queue
        .backfill(deps.as_mut().storage, 8, backfill_data.into_iter(), (2, 3))
        .unwrap();

    assert_eq!(queue.len(deps.as_ref().storage).unwrap(), 8);
    assert_eq!(queue.index, 10);

    assert_eq!(
        queue
            .get(deps.as_ref().storage, 3)
            .unwrap()
            .sigset
            .redeem_script(&[0], (2, 3))
            .unwrap(),
        sigset(3).redeem_script(&[0], (2, 3)).unwrap(),
    );

    assert_eq!(
        queue
            .get(deps.as_ref().storage, 10)
            .unwrap()
            .sigset
            .redeem_script(&[0], (2, 3))
            .unwrap(),
        sigset(10).redeem_script(&[0], (2, 3)).unwrap(),
    );
}

#[test]
fn backfill_with_zeroth() {
    let mut deps = mock_dependencies();
    let mut queue = CheckpointQueue::default();
    queue.index = 1;
    CHECKPOINTS
        .push_back(deps.as_mut().storage, &Checkpoint::new(sigset(1)).unwrap())
        .unwrap();

    let backfill_data = vec![sigset(0).redeem_script(&[0], (2, 3)).unwrap()];
    queue
        .backfill(deps.as_mut().storage, 0, backfill_data.into_iter(), (2, 3))
        .unwrap();

    assert_eq!(queue.len(deps.as_ref().storage).unwrap(), 2);
    assert_eq!(queue.index, 1);
    assert_eq!(
        queue
            .get(deps.as_ref().storage, 0)
            .unwrap()
            .sigset
            .redeem_script(&[0], (2, 3))
            .unwrap(),
        sigset(0).redeem_script(&[0], (2, 3)).unwrap(),
    );
    assert_eq!(
        queue
            .get(deps.as_ref().storage, 1)
            .unwrap()
            .sigset
            .redeem_script(&[0], (2, 3))
            .unwrap(),
        sigset(1).redeem_script(&[0], (2, 3)).unwrap(),
    );
}
