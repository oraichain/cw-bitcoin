use bitcoin::blockdata::transaction::ParseOutPointError;

#[derive(thiserror::Error, Debug)]
pub enum ContractError {
    #[error(transparent)]
    Wasm(#[from] serde_wasm_bindgen::Error),
    #[error(transparent)]
    Bitcoin(#[from] bitcoin::Error),
    #[error(transparent)]
    ParseOutPoint(#[from] ParseOutPointError),
    #[error(transparent)]
    BitcoinAddress(#[from] bitcoin::util::address::Error),
    #[error(transparent)]
    BitcoinHash(#[from] bitcoin::hashes::Error),
    #[error(transparent)]
    BitcoinLockTime(#[from] bitcoin::locktime::Error),
    #[error(transparent)]
    Base64Decode(#[from] base64::DecodeError),
    #[error(transparent)]
    BitcoinHashes(#[from] bitcoin::hashes::hex::Error),
    #[error(transparent)]
    BitcoinEncode(#[from] bitcoin::consensus::encode::Error),
    #[error(transparent)]
    Bip32(#[from] bitcoin::util::bip32::Error),
    #[error(transparent)]
    Sighash(#[from] bitcoin::util::sighash::Error),
    #[error(transparent)]
    TryFrom(#[from] std::num::TryFromIntError),
    #[error("App Error: {0}")]
    App(String),
    #[error(transparent)]
    Secp(#[from] bitcoin::secp256k1::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<ContractError> for wasm_bindgen::JsValue {
    fn from(failure: ContractError) -> Self {
        js_sys::Error::new(&failure.to_string()).into()
    }
}

pub type ContractResult<T> = std::result::Result<T, ContractError>;
