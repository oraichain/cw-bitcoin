use bitcoin::blockdata::transaction::ParseOutPointError;

#[derive(thiserror::Error, Debug)]
pub enum ContractError {
    #[error(transparent)]
    Ed(#[from] ed::Error),
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

pub type ContractResult<T> = std::result::Result<T, ContractError>;
