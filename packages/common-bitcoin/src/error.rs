use cosmwasm_std::{StdError, VerificationError};

#[derive(thiserror::Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),
    #[error(transparent)]
    Verify(#[from] VerificationError),
    #[error("Account Error: {0}")]
    Account(String),
    #[error("Coins Error: {0}")]
    Coins(String),
    #[error("Address Error: {0}")]
    Address(String),
    #[error(transparent)]
    Bitcoin(#[from] bitcoin::Error),
    #[error(transparent)]
    ParseOutPoint(#[from] bitcoin::blockdata::transaction::ParseOutPointError),
    #[error(transparent)]
    BitcoinHash(#[from] bitcoin::hashes::Error),
    #[error(transparent)]
    BitcoinEncode(#[from] bitcoin::consensus::encode::Error),
    #[error("Unable to deduct fee: {0}")]
    BitcoinFee(u64),
    #[error("{0}")]
    BitcoinRecoveryScript(String),
    #[error(transparent)]
    Bip32(#[from] bitcoin::util::bip32::Error),
    #[error("{0}")]
    Checkpoint(String),
    #[error(transparent)]
    Sighash(#[from] bitcoin::util::sighash::Error),
    #[error(transparent)]
    TryFrom(#[from] std::num::TryFromIntError),
    #[error("App Error: {0}")]
    App(String),
    #[error(transparent)]
    Secp(#[from] bitcoin::secp256k1::Error),
    #[error("Could not verify merkle proof")]
    BitcoinMerkleBlockError,
    #[error("{0}")]
    Header(String),
    #[error("{0}")]
    Ibc(String),
    #[error("Input index: {0} out of bounds")]
    InputIndexOutOfBounds(usize),
    #[error("{0}")]
    Signer(String),
    #[error("unauthorized")]
    Unauthorized {},
    #[error("Validator is not on whitelisted set")]
    ValidatorUnwhitelisted {},
    #[error("Validator is on jailed")]
    ValidatorJailed {},
    #[error("Validator does not have consensus keys")]
    ValidatorNoConsensusPubKey {},
    #[error("Validator is not in bonded status")]
    ValidatorNotBonded {},
}

impl From<ContractError> for StdError {
    fn from(source: ContractError) -> Self {
        Self::generic_err(source.to_string())
    }
}

pub type ContractResult<T> = std::result::Result<T, ContractError>;
