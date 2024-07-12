use wasm_bindgen::prelude::*;

use crate::interface::PartialMerkleTree;
use bitcoin::{
    consensus::{Decodable, Encodable},
    util::uint::Uint256,
    BlockHeader, Script, Transaction,
};

#[macro_export]
macro_rules! encode_ops {
    ($inner:ty) => {
        ::paste::paste! {

            #[wasm_bindgen]
            pub fn [<toBinary $inner>] (value: JsValue) -> Result<String, JsValue> {
                    let inner: $inner = serde_wasm_bindgen::from_value(value)?;
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(base64::encode(dest))
            }

            #[wasm_bindgen]
            pub fn [<fromBinary $inner>] (value: &str) -> Result<JsValue, JsValue> {
                    let slice = base64::decode(value).unwrap();
                    let inner: $inner = Decodable::consensus_decode(&mut slice.as_slice())
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
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
