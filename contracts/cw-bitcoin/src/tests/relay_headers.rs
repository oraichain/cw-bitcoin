use super::helper::MockApp;
use crate::adapter::Adapter;
use crate::header::WrappedHeader;
use crate::interface::{Dest, HeaderConfig};
use crate::msg;
use bitcoin::hashes::hex::FromHex;
use bitcoin::hashes::Hash;
use bitcoin::util::merkleblock::PartialMerkleTree;
use bitcoin::util::uint;
use bitcoin::{BlockHash, BlockHeader, Transaction, TxMerkleNode, Txid};
use chrono::{TimeZone, Utc};
use cosmwasm_std::Addr;
use serial_test::serial;

#[test]
#[serial]
fn test_relay_headers() {
    let mut app = MockApp::new(&[]);
    let token_factory_addr = app
        .create_tokenfactory(Addr::unchecked("obtc_minter"))
        .unwrap();
    let bridge_addr = app
        .create_bridge(
            Addr::unchecked("perfogic"),
            &msg::InstantiateMsg { token_factory_addr },
        )
        .unwrap();

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 39, 13).unwrap();

    // Init block 42
    let trusted_header = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "00000000ad2b48c7032b6d7d4f2e19e54d79b1c159f5599056492f2cd7bb528b",
        )
        .unwrap(),
        merkle_root: "27c4d937dca276fb2b61e579902e8a876fd5b5abc17590410ced02d5a9f8e483"
            .parse()
            .unwrap(),
        time: 1231609153,
        bits: 486_604_799,
        nonce: 3_600_650_283,
    };
    let header_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: 42,
        retarget_interval: 2016,
        target_spacing: 10 * 60,
        target_timespan: 2016 * (10 * 60),
        max_target: 0x1d00ffff,
        retargeting: true,
        min_difficulty_blocks: false,
        trusted_header: Adapter::from(trusted_header),
    };

    let _res = app
        .execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::UpdateHeaderConfig {
                config: header_config,
            },
            &[],
        )
        .unwrap();

    // set up headers
    let header_43 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "00000000314e90489514c787d615cea50003af2023796ccdd085b6bcc1fa28f5",
        )
        .unwrap(),
        merkle_root: TxMerkleNode::from_hex(
            "2f5c03ce19e9a855ac93087a1b68fe6592bcf4bd7cbb9c1ef264d886a785894e",
        )
        .unwrap(),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_093_702_200,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 59, 21).unwrap();

    let header_44 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "00000000ac21f2862aaab177fd3c5c8b395de842f84d88c9cf3420b2d393e550",
        )
        .unwrap(),
        merkle_root: TxMerkleNode::from_hex(
            "439aee1e1aa6923ad61c1990459f88de1faa3e18b4ee125f99b94b82e1e0af5f",
        )
        .unwrap(),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 429_798_192,
    };

    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 18, 11, 8).unwrap();

    let header_45 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "000000002978eecde8d020f7f057083bc990002fff495121d7dc1c26d00c00f8",
        )
        .unwrap(),
        merkle_root: TxMerkleNode::from_hex(
            "f69778085f1e78a1ea1cfcfe3b61ffb5c99870f5ae382e41ec43cf165d66a6d9",
        )
        .unwrap(),
        time: stamp.timestamp() as u32,
        bits: 486_604_799,
        nonce: 2_771_238_433,
    };

    let header_list = vec![
        WrappedHeader::new(Adapter::new(header_43), 43),
        WrappedHeader::new(Adapter::new(header_44), 44),
        WrappedHeader::new(Adapter::new(header_45), 45),
    ];
    let _res = app
        .execute(
            Addr::unchecked("alice"),
            bridge_addr.clone(),
            &msg::ExecuteMsg::RelayHeaders {
                headers: header_list,
            },
            &[],
        )
        .unwrap();
}
