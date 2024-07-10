use super::signatory::Signatory;
use super::signatory::SignatorySet;
use super::SIGSET_THRESHOLD;
use crate::error::ContractResult;
use crate::interface::Dest;
use crate::utils::time_now;
use bitcoin::hashes::hex::ToHex;
use log::info;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use tsify::Tsify;

use wasm_bindgen::prelude::*;

#[derive(Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct DepositsQuery {
    pub receiver: String,
}

#[derive(Serialize, Deserialize, Clone, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct DepositAddress {
    pub sigset_index: u32,
    pub deposit_addr: String,
}

#[derive(Serialize, Deserialize, Clone, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Sigset {
    pub sigset_index: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RawSignatorySet {
    pub signatories: Vec<RawSignatory>,
    pub index: u32,
    #[serde(rename = "bridgeFeeRate")]
    pub bridge_fee_rate: f64,
    #[serde(rename = "minerFeeRate")]
    pub miner_fee_rate: f64,
    #[serde(rename = "depositsEnabled")]
    pub deposits_enabled: bool,
    pub threshold: (u64, u64),
}

impl RawSignatorySet {
    pub fn new(
        sigset: SignatorySet,
        bridge_fee_rate: f64,
        miner_fee_rate: f64,
        deposits_enabled: bool,
    ) -> Self {
        let signatories = sigset
            .iter()
            .map(|s| RawSignatory::from(s.clone()))
            .collect();

        RawSignatorySet {
            signatories,
            index: sigset.index(),
            bridge_fee_rate,
            miner_fee_rate,
            deposits_enabled,
            threshold: SIGSET_THRESHOLD,
        }
    }
}

#[wasm_bindgen]
pub fn newRawSignatorySet(
    sigset: SignatorySet,
    bridge_fee_rate: f64,
    miner_fee_rate: f64,
    deposits_enabled: bool,
) -> RawSignatorySet {
    RawSignatorySet::new(sigset, bridge_fee_rate, miner_fee_rate, deposits_enabled)
}

#[derive(Clone, Serialize, Deserialize, Debug, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RawSignatory {
    pub voting_power: u64,
    pub pubkey: Vec<u8>,
}

impl From<Signatory> for RawSignatory {
    fn from(sig: Signatory) -> Self {
        RawSignatory {
            voting_power: sig.voting_power,
            pubkey: sig.pubkey.as_slice().to_vec(),
        }
    }
}

/// A collection which stores all watched addresses and signatory sets, for
/// efficiently detecting deposit output scripts.
#[derive(Default, Deserialize, Serialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WatchedScripts {
    scripts: HashMap<String, (Dest, u32)>,
    sigsets: BTreeMap<u32, (SignatorySet, Vec<Dest>)>,
}

impl WatchedScripts {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get(&self, script: &::bitcoin::Script) -> Option<(Dest, u32)> {
        self.scripts.get(&script.to_hex()).cloned()
    }

    pub fn has(&self, script: &::bitcoin::Script) -> bool {
        self.scripts.contains_key(&script.to_hex())
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    pub fn insert(&mut self, dest: Dest, sigset: &SignatorySet) -> ContractResult<bool> {
        let script = self.derive_script(&dest, sigset, SIGSET_THRESHOLD)?;

        if self.scripts.contains_key(&script.to_hex()) {
            return Ok(false);
        }

        self.scripts
            .insert(script.to_hex(), (dest.clone(), sigset.index()));

        let (_, dests) = self
            .sigsets
            .entry(sigset.index())
            .or_insert((sigset.clone(), vec![]));
        dests.push(dest);

        Ok(true)
    }

    pub fn remove_expired(&mut self, max_age: u64) -> ContractResult<()> {
        let now = time_now();

        for (_, (sigset, dests)) in self.sigsets.iter() {
            if now < sigset.create_time() + max_age {
                break;
            }

            for dest in dests {
                info!("preparing to remove dest: {:?}", dest);
                let script = self.derive_script(dest, sigset, SIGSET_THRESHOLD)?; // TODO: get threshold from state
                self.scripts.remove(&script.to_hex());
            }
        }

        Ok(())
    }

    fn derive_script(
        &self,
        dest: &Dest,
        sigset: &SignatorySet,
        threshold: (u64, u64),
    ) -> ContractResult<::bitcoin::Script> {
        sigset.output_script(dest.commitment_bytes()?.as_slice(), threshold)
    }
}

#[derive(Deserialize, Serialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WatchedScriptStore {
    scripts: WatchedScripts,
    file_path: String,
}

impl WatchedScriptStore {
    pub async fn open<P: AsRef<Path>>(path: P) -> ContractResult<Self> {
        let path = path.as_ref().join("watched-addrs.csv");

        let scripts = WatchedScripts::new();

        let tmp_path = path.with_file_name("watched-addrs-tmp.csv");
        let mut tmp_file = File::create(&tmp_path)?;
        for (addr, sigset_index) in scripts.scripts.values() {
            Self::write(&mut tmp_file, addr, *sigset_index)?;
        }
        tmp_file.flush()?;
        drop(tmp_file);
        std::fs::rename(tmp_path, &path)?;

        info!("Keeping track of deposit addresses at {}", path.display());

        Ok(WatchedScriptStore {
            scripts,
            file_path: path.display().to_string(),
        })
    }

    pub fn insert(&mut self, dest: Dest, sigset: &SignatorySet) -> ContractResult<()> {
        if self.scripts.insert(dest.clone(), sigset)? {
            let mut file = File::options()
                .append(true)
                .create(true)
                .open(Path::new(&self.file_path))?;
            Self::write(&mut file, &dest, sigset.index())?;
        }

        Ok(())
    }

    fn write(file: &mut File, dest: &Dest, sigset_index: u32) -> ContractResult<()> {
        writeln!(file, "{},{}", dest.to_receiver_addr(), sigset_index)?;
        file.flush()?;
        Ok(())
    }
}
