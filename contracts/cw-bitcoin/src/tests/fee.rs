use crate::{
    error::ContractResult,
    fee::process_deduct_fee,
    state::{Ratio, CONFIG, TOKEN_FEE_RATIO},
};
use cosmwasm_std::{testing::mock_dependencies, Addr, Coin, Uint128};
use oraiswap::asset::AssetInfo;

#[test]
fn test_fee_collected() -> ContractResult<()> {
    let mut deps = mock_dependencies();
    CONFIG.save(
        deps.as_mut().storage,
        &crate::msg::Config {
            owner: Addr::unchecked("owner"),
            token_factory_addr: Addr::unchecked("token_factory_addr"),
            relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
            token_fee_receiver: Addr::unchecked("token_fee_receiver"),
            relayer_fee_token: AssetInfo::NativeToken {
                denom: "orai".to_string(),
            },
            relayer_fee: Uint128::from(0u128),
            swap_router_contract: None,
            osor_entry_point_contract: None,
        },
    )?;
    TOKEN_FEE_RATIO.save(
        deps.as_mut().storage,
        &Ratio {
            nominator: 1,
            denominator: 1000,
        },
    )?;
    let fee = process_deduct_fee(
        deps.as_ref().storage,
        &deps.as_ref().querier,
        deps.as_ref().api,
        Coin {
            denom: "btc".to_string(),
            amount: Uint128::from(5000u128),
        },
    )?;
    assert_eq!(fee.deducted_amount, Uint128::from(4995u128));
    assert_eq!(fee.token_fee.amount, Uint128::from(5u128));
    assert_eq!(fee.relayer_fee.amount, Uint128::from(0u128));

    TOKEN_FEE_RATIO.save(
        deps.as_mut().storage,
        &Ratio {
            nominator: 0,
            denominator: 1000,
        },
    )?;
    let fee = process_deduct_fee(
        deps.as_ref().storage,
        &deps.as_ref().querier,
        deps.as_ref().api,
        Coin {
            denom: "btc".to_string(),
            amount: Uint128::from(5000u128),
        },
    )?;
    assert_eq!(fee.deducted_amount, Uint128::from(5000u128));
    assert_eq!(fee.token_fee.amount, Uint128::from(0u128));
    assert_eq!(fee.relayer_fee.amount, Uint128::from(0u128));

    TOKEN_FEE_RATIO.save(
        deps.as_mut().storage,
        &Ratio {
            nominator: 1000,
            denominator: 0,
        },
    )?;
    let fee = process_deduct_fee(
        deps.as_ref().storage,
        &deps.as_ref().querier,
        deps.as_ref().api,
        Coin {
            denom: "btc".to_string(),
            amount: Uint128::from(5000u128),
        },
    )?;
    assert_eq!(fee.deducted_amount, Uint128::from(5000u128));
    assert_eq!(fee.token_fee.amount, Uint128::from(0u128));
    assert_eq!(fee.relayer_fee.amount, Uint128::from(0u128));
    Ok(())
}
