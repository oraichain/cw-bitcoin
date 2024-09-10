use crate::constants::MAX_HEADERS_RELAY_ONE_TIME;
use crate::state::header_height;
use crate::state::CURRENT_WORK;
use crate::state::HEADERS;
use crate::state::HEADER_CONFIG;
use bitcoin::blockdata::block::BlockHeader;
use common_bitcoin::adapter::Adapter;
use common_bitcoin::error::ContractError;
use common_bitcoin::error::ContractResult;
use light_client_bitcoin::header::{WorkHeader, WrappedHeader};
use light_client_bitcoin::interface::HeaderConfig;
use std::collections::HashMap;

use bitcoin::util::uint::Uint256;
use bitcoin::util::BitArray;
use bitcoin::BlockHash;
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::Storage;
// use ed::Terminated;

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
#[derive(Default)]
pub struct HeaderQueue {}

impl HeaderQueue {
    pub fn config(&self, store: &dyn Storage) -> ContractResult<HeaderConfig> {
        let config = HEADER_CONFIG.load(store)?;
        Ok(config)
    }

    pub fn current_work(&self, store: &dyn Storage) -> ContractResult<Adapter<Uint256>> {
        let work = CURRENT_WORK.load(store)?;
        Ok(work)
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
        let config = self.config(store)?;

        let first = headers
            .first()
            .ok_or_else(|| ContractError::Header("Passed header list empty".into()))?;

        let mut removed_work = Uint256::default();
        if first.height <= current_height {
            let first_replaced = self
                .get_by_height(store, first.height, None)?
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
        let mut queue_len = self.len(store)?;
        let mut current_work = *CURRENT_WORK.load(store)?;
        while queue_len > config.max_length {
            let header = match HEADERS.pop_front(store)? {
                Some(inner) => inner,
                None => {
                    break;
                }
            };
            queue_len -= 1;

            // TODO: do we really want to subtract work when pruning?
            current_work = current_work - header.work();
        }
        CURRENT_WORK.save(store, &Adapter::new(current_work))?;
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

        // get header right before first header of headers (which are going to be relayed)
        let prev_header = [self
            .get_by_height(store, first_height - 1, None)?
            .ok_or_else(|| ContractError::Header("Headers not connect to chain".into()))?
            .header];

        // create tupple of headers
        // [prev_header, headers[0], headers[1], ...]
        // [headers[0], headers[1], headers[2]...]
        let headers = prev_header.iter().chain(headers.iter()).zip(headers.iter());

        let mut work = Uint256::zero();

        let mut cache_headers_map = HashMap::new();
        for (prev_header, header) in headers {
            // prove: prev_header and header are adjacent
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

            // make sure header is <= median timestamp of last 11 headers
            if HEADERS.len(store)? >= 11 {
                self.validate_time(store, header)?;
            }

            let initial_height = self.get_initial_height(store)?;

            let target = self.get_next_target(
                store,
                header,
                prev_header,
                initial_height,
                &mut cache_headers_map,
            )?;
            header.validate_pow(&target)?;

            let header_work = header.work();
            work = work + header_work;

            let chain_work = *self.current_work(store)? + header_work;
            let work_header = WorkHeader::new(header.clone(), chain_work);
            HEADERS.push_back(store, &work_header)?;
            // self.current_work = Adapter::new(chain_work);
            CURRENT_WORK.save(store, &Adapter::new(chain_work))?;
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
        initial_height: u32,
        cache_headers_map: &mut HashMap<u32, u32>,
    ) -> ContractResult<Uint256> {
        let config = self.config(store)?;
        if header.height() % config.retarget_interval == 0 {
            let first_reorg_height = header.height() - config.retarget_interval;
            return self.calculate_next_target(store, previous_header, first_reorg_height);
        }

        if !config.min_difficulty_blocks {
            return Ok(previous_header.target());
        }

        if header.time() > previous_header.time() + config.target_spacing * 2 {
            return Ok(WrappedHeader::u256_from_compact(config.max_target));
        }

        let mut current_header_height = previous_header.height();
        let mut current_bits = previous_header.bits();

        while current_header_height > 0
            && current_header_height % config.retarget_interval != 0
            && current_bits == config.max_target
        {
            current_header_height -= 1;

            current_bits = match cache_headers_map.get(&current_header_height) {
                Some(val) => *val,
                None => {
                    cache_headers_map.insert(current_header_height, current_bits);
                    HEADERS
                        .get(store, current_header_height - initial_height)?
                        .ok_or_else(|| ContractError::Header("No previous header exists".into()))?
                        .header
                        .bits()
                }
            }
        }
        Ok(WrappedHeader::u256_from_compact(current_bits))
    }

    /// Calculate the expected next target based on the passed header and the
    /// height of the previous retargeting header.
    fn calculate_next_target(
        &self,
        store: &dyn Storage,
        header: &WrappedHeader,
        first_reorg_height: u32,
    ) -> ContractResult<Uint256> {
        let config = self.config(store)?;

        if !config.retargeting {
            return Ok(WrappedHeader::u256_from_compact(header.bits()));
        }

        if header.height() < config.retarget_interval {
            return Err(ContractError::Header("Invalid trusted header. Trusted header have height which is a multiple of the retarget interval".into()));
        }

        let prev_retarget = match self.get_by_height(store, first_reorg_height, None)? {
            Some(inner) => inner.time(),
            None => {
                return Err(ContractError::Header(
                    "No previous retargeting header exists".into(),
                ));
            }
        };

        let timespan = (header.time() - prev_retarget)
            .clamp(config.target_timespan / 4, config.target_timespan * 4);

        let target_timespan = WrappedHeader::u32_to_u256(config.target_timespan);
        let timespan = WrappedHeader::u32_to_u256(timespan);

        let target = header.target() * timespan / target_timespan;
        let target_u32 = BlockHeader::compact_target_from_u256(&target);
        let target = WrappedHeader::u256_from_compact(target_u32);

        Ok(target.min(WrappedHeader::u256_from_compact(config.max_target)))
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
        let initial_height = self.get_initial_height(store)?;
        let height = self.height(store)?;
        for prev_height in height - 10..=height {
            let current_item = match self.get_by_height(store, prev_height, Some(initial_height))? {
                Some(inner) => inner.time(),
                None => {
                    return Err(ContractError::Header(
                        "Deque does not contain any elements".into(),
                    ))
                }
            };
            prev_stamps.push(current_item);
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
    pub fn len(&self, store: &dyn Storage) -> ContractResult<u64> {
        Ok(HEADERS.len(store).unwrap_or(0) as u64)
    }

    /// Whether or not the header queue is empty.
    ///
    /// This will always return `false`, as the header queue is initialized with
    /// a trusted header.
    pub fn is_empty(&self, store: &dyn Storage) -> ContractResult<bool> {
        Ok(self.len(store)? == 0)
    }

    pub fn get_initial_height(&self, store: &dyn Storage) -> ContractResult<u32> {
        match HEADERS.front(store)? {
            Some(inner) => Ok(inner.height()),
            None => {
                return Err(ContractError::Header(
                    "Queue does not contain any headers".into(),
                ))
            }
        }
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
        initial_height: Option<u32>,
    ) -> ContractResult<Option<WorkHeader>> {
        let initial_height = match initial_height {
            Some(val) => val,
            None => self.get_initial_height(store)?,
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
    pub fn trusted_height(&self, store: &dyn Storage) -> ContractResult<u32> {
        let config = HEADER_CONFIG.load(store)?;
        Ok(config.trusted_height)
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
        let work = wrapped_header.work();
        let work_header = WorkHeader::new(wrapped_header, work);
        CURRENT_WORK.save(store, &work_header.chain_work)?;
        HEADERS.push_front(store, &work_header)?;
        HEADER_CONFIG.save(store, &config)?;
        Ok(())
    }

    /// The network the header queue is configured for.
    pub fn network(&self) -> bitcoin::Network {
        // TODO: should be dynamic, from config
        #[cfg(feature = "mainnet")]
        return bitcoin::Network::Bitcoin;

        #[cfg(not(feature = "mainnet"))]
        return bitcoin::Network::Testnet;
    }
}
