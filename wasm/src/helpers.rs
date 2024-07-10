#[macro_export]
macro_rules! adapter_ops {
    ($inner:ident) => {
        ::paste::paste! {
            use bitcoin::consensus::{Decodable,Encodable};
            #[wasm_bindgen]
            pub fn [<serialize $inner>] (value: JsValue) -> Result<Vec<u8>, JsValue> {
                    let inner: $inner = serde_wasm_bindgen::from_value(value)?;
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(dest)
            }

            #[wasm_bindgen]
            pub fn [<deserialize $inner>] (value: Vec<u8>) -> Result<JsValue, JsValue> {
                    let inner: $inner = Decodable::consensus_decode(&mut value.as_slice())
                        .map_err(|err| JsValue::from_str(&err.to_string()))?;
                    Ok(serde_wasm_bindgen::to_value(&inner)?)
            }
        }
    };
}
