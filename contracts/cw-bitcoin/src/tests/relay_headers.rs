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

    // set up
    let stamp = Utc.with_ymd_and_hms(2009, 1, 10, 17, 44, 37).unwrap();
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

#[test]
#[serial]
fn test_relay_headers_2() {
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

    // Init block 852711
    let trusted_header = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "000000000000000000000e197885b8fcb02eaed6a35e54fbe743df527e090956",
        )
        .unwrap(),
        merkle_root: "f99baa1ecd7a806510c50c103e7a53480e360626327c6f517f5f4963c44bd971"
            .parse()
            .unwrap(),
        time: 1721285108,
        bits: 386108013,
        nonce: 467428825,
    };

    let header_config = HeaderConfig {
        max_length: 2000,
        max_time_increase: 8 * 60 * 60,
        trusted_height: 852711,
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
    let header_852712 = BlockHeader {
        version: 0x1,
        prev_blockhash: BlockHash::from_hex(
            "00000000000000000001459fbe0477dceedc0f72b7baae70e124efac1b410b26",
        )
        .unwrap(),
        merkle_root: TxMerkleNode::from_hex(
            "bc359a26203a96855754722cecbdd4e6a58cbe453e81017bbddcc0c117748ed8",
        )
        .unwrap(),
        time: 1721287419,
        bits: 386108013,
        nonce: 879772167,
    };
    // let header_852713 = BlockHeader {
    //     version: 0x1,
    //     prev_blockhash: BlockHash::from_hex(
    //         "00000000000000000001672387abddd1b6bdb71f8abbef087b1b44b9c731313f",
    //     )
    //     .unwrap(),
    //     merkle_root: TxMerkleNode::from_hex(
    //         "a553e0fcdc3b1d19df13d9ea8a8635cbcb9f32969ffe4676380b13e05ecb75e2",
    //     )
    //     .unwrap(),
    //     time: 1721288473,
    //     bits: 386108013,
    //     nonce: 2866698366,
    // };
    // let header_852714 = BlockHeader {
    //     version: 0x1,
    //     prev_blockhash: BlockHash::from_hex(
    //         "000000000000000000034adba1b5ba7f6fe047c5d4324867b5acc4abdf138c2f",
    //     )
    //     .unwrap(),
    //     merkle_root: TxMerkleNode::from_hex(
    //         "1b17ada217f478cdc58b2da0144e0ae21e85259324ef257840bf322705a68289",
    //     )
    //     .unwrap(),
    //     time: 1721288988,
    //     bits: 386108013,
    //     nonce: 1316334967,
    // };
    // let header_852715 = BlockHeader {
    //     version: 0x1,
    //     prev_blockhash: BlockHash::from_hex(
    //         "00000000000000000002ce043b6be0ef1ff73b5367984304be51426a34b67513",
    //     )
    //     .unwrap(),
    //     merkle_root: TxMerkleNode::from_hex(
    //         "7dee6993c2b4b994c9f6e159dbceec6b4b8cae9e512d4de62f2616ed56089be1",
    //     )
    //     .unwrap(),
    //     time: 1721290322,
    //     bits: 386108013,
    //     nonce: 1561485854,
    // };
    // let header_852716 = BlockHeader {
    //     version: 0x1,
    //     prev_blockhash: BlockHash::from_hex(
    //         "0000000000000000000047da97a6099e8d7fef9e46e70f6f78ced349cbf18535",
    //     )
    //     .unwrap(),
    //     merkle_root: TxMerkleNode::from_hex(
    //         "933547277d9fb987b422c1b33a8ffd45d4a1b1d6336b1e385fa5427e3148d861",
    //     )
    //     .unwrap(),
    //     time: 1721291358,
    //     bits: 386108013,
    //     nonce: 1266111310,
    // };

    let header_list = vec![
        WrappedHeader::new(Adapter::new(header_852712), 852712),
        // WrappedHeader::new(Adapter::new(header_852713), 852713),
        // WrappedHeader::new(Adapter::new(header_852714), 852714),
        // WrappedHeader::new(Adapter::new(header_852715), 852715),
        // WrappedHeader::new(Adapter::new(header_852716), 852716),
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
