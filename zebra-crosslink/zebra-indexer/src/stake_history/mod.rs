//! Finalized Crosslink staking-action history queries.

mod cursor;

use tower::ServiceExt;
use zebra_chain::{block::Height, transparent::Address};
use zebra_state::{
    ExplorerPageDirection, ExplorerReadRequest, ExplorerReadResponse, ExplorerStakeAction,
    ExplorerStakeHistoryCursor, ExplorerStakeHistoryFilter, ExplorerStakeSource, ReadRequest,
    ReadResponse, ReadState,
};

use crate::{
    height_range::TransactionHeightRange,
    types::{
        CrosslinkStakeAction, CrosslinkStakeHistoryEntry, CrosslinkStakeHistoryPagination,
        CrosslinkStakeHistoryRequest, CrosslinkStakeHistoryResponse, CrosslinkStakeHistorySource,
        PageDirection,
    },
    Error,
};

use self::cursor::StakeHistoryCursor;

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

/// Returns finalized staking actions, optionally filtered by source, finalizer, bond, and action.
pub async fn stake_history_from_state<State>(
    read_state: State,
    address: Option<Address>,
    finalizer: Option<[u8; 32]>,
    bond_key: Option<[u8; 32]>,
    request: CrosslinkStakeHistoryRequest,
) -> Result<CrosslinkStakeHistoryResponse, Error>
where
    State: ReadState,
{
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(Error::InvalidQuery(format!(
            "stake-history limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let height_range = TransactionHeightRange::new(
        request.from_height.unwrap_or(Height::MIN.0)..=request.to_height.unwrap_or(Height::MAX.0),
    )?;
    let cursor = request
        .cursor
        .as_deref()
        .map(StakeHistoryCursor::decode)
        .transpose()?;
    if request.direction == PageDirection::Previous && cursor.is_none() {
        return Err(Error::InvalidCursor(
            "direction=prev requires a cursor".to_string(),
        ));
    }
    if cursor.is_some_and(|cursor| {
        !cursor.matches(address, finalizer, bond_key, request.action, height_range)
    }) {
        return Err(Error::InvalidCursor(
            "stake-history cursor was created for different filters".to_string(),
        ));
    }

    let state_filter = ExplorerStakeHistoryFilter {
        address,
        finalizer,
        bond_key,
        action: request.action.map(state_action),
    };
    let response = read_state
        .oneshot(ReadRequest::Explorer(
            ExplorerReadRequest::StakeHistoryPage {
                filter: state_filter,
                limit,
                cursor: cursor.map(|cursor| ExplorerStakeHistoryCursor {
                    location: cursor.location,
                    block_hash: cursor.block_hash,
                }),
                direction: match request.direction {
                    PageDirection::Next => ExplorerPageDirection::Older,
                    PageDirection::Previous => ExplorerPageDirection::Newer,
                },
                height_range: height_range.from..=height_range.to,
            },
        ))
        .await
        .map_err(|error| Error::StateRequest(error.to_string()))?;
    let ReadResponse::Explorer(ExplorerReadResponse::StakeHistoryPage(page)) = response else {
        return Err(Error::StateResponse(
            "state returned the wrong response for stake history".to_string(),
        ));
    };
    if !page.cursor_valid {
        return Err(Error::InvalidCursor(
            "stake-history cursor is no longer canonical or does not match the filters".to_string(),
        ));
    }

    let has_rows = !page.entries.is_empty();
    let (has_next, has_prev) = match request.direction {
        PageDirection::Next => (page.has_more, cursor.is_some() && has_rows),
        PageDirection::Previous => (cursor.is_some() && has_rows, page.has_more),
    };
    let cursor_for = |entry: &zebra_state::ExplorerStakeHistoryEntry| {
        StakeHistoryCursor::new(
            entry.location,
            entry.block_hash,
            address,
            finalizer,
            bond_key,
            request.action,
            height_range,
        )
        .encode()
    };
    let next_cursor = page.entries.last().filter(|_| has_next).map(cursor_for);
    let prev_cursor = page.entries.first().filter(|_| has_prev).map(cursor_for);
    let items = page
        .entries
        .into_iter()
        .map(|entry| {
            let (address, source_type) = match entry.record.source {
                ExplorerStakeSource::Transparent(address) => (
                    Some(address.to_string()),
                    CrosslinkStakeHistorySource::Transparent,
                ),
                ExplorerStakeSource::Shielded => (None, CrosslinkStakeHistorySource::Shielded),
                ExplorerStakeSource::Unknown => (None, CrosslinkStakeHistorySource::Unknown),
                ExplorerStakeSource::Rewards => {
                    (None, CrosslinkStakeHistorySource::FinalizerRewards)
                }
            };
            CrosslinkStakeHistoryEntry {
                txid: entry.txid.to_string(),
                action: public_action(entry.record.action),
                bond_key: hex::encode(entry.record.bond_key),
                address,
                source_type,
                from_finalizer_public_key: entry.record.from_finalizer.map(hex::encode),
                to_finalizer_public_key: entry.record.to_finalizer.map(hex::encode),
                amount_zat: entry.record.amount_zat.map(|amount| amount.to_string()),
                block_height: entry.location.height.0.to_string(),
                block_hash: entry.block_hash.to_string(),
                block_time: entry.block_time.to_string(),
                transaction_index: u32::from(entry.location.index.index()),
            }
        })
        .collect();

    Ok(CrosslinkStakeHistoryResponse {
        items,
        pagination: CrosslinkStakeHistoryPagination {
            limit,
            has_next,
            has_prev,
            next_cursor,
            prev_cursor,
        },
        indexed_height: page.best_tip.map(|(height, _)| height.0.to_string()),
        indexed_block_hash: page.best_tip.map(|(_, hash)| hash.to_string()),
    })
}

fn state_action(action: CrosslinkStakeAction) -> ExplorerStakeAction {
    match action {
        CrosslinkStakeAction::Create => ExplorerStakeAction::Create,
        CrosslinkStakeAction::BeginUnbonding => ExplorerStakeAction::BeginUnbonding,
        CrosslinkStakeAction::Withdraw => ExplorerStakeAction::Withdraw,
        CrosslinkStakeAction::Retarget => ExplorerStakeAction::Retarget,
        CrosslinkStakeAction::ConvertReward => ExplorerStakeAction::ConvertReward,
    }
}

fn public_action(action: ExplorerStakeAction) -> CrosslinkStakeAction {
    match action {
        ExplorerStakeAction::Create => CrosslinkStakeAction::Create,
        ExplorerStakeAction::BeginUnbonding => CrosslinkStakeAction::BeginUnbonding,
        ExplorerStakeAction::Withdraw => CrosslinkStakeAction::Withdraw,
        ExplorerStakeAction::Retarget => CrosslinkStakeAction::Retarget,
        ExplorerStakeAction::ConvertReward => CrosslinkStakeAction::ConvertReward,
    }
}
