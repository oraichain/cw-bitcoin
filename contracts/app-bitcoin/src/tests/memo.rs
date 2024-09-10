use cosmwasm_std::Binary;
use oraiswap::universal_swap_memo::{
    memo::{IbcTransfer, PostAction},
    Memo,
};
use prost::Message;

#[test]
fn test_memo() {
    let memo = Memo {
        minimum_receive: "1000_000".to_string(),
        post_swap_action: Some(PostAction {
            ibc_transfer_msg: Some(IbcTransfer {
                receiver: "receiver".to_string(),
                source_port: "source_port".to_string(),
                source_channel: "source_channel".to_string(),
                memo: "memo".to_string(),
                recover_address: "orai1ehmhqcn8erf3dgavrca69zgp4rtxj5kqgtcnyd".to_string(),
            }),
            contract_call: None,
            ibc_wasm_transfer_msg: None,
            transfer_msg: None,
        }),
        recovery_addr: "orai1ehmhqcn8erf3dgavrca69zgp4rtxj5kqgtcnyd".to_string(),
        timeout_timestamp: 19219319231,
        user_swap: None,
    };
    memo.validate().unwrap();
    let encode_memo = Memo::encode_to_vec(&memo);
    let str_memo = Binary::from(encode_memo).to_string();
    let decode_memo = Memo::decode_memo(Binary::from_base64(str_memo.as_str()).unwrap()).unwrap();
    assert_eq!(memo, decode_memo);
}
