//! Explorer JSON-RPC adapters for [`RpcImpl`].

#[cfg(feature = "indexer")]
use chrono::Utc;
use jsonrpsee::core::RpcResult as Result;
use tower::Service;
#[cfg(feature = "indexer")]
use zcash_primitives::bft::ACTIVE_ROSTER_MAX_N;
#[cfg(feature = "indexer")]
use zebra_chain::{
    block::{self, Height},
    parameters::NetworkUpgrade,
    transparent::Address,
};
use zebra_chain::{chain_sync_status::ChainSyncStatus, chain_tip::ChainTip, transaction};
use zebra_consensus::router::service_trait::BlockVerifierService;
#[cfg(feature = "indexer")]
use zebra_indexer::{
    address_summary_from_state, address_transactions_page_from_state,
    address_utxos_page_from_state, block_details_from_state, blocks_page_from_state,
    chart_data_from_state, miner_info_from_state, stats_from_state, top_balances_from_state,
    top_miners_from_state, transaction_details_from_state, transactions_page_from_state,
};
use zebra_network::address_book_peers::AddressBookPeers;
use zebra_node_services::mempool::{self as node_mempool, MempoolService};
use zebra_state::crosslink::{TFLServiceRequest, TFLServiceResponse};
#[cfg(feature = "indexer")]
use zebra_state::{HashOrHeight, ReadRequest, ReadResponse};
use zebra_state::{ReadState as ReadStateService, State as StateService};

use crate::server::{self, error::MapError};

#[cfg(feature = "indexer")]
use super::types::{
    BlockchainRuntimeStats, CrosslinkActivationMilestone, CrosslinkActivationOverview,
    CrosslinkFinalityOverview, CrosslinkFinalityStatus, CrosslinkFinalizersOverview,
    CrosslinkMinersOverview, CrosslinkNetworkStats, CrosslinkPhase, CrosslinkStakingChange,
    CrosslinkStakingOverview, CrosslinkStakingStatus, MempoolStats, MiningStats, NetworkStats,
    NodeSyncStats, SupplyPoolStats, SupplyStats,
};
use super::{
    mempool,
    types::{
        AddressSummary, AddressTransactionsResponse, AddressUtxosResponse, BlockDetails,
        BlocksResponse, ChartDataRequest, ChartDataResponse, ExplorerNetworkStatsResponse,
        GetAddressTransactionsRequest, GetAddressUtxosPageRequest, GetBlocksRequest,
        GetMempoolTransactionsRequest, GetTransactionsRequest, IndexerStatusResponse,
        MempoolTransactionsResponse, MinerInfoResponse, TopBalancesRequest, TopBalancesResponse,
        TopMinersRequest, TopMinersResponse, TransactionDetailsResponse, TransactionsResponse,
    },
};
#[cfg(feature = "indexer")]
use crate::methods::RpcServer;
use crate::methods::{call_service, RpcImpl};

impl<Mempool, TFLService, State, ReadState, Tip, AddressBook, BlockVerifierRouter, SyncStatus>
    RpcImpl<
        Mempool,
        TFLService,
        State,
        ReadState,
        Tip,
        AddressBook,
        BlockVerifierRouter,
        SyncStatus,
    >
where
    Mempool: MempoolService,
    TFLService: Service<
            TFLServiceRequest,
            Response = TFLServiceResponse,
            Error = zebra_node_services::BoxError,
        > + Clone
        + Send
        + Sync
        + 'static,
    TFLService::Future: Send,
    State: StateService,
    ReadState: ReadStateService,
    Tip: ChainTip + Clone + Send + Sync + 'static,
    AddressBook: AddressBookPeers + Clone + Send + Sync + 'static,
    BlockVerifierRouter: BlockVerifierService,
    SyncStatus: ChainSyncStatus + Clone + Send + Sync + 'static,
{
    pub(in crate::methods) async fn explorer_get_blocks(
        &self,
        request: Option<GetBlocksRequest>,
    ) -> Result<BlocksResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let request = request.unwrap_or_default();

            match blocks_page_from_state(
                self.read_state.clone(),
                &self.network,
                request.limit,
                request.cursor,
                request.direction,
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(error @ zebra_indexer::Error::InvalidCursor(_)) => {
                    Err(error).map_error(server::error::LegacyCode::InvalidParameter)
                }
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_block_details(
        &self,
        hash_or_height: String,
    ) -> Result<BlockDetails> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = hash_or_height;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let identifier =
                HashOrHeight::new(&hash_or_height, self.latest_chain_tip.best_tip_height())
                    .map_error(server::error::LegacyCode::InvalidParameter)?;

            match block_details_from_state(self.read_state.clone(), &self.network, identifier).await
            {
                Ok(Some(details)) => Ok(details),
                Ok(None) => Err("Block not found in the best chain")
                    .map_error(server::error::LegacyCode::InvalidAddressOrKey),
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_transactions(
        &self,
        request: Option<GetTransactionsRequest>,
    ) -> Result<TransactionsResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let request = request.unwrap_or_default();
            let query = request
                .transaction_query()
                .map_error(server::error::LegacyCode::InvalidParameter)?;
            let height_range = request.height_range();

            match transactions_page_from_state(
                self.read_state.clone(),
                query,
                request.limit,
                request.cursor,
                request.direction,
                height_range,
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(
                    error @ (zebra_indexer::Error::InvalidCursor(_)
                    | zebra_indexer::Error::InvalidQuery(_)),
                ) => Err(error).map_error(server::error::LegacyCode::InvalidParameter),
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_mempool_transactions(
        &self,
        request: Option<GetMempoolTransactionsRequest>,
    ) -> Result<MempoolTransactionsResponse> {
        let response = call_service(
            self.mempool.clone(),
            node_mempool::Request::FullTransactions,
        )
        .await?;
        let node_mempool::Response::FullTransactions {
            transactions,
            transaction_dependencies,
            last_seen_tip_hash: _,
        } = response
        else {
            unreachable!("unmatched response to a mempool FullTransactions request")
        };
        let request = request.unwrap_or_default();

        match mempool::transactions_page(
            &self.network,
            transactions,
            &transaction_dependencies,
            &request,
        ) {
            Ok(response) => Ok(response),
            Err(
                error @ (zebra_indexer::Error::InvalidCursor(_)
                | zebra_indexer::Error::InvalidQuery(_)),
            ) => Err(error).map_error(server::error::LegacyCode::InvalidParameter),
            Err(error) => Err(error).map_misc_error(),
        }
    }

    pub(in crate::methods) async fn explorer_get_transaction_details(
        &self,
        txid: String,
    ) -> Result<TransactionDetailsResponse> {
        let txid = txid
            .parse::<transaction::Hash>()
            .map_error(server::error::LegacyCode::InvalidParameter)?;

        match call_service(
            self.mempool.clone(),
            node_mempool::Request::FullTransactions,
        )
        .await
        {
            Ok(node_mempool::Response::FullTransactions {
                transactions,
                transaction_dependencies,
                last_seen_tip_hash: _,
            }) => {
                if let Some(transaction) = transactions
                    .iter()
                    .find(|transaction| transaction.transaction.id.mined_id() == txid)
                {
                    return mempool::transaction_details(
                        transaction,
                        &transactions,
                        &transaction_dependencies,
                        &self.network,
                    )
                    .map(TransactionDetailsResponse::Pending)
                    .map_misc_error();
                }
            }
            Ok(_) => unreachable!("unmatched response to a mempool FullTransactions request"),
            Err(error) => {
                tracing::debug!(?error, %txid, "mempool lookup failed; checking canonical index");
            }
        }

        #[cfg(not(feature = "indexer"))]
        return explorer_index_disabled();

        #[cfg(feature = "indexer")]
        match transaction_details_from_state(self.read_state.clone(), &self.network, txid).await {
            Ok(Some(details)) => Ok(TransactionDetailsResponse::Mined(details)),
            Ok(None) => Err("Transaction not found in the best chain")
                .map_error(server::error::LegacyCode::InvalidAddressOrKey),
            Err(error @ zebra_indexer::Error::ExplorerDataUnavailable(_)) => {
                Err(error).map_error(server::error::LegacyCode::InWarmup)
            }
            Err(error) => Err(error).map_misc_error(),
        }
    }

    pub(in crate::methods) async fn explorer_get_address_summary(
        &self,
        address: String,
    ) -> Result<AddressSummary> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = address;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let address = explorer_transparent_address(&self.network, &address)
                .map_error(server::error::LegacyCode::InvalidAddressOrKey)?;

            address_summary_from_state(self.read_state.clone(), &self.network, address)
                .await
                .map_misc_error()
        }
    }

    pub(in crate::methods) async fn explorer_get_address_transactions(
        &self,
        request: GetAddressTransactionsRequest,
    ) -> Result<AddressTransactionsResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let address = explorer_transparent_address(&self.network, &request.address)
                .map_error(server::error::LegacyCode::InvalidAddressOrKey)?;
            let height_range = request.height_range();

            match address_transactions_page_from_state(
                self.read_state.clone(),
                &self.network,
                address,
                request.limit,
                request.cursor,
                request.direction,
                height_range,
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(
                    error @ (zebra_indexer::Error::InvalidCursor(_)
                    | zebra_indexer::Error::InvalidQuery(_)),
                ) => Err(error).map_error(server::error::LegacyCode::InvalidParameter),
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_address_utxos_page(
        &self,
        request: GetAddressUtxosPageRequest,
    ) -> Result<AddressUtxosResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let address = explorer_transparent_address(&self.network, &request.address)
                .map_error(server::error::LegacyCode::InvalidAddressOrKey)?;

            match address_utxos_page_from_state(
                self.read_state.clone(),
                address,
                request.limit,
                request.cursor,
                request.direction,
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(error @ zebra_indexer::Error::InvalidCursor(_)) => {
                    Err(error).map_error(server::error::LegacyCode::InvalidParameter)
                }
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_indexer_status(
        &self,
    ) -> Result<IndexerStatusResponse> {
        #[cfg(not(feature = "indexer"))]
        return explorer_index_disabled();

        #[cfg(feature = "indexer")]
        {
            let chain_tip = self.latest_chain_tip.best_tip_height_and_hash();
            let indexer_stats = stats_from_state(self.read_state.clone())
                .await
                .map_misc_error()?;
            Ok(indexer_status(
                chain_tip,
                indexer_stats.indexed_height.as_deref(),
                indexer_stats.indexed_block_hash.as_deref(),
            ))
        }
    }

    pub(in crate::methods) async fn explorer_get_network_stats(
        &self,
    ) -> Result<ExplorerNetworkStatsResponse> {
        #[cfg(not(feature = "indexer"))]
        return explorer_index_disabled();

        #[cfg(feature = "indexer")]
        {
            let indexer_stats = stats_from_state(self.read_state.clone())
                .await
                .map_misc_error()?;

            let (
                blockchain,
                network_solps,
                mempool,
                subsidy,
                activated_response,
                finalized_response,
                roster_response,
            ) = tokio::join!(
                self.get_blockchain_info(),
                self.get_network_sol_ps(None, None),
                self.get_mempool_info(),
                self.get_block_subsidy(None),
                call_service(self.read_state.clone(), ReadRequest::CrosslinkIsActivated),
                call_service(self.read_state.clone(), ReadRequest::CrosslinkFinalizedTip),
                call_service(self.read_state.clone(), ReadRequest::CrosslinkRoster),
            );
            let blockchain = blockchain?;
            let network_solps = network_solps?;
            let mempool = mempool?;
            let subsidy = subsidy.ok();
            let activated = match activated_response? {
                ReadResponse::CrosslinkIsActivated(activated) => activated,
                _ => unreachable!("unmatched response to CrosslinkIsActivated"),
            };
            let finalized_tip = match finalized_response? {
                ReadResponse::CrosslinkFinalizedTip(tip) => tip,
                _ => unreachable!("unmatched response to CrosslinkFinalizedTip"),
            };
            let roster = match roster_response? {
                ReadResponse::CrosslinkRoster(roster) => roster,
                _ => unreachable!("unmatched response to CrosslinkRoster"),
            };

            let generated_at = Utc::now().timestamp();
            let chain_tip = self.latest_chain_tip.best_tip_height_and_hash();
            let node_block_timestamp = chain_tip.and_then(|(height, _)| {
                self.latest_chain_tip
                    .best_tip_height_and_block_time()
                    .filter(|(time_height, _)| *time_height == height)
                    .map(|(_, time)| time.timestamp())
            });
            let estimated_network_height = blockchain.estimated_height();
            let node_height = chain_tip.map(|(height, _)| height);
            let sync = NodeSyncStats {
                estimated_network_height: estimated_network_height.0.to_string(),
                node_height: node_height.map(|height| height.0.to_string()),
                node_block_hash: chain_tip.map(|(_, hash)| hash.to_string()),
                node_block_timestamp: node_block_timestamp.map(|time| time.to_string()),
                node_block_age_seconds: node_block_timestamp
                    .map(|time| generated_at.saturating_sub(time).max(0).to_string()),
                lag: node_height.map(|height| {
                    estimated_network_height
                        .0
                        .saturating_sub(height.0)
                        .to_string()
                }),
                verification_progress: format!("{:.6}", blockchain.verification_progress()),
                synced: self
                    .latest_chain_tip
                    .is_at_or_near_network_tip(&self.network),
            };
            let target_block_time_seconds = chain_tip.as_ref().and_then(|(height, _)| {
                u64::try_from(
                    NetworkUpgrade::target_spacing_for_height(&self.network, *height).num_seconds(),
                )
                .ok()
            });
            let tip_height = node_height.map(|height| height.0);
            let params = self.network.crosslink_parameters();
            let staking_height = params.bootstrap.staking_height();
            let roster_height = params.bootstrap.roster_height();
            let activation_height = params.bootstrap.activation_height();
            let pool_balance = |id: &str| {
                blockchain
                    .value_pools()
                    .iter()
                    .find(|pool| pool.id().as_str() == id)
                    .map(|pool| pool.chain_value_zat().zatoshis().to_string())
                    .unwrap_or_else(|| "0".to_string())
            };
            let staking = crosslink_staking_overview(
                tip_height,
                staking_height,
                params.staking.period,
                params.staking.day_window,
                generated_at,
                target_block_time_seconds,
                pool_balance("staking_bonded"),
                pool_balance("staking_unbonded"),
                pool_balance("finalizer_rewards"),
            );
            let activation = crosslink_activation_overview(
                tip_height,
                staking_height,
                roster_height,
                activation_height,
                generated_at,
                target_block_time_seconds,
            );
            let finality_status = if finalized_tip.is_some() {
                CrosslinkFinalityStatus::Active
            } else if activated
                || tip_height
                    .zip(activation_height)
                    .is_some_and(|(tip, height)| tip >= height)
            {
                CrosslinkFinalityStatus::Starting
            } else {
                CrosslinkFinalityStatus::NotActivated
            };
            let finalized_height = finalized_tip.map(|(height, _)| height.0);
            let active_count = roster.len().min(ACTIVE_ROSTER_MAX_N);
            let active_voting_power_zat = roster
                .iter()
                .take(active_count)
                .fold(0_u64, |total, member| {
                    total.saturating_add(member.voting_power)
                });
            let crosslink = CrosslinkNetworkStats {
                finality: CrosslinkFinalityOverview {
                    status: finality_status,
                    activated,
                    finalized_height: finalized_height.map(|height| height.to_string()),
                    finalized_hash: finalized_tip.map(|(_, hash)| hash.to_string()),
                    lag_blocks: tip_height
                        .zip(finalized_height)
                        .map(|(tip, finalized)| tip.saturating_sub(finalized).to_string()),
                    confirmation_depth_blocks: params.bc_confirmation_depth_sigma.to_string(),
                },
                staking,
                miners: CrosslinkMinersOverview {
                    count: indexer_stats.miner_count.clone(),
                    count_scope: "all".to_string(),
                    blocks_24h: indexer_stats.trailing_24h.block_count.clone(),
                    blocks_24h_complete: indexer_stats.trailing_24h.complete,
                },
                finalizers: CrosslinkFinalizersOverview {
                    roster_count: roster.len().to_string(),
                    active_count: active_count.to_string(),
                    active_limit: ACTIVE_ROSTER_MAX_N.to_string(),
                    active_voting_power_zat: active_voting_power_zat.to_string(),
                },
                activation,
            };
            let supply = SupplyStats {
                chain_supply_zat: blockchain
                    .chain_supply()
                    .chain_value_zat()
                    .zatoshis()
                    .to_string(),
                pools: blockchain
                    .value_pools()
                    .iter()
                    .map(|pool| SupplyPoolStats {
                        id: pool.id().clone(),
                        balance_zat: pool.chain_value_zat().zatoshis().to_string(),
                        monitored: pool.monitored(),
                    })
                    .collect(),
            };
            let response = ExplorerNetworkStatsResponse {
                sync,
                totals: indexer_stats.totals,
                trailing_24h: indexer_stats.trailing_24h,
                mining: MiningStats {
                    difficulty: format!("{:.6}", blockchain.difficulty()),
                    network_solps: network_solps.to_string(),
                    block_reward_zat: subsidy
                        .as_ref()
                        .map(|subsidy| subsidy.total_block_subsidy().zatoshis().to_string()),
                    miner_reward_zat: subsidy
                        .as_ref()
                        .map(|subsidy| subsidy.miner().zatoshis().to_string()),
                    founders_reward_zat: subsidy
                        .as_ref()
                        .map(|subsidy| subsidy.founders().zatoshis().to_string()),
                    funding_streams_zat: subsidy
                        .as_ref()
                        .map(|subsidy| subsidy.funding_streams_total().zatoshis().to_string()),
                    lockbox_zat: subsidy
                        .as_ref()
                        .map(|subsidy| subsidy.lockbox_total().zatoshis().to_string()),
                    target_block_time_seconds,
                },
                network: NetworkStats {
                    peer_count: self
                        .address_book
                        .recently_live_peers(Utc::now())
                        .len()
                        .to_string(),
                    protocol_version: zebra_network::constants::CURRENT_NETWORK_PROTOCOL_VERSION.0,
                    node_version: self.user_agent.clone(),
                },
                mempool: MempoolStats {
                    transaction_count: mempool.size.to_string(),
                    bytes: mempool.bytes.to_string(),
                    memory_usage: mempool.usage.to_string(),
                },
                supply,
                blockchain: BlockchainRuntimeStats {
                    state_size_bytes: blockchain.size_on_disk().to_string(),
                    pruned: blockchain.pruned(),
                },
                crosslink,
                generated_at: generated_at.to_string(),
            };

            Ok(response)
        }
    }

    pub(in crate::methods) async fn explorer_get_chart_data(
        &self,
        request: ChartDataRequest,
    ) -> Result<ChartDataResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            match chart_data_from_state(self.read_state.clone(), &self.network, request).await {
                Ok(response) => Ok(response),
                Err(error @ zebra_indexer::Error::InvalidQuery(_)) => {
                    Err(error).map_error(server::error::LegacyCode::InvalidParameter)
                }
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_top_balances(
        &self,
        request: TopBalancesRequest,
    ) -> Result<TopBalancesResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            match top_balances_from_state(self.read_state.clone(), request).await {
                Ok(response) => Ok(response),
                Err(error @ zebra_indexer::Error::InvalidCursor(_))
                | Err(error @ zebra_indexer::Error::InvalidQuery(_)) => {
                    Err(error).map_error(server::error::LegacyCode::InvalidParameter)
                }
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_top_miners(
        &self,
        request: Option<TopMinersRequest>,
    ) -> Result<TopMinersResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = request;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            match top_miners_from_state(
                self.read_state.clone(),
                &self.network,
                request.unwrap_or_default(),
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(error @ zebra_indexer::Error::InvalidCursor(_))
                | Err(error @ zebra_indexer::Error::InvalidQuery(_)) => {
                    Err(error).map_error(server::error::LegacyCode::InvalidParameter)
                }
                Err(error) => Err(error).map_misc_error(),
            }
        }
    }

    pub(in crate::methods) async fn explorer_get_miner_info(
        &self,
        address: String,
    ) -> Result<MinerInfoResponse> {
        #[cfg(not(feature = "indexer"))]
        {
            let _ = address;
            return explorer_index_disabled();
        }
        #[cfg(feature = "indexer")]
        {
            let address = explorer_transparent_address(&self.network, &address)
                .map_error(server::error::LegacyCode::InvalidAddressOrKey)?;
            miner_info_from_state(self.read_state.clone(), &self.network, address)
                .await
                .map_misc_error()
        }
    }
}

#[cfg(feature = "indexer")]
#[allow(clippy::too_many_arguments)]
fn crosslink_staking_overview(
    tip_height: Option<u32>,
    staking_height: Option<u32>,
    period: u32,
    window: u32,
    now: i64,
    target_block_time_seconds: Option<u64>,
    bonded_zat: String,
    unbonded_zat: String,
    finalizer_rewards_zat: String,
) -> CrosslinkStakingOverview {
    let first_staking_height = staking_height.unwrap_or(0);
    let (status, window_start, window_end, next_change, next_change_height, blocks_remaining) =
        match tip_height {
            None => (
                CrosslinkStakingStatus::NotStarted,
                None,
                None,
                Some(CrosslinkStakingChange::Opens),
                staking_height,
                None,
            ),
            Some(tip) if tip < first_staking_height => (
                CrosslinkStakingStatus::NotStarted,
                None,
                None,
                Some(CrosslinkStakingChange::Opens),
                Some(first_staking_height),
                Some(first_staking_height - tip),
            ),
            Some(tip) => {
                let offset = tip % period;
                let period_start = tip - offset;
                if offset < window {
                    let closes_at = period_start.saturating_add(window);
                    (
                        CrosslinkStakingStatus::Open,
                        Some(period_start),
                        Some(closes_at.saturating_sub(1)),
                        Some(CrosslinkStakingChange::Closes),
                        Some(closes_at),
                        Some(closes_at.saturating_sub(tip)),
                    )
                } else {
                    let opens_at = period_start.saturating_add(period);
                    (
                        CrosslinkStakingStatus::Closed,
                        None,
                        None,
                        Some(CrosslinkStakingChange::Opens),
                        Some(opens_at),
                        Some(opens_at.saturating_sub(tip)),
                    )
                }
            }
        };

    CrosslinkStakingOverview {
        status,
        window_open: status == CrosslinkStakingStatus::Open,
        window_start_height: window_start.map(|height| height.to_string()),
        window_end_height: window_end.map(|height| height.to_string()),
        next_change,
        next_change_height: next_change_height.map(|height| height.to_string()),
        blocks_remaining: blocks_remaining.map(|blocks| blocks.to_string()),
        estimated_at: blocks_remaining
            .and_then(|blocks| estimated_transition_time(now, blocks, target_block_time_seconds)),
        period_blocks: period.to_string(),
        window_blocks: window.to_string(),
        bonded_zat,
        unbonded_zat,
        finalizer_rewards_zat,
    }
}

#[cfg(feature = "indexer")]
fn crosslink_activation_overview(
    tip_height: Option<u32>,
    staking_height: Option<u32>,
    roster_height: Option<u32>,
    activation_height: Option<u32>,
    now: i64,
    target_block_time_seconds: Option<u64>,
) -> CrosslinkActivationOverview {
    let milestones = [
        (CrosslinkPhase::Staking, staking_height),
        (CrosslinkPhase::FirstFinalizers, roster_height),
        (CrosslinkPhase::Finality, activation_height),
    ]
    .into_iter()
    .filter_map(|(phase, height)| {
        let height = height?;
        let reached = tip_height.is_some_and(|tip| tip >= height);
        let blocks_remaining = tip_height.map_or(height, |tip| height.saturating_sub(tip));
        let estimated_at = if reached || tip_height.is_none() {
            None
        } else {
            estimated_transition_time(now, blocks_remaining, target_block_time_seconds)
        };
        Some(CrosslinkActivationMilestone {
            phase,
            height: height.to_string(),
            reached,
            blocks_remaining: blocks_remaining.to_string(),
            estimated_at,
        })
    })
    .collect();

    let (current_phase, progress_percent) = match (
        tip_height,
        staking_height,
        roster_height,
        activation_height,
    ) {
        (None, _, _, _) => (None, None),
        (Some(_), None, None, None) => (Some(CrosslinkPhase::Finality), None),
        (Some(tip), Some(staking), Some(_), Some(_)) if tip < staking => (
            Some(CrosslinkPhase::Mining),
            Some(phase_progress_percent(tip, 0, staking)),
        ),
        (Some(tip), Some(staking), Some(roster), Some(_)) if tip < roster => (
            Some(CrosslinkPhase::Staking),
            Some(phase_progress_percent(tip, staking, roster)),
        ),
        (Some(tip), Some(_), Some(roster), Some(activation)) if tip < activation => (
            Some(CrosslinkPhase::FirstFinalizers),
            Some(phase_progress_percent(tip, roster, activation)),
        ),
        (Some(_), _, _, _) => (Some(CrosslinkPhase::Finality), None),
    };

    CrosslinkActivationOverview {
        current_phase,
        progress_percent,
        milestones,
    }
}

#[cfg(feature = "indexer")]
fn phase_progress_percent(height: u32, start: u32, end: u32) -> String {
    let span = u64::from(end.saturating_sub(start));
    if span == 0 {
        return "100.0".to_string();
    }

    let elapsed = u64::from(height.saturating_sub(start)).min(span);
    let tenths = elapsed.saturating_mul(1_000).saturating_add(span / 2) / span;
    format!("{}.{:01}", tenths / 10, tenths % 10)
}

#[cfg(feature = "indexer")]
fn estimated_transition_time(
    now: i64,
    blocks_remaining: u32,
    target_block_time_seconds: Option<u64>,
) -> Option<String> {
    let seconds = u64::from(blocks_remaining).checked_mul(target_block_time_seconds?)?;
    let seconds = i64::try_from(seconds).ok()?;
    now.checked_add(seconds).map(|timestamp| timestamp.to_string())
}

#[cfg(not(feature = "indexer"))]
fn explorer_index_disabled<T>() -> Result<T> {
    Err("explorer state index is not enabled in this zebrad process").map_misc_error()
}

#[cfg(feature = "indexer")]
fn explorer_transparent_address(
    network: &zebra_chain::parameters::Network,
    encoded: &str,
) -> std::result::Result<Address, String> {
    let address = encoded
        .parse::<Address>()
        .map_err(|_| "invalid transparent address".to_string())?;
    if address.network_kind() != network.kind() {
        return Err("transparent address belongs to a different network".to_string());
    }
    if matches!(address, Address::Tex { .. }) {
        return Err(
            "TEX addresses do not identify the receiving address stored on-chain".to_string(),
        );
    }

    Ok(address)
}

#[cfg(feature = "indexer")]
fn indexer_status(
    chain_tip: Option<(Height, block::Hash)>,
    indexed_height: Option<&str>,
    indexed_block_hash: Option<&str>,
) -> IndexerStatusResponse {
    let indexed_height_value = indexed_height.map(|height| {
        height
            .parse::<u64>()
            .expect("indexer heights are generated from valid u32 values")
    });
    let chain_height = chain_tip.map(|(height, _)| u64::from(height.0));
    let indexed_block_count = indexed_height_value.map_or(0, |height| height.saturating_add(1));
    let chain_block_count = chain_height.map_or(0, |height| height.saturating_add(1));
    let lag = chain_block_count.saturating_sub(indexed_block_count);
    let empty_chain_progress = if indexed_block_count == 0 {
        1_000_000
    } else {
        0
    };
    let progress_units = indexed_block_count
        .min(chain_block_count)
        .saturating_mul(1_000_000)
        .checked_div(chain_block_count)
        .unwrap_or(empty_chain_progress);
    let synced = match (chain_tip, indexed_height_value.as_ref()) {
        (None, None) => true,
        (Some((height, hash)), Some(indexed_height)) => {
            let chain_hash = hash.to_string();
            u64::from(height.0) == *indexed_height
                && indexed_block_hash == Some(chain_hash.as_str())
        }
        (None, Some(_)) | (Some(_), None) => false,
    };

    IndexerStatusResponse {
        chain_height: chain_tip.map(|(height, _)| height.0.to_string()),
        chain_block_hash: chain_tip.map(|(_, hash)| hash.to_string()),
        indexed_height: indexed_height.map(ToOwned::to_owned),
        indexed_block_hash: indexed_block_hash.map(ToOwned::to_owned),
        lag: lag.to_string(),
        sync_progress: format!("{}.{:04}", progress_units / 10_000, progress_units % 10_000),
        synced,
    }
}

#[cfg(all(test, feature = "indexer"))]
mod tests {
    use zebra_chain::block::{Hash, Height};

    use super::{
        crosslink_activation_overview, crosslink_staking_overview, indexer_status, CrosslinkPhase,
        CrosslinkStakingChange, CrosslinkStakingStatus,
    };

    #[test]
    fn indexer_status_compares_height_and_hash() {
        let hash = Hash([0x51; 32]);
        let encoded_hash = hash.to_string();

        let synced = indexer_status(Some((Height(10), hash)), Some("10"), Some(&encoded_hash));
        assert!(synced.synced);
        assert_eq!(synced.lag, "0");
        assert_eq!(synced.sync_progress, "100.0000");

        let lagged = indexer_status(Some((Height(10), hash)), Some("9"), Some(&encoded_hash));
        assert!(!lagged.synced);
        assert_eq!(lagged.lag, "1");
        assert_eq!(lagged.sync_progress, "90.9090");

        let empty = indexer_status(None, None, None);
        assert!(empty.synced);
        assert_eq!(empty.sync_progress, "100.0000");
    }

    #[test]
    fn crosslink_activation_matches_the_prototype_timeline() {
        let overview = crosslink_activation_overview(
            Some(20_091),
            Some(20_736),
            Some(34_560),
            Some(36_288),
            1_000,
            Some(25),
        );

        assert_eq!(overview.current_phase, Some(CrosslinkPhase::Mining));
        assert_eq!(overview.progress_percent.as_deref(), Some("96.9"));
        assert_eq!(overview.milestones[0].blocks_remaining, "645");
        assert_eq!(overview.milestones[0].estimated_at.as_deref(), Some("17125"));
        assert_eq!(overview.milestones[2].height, "36288");
    }

    #[test]
    fn crosslink_staking_reports_open_and_closed_windows() {
        let open = crosslink_staking_overview(
            Some(20_736),
            Some(20_736),
            10_368,
            3_456,
            1_000,
            Some(25),
            "1".to_string(),
            "2".to_string(),
            "3".to_string(),
        );
        assert_eq!(open.status, CrosslinkStakingStatus::Open);
        assert_eq!(open.window_start_height.as_deref(), Some("20736"));
        assert_eq!(open.window_end_height.as_deref(), Some("24191"));
        assert_eq!(open.next_change, Some(CrosslinkStakingChange::Closes));
        assert_eq!(open.blocks_remaining.as_deref(), Some("3456"));

        let closed = crosslink_staking_overview(
            Some(24_192),
            Some(20_736),
            10_368,
            3_456,
            1_000,
            Some(25),
            "1".to_string(),
            "2".to_string(),
            "3".to_string(),
        );
        assert_eq!(closed.status, CrosslinkStakingStatus::Closed);
        assert_eq!(closed.next_change, Some(CrosslinkStakingChange::Opens));
        assert_eq!(closed.next_change_height.as_deref(), Some("31104"));
        assert_eq!(closed.blocks_remaining.as_deref(), Some("6912"));
    }
}
