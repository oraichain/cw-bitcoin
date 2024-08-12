use crate::error::ContractResult;
use bitcoin::{
    consensus::{Decodable, Encodable},
    util::merkleblock::{MerkleBlock, PartialMerkleTree},
    BlockHeader, Script, Transaction,
};
use wasm_bindgen::prelude::*;

#[macro_export]
macro_rules! encode_ops {
    ($inner:ty) => {
        ::paste::paste! {
            #[wasm_bindgen]
            pub fn [<toBase64 $inner>] (inner: $inner) -> ContractResult<String> {
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)?;
                    Ok(base64::encode(dest))
            }

            #[wasm_bindgen]
            pub fn [<fromBase64 $inner>] (value: &str) -> ContractResult<$inner> {
                    let slice = base64::decode(value)?;
                    let inner: $inner = Decodable::consensus_decode(&mut slice.as_slice())?;
                    Ok(inner)
            }

            #[wasm_bindgen]
            pub fn [<toBinary $inner>] (inner: $inner) -> ContractResult<js_sys::Uint8Array> {
                    let mut dest: Vec<u8> = Vec::new();
                    inner.consensus_encode(&mut dest)?;
                    let buffer = js_sys::Uint8Array::new_with_length(dest.len() as u32);
                    buffer.copy_from(&dest);
                    Ok(buffer)
            }

            #[wasm_bindgen]
            pub fn [<fromBinary $inner>] (value: js_sys::Uint8Array) -> ContractResult<$inner> {
                    let inner: $inner = Decodable::consensus_decode(&mut value.to_vec().as_slice())?;
                    Ok(inner)
            }


        }
    };
}

encode_ops!(BlockHeader);
encode_ops!(Script);
encode_ops!(PartialMerkleTree);
encode_ops!(Transaction);
encode_ops!(MerkleBlock);
