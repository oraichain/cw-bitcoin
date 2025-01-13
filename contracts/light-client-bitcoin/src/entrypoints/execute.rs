use common_bitcoin::error::ContractResult;
use cosmwasm_std::{Addr, MessageInfo, Response, Storage};

use crate::{
    header::{HeaderList, HeaderQueue},
    state::CONFIG,
};
use light_client_bitcoin::{header::WrappedHeader, interface::HeaderConfig};

pub fn relay_headers(
    store: &mut dyn Storage,
    headers: Vec<WrappedHeader>,
) -> ContractResult<Response> {
    let mut header_queue = HeaderQueue::default();
    header_queue.add(store, HeaderList::from(headers))?;
    Ok(Response::new().add_attribute("action", "add_headers"))
}

pub fn update_header_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    config: HeaderConfig,
) -> ContractResult<Response> {
    assert_eq!(info.sender, CONFIG.load(store)?.owner);
    let mut header_queue = HeaderQueue::default();
    header_queue.configure(store, config.clone())?;
    Ok(Response::new().add_attribute("action", "update_header_config"))
}

pub fn update_config(
    store: &mut dyn Storage,
    info: MessageInfo,
    owner: Option<Addr>,
) -> ContractResult<Response> {
    let mut config = CONFIG.load(store)?;
    assert_eq!(info.sender, config.owner);

    if let Some(owner) = owner {
        config.owner = owner;
    }

    CONFIG.save(store, &config)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}
