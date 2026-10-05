//! Conversion of state value-pool accounting into REST response values.

use zebra_chain::{
    amount::{Amount, NegativeAllowed, NonNegative, COIN},
    value_balance::ValueBalance,
};

use crate::{types::ValuePoolBalance, Error};

pub(super) fn responses(
    current: ValueBalance<NonNegative>,
    previous: Option<ValueBalance<NonNegative>>,
) -> Result<(ValuePoolBalance, Vec<ValuePoolBalance>), Error> {
    let current_signed = current
        .constrain::<NegativeAllowed>()
        .map_err(|error| Error::Calculation(error.to_string()))?;
    let previous_signed = previous
        .unwrap_or_else(ValueBalance::zero)
        .constrain::<NegativeAllowed>()
        .map_err(|error| Error::Calculation(error.to_string()))?;
    let delta = (current_signed - previous_signed)
        .map_err(|error| Error::Calculation(error.to_string()))?;

    let pools = [
        pool(
            "transparent",
            current.transparent_amount(),
            delta.transparent_amount(),
        ),
        pool("sprout", current.sprout_amount(), delta.sprout_amount()),
        pool("sapling", current.sapling_amount(), delta.sapling_amount()),
        pool("orchard", current.orchard_amount(), delta.orchard_amount()),
        pool(
            "lockbox",
            current.deferred_amount(),
            delta.deferred_amount(),
        ),
        pool(
            "ironwood",
            current.ironwood_amount(),
            delta.ironwood_amount(),
        ),
        pool(
            "staking_bonded",
            current.staking_bonded_amount(),
            delta.staking_bonded_amount(),
        ),
        pool(
            "staking_unbonded",
            current.staking_unbonded_amount(),
            delta.staking_unbonded_amount(),
        ),
        pool(
            "finalizer_rewards",
            current.finalizer_rewards_amount(),
            delta.finalizer_rewards_amount(),
        ),
    ];

    let total_zatoshis = current
        .total()
        .map_err(|error| Error::Calculation(error.to_string()))?
        .zatoshis();
    let chain_supply = ValuePoolBalance {
        id: None,
        chain_value: zec(total_zatoshis),
        chain_value_zat: total_zatoshis.to_string(),
        monitored: total_zatoshis != 0,
        value_delta: None,
        value_delta_zat: None,
    };

    Ok((chain_supply, pools.into_iter().collect()))
}

fn pool(
    id: &str,
    current: Amount<NonNegative>,
    delta: Amount<NegativeAllowed>,
) -> ValuePoolBalance {
    let current = current.zatoshis();
    let delta = delta.zatoshis();
    ValuePoolBalance {
        id: Some(id.to_string()),
        chain_value: zec(current),
        chain_value_zat: current.to_string(),
        monitored: current != 0,
        value_delta: Some(zec(delta)),
        value_delta_zat: Some(delta.to_string()),
    }
}

fn zec(zatoshis: i64) -> f64 {
    // Both values are below 2^53, so these integer-to-f64 conversions are exact.
    zatoshis as f64 / COIN as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_include_crosslink_staking_pools_in_chain_supply() {
        let mut current = ValueBalance::zero();
        current.set_staking_bonded_amount(Amount::try_from(3).unwrap());
        current.set_staking_unbonded_amount(Amount::try_from(5).unwrap());
        current.set_finalizer_rewards_amount(Amount::try_from(7).unwrap());

        let (chain_supply, pools) = responses(current, None).unwrap();
        let pool_ids = pools
            .iter()
            .map(|pool| pool.id.as_deref().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(chain_supply.chain_value_zat, "15");
        assert_eq!(
            pool_ids,
            [
                "transparent",
                "sprout",
                "sapling",
                "orchard",
                "lockbox",
                "ironwood",
                "staking_bonded",
                "staking_unbonded",
                "finalizer_rewards",
            ]
        );
    }
}
