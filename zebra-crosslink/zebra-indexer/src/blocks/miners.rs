//! Persisted all-time miner ranking.

use tower::ServiceExt;
use zebra_chain::{block::Height, transparent::Address};
use zebra_state::{
    ExplorerMinerRankCursor, ExplorerPageDirection, ExplorerReadRequest, ExplorerReadResponse,
    ReadRequest, ReadResponse, ReadState,
};

use crate::{
    types::{
        MinerInfoResponse, PageDirection, TopMinerEntry, TopMinersPagination, TopMinersRequest,
        TopMinersResponse, TopMinersSummary,
    },
    Error,
};

use super::miner_cursor::MinerCursor;

const DEFAULT_MINERS_LIMIT: u32 = 30;
const MAX_MINERS_LIMIT: u32 = 100;

/// Returns all-time mining information for one payout address without scanning the ranking.
pub async fn miner_info_from_state<State>(
    read_state: State,
    network: &zebra_chain::parameters::Network,
    address: Address,
) -> Result<MinerInfoResponse, Error>
where
    State: ReadState,
{
    let response = read_state
        .clone()
        .oneshot(ReadRequest::Explorer(ExplorerReadRequest::Miner {
            address,
        }))
        .await
        .map_err(|error| Error::StateRequest(error.to_string()))?;
    let ReadResponse::Explorer(ExplorerReadResponse::Miner(miner)) = response else {
        return Err(Error::StateResponse(
            "state returned the wrong response for a miner lookup".to_string(),
        ));
    };

    let encoded_address = address.to_string();
    let pool = if let Some(record) = miner.record {
        let response = read_state
            .oneshot(ReadRequest::Explorer(ExplorerReadRequest::BlockSummaries(
                vec![Height(record.latest_height)].into(),
            )))
            .await
            .map_err(|error| Error::StateRequest(error.to_string()))?;
        let ReadResponse::Explorer(ExplorerReadResponse::BlockSummaries(blocks)) = response else {
            return Err(Error::StateResponse(
                "state returned the wrong response for the miner's latest block".to_string(),
            ));
        };
        let block = blocks.into_iter().next().flatten().ok_or_else(|| {
            Error::StateResponse(
                "the miner's latest block is missing from canonical state".to_string(),
            )
        })?;
        let coinbase = block.block.transactions.first().ok_or_else(|| {
            Error::CorruptData("a canonical miner block has no coinbase".to_string())
        })?;
        let (identified_address, identified_pool) =
            super::miner_attribution::identify_miner(coinbase, network);
        if identified_address.as_deref() == Some(encoded_address.as_str()) {
            identified_pool
        } else {
            super::miner_attribution::pool_from_address(&encoded_address)
                .unwrap_or("Unknown")
                .to_string()
        }
    } else {
        "Unknown".to_string()
    };
    let blocks_mined = miner.record.map_or(0, |record| record.block_count);

    Ok(MinerInfoResponse {
        address: encoded_address,
        is_miner: miner.record.is_some(),
        pool,
        blocks_mined: blocks_mined.to_string(),
        mined_zat: miner
            .record
            .map_or(0, |record| record.mined_zat)
            .to_string(),
        block_share_percent: share_percent(blocks_mined, miner.chain_block_count),
        last_mined_height: miner.record.map(|record| record.latest_height.to_string()),
        last_mined_block_hash: miner
            .record
            .map(|record| record.latest_block_hash.to_string()),
        last_mined_at: miner
            .record
            .map(|record| record.latest_timestamp.to_string()),
        staked_zat: Some(miner.staked_zat.to_string()),
        finalizer_count: Some(miner.finalizer_count.to_string()),
        indexed_height: miner.best_tip.map(|(height, _)| height.0.to_string()),
        indexed_block_hash: miner.best_tip.map(|(_, hash)| hash.to_string()),
    })
}

/// Returns the all-time miner ranking without scanning blocks or the complete miner set.
pub async fn top_miners_from_state<State>(
    read_state: State,
    network: &zebra_chain::parameters::Network,
    request: TopMinersRequest,
) -> Result<TopMinersResponse, Error>
where
    State: ReadState,
{
    let limit = request.limit.unwrap_or(DEFAULT_MINERS_LIMIT);
    if !(1..=MAX_MINERS_LIMIT).contains(&limit) {
        return Err(Error::InvalidQuery(format!(
            "top-miners limit must be between 1 and {MAX_MINERS_LIMIT}"
        )));
    }
    let cursor = request
        .cursor
        .as_deref()
        .map(MinerCursor::decode)
        .transpose()?;
    if request.direction == PageDirection::Previous && cursor.is_none() {
        return Err(Error::InvalidCursor(
            "direction=prev requires a cursor".to_string(),
        ));
    }
    let state_cursor = cursor.map(|cursor| ExplorerMinerRankCursor {
        address: cursor.address,
        block_count: cursor.block_count,
        rank: cursor.rank,
        block_hash: cursor.indexed_block_hash,
    });
    let state_direction = match request.direction {
        PageDirection::Next => ExplorerPageDirection::Older,
        PageDirection::Previous => ExplorerPageDirection::Newer,
    };
    let response = read_state
        .clone()
        .oneshot(ReadRequest::Explorer(ExplorerReadRequest::MinerPage {
            limit,
            cursor: state_cursor,
            direction: state_direction,
        }))
        .await
        .map_err(|error| Error::StateRequest(error.to_string()))?;
    let ReadResponse::Explorer(ExplorerReadResponse::MinerPage(page)) = response else {
        return Err(Error::StateResponse(
            "state returned the wrong response for the top-miner ranking".to_string(),
        ));
    };
    if !page.cursor_valid {
        return Err(Error::InvalidCursor(
            "top-miners ranking changed; restart from the first page".to_string(),
        ));
    }

    let has_rows = !page.entries.is_empty();
    let (has_next, has_prev) = match request.direction {
        PageDirection::Next => (page.has_more, cursor.is_some() && has_rows),
        PageDirection::Previous => (cursor.is_some() && has_rows, page.has_more),
    };
    let row_count = u64::try_from(page.entries.len()).expect("top-miner page size fits in u64");
    let first_rank = match (request.direction, cursor) {
        (PageDirection::Next, Some(cursor)) => cursor
            .rank
            .checked_add(1)
            .ok_or_else(|| Error::InvalidCursor("top-miners cursor rank overflow".to_string()))?,
        (PageDirection::Next, None) => 1,
        (PageDirection::Previous, Some(cursor)) => cursor
            .rank
            .checked_sub(row_count)
            .filter(|rank| !has_rows || *rank > 0)
            .ok_or_else(|| Error::InvalidCursor("top-miners cursor rank is invalid".to_string()))?,
        (PageDirection::Previous, None) => unreachable!("previous pages require a cursor"),
    };

    let latest_heights = page
        .entries
        .iter()
        .map(|entry| Height(entry.record.latest_height))
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
            "state returned the wrong response for top-miner blocks".to_string(),
        ));
    };

    let miners = page
        .entries
        .iter()
        .zip(latest_blocks)
        .enumerate()
        .map(|(offset, (entry, block))| {
            let block = block.ok_or_else(|| {
                Error::StateResponse(
                    "a ranked miner's latest block is missing from canonical state".to_string(),
                )
            })?;
            let coinbase = block.block.transactions.first().ok_or_else(|| {
                Error::CorruptData("a canonical miner block has no coinbase".to_string())
            })?;
            let (identified_address, identified_pool) =
                super::miner_attribution::identify_miner(coinbase, network);
            let address = entry.address.to_string();
            let pool = if identified_address.as_deref() == Some(address.as_str()) {
                identified_pool
            } else {
                super::miner_attribution::pool_from_address(&address)
                    .unwrap_or("Unknown")
                    .to_string()
            };
            let record = entry.record;

            Ok(TopMinerEntry {
                rank: first_rank
                    .checked_add(u64::try_from(offset).expect("top-miner page offset fits in u64"))
                    .ok_or_else(|| {
                        Error::InvalidCursor("top-miners cursor rank overflow".to_string())
                    })?,
                address,
                pool,
                blocks_mined: record.block_count.to_string(),
                mined_zat: record.mined_zat.to_string(),
                block_share_percent: share_percent(record.block_count, page.block_count),
                last_mined_height: record.latest_height.to_string(),
                last_mined_block_hash: record.latest_block_hash.to_string(),
                last_mined_at: record.latest_timestamp.to_string(),
                staked_zat: None,
                finalizer_count: None,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let unattributed_block_count = page.block_count.saturating_sub(page.attributed_block_count);
    let indexed_block_hash = page.best_tip.map(|(_, hash)| hash);
    let next_cursor = page
        .entries
        .last()
        .zip(miners.last())
        .filter(|_| has_next)
        .map(|(entry, miner)| {
            MinerCursor::new(
                entry.address,
                entry.record.block_count,
                miner.rank,
                indexed_block_hash.expect("a non-empty miner ranking has a finalized tip"),
            )
            .encode()
        });
    let prev_cursor = page
        .entries
        .first()
        .zip(miners.first())
        .filter(|_| has_prev)
        .map(|(entry, miner)| {
            MinerCursor::new(
                entry.address,
                entry.record.block_count,
                miner.rank,
                indexed_block_hash.expect("a non-empty miner ranking has a finalized tip"),
            )
            .encode()
        });

    Ok(TopMinersResponse {
        miners,
        summary: TopMinersSummary {
            block_count: page.block_count.to_string(),
            attributed_block_count: page.attributed_block_count.to_string(),
            unattributed_block_count: unattributed_block_count.to_string(),
            miner_count: page.miner_count.to_string(),
        },
        pagination: TopMinersPagination {
            limit,
            total: page.miner_count.to_string(),
            has_next,
            has_prev,
            next_cursor,
            prev_cursor,
        },
        indexed_height: page.best_tip.map(|(height, _)| height.0.to_string()),
        indexed_block_hash: indexed_block_hash.map(|hash| hash.to_string()),
    })
}

fn share_percent(blocks: u64, total_blocks: u64) -> String {
    if total_blocks == 0 {
        return "0.0".to_string();
    }
    let tenths = u128::from(blocks)
        .saturating_mul(1_000)
        .saturating_add(u128::from(total_blocks) / 2)
        / u128::from(total_blocks);
    format!("{}.{:01}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::share_percent;

    #[test]
    fn miner_share_is_rounded_to_one_decimal_place() {
        assert_eq!(share_percent(1_299, 3_451), "37.6");
        assert_eq!(share_percent(0, 0), "0.0");
    }
}
