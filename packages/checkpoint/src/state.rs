use cw_storage_plus::Map;

use crate::adapter::Adapter;

pub const RECOVERY_SCRIPTS: Map<String, Adapter<bitcoin::Script>> = Map::new("recovery_scripts");
