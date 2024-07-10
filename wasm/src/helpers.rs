use bitcoin::{
    consensus::{Decodable, Encodable},
    util::uint::Uint256,
    BlockHeader, Script,
};
use wasm_bindgen::prelude::*;

#[macro_export]
macro_rules! adapter_ops {
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

adapter_ops!(BlockHeader);
adapter_ops!(Script);
adapter_ops!(Uint256);
