use crate::error::ContractResult;
use crate::interface::PartialMerkleTree;
use bitcoin::{
    consensus::{Decodable, Encodable},
    util::uint::Uint256,
    BlockHeader, Script, Transaction,
};
use wasm_bindgen::prelude::*;

#[macro_export]
macro_rules! encode_ops {
    ($inner:ty) => {
        ::paste::paste! {

            #[wasm_bindgen]
            pub fn [<toBinary $inner>] (value: JsValue) -> ContractResult<String> {
                    let inner: $inner = serde_wasm_bindgen::from_value(value)?;
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)?;
                    Ok(base64::encode(dest))
            }

            #[wasm_bindgen]
            pub fn [<fromBinary $inner>] (value: &str) -> ContractResult<JsValue> {
                    let slice = base64::decode(value)?;
                    let inner: $inner = Decodable::consensus_decode(&mut slice.as_slice())?;
                    Ok(serde_wasm_bindgen::to_value(&inner)?)
            }
        }
    };
}

encode_ops!(BlockHeader);
encode_ops!(Script);
encode_ops!(Uint256);
encode_ops!(Transaction);
encode_ops!(PartialMerkleTree);
