use std::io;

use bitcoin::{
    consensus::{encode, Decodable, Encodable},
    BlockHash, TxMerkleNode, Txid,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tsify::Tsify;

use crate::error::ContractResult;

#[derive(Clone, Debug, PartialOrd, PartialEq, Eq, Ord, Deserialize, Serialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct IbcDest {
    pub source_port: String,
    pub source_channel: String,
    #[serde(skip)]
    pub receiver: String,
    #[serde(skip)]
    pub sender: String,
    pub timeout_timestamp: u64,
    pub memo: String,
}

#[derive(Clone, Debug, PartialOrd, PartialEq, Eq, Ord, Deserialize, Serialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum Dest {
    Address(String),
    Ibc(IbcDest),
}

impl Dest {
    pub fn to_receiver_addr(&self) -> String {
        match self {
            Self::Address(addr) => addr.to_string(),
            Self::Ibc(dest) => dest.receiver.to_string(),
        }
    }

    pub fn commitment_bytes(&self) -> ContractResult<Vec<u8>> {
        let bytes = match self {
            Self::Address(addr) => addr.as_bytes().into(),
            Self::Ibc(dest) => Sha256::digest(dest.receiver.as_bytes()).to_vec(),
        };

        Ok(bytes)
    }
}

#[derive(
    Copy, PartialEq, Eq, Clone, Debug, PartialOrd, Ord, Hash, Serialize, Deserialize, Tsify,
)]
pub struct BlockHeader {
    /// Originally protocol version, but repurposed for soft-fork signaling.
    ///
    /// ### Relevant BIPs
    ///
    /// * [BIP9 - Version bits with timeout and delay](https://github.com/bitcoin/bips/blob/master/bip-0009.mediawiki) (current usage)
    /// * [BIP34 - Block v2, Height in Coinbase](https://github.com/bitcoin/bips/blob/master/bip-0034.mediawiki)
    pub version: i32,
    /// Reference to the previous block in the chain.
    pub prev_blockhash: BlockHash,
    /// The root hash of the merkle tree of transactions in the block.
    pub merkle_root: TxMerkleNode,
    /// The timestamp of the block, as claimed by the miner.
    pub time: u32,
    /// The target value below which the blockhash must lie, encoded as a
    /// a float (with well-defined rounding, of course).
    pub bits: u32,
    /// The nonce, selected to obtain a low enough blockhash.
    pub nonce: u32,
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PartialMerkleTree {
    /// The total number of transactions in the block
    num_transactions: u32,
    /// node-is-parent-of-matched-txid bits
    bits: Vec<bool>,
    /// Transaction ids and internal hashes
    hashes: Vec<TxMerkleNode>,
}

impl Encodable for PartialMerkleTree {
    fn consensus_encode<W: io::Write + ?Sized>(&self, w: &mut W) -> Result<usize, io::Error> {
        let ret = self.num_transactions.consensus_encode(w)? + self.hashes.consensus_encode(w)?;
        let mut bytes: Vec<u8> = vec![0; (self.bits.len() + 7) / 8];
        for p in 0..self.bits.len() {
            bytes[p / 8] |= (self.bits[p] as u8) << (p % 8) as u8;
        }
        Ok(ret + bytes.consensus_encode(w)?)
    }
}

impl Decodable for PartialMerkleTree {
    fn consensus_decode<R: io::Read + ?Sized>(r: &mut R) -> Result<Self, encode::Error> {
        let num_transactions: u32 = Decodable::consensus_decode(r)?;
        let hashes: Vec<TxMerkleNode> = Decodable::consensus_decode(r)?;

        let bytes: Vec<u8> = Decodable::consensus_decode(r)?;
        let mut bits: Vec<bool> = vec![false; bytes.len() * 8];

        for (p, bit) in bits.iter_mut().enumerate() {
            *bit = (bytes[p / 8] & (1 << (p % 8) as u8)) != 0;
        }
        Ok(PartialMerkleTree {
            num_transactions,
            hashes,
            bits,
        })
    }
}

#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PackedLockTime(pub u32);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Transaction {
    /// The protocol version, is currently expected to be 1 or 2 (BIP 68).
    pub version: i32,
    /// Block height or timestamp. Transaction cannot be included in a block until this height/time.
    ///
    /// ### Relevant BIPs
    ///
    /// * [BIP-65 OP_CHECKLOCKTIMEVERIFY](https://github.com/bitcoin/bips/blob/master/bip-0065.mediawiki)
    /// * [BIP-113 Median time-past as endpoint for lock-time calculations](https://github.com/bitcoin/bips/blob/master/bip-0113.mediawiki)
    pub lock_time: PackedLockTime,
    /// List of transaction inputs.
    pub input: Vec<TxIn>,
    /// List of transaction outputs.
    pub output: Vec<TxOut>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct TxIn {
    /// The reference to the previous output that is being used an an input.
    pub previous_output: OutPoint,
    /// The script which pushes values on the stack which will cause
    /// the referenced output's script to be accepted.
    pub script_sig: Script,
    /// The sequence number, which suggests to miners which of two
    /// conflicting transactions should be preferred, or 0xFFFFFFFF
    /// to ignore this feature. This is generally never used since
    /// the miner behaviour cannot be enforced.
    pub sequence: Sequence,
    /// Witness data: an array of byte-arrays.
    /// Note that this field is *not* (de)serialized with the rest of the TxIn in
    /// Encodable/Decodable, as it is (de)serialized at the end of the full
    /// Transaction. It *is* (de)serialized with the rest of the TxIn in other
    /// (de)serialization routines.
    pub witness: Witness,
}

#[derive(
    Copy, Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize, Deserialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct OutPoint {
    /// The referenced transaction's txid.
    #[tsify(type = "string")]
    pub txid: Txid,
    /// The index of the referenced output in its transaction's vout.
    pub vout: u32,
}

#[derive(
    Clone, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Script(Box<[u8]>);

#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Sequence(pub u32);

#[derive(
    Clone, Default, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify,
)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Witness {
    /// contains the witness Vec<Vec<u8>> serialization without the initial varint indicating the
    /// number of elements (which is stored in `witness_elements`)
    content: Vec<u8>,

    /// Number of elements in the witness.
    /// It is stored separately (instead of as VarInt in the initial part of content) so that method
    /// like [`Witness::push`] doesn't have case requiring to shift the entire array
    witness_elements: usize,

    /// If `witness_elements > 0` it's a valid index pointing to the last witness element in `content`
    /// (Including the varint specifying the length of the element)
    last: usize,

    /// If `witness_elements > 1` it's a valid index pointing to the second-to-last witness element in `content`
    /// (Including the varint specifying the length of the element)
    second_to_last: usize,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct TxOut {
    /// The value of the output, in satoshis.
    pub value: u64,
    /// The script which must be satisfied for the output to be spent.
    pub script_pubkey: Script,
}
