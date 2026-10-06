//! Current miner-to-finalizer stake ranking.

use tower::ServiceExt;
use zebra_chain::block::Height;
use zebra_state::{
    ExplorerMinerStakeRankCursor, ExplorerPageDirection, ExplorerReadRequest, ExplorerReadResponse,
    ReadRequest, ReadResponse, ReadState,
};

use crate::{
    types::{
        CrosslinkMinerStakeEntry, CrosslinkMinerStakePagination, CrosslinkMinerStakeRequest,
        CrosslinkMinerStakeResponse, CrosslinkMinerStakeSummary, CrosslinkStakeSourceAmount,
        CrosslinkStakeSourceBreakdown, CrosslinkStakeSourceGroup, PageDirection,
    },
    Error,
};

use super::miner_stake_cursor::MinerStakeCursor;

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

/// Returns current miner-attributed stake globally or for one finalizer.
pub async fn miner_stake_from_state<State>(
    read_state: State,
    network: &zebra_chain::parameters::Network,
    finalizer: Option<[u8; 32]>,
    request: CrosslinkMinerStakeRequest,
) -> Result<CrosslinkMinerStakeResponse, Error>
where
    State: ReadState,
{
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(Error::InvalidQuery(format!(
            "miner-stake limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let cursor = request
        .cursor
        .as_deref()
        .map(MinerStakeCursor::decode)
        .transpose()?;
    if cursor.is_some_and(|cursor| cursor.scope_finalizer != finalizer) {
        return Err(Error::InvalidCursor(
            "miner-stake cursor belongs to a different ranking".to_string(),
        ));
    }
    if request.direction == PageDirection::Previous && cursor.is_none() {
        return Err(Error::InvalidCursor(
            "direction=prev requires a cursor".to_string(),
        ));
    }
    let state_cursor = cursor.map(|cursor| ExplorerMinerStakeRankCursor {
        miner_address: cursor.miner_address,
        finalizer: cursor.finalizer,
        current_stake_zat: cursor.current_stake_zat,
        rank: cursor.rank,
        block_hash: cursor.indexed_block_hash,
    });
    let state_direction = match request.direction {
        PageDirection::Next => ExplorerPageDirection::Older,
        PageDirection::Previous => ExplorerPageDirection::Newer,
    };
    let response = read_state
        .clone()
        .oneshot(ReadRequest::Explorer(ExplorerReadRequest::MinerStakePage {
            finalizer,
            limit,
            cursor: state_cursor,
            direction: state_direction,
        }))
        .await
        .map_err(|error| Error::StateRequest(error.to_string()))?;
    let ReadResponse::Explorer(ExplorerReadResponse::MinerStakePage(page)) = response else {
        return Err(Error::StateResponse(
            "state returned the wrong response for miner stake".to_string(),
        ));
    };
    if !page.cursor_valid {
        return Err(Error::InvalidCursor(
            "miner-stake ranking changed; restart from the first page".to_string(),
        ));
    }

    let has_rows = !page.entries.is_empty();
    let (has_next, has_prev) = match request.direction {
        PageDirection::Next => (page.has_more, cursor.is_some() && has_rows),
        PageDirection::Previous => (cursor.is_some() && has_rows, page.has_more),
    };
    let row_count = u64::try_from(page.entries.len()).expect("miner-stake page size fits in u64");
    let first_rank = match (request.direction, cursor) {
        (PageDirection::Next, Some(cursor)) => cursor
            .rank
            .checked_add(1)
            .ok_or_else(|| Error::InvalidCursor("miner-stake cursor rank overflow".to_string()))?,
        (PageDirection::Next, None) => 1,
        (PageDirection::Previous, Some(cursor)) => cursor
            .rank
            .checked_sub(row_count)
            .filter(|rank| !has_rows || *rank > 0)
            .ok_or_else(|| {
                Error::InvalidCursor("miner-stake cursor rank is invalid".to_string())
            })?,
        (PageDirection::Previous, None) => unreachable!("previous pages require a cursor"),
    };

    let latest_heights = page
        .entries
        .iter()
        .map(|entry| Height(entry.miner_record.latest_height))
        .collect::<Vec<_>>();
    let response = read_state
        .oneshot(ReadRequest::Explorer(ExplorerReadRequest::BlockSummaries(
            latest_heights.into(),
        )))
        .await
        .map_err(|error| Error::StateRequest(error.to_string()))?;
    let ReadResponse::Explorer(ExplorerReadResponse::BlockSummaries(latest_blocks)) = response
    else {
        return Err(Error::StateResponse(
            "state returned the wrong response for miner stake blocks".to_string(),
        ));
    };

    let items = page
        .entries
        .iter()
        .zip(latest_blocks)
        .enumerate()
        .map(|(offset, (entry, block))| {
            let block = block.ok_or_else(|| {
                Error::StateResponse(
                    "a miner stake row's latest miner block is missing".to_string(),
                )
            })?;
            let coinbase = block.block.transactions.first().ok_or_else(|| {
                Error::CorruptData("a canonical miner block has no coinbase".to_string())
            })?;
            let address = entry.miner_address.to_string();
            let (identified_address, identified_pool) =
                super::miner_attribution::identify_miner(coinbase, network);
            let pool = if identified_address.as_deref() == Some(address.as_str()) {
                identified_pool
            } else {
                super::miner_attribution::pool_from_address(&address)
                    .unwrap_or("Unknown")
                    .to_string()
            };
            Ok(CrosslinkMinerStakeEntry {
                rank: first_rank
                    .checked_add(u64::try_from(offset).expect("page offset fits in u64"))
                    .ok_or_else(|| {
                        Error::InvalidCursor("miner-stake cursor rank overflow".to_string())
                    })?,
                miner_address: address,
                pool,
                blocks_mined: entry.miner_record.block_count.to_string(),
                finalizer_public_key: hex::encode(entry.finalizer),
                finalizer_address: entry.finalizer_address.map(|address| address.encode()),
                current_stake_zat: entry.record.current_stake_zat.to_string(),
                stake_share_percent: percentage_two_decimals(
                    entry.record.current_stake_zat,
                    page.total_current_stake_zat,
                ),
                active_bond_count: entry.record.active_bond_count.to_string(),
                stake_action_count: entry.record.stake_action_count.to_string(),
                last_staked_height: entry.record.latest_height.to_string(),
                last_staked_block_hash: entry.record.latest_block_hash.to_string(),
                last_staked_at: entry.record.latest_timestamp.to_string(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let indexed_block_hash = page.best_tip.map(|(_, hash)| hash);
    let make_cursor =
        |entry: &zebra_state::ExplorerMinerStakeRankEntry, rank: u64, enabled: bool| {
            enabled.then(|| {
                MinerStakeCursor {
                    scope_finalizer: finalizer,
                    miner_address: entry.miner_address,
                    finalizer: entry.finalizer,
                    current_stake_zat: entry.record.current_stake_zat,
                    rank,
                    indexed_block_hash: indexed_block_hash
                        .expect("a non-empty miner stake ranking has a finalized tip"),
                }
                .encode()
            })
        };
    let next_cursor = page
        .entries
        .last()
        .zip(items.last())
        .and_then(|(entry, item)| make_cursor(entry, item.rank, has_next));
    let prev_cursor = page
        .entries
        .first()
        .zip(items.first())
        .and_then(|(entry, item)| make_cursor(entry, item.rank, has_prev));
    let totals = page.totals;
    let tracked_bond_stake_zat = totals
        .miner_stake_zat
        .saturating_add(totals.other_transparent_stake_zat)
        .saturating_add(totals.shielded_stake_zat)
        .saturating_add(totals.unknown_stake_zat)
        .saturating_add(totals.reward_bond_stake_zat);
    let reward_bank_stake_zat = page
        .total_current_stake_zat
        .saturating_sub(tracked_bond_stake_zat);
    let rewards_stake_zat = totals
        .reward_bond_stake_zat
        .saturating_add(reward_bank_stake_zat);
    let unattributed_stake_zat = page
        .total_current_stake_zat
        .saturating_sub(totals.miner_stake_zat);
    let source_amount = |stake_zat: u64| CrosslinkStakeSourceAmount {
        stake_zat: stake_zat.to_string(),
        share_percent: percentage_two_decimals(stake_zat, page.total_current_stake_zat),
    };

    Ok(CrosslinkMinerStakeResponse {
        items,
        summary: CrosslinkMinerStakeSummary {
            total_current_stake_zat: page.total_current_stake_zat.to_string(),
            miner_attributed_stake_zat: totals.miner_stake_zat.to_string(),
            miner_attributed_percent: percentage_one_decimal(
                totals.miner_stake_zat,
                page.total_current_stake_zat,
            ),
            unattributed_stake_zat: unattributed_stake_zat.to_string(),
            pair_count: totals.miner_address_count.to_string(),
            sources: CrosslinkStakeSourceBreakdown {
                miners: CrosslinkStakeSourceGroup {
                    stake_zat: totals.miner_stake_zat.to_string(),
                    share_percent: percentage_two_decimals(
                        totals.miner_stake_zat,
                        page.total_current_stake_zat,
                    ),
                    count: totals.miner_address_count.to_string(),
                },
                others: CrosslinkStakeSourceGroup {
                    stake_zat: totals.other_transparent_stake_zat.to_string(),
                    share_percent: percentage_two_decimals(
                        totals.other_transparent_stake_zat,
                        page.total_current_stake_zat,
                    ),
                    count: totals.other_transparent_address_count.to_string(),
                },
                shielded: source_amount(totals.shielded_stake_zat),
                unknown: (totals.unknown_stake_zat > 0)
                    .then(|| source_amount(totals.unknown_stake_zat)),
                rewards: (rewards_stake_zat > 0).then(|| source_amount(rewards_stake_zat)),
            },
        },
        pagination: CrosslinkMinerStakePagination {
            limit,
            total: totals.miner_address_count.to_string(),
            has_next,
            has_prev,
            next_cursor,
            prev_cursor,
        },
        indexed_height: page.best_tip.map(|(height, _)| height.0.to_string()),
        indexed_block_hash: indexed_block_hash.map(|hash| hash.to_string()),
    })
}

fn percentage_one_decimal(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "0.0".to_string();
    }
    let tenths = u128::from(numerator)
        .saturating_mul(1_000)
        .saturating_add(u128::from(denominator) / 2)
        / u128::from(denominator);
    format!("{}.{:01}", tenths / 10, tenths % 10)
}

fn percentage_two_decimals(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "0.00".to_string();
    }
    let hundredths = u128::from(numerator)
        .saturating_mul(10_000)
        .saturating_add(u128::from(denominator) / 2)
        / u128::from(denominator);
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

#[cfg(test)]
mod tests {
    use super::{percentage_one_decimal, percentage_two_decimals};

    #[test]
    fn attribution_percentage_is_rounded_to_one_decimal_place() {
        assert_eq!(percentage_one_decimal(0, 0), "0.0");
        assert_eq!(percentage_one_decimal(1, 3), "33.3");
        assert_eq!(percentage_one_decimal(2, 3), "66.7");
        assert_eq!(percentage_one_decimal(1, 1), "100.0");
        assert_eq!(percentage_two_decimals(0, 0), "0.00");
        assert_eq!(percentage_two_decimals(3_445_03, 16_194_95), "21.27");
    }
}
