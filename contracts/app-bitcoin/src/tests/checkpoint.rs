use bitcoin::util::bip32::ExtendedPubKey;
use cosmwasm_std::{testing::mock_dependencies, Binary, Storage};

use crate::{
    checkpoint::{adjust_fee_rate, BitcoinTx, Checkpoint, CheckpointQueue, CheckpointStatus},
    constants::DEFAULT_FEE_RATE,
    interface::{BitcoinConfig, CheckpointConfig},
    signatory::{Signatory, SignatoryKeys, SignatorySet},
    state::{
        BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINTS, CHECKPOINT_CONFIG, CONFIRMED_INDEX, FEE_POOL,
        SIGNERS, VALIDATORS,
    },
    tests::helper::push_bitcoin_tx_output,
    threshold_sig::Pubkey,
};
use common_bitcoin::{error::ContractResult, xpub::Xpub};

fn cons_keys_real_validators() -> Vec<[u8; 32]> {
    vec![
        Binary::from_base64("E6NYC3EdWPreSGwucQ1jUpMnIFFLLyZcwA3tG7jAhT4=")
            .unwrap()
            .rchunks(32)
            .next()
            .unwrap()
            .try_into()
            .unwrap(),
        Binary::from_base64("f4QfZU1vYhUiEuBeAAXA4RTYGGiStpjktkKPbn6ZpjM=")
            .unwrap()
            .rchunks(32)
            .next()
            .unwrap()
            .try_into()
            .unwrap(),
        Binary::from_base64("bJPZePKkNzz3V/WABeHPVdmn4Gk6uHbq1Toro76u4SQ=")
            .unwrap()
            .rchunks(32)
            .next()
            .unwrap()
            .try_into()
            .unwrap(),
    ]
}

fn xpub_real_validators() -> Vec<Xpub> {
    let mut xpubs = vec![];
    let decode_base64 = ExtendedPubKey::decode(&Binary::from_base64("BIiyHgAAAAAAAAAAAJwuXJlnKyOcQ/hBOlDMZ/lo3XYZ0acAAsFXSXO00X44AwGI2HzHhD8JFKX0md9zGNRq0H6q0kxBU2qKTjA5zcYN").unwrap().to_vec()).unwrap();
    let xpub: Xpub = Xpub::new(decode_base64);
    xpubs.push(xpub);

    let decode_base64 = ExtendedPubKey::decode(&Binary::from_base64("BIiyHgAAAAAAAAAAAJf1C4vBY96sVBQo0nIrImUWq0MuNzFEknM7rqUzL2UgA645Rw7OhhV5Y2LGs72m127rxtzkPLVgG7Au2/ynrBEM").unwrap().to_vec()).unwrap();
    let xpub: Xpub = Xpub::new(decode_base64);
    xpubs.push(xpub);

    let decode_base64 = ExtendedPubKey::decode(&Binary::from_base64("BIiyHgAAAAAAAAAAAILSFhI3O5Z/I9/d2Gcj390ZbrUMOxMQBMrQxZOcL9B8A3gEq8AXH3ve8fBPSHd4UL7QnqdHew0BaShnRx7ygjVO").unwrap().to_vec()).unwrap();
    let xpub: Xpub = Xpub::new(decode_base64);
    xpubs.push(xpub);
    xpubs
}

#[test]
fn test_with_real_data() -> Result<(), common_bitcoin::error::ContractError> {
    let mut deps = mock_dependencies();
    static JSON: &[u8] = include_bytes!("testdata/checkpoints.json");
    let checkpoints: Vec<Checkpoint> = serde_json::from_slice(JSON).unwrap();
    let mut checkpoint_queue = CheckpointQueue::default();
    checkpoint_queue.reset(&mut deps.storage).unwrap();
    // Set up validators
    let mut signatory_keys = SignatoryKeys::default();
    let cons_keys = cons_keys_real_validators();
    let xpubs = xpub_real_validators();
    SIGNERS
        .save(
            &mut deps.storage,
            &"orai1qv5jn7tueeqw7xqdn5rem7s09n7zletrsnc5vq",
            &cons_keys[0],
        )
        .unwrap();
    VALIDATORS
        .save(
            &mut deps.storage,
            &cons_keys[0],
            &(
                119251177812,
                "orai1qv5jn7tueeqw7xqdn5rem7s09n7zletrsnc5vq".to_string(),
            ),
        )
        .unwrap();
    signatory_keys
        .insert(&mut deps.storage, cons_keys[0], xpubs[0])
        .unwrap();
    SIGNERS
        .save(
            &mut deps.storage,
            &"orai1q53ujvvrcd0t543dsh5445lu6ar0qr2z9ll7ux",
            &cons_keys[1],
        )
        .unwrap();
    VALIDATORS
        .save(
            &mut deps.storage,
            &cons_keys[1],
            &(
                72778342087,
                "orai1q53ujvvrcd0t543dsh5445lu6ar0qr2z9ll7ux".to_string(),
            ),
        )
        .unwrap();
    signatory_keys
        .insert(&mut deps.storage, cons_keys[1], xpubs[1])
        .unwrap();
    SIGNERS
        .save(
            &mut deps.storage,
            &"orai1ltr3sx9vm9hq4ueajvs7ng24gw3k8t9t67y73h",
            &cons_keys[2],
        )
        .unwrap();
    VALIDATORS
        .save(
            &mut deps.storage,
            &cons_keys[2],
            &(
                35556132100,
                "orai1ltr3sx9vm9hq4ueajvs7ng24gw3k8t9t67y73h".to_string(),
            ),
        )
        .unwrap();
    signatory_keys
        .insert(&mut deps.storage, cons_keys[2], xpubs[2])
        .unwrap();
    // End of setting up validators
    for cp in checkpoints {
        CHECKPOINTS.push_back(&mut deps.storage, &cp).unwrap();
    }
    FEE_POOL.save(&mut deps.storage, &229030000000).unwrap();
    CONFIRMED_INDEX.save(&mut deps.storage, &18).unwrap();
    BUILDING_INDEX.save(&mut deps.storage, &19).unwrap();
    CHECKPOINT_CONFIG
        .save(&mut deps.storage, &CheckpointConfig::default())
        .unwrap();
    BITCOIN_CONFIG
        .save(&mut deps.storage, &BitcoinConfig::default())
        .unwrap();
    let bitcoin_config = BITCOIN_CONFIG.load(&deps.storage).unwrap();
    let maybe_step = checkpoint_queue
        .simulate_maybe_step(
            1729678400,
            &mut deps.storage,
            866985,
            true,
            Binary::from_base64("S/msfBNFBq0MNKe7cVMIkY8n3eEtRydId/7q6Tpn1Lc=")
                .unwrap()
                .to_vec(),
            &bitcoin_config,
        )
        .unwrap();
    assert_eq!(maybe_step, true);
    let queue_len = CHECKPOINTS.len(&deps.storage).unwrap();
    println!("queue_len: {}", queue_len);
    println!("[======================CHECKPOINT_QUEUE=======================]");
    let cp_19 = checkpoint_queue.get(&deps.storage, 19).unwrap();
    assert_eq!(cp_19.status, CheckpointStatus::Building);
    let cp_13 = checkpoint_queue.get(&deps.storage, 13).unwrap();
    assert_eq!(cp_13.status, CheckpointStatus::Signing);
    // let cp_20 = checkpoint_queue.get(&deps.storage, 20).unwrap();
    // assert_eq!(cp_20.status, CheckpointStatus::Building);
    Ok(())
}

#[test]
fn deduct_fee() {
    let mut bitcoin_tx = BitcoinTx::default();
    push_bitcoin_tx_output(&mut bitcoin_tx, 0);
    push_bitcoin_tx_output(&mut bitcoin_tx, 10000);

    bitcoin_tx.deduct_fee(100).unwrap();

    assert_eq!(bitcoin_tx.output.len(), 1);
    assert_eq!(bitcoin_tx.output.first().unwrap().value, 9900);
}

#[test]
fn deduct_fee_multi_pass() {
    let mut bitcoin_tx = BitcoinTx::default();
    push_bitcoin_tx_output(&mut bitcoin_tx, 502);
    push_bitcoin_tx_output(&mut bitcoin_tx, 482);
    push_bitcoin_tx_output(&mut bitcoin_tx, 300);

    bitcoin_tx.deduct_fee(30).unwrap();

    assert_eq!(bitcoin_tx.output.len(), 1);
    assert_eq!(bitcoin_tx.output.first().unwrap().value, 472);
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
    BUILDING_INDEX
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
    CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 4);

    let queue = create_queue_with_status(deps.as_mut().storage, 0, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let queue = create_queue_with_status(deps.as_mut().storage, 0, true).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 0);

    let queue = create_queue_with_status(deps.as_mut().storage, 1, false).unwrap();
    CONFIRMED_INDEX.remove(deps.as_mut().storage);
    assert_eq!(queue.num_unconfirmed(deps.as_ref().storage).unwrap(), 1);

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
    CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
    assert_eq!(
        queue
            .first_unconfirmed_index(deps.as_ref().storage)
            .unwrap(),
        Some(6)
    );

    let queue = create_queue_with_status(deps.as_mut().storage, 10, true).unwrap();
    CONFIRMED_INDEX.save(deps.as_mut().storage, &5).unwrap();
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
