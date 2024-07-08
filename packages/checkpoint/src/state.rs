use cw_storage_plus::Map;

use crate::{adapter::Adapter, msg::Xpub};

pub const RECOVERY_SCRIPTS: Map<String, Adapter<bitcoin::Script>> = Map::new("recovery_scripts");

pub const VALIDATORS: Map<&[u8], u64> = Map::new("validators");

pub const SIG_KEYS: Map<&[u8], Xpub> = Map::new("sig_keys");
