use bitcoin::consensus::Decodable;
use bitcoin::hashes::hex::FromHex;
use bitcoin::hashes::sha256d::Hash;
use bitcoin::BlockHash;
use bitcoin::{hash_types::TxMerkleNode, BlockHeader};
use chrono::{TimeZone, Utc};
use cosmwasm_std::Binary;
use cosmwasm_std::{from_binary, testing::mock_dependencies, to_binary};
use serial_test::serial;

use crate::adapter::Adapter;
use crate::header::{HeaderQueue, WrappedHeader};
use crate::interface::HeaderConfig;
use crate::state::{HEADERS, HEADER_CONFIG};

#[test]
fn primitive_adapter_encode_decode() {
    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 39, 13).unwrap();
    //Bitcoin block 42
    let header = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("00000000ad2b48c7032b6d7d4f2e19e54d79b1c159f5599056492f2cd7bb528b")
                .unwrap(),
        ),
        merkle_root: "27c4d937dca276fb2b61e579902e8a876fd5b5abc17590410ced02d5a9f8e483"
            .parse()
            .unwrap(),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 3_600_650_283,
    };

    let adapter = Adapter::new(header);
    let encoded_adapter = to_binary(&adapter).unwrap();
    let decoded_adapter: Adapter<BlockHeader> = from_binary(&encoded_adapter).unwrap();

    assert_eq!(*decoded_adapter, header);

    // post from client
    let header_str="AQAAAItSu9csL0lWkFn1WcGxeU3lGS5PfW0rA8dIK60AAAAAg+T4qdUC7QxBkHXBq7XVb4eKLpB55WEr+3ai3DfZxCdB3WhJ//8AHSuQndY=";
    let header_wasm: BlockHeader =
        Decodable::consensus_decode(&mut Binary::from_base64(header_str).unwrap().as_slice())
            .unwrap();
    assert_eq!(header_wasm, header);
}

#[test]
#[serial]
fn add_multiple() {
    let mut deps = mock_dependencies();

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 44, 37).unwrap();

    let header_43 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("00000000314e90489514c787d615cea50003af2023796ccdd085b6bcc1fa28f5")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("2f5c03ce19e9a855ac93087a1b68fe6592bcf4bd7cbb9c1ef264d886a785894e")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_093_702_200,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 59, 21).unwrap();

    let header_44 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("00000000ac21f2862aaab177fd3c5c8b395de842f84d88c9cf3420b2d393e550")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("439aee1e1aa6923ad61c1990459f88de1faa3e18b4ee125f99b94b82e1e0af5f")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 429_798_192,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 11, 8).unwrap();

    let header_45 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("000000002978eecde8d020f7f057083bc990002fff495121d7dc1c26d00c00f8")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("f69778085f1e78a1ea1cfcfe3b61ffb5c99870f5ae382e41ec43cf165d66a6d9")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_771_238_433,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 23, 13).unwrap();

    let header_46 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("000000009189006e461d2f4037a819d00217412ac01900ddbf09461100b836bb")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("ddd4d06365155ab4caaaee552fb3d8643207bd06efe14f920698a6dd4eb22ffa")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 1_626_117_377,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 41, 28).unwrap();

    let header_47 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("0000000002d5f429a2e3a9d9f82b777469696deb64038803c87833aa8ee9c08e")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("d17b9c9c609309049dfb9005edd7011f02d7875ca7dab6effddf4648bb70eff6")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_957_174_816,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 45, 40).unwrap();

    let header_48 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("000000001a5c4531f86aa874e711e1882038336e2610f70ce750cdd690c57a81")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("32edede0b7d0c37340a665de057f418df634452f6bb80dcb8a5ff0aeddf1158a")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 3_759_171_867,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 56, 42).unwrap();

    let header_49 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("0000000088960278f4060b8747027b2aac0eb443aedbb1b75d1a72cf71826e89")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("194c9715279d8626bc66f2b6552f2ae67b3df3a00b88553245b12bffffad5b59")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 3_014_810_412,
    };

    let header_list = vec![
        WrappedHeader::new(Adapter::new(header_43), 43),
        WrappedHeader::new(Adapter::new(header_44), 44),
        WrappedHeader::new(Adapter::new(header_45), 45),
        WrappedHeader::new(Adapter::new(header_46), 46),
        WrappedHeader::new(Adapter::new(header_47), 47),
        WrappedHeader::new(Adapter::new(header_48), 48),
        WrappedHeader::new(Adapter::new(header_49), 49),
    ];

    let test_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: 42,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: BlockHeader {
            version: 1,
            prev_blockhash: BlockHash::from_hex(
                "00000000ad2b48c7032b6d7d4f2e19e54d79b1c159f5599056492f2cd7bb528b",
            )
            .unwrap(),
            merkle_root: TxMerkleNode::from_hex(
                "27c4d937dca276fb2b61e579902e8a876fd5b5abc17590410ced02d5a9f8e483",
            )
            .unwrap(),
            time: 1231609153,
            bits: 486604799,
            nonce: 3600650283,
        }
        .into(),
    };

    HEADER_CONFIG
        .save(deps.as_mut().storage, &test_config)
        .unwrap();
    HEADERS
        .push_back(deps.as_mut().storage, &test_config.work_header())
        .unwrap();

    let mut q = HeaderQueue::new(test_config.clone());
    q.configure(deps.as_mut().storage, test_config).unwrap();
    q.add(deps.as_mut().storage, header_list.into()).unwrap();
}

#[test]
fn add_into_iterator() {
    let mut deps = mock_dependencies();
    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 44, 37).unwrap();

    let header = BlockHeader {
        version: 0x1,
        prev_blockhash: Hash::from_hex(
            "00000000314e90489514c787d615cea50003af2023796ccdd085b6bcc1fa28f5",
        )
        .unwrap()
        .into(),
        merkle_root: Hash::from_hex(
            "2f5c03ce19e9a855ac93087a1b68fe6592bcf4bd7cbb9c1ef264d886a785894e",
        )
        .unwrap()
        .into(),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_093_702_200,
    };

    let test_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: 42,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: BlockHeader {
            version: 1,
            prev_blockhash: Hash::from_hex(
                "00000000ad2b48c7032b6d7d4f2e19e54d79b1c159f5599056492f2cd7bb528b",
            )
            .unwrap()
            .into(),
            merkle_root: Hash::from_hex(
                "27c4d937dca276fb2b61e579902e8a876fd5b5abc17590410ced02d5a9f8e483",
            )
            .unwrap()
            .into(),
            time: 1231609153,
            bits: 486604799,
            nonce: 3600650283,
        }
        .into(),
    };
    HEADER_CONFIG
        .save(deps.as_mut().storage, &test_config)
        .unwrap();
    HEADERS
        .push_back(deps.as_mut().storage, &test_config.work_header())
        .unwrap();

    let adapter = Adapter::new(header);
    let header_list = [WrappedHeader::new(adapter, 43)];
    let mut q = HeaderQueue::new(test_config.clone());
    q.configure(deps.as_mut().storage, test_config).unwrap();
    q.add_into_iter(deps.as_mut().storage, header_list).unwrap();
}

#[test]
#[should_panic(expected = "Bitcoin(BlockBadTarget)")]
fn add_wrong_bits_non_retarget() {
    let mut deps = mock_dependencies();
    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 44, 37).unwrap();

    let header = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hash(
            Hash::from_hex("00000000314e90489514c787d615cea50003af2023796ccdd085b6bcc1fa28f5")
                .unwrap(),
        ),
        merkle_root: TxMerkleNode::from_hash(
            Hash::from_hex("2f5c03ce19e9a855ac93087a1b68fe6592bcf4bd7cbb9c1ef264d886a785894e")
                .unwrap(),
        ),
        time: stamp.timestamp() as u32,
        bits: 486_604_420,
        nonce: 2_093_702_200,
    };

    let test_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: 42,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: BlockHeader {
            version: 1,
            prev_blockhash: Hash::from_hex(
                "00000000ad2b48c7032b6d7d4f2e19e54d79b1c159f5599056492f2cd7bb528b",
            )
            .unwrap()
            .into(),
            merkle_root: Hash::from_hex(
                "27c4d937dca276fb2b61e579902e8a876fd5b5abc17590410ced02d5a9f8e483",
            )
            .unwrap()
            .into(),
            time: 1231609153,
            bits: 486604799,
            nonce: 3600650283,
        }
        .into(),
    };

    HEADER_CONFIG
        .save(deps.as_mut().storage, &test_config)
        .unwrap();
    HEADERS
        .push_back(deps.as_mut().storage, &test_config.work_header())
        .unwrap();

    let adapter = Adapter::new(header);
    let header_list = [WrappedHeader::new(adapter, 43)];
    let mut q = HeaderQueue::new(test_config.clone());
    q.configure(deps.as_mut().storage, test_config).unwrap();
    q.add_into_iter(deps.as_mut().storage, header_list).unwrap();
}
