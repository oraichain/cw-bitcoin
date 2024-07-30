use crate::adapter::Adapter;
use crate::constants::MAX_HEADERS_RELAY_ONE_TIME;
use crate::error::ContractError;
use crate::error::ContractResult;
use crate::interface::HeaderConfig;
use crate::state::header_height;
use crate::state::HEADERS;
use bitcoin::blockdata::block::BlockHeader;

use bitcoin::util::uint::Uint256;
use bitcoin::BlockHash;
use bitcoin::TxMerkleNode;
use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::Storage;
// use ed::Terminated;

/// A wrapper around a bitcoin::BlockHeader that implements the core orga
/// traits, and includes the block's height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "cosmwasm_schema::serde")]
#[schemars(crate = "cosmwasm_schema::schemars")]
pub struct WrappedHeader {
    height: u32,
    header: Adapter<BlockHeader>,
}

impl WrappedHeader {
    /// Create a new WrappedHeader from an Adapter<bitcoin::BlockHeader> and a
    /// height.
    pub fn new(header: Adapter<BlockHeader>, height: u32) -> Self {
        WrappedHeader { height, header }
    }

    /// Create a new WrappedHeader from a bitcoin::BlockHeader and a height.
    pub fn from_header(header: &BlockHeader, height: u32) -> Self {
        WrappedHeader {
            height,
            header: Adapter::new(*header),
        }
    }

    /// The timestamp of the block header.
    pub fn time(&self) -> u32 {
        self.header.time
    }

    /// The target - the value the hash must be less than to be valid
    /// proof-of-work.
    pub fn target(&self) -> Uint256 {
        self.header.target()
    }

    /// The block hash.
    pub fn block_hash(&self) -> BlockHash {
        self.header.block_hash()
    }

    /// The previous block hash.
    pub fn prev_blockhash(&self) -> BlockHash {
        self.header.prev_blockhash
    }

    /// The total estimated number of work (measured in hashes) represented by
    /// the block header's proof-of-work.
    pub fn work(&self) -> Uint256 {
        self.header.work()
    }

    /// The height of the block header.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The target, in compact form.
    pub fn bits(&self) -> u32 {
        self.header.bits
    }

    /// Converts a compact target to a Uint256.
    pub fn u256_from_compact(compact: u32) -> Uint256 {
        BlockHeader::u256_from_compact_target(compact)
    }

    /// Converts a Uint256 to a compact target.
    pub fn compact_target_from_u256(target: &Uint256) -> u32 {
        BlockHeader::compact_target_from_u256(target)
    }

    /// Converts a u32 to a Uint256.
    fn u32_to_u256(value: u32) -> Uint256 {
        let bytes = value.to_be_bytes();
        let mut buffer = [0u8; 32];
        buffer[32 - bytes.len()..].copy_from_slice(&bytes);

        Uint256::from_be_bytes(buffer)
    }

    /// Validates the proof-of-work of the block header, returning an error if
    /// the proof-of-work is invalid.
    fn validate_pow(&self, required_target: &Uint256) -> ContractResult<BlockHash> {
        Ok(self.header.validate_pow(required_target)?)
    }
}

/// A list of WrappedHeaders.
// TODO: remove this in favor of e.g. `LengthVec<u8, WrappedHeader>`
#[derive(Debug, Clone)]
pub struct HeaderList(Vec<WrappedHeader>);

impl From<Vec<WrappedHeader>> for HeaderList {
    fn from(headers: Vec<WrappedHeader>) -> Self {
        HeaderList(headers)
    }
}

impl From<HeaderList> for Vec<WrappedHeader> {
    fn from(headers: HeaderList) -> Self {
        headers.0
    }
}

impl FromIterator<WrappedHeader> for HeaderList {
    fn from_iter<T: IntoIterator<Item = WrappedHeader>>(iter: T) -> Self {
        HeaderList(iter.into_iter().collect())
    }
}

// impl Terminated for HeaderList {}

/// A `WrappedHeader`, along with a total estimated amount of work (measured in
/// hashes) done in the header and previous headers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "cosmwasm_schema::serde")]
#[schemars(crate = "cosmwasm_schema::schemars")]
pub struct WorkHeader {
    pub chain_work: Adapter<Uint256>,
    pub header: WrappedHeader,
}

impl WorkHeader {
    /// Create a new `WorkHeader`` from a `WrappedHeader` and a `Uint256`.
    pub fn new(header: WrappedHeader, chain_work: Uint256) -> WorkHeader {
        WorkHeader {
            header,
            chain_work: Adapter::new(chain_work),
        }
    }

    /// The timestamp of the block header.
    pub fn time(&self) -> u32 {
        self.header.time()
    }

    /// The target - the value the hash must be less than to be valid
    /// proof-of-work.
    pub fn block_hash(&self) -> BlockHash {
        self.header.block_hash()
    }

    /// The estimated amount of work (measured in hashes) done in the header,
    /// not including work done in any previous headers.
    pub fn work(&self) -> Uint256 {
        self.header.work()
    }

    /// The height of the block header.
    pub fn height(&self) -> u32 {
        self.header.height()
    }

    /// The Merkle root of the block header.
    pub fn merkle_root(&self) -> TxMerkleNode {
        self.header.header.merkle_root
    }
}

/// A queue of Bitcoin block headers, along with the total estimated amount of
/// work (measured in hashes) done in the headers included in the queue.
///
/// The header queue is used to validate headers as they are received from the
/// Bitcoin network, ensuring each header is associated with a valid
/// proof-of-work and that the chain of headers is valid.
///
/// The queue is able to reorg if a new chain of headers is received that
/// contains more work than the current chain, however it can not process reorgs
/// that are deeper than the length of the queue (the length will be at the
/// configured pruning level based on the `max_length` config parameter).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "cosmwasm_schema::serde")]
pub struct HeaderQueue {
    pub(crate) current_work: Adapter<Uint256>,
    pub(crate) config: HeaderConfig,
}

impl HeaderQueue {
    pub fn new(config: HeaderConfig) -> Self {
        let current_work = Adapter::new(config.work_header().work());
        Self {
            current_work,
            config,
        }
    }

    /// Verify and add a list of headers to the header queue.
    ///
    /// The headers must be consecutive and must bring the chain to a final
    /// state that has more work than the current chain.
    ///
    /// If the headers are valid, they will be added to the header queue. If the
    /// headers start from a height lower than the current height, the header
    /// queue will be reorged to the new chain.
    ///
    /// If the headers are invalid (e.g. by not including a valid proof-of-work,
    /// using a difficulty other than what was expected, using invalid
    /// timestamps, etc.), an error will be returned and the header queue will
    /// not be modified.    
    pub fn add(&mut self, store: &mut dyn Storage, headers: HeaderList) -> ContractResult<()> {
        let headers: Vec<_> = headers.into();

        if headers.len() as u64 > MAX_HEADERS_RELAY_ONE_TIME {
            return Err(ContractError::App(
                "Exceeded maximum amount of relayed headers".to_string(),
            ));
        }

        self.add_into_iter(store, headers)
            .map_err(|err| ContractError::App(err.to_string()))
    }

    /// Verify and add an iterator of headers to the header queue.
    ///
    /// The headers must be consecutive and must bring the chain to a final
    /// state that has more work than the current chain.
    ///
    /// If the headers are valid, they will be added to the header queue. If the
    /// headers start from a height lower than the current height, the header
    /// queue will be reorged to the new chain.
    ///
    /// If the headers are invalid (e.g. by not including a valid proof-of-work,
    /// using a difficulty other than what was expected, using invalid
    /// timestamps, etc.), an error will be returned and the header queue will
    /// not be modified.
    pub fn add_into_iter<T>(&mut self, store: &mut dyn Storage, headers: T) -> ContractResult<()>
    where
        T: IntoIterator<Item = WrappedHeader>,
    {
        let headers: Vec<WrappedHeader> = headers.into_iter().collect();
        let current_height = self.height(store)?;

        let first = headers
            .first()
            .ok_or_else(|| ContractError::Header("Passed header list empty".into()))?;

        let mut removed_work = Uint256::default();
        if first.height <= current_height {
            let first_replaced = self
                .get_by_height(store, first.height)?
                .ok_or_else(|| ContractError::Header("Header not found".into()))?;

            if first_replaced.block_hash() == first.block_hash() {
                return Err(ContractError::Header("Provided redundant header.".into()));
            }

            removed_work = self.pop_back_to(store, first.height)?;
        }

        let added_work = self.verify_and_add_headers(store, &headers)?;
        if added_work <= removed_work {
            return Err(ContractError::Header(
                "New best chain must include more work than old best chain.".into(),
            ));
        }

        // Prune the header queue if it has grown too large.
        let mut queue_len = self.len(store);
        while queue_len > self.config.max_length {
            let header = match HEADERS.pop_front(store)? {
                Some(inner) => inner,
                None => {
                    break;
                }
            };
            queue_len -= 1;
            // TODO: do we really want to subtract work when pruning?
            let current_work = *self.current_work - header.work();
            self.current_work = Adapter::new(current_work);
        }

        Ok(())
    }

    /// Verify and add a list of headers to the header queue, returning the
    /// amount of additional estimated work added to the header queue.
    fn verify_and_add_headers(
        &mut self,
        store: &mut dyn Storage,
        headers: &[WrappedHeader],
    ) -> ContractResult<Uint256> {
        let first_height = headers
            .first()
            .ok_or_else(|| ContractError::Header("Passed header list is empty".into()))?
            .height;
        if first_height == 0 {
            return Err(ContractError::Header(
                "Headers must start after height 0".into(),
            ));
        }

        let prev_header = [self
            .get_by_height(store, first_height - 1)?
            .ok_or_else(|| ContractError::Header("Headers not connect to chain".into()))?
            .header];

        let headers = prev_header.iter().chain(headers.iter()).zip(headers.iter());

        let mut work = Uint256::default();

        // println!("headers: {:?}", headers);

        for (prev_header, header) in headers {
            if header.height() != prev_header.height() + 1 {
                return Err(ContractError::Header(
                    "Non-consecutive headers passed".into(),
                ));
            }

            if header.prev_blockhash() != prev_header.block_hash() {
                #[cfg(debug_assertions)]
                println!(
                    "header.prev_blockhash(): {:?}\nprev_header.block_hash(): {:?}",
                    header, prev_header
                );

                return Err(ContractError::Header(
                    "Passed header references incorrect previous block hash".into(),
                ));
            }

            if HEADERS.len(store)? >= 11 {
                self.validate_time(store, header)?;
            }

            let target = self.get_next_target(store, header, prev_header)?;
            header.validate_pow(&target)?;

            let header_work = header.work();
            work = work + header_work;

            let chain_work = *self.current_work + header_work;
            let work_header = WorkHeader::new(header.clone(), chain_work);
            HEADERS.push_back(store, &work_header)?;
            self.current_work = Adapter::new(chain_work);
        }

        Ok(work)
    }

    /// Calculate the expected next target based on the passed header and the
    /// previous header.
    fn get_next_target(
        &self,
        store: &dyn Storage,
        header: &WrappedHeader,
        previous_header: &WrappedHeader,
    ) -> ContractResult<Uint256> {
        if header.height() % self.config.retarget_interval == 0 {
            let first_reorg_height = header.height() - self.config.retarget_interval;
            return self.calculate_next_target(store, previous_header, first_reorg_height);
        }

        if !self.config.min_difficulty_blocks {
            return Ok(previous_header.target());
        }

        if header.time() > previous_header.time() + self.config.target_spacing * 2 {
            return Ok(WrappedHeader::u256_from_compact(self.config.max_target));
        }

        let mut current_header_index = previous_header.height();
        let mut current_header = previous_header.to_owned();

        while current_header_index > 0
            && current_header_index % self.config.retarget_interval != 0
            && current_header.bits() == self.config.max_target
        {
            current_header_index -= 1;

            current_header = match self.get_by_height(store, current_header_index)? {
                Some(inner) => inner.header.clone(),
                None => {
                    return Err(ContractError::Header("No previous header exists".into()));
                }
            };
        }
        Ok(WrappedHeader::u256_from_compact(current_header.bits()))
    }

    /// Calculate the expected next target based on the passed header and the
    /// height of the previous retargeting header.
    fn calculate_next_target(
        &self,
        store: &dyn Storage,
        header: &WrappedHeader,
        first_reorg_height: u32,
    ) -> ContractResult<Uint256> {
        if !self.config.retargeting {
            return Ok(WrappedHeader::u256_from_compact(header.bits()));
        }

        if header.height() < self.config.retarget_interval {
            return Err(ContractError::Header("Invalid trusted header. Trusted header have height which is a multiple of the retarget interval".into()));
        }

        let prev_retarget = match self.get_by_height(store, first_reorg_height)? {
            Some(inner) => inner.time(),
            None => {
                return Err(ContractError::Header(
                    "No previous retargeting header exists".into(),
                ));
            }
        };

        let mut timespan = header.time() - prev_retarget;

        if timespan < self.config.target_timespan / 4 {
            timespan = self.config.target_timespan / 4;
        }

        if timespan > self.config.target_timespan * 4 {
            timespan = self.config.target_timespan * 4;
        }

        let target_timespan = WrappedHeader::u32_to_u256(self.config.target_timespan);
        let timespan = WrappedHeader::u32_to_u256(timespan);

        let target = header.target() * timespan / target_timespan;
        let target_u32 = BlockHeader::compact_target_from_u256(&target);
        let target = WrappedHeader::u256_from_compact(target_u32);

        if target > WrappedHeader::u256_from_compact(self.config.max_target) {
            Ok(WrappedHeader::u256_from_compact(self.config.max_target))
        } else {
            Ok(target)
        }
    }

    /// Remove headers from the header queue until the height of the last header
    /// in the queue is equal to the passed height.
    fn pop_back_to(&mut self, store: &mut dyn Storage, height: u32) -> ContractResult<Uint256> {
        let mut work = Uint256::default();

        while self.height(store)? >= height {
            let header = HEADERS
                .pop_back(store)?
                .ok_or_else(|| ContractError::Header("Removed all headers".into()))?;

            work = work + header.work();
        }

        Ok(work)
    }

    /// Validate the timestamp of the passed header.
    fn validate_time(
        &self,
        store: &dyn Storage,
        current_header: &WrappedHeader,
    ) -> ContractResult<()> {
        let mut prev_stamps: Vec<u32> = Vec::with_capacity(11);

        for i in 0..11 {
            let index = self.height(store)? - i;

            let current_item = match self.get_by_height(store, index)? {
                Some(inner) => inner,
                None => {
                    return Err(ContractError::Header(
                        "Deque does not contain any elements".into(),
                    ))
                }
            };
            prev_stamps.push(current_item.time());
        }

        prev_stamps.sort_unstable();

        let median_stamp = match prev_stamps.get(5) {
            Some(inner) => inner,
            None => {
                return Err(ContractError::Header(
                    "Median timestamp does not exist".into(),
                ));
            }
        };

        if current_header.time() <= *median_stamp {
            return Err(ContractError::Header(
                "Header contains an invalid timestamp".into(),
            ));
        }

        // TODO: compare timestamps with max_time_increase over the current
        // clock time (not the previous header's time)
        // if max(current_header.time(), previous_header.time())
        //     - min(current_header.time(), previous_header.time())
        //     > self.config.max_time_increase
        // {
        //     return Err(ContractError::Header(
        //         "Timestamp is too far ahead of previous timestamp".into(),
        //     ));
        // }

        Ok(())
    }

    /// The height of the last header in the header queue.    
    pub fn height(&self, store: &dyn Storage) -> ContractResult<u32> {
        header_height(store)
    }

    /// The hash of the last header in the header queue.    
    pub fn hash(&self, store: &dyn Storage) -> ContractResult<BlockHash> {
        match HEADERS.back(store)? {
            Some(inner) => Ok(inner.block_hash()),
            None => Err(ContractError::Header("HeaderQueue is empty".into())),
        }
    }

    /// The number of headers in the header queue.
    // TODO: remove this attribute, not sure why clippy is complaining when is_empty is defined
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self, store: &dyn Storage) -> u64 {
        HEADERS.len(store).unwrap_or(0) as u64
    }

    /// Whether or not the header queue is empty.
    ///
    /// This will always return `false`, as the header queue is initialized with
    /// a trusted header.
    pub fn is_empty(&self, store: &dyn Storage) -> bool {
        self.len(store) == 0
    }

    /// Get a header from the header queue by its height.
    ///
    /// If the header queue does not contain a header at the passed height,
    /// `None` will be returned.
    ///
    /// If the passed height is less than the initial height of the header queue,
    /// an error will be returned.    
    pub fn get_by_height(
        &self,
        store: &dyn Storage,
        height: u32,
    ) -> ContractResult<Option<WorkHeader>> {
        let initial_height = match HEADERS.front(store)? {
            Some(inner) => inner.height(),
            None => {
                return Err(ContractError::Header(
                    "Queue does not contain any headers".into(),
                ))
            }
        };

        if height < initial_height {
            // TODO: error message is wrong
            // TODO: shouldn't this just return None?
            return Err(ContractError::Header(
                "Passed index is greater than initial height. Referenced header does not exist on the Header Queue".into(),
            ));
        }

        let header = HEADERS.get(store, height - initial_height)?;
        Ok(header)
    }

    /// The height of the configured trusted header.    
    pub fn trusted_height(&self) -> u32 {
        self.config.trusted_height
    }

    /// Clears the header queue and configures it with the passed config,
    /// adding the trusted header to the queue.
    pub fn configure(
        &mut self,
        store: &mut dyn Storage,
        config: HeaderConfig,
    ) -> ContractResult<()> {
        HEADERS.clear(store)?;
        let wrapped_header = WrappedHeader::new(config.trusted_header, config.trusted_height);
        let work_header = WorkHeader::new(wrapped_header.clone(), wrapped_header.work());

        self.current_work = work_header.chain_work;

        HEADERS.push_front(store, &work_header)?;

        self.config = config;

        Ok(())
    }

    /// The network the header queue is configured for.
    pub fn network(&self) -> bitcoin::Network {
        // TODO: should be dynamic, from config
        bitcoin::Network::Bitcoin
    }
}
