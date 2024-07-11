use wasm_bindgen::prelude::*;

use crate::interface::PartialMerkleTree;
use bitcoin::{
    consensus::{Decodable, Encodable},
    util::uint::Uint256,
    BlockHeader, Script, Transaction,
};

use crate::header_queue::{HeaderConfig, WorkHeader};

#[macro_export]
macro_rules! convert_ops {
    ($inner:ty) => {
        ::paste::paste! {

            #[wasm_bindgen]
            pub fn [<toBinary $inner>] (value: JsValue) -> Result<Vec<u8>, JsValue> {
                    let inner: $inner = serde_wasm_bindgen::from_value(value)?;
                    let dest = serde_json_wasm::to_vec(&inner)
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(dest)
            }

            #[wasm_bindgen]
            pub fn [<fromBinary $inner>] (value: Vec<u8>) -> Result<JsValue, JsValue> {
                    let inner: $inner = serde_json_wasm::from_slice(&value)
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(serde_wasm_bindgen::to_value(&inner)?)
            }
        }
    };
}

#[macro_export]
macro_rules! encode_ops {
    ($inner:ty) => {
        ::paste::paste! {

            #[wasm_bindgen]
            pub fn [<encode $inner>] (value: JsValue) -> Result<Vec<u8>, JsValue> {
                    let inner: $inner = serde_wasm_bindgen::from_value(value)?;
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(dest)
            }

            #[wasm_bindgen]
            pub fn [<decode $inner>] (value: Vec<u8>) -> Result<JsValue, JsValue> {
                    let inner: $inner = Decodable::consensus_decode(&mut value.as_slice())
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

convert_ops!(HeaderConfig);
convert_ops!(WorkHeader);
