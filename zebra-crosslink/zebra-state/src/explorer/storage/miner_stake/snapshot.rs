//! Best-chain stake attribution over a pinned finalized database view.

use super::super::stake_history;
use super::*;
use crate::service::{finalized_state::TypedColumnFamily, non_finalized_state::Chain};
use std::ops::RangeBounds;
use zebra_chain::parameters::subsidy::{block_subsidy, miner_subsidy};

impl<'db> MinerStakeUpdates<'db> {
    pub(crate) fn for_chain(db: &'db ZebraDb, chain: Option<&Chain>) -> Self {
        let mut updates = Self::new(db, None);
        let mut identities = HashMap::new();
        let finalized_tip = updates.finalized_tip();
        let network = db.network();
        if let Some(chain) = chain {
            for (index, contextual) in chain.blocks.values().enumerate() {
                if finalized_tip.is_some_and(|(height, _)| contextual.height <= height) {
                    continue;
                }
                for address in contextual
                    .block
                    .transactions
                    .iter()
                    .filter_map(|transaction| transaction.staking_action())
                    .flat_map(|action| {
                        [
                            action.target_finalizer_address(),
                            action.from_finalizer_address(),
                        ]
                    })
                    .flatten()
                {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        updates.addresses.entry(address.pub_key.0)
                    {
                        if address.verify() {
                            entry.insert(address);
                        }
                    }
                }
                let miner = crate::explorer::analytics::miner_address(&contextual.block, &network);
                updates.current_block_miner = miner;
                if let Some(address) = miner {
                    let previous = updates.miner_record(address);
                    if previous.is_none() {
                        updates.promote_transparent_source(address);
                    }
                    let subsidy = block_subsidy(contextual.height, &network)
                        .expect("verified block height has a valid subsidy");
                    let subsidy = miner_subsidy(contextual.height, &network, subsidy)
                        .expect("verified block subsidy has a valid miner share");
                    let mut record = previous.unwrap_or(ExplorerMinerRecord {
                        block_count: 0,
                        mined_zat: 0,
                        latest_height: contextual.height.0,
                        latest_block_hash: contextual.hash,
                        latest_timestamp: contextual.block.header.time.timestamp(),
                    });
                    record.block_count = record
                        .block_count
                        .checked_add(1)
                        .expect("canonical block count fits in u64");
                    record.mined_zat = record
                        .mined_zat
                        .checked_add(u128::from(u64::from(subsidy)))
                        .expect("canonical miner subsidy total fits in u128");
                    record.latest_height = contextual.height.0;
                    record.latest_block_hash = contextual.hash;
                    record.latest_timestamp = contextual.block.header.time.timestamp();
                    updates.miners.insert(address, record);
                }
                let spent_utxos = contextual
                    .spent_outputs
                    .iter()
                    .map(|(key, utxo)| (*key, utxo.utxo.clone()))
                    .collect();
                let records = stake_history::stake_history_records(
                    contextual.height,
                    &contextual.block.transactions,
                    &mut identities,
                    |key| updates.bond_identity(key),
                    |_, transaction| classify_stake_source(&network, transaction, &spent_utxos),
                );
                updates
                    .history
                    .extend(records.into_iter().map(|(location, record)| {
                        crate::ExplorerStakeHistoryEntry {
                            location,
                            record,
                            txid: contextual.transaction_hashes[location.index.as_usize()],
                            block_hash: contextual.hash,
                            block_time: contextual.block.header.time.timestamp(),
                        }
                    }));
                let burns = chain.bond_burns[index]
                    .iter()
                    .map(|(key, _)| *key)
                    .collect::<Vec<_>>();
                updates.apply_block(
                    &network,
                    &contextual.block,
                    (contextual.height, contextual.hash),
                    &spent_utxos,
                    &chain.bond_rewards[index],
                    &burns,
                );
                for (key, reward) in &chain.bond_rewards[index] {
                    if let Some(Some(identity)) = identities.get_mut(key) {
                        identity.amount_zat = identity.amount_zat.map(|amount| {
                            amount
                                .checked_add(*reward)
                                .expect("verified bond value fits in u64")
                        });
                    }
                }
            }
        }
        updates.current_block_miner = None;
        updates
    }

    #[allow(clippy::unwrap_in_result)]
    fn finalized_tip(&self) -> Option<(Height, block::Hash)> {
        let cf = TypedColumnFamily::<Height, block::Hash>::new(self.db.disk_db(), "hash_by_height")
            .expect("canonical block hash column family is registered");
        let tip = cf.zs_range_iter_at(&self.snapshot, .., true).next();
        tip
    }

    pub(crate) fn best_tip(&self, chain: Option<&Chain>) -> Option<(Height, block::Hash)> {
        let finalized = self.finalized_tip();
        chain
            .and_then(Chain::tip_block)
            .map(|block| (block.height, block.hash))
            .filter(|(height, _)| {
                finalized.is_none_or(|(finalized_height, _)| *height > finalized_height)
            })
            .or(finalized)
    }

    pub(crate) fn aggregated_stakes(&self, chain: Option<&Chain>) -> Option<Vec<([u8; 32], u64)>> {
        let (_, hash) = self.best_tip(chain)?;
        chain
            .and_then(|chain| chain.aggregated_stakes_at(hash))
            .or_else(|| {
                self.db
                    .aggregated_stakes_by_hash_cf()
                    .zs_get_at(&self.snapshot, &hash)
                    .map(|stakes| stakes.0)
            })
    }

    pub(crate) fn finalizer_address(
        &self,
        finalizer: [u8; 32],
    ) -> Option<zcash_primitives::bft::FinalizerAddress> {
        self.addresses
            .get(&finalizer)
            .copied()
            .or_else(|| {
                self.db
                    .finalizer_address_by_key_cf()
                    .zs_get_at(&self.snapshot, &finalizer)
            })
            .filter(|address| address.pub_key.0 == finalizer && address.verify())
    }

    pub(crate) fn miner_record(&self, address: Address) -> Option<ExplorerMinerRecord> {
        self.miners.get(&address).copied().or_else(|| {
            self.db
                .explorer_miner_meta_cf()
                .zs_get_at(&self.snapshot, &address.into())
        })
    }

    pub(crate) fn totals(&self, finalizer: Option<[u8; 32]>) -> ExplorerMinerStakeTotals {
        finalizer.map_or(self.global_totals, |key| {
            self.finalizer_totals.get(&key).copied().unwrap_or_else(|| {
                self.db
                    .explorer_finalizer_miner_totals_cf()
                    .zs_get_at(&self.snapshot, &key)
                    .unwrap_or_default()
            })
        })
    }

    pub(crate) fn contains_entry(
        &self,
        finalizer: Option<[u8; 32]>,
        address: Option<Address>,
        is_miner: Option<bool>,
        cursor: crate::ExplorerMinerStakeRankCursor,
    ) -> bool {
        let key = ExplorerMinerFinalizerKey::new(cursor.address, cursor.finalizer);
        let record = self.pairs.get(&key).copied().unwrap_or_else(|| {
            self.db
                .explorer_miner_finalizer_meta_cf()
                .zs_get_at(&self.snapshot, &key)
        });
        record.is_some_and(|record| {
            record.active_bond_count > 0
                && record.current_stake_zat > 0
                && record.current_stake_zat == cursor.current_stake_zat
        }) && finalizer.is_none_or(|key| key == cursor.finalizer)
            && address.is_none_or(|address| address == cursor.address)
            && is_miner
                .is_none_or(|is_miner| self.miner_record(cursor.address).is_some() == is_miner)
    }

    fn bond_identity(&mut self, key: [u8; 32]) -> Option<stake_history::BondIdentity> {
        if let Some(bond) = self.bond(key) {
            return Some(stake_history::BondIdentity {
                source: bond.source,
                finalizer: Some(bond.current_finalizer),
                amount_zat: Some(bond.current_stake_zat),
            });
        }
        let bond = self
            .db
            .delegation_bond_by_key_cf()
            .zs_get_at(&self.snapshot, &key)?;
        let status = self
            .db
            .bond_status_by_key_cf()
            .zs_get_at(&self.snapshot, &key)?;
        if status.is_withdrawn() || status.is_burned() {
            return None;
        }
        let creation = self
            .db
            .explorer_stake_history_cf()
            .zs_get_at(&self.snapshot, &bond.created_at)?;
        Some(stake_history::BondIdentity {
            source: creation.source,
            finalizer: Some(bond.target_finalizer),
            amount_zat: Some(u64::from(bond.amount)),
        })
    }

    pub(crate) fn history_record(
        &self,
        location: crate::TransactionLocation,
    ) -> Option<crate::ExplorerStakeHistoryRecord> {
        self.history
            .iter()
            .find(|entry| entry.location == location)
            .map(|entry| entry.record)
            .or_else(|| {
                self.db
                    .explorer_stake_history_cf()
                    .zs_get_at(&self.snapshot, &location)
            })
    }

    #[allow(clippy::unwrap_in_result)]
    pub(crate) fn block_hash(&self, chain: Option<&Chain>, height: Height) -> Option<block::Hash> {
        let cf = TypedColumnFamily::<Height, block::Hash>::new(self.db.disk_db(), "hash_by_height")
            .expect("canonical block hash column family is registered");
        cf.zs_get_at(&self.snapshot, &height).or_else(|| {
            chain
                .and_then(|chain| chain.blocks.get(&height))
                .map(|block| block.hash)
        })
    }

    pub(crate) fn history_entries(
        &self,
        filter: crate::ExplorerStakeHistoryFilter,
        cursor: Option<crate::TransactionLocation>,
        direction: ExplorerPageDirection,
        from: Height,
        to: Height,
        limit: usize,
    ) -> Vec<crate::ExplorerStakeHistoryEntry> {
        let minimum = crate::TransactionLocation::min_for_height(from);
        let maximum = crate::TransactionLocation::max_for_height(to);
        let reverse = direction == ExplorerPageDirection::Older;
        let range = match (direction, cursor) {
            (ExplorerPageDirection::Older, Some(cursor)) => (Included(minimum), Excluded(cursor)),
            (ExplorerPageDirection::Newer, Some(cursor)) => (Excluded(cursor), Included(maximum)),
            (ExplorerPageDirection::Older, None) => (Included(minimum), Included(maximum)),
            (ExplorerPageDirection::Newer, None) => return Vec::new(),
        };
        let mut entries = self
            .history
            .iter()
            .filter(|entry| range.contains(&entry.location) && filter.matches(entry.record))
            .copied()
            .collect::<Vec<_>>();
        entries.extend(
            self.db
                .explorer_stake_history_cf()
                .zs_range_iter_at(&self.snapshot, range, reverse)
                .filter(|(_, record)| filter.matches(*record))
                .take(limit)
                .map(|(location, record)| crate::ExplorerStakeHistoryEntry {
                    location,
                    record,
                    txid: self
                        .db
                        .transaction_hash(location)
                        .expect("stake history location has a transaction hash"),
                    block_hash: self
                        .block_hash(None, location.height)
                        .expect("stake history location has a block hash"),
                    block_time: self
                        .db
                        .block_header(location.height.into())
                        .expect("stake history location has a block header")
                        .time
                        .timestamp(),
                }),
        );
        entries.sort_by_key(|entry| entry.location);
        if reverse {
            entries.reverse();
        }
        entries.truncate(limit);
        if !reverse {
            entries.reverse();
        }
        entries
    }

    fn entry(
        &self,
        address: Address,
        finalizer: [u8; 32],
        record: ExplorerMinerFinalizerRecord,
    ) -> ExplorerMinerStakeRankEntry {
        ExplorerMinerStakeRankEntry {
            address,
            finalizer,
            record,
            miner_record: self.miner_record(address),
            finalizer_address: self.finalizer_address(finalizer),
        }
    }

    pub(crate) fn entries(
        &self,
        finalizer: Option<[u8; 32]>,
        address: Option<Address>,
        is_miner: Option<bool>,
        cursor: Option<(Address, [u8; 32], u64)>,
        direction: ExplorerPageDirection,
        limit: usize,
    ) -> Vec<ExplorerMinerStakeRankEntry> {
        if direction == ExplorerPageDirection::Newer && cursor.is_none() {
            return Vec::new();
        }
        let matches = |key: ExplorerMinerFinalizerKey, record: ExplorerMinerFinalizerRecord| {
            record.active_bond_count > 0
                && record.current_stake_zat > 0
                && finalizer.is_none_or(|finalizer| key.finalizer() == finalizer)
                && address.is_none_or(|address| key.address() == address)
                && is_miner
                    .is_none_or(|is_miner| self.miner_record(key.address()).is_some() == is_miner)
                && cursor.is_none_or(|(address, finalizer, stake)| {
                    let cursor = ExplorerMinerFinalizerRankKey::new(address, finalizer, stake);
                    let key = ExplorerMinerFinalizerRankKey::new(
                        key.address(),
                        key.finalizer(),
                        record.current_stake_zat,
                    );
                    match direction {
                        ExplorerPageDirection::Older => key > cursor,
                        ExplorerPageDirection::Newer => key < cursor,
                    }
                })
        };
        let mut entries = self
            .pairs
            .iter()
            .filter_map(|(key, record)| {
                let record = (*record)?;
                matches(*key, record).then(|| self.entry(key.address(), key.finalizer(), record))
            })
            .collect::<Vec<_>>();
        let load = |address, finalizer| {
            let key = ExplorerMinerFinalizerKey::new(address, finalizer);
            if self.pairs.contains_key(&key) {
                return None;
            }
            let record = self
                .db
                .explorer_miner_finalizer_meta_cf()
                .zs_get_at(&self.snapshot, &key)
                .expect("ranked address-finalizer pairs have metadata");
            matches(key, record).then(|| self.entry(address, finalizer, record))
        };
        let reverse = direction == ExplorerPageDirection::Newer;
        if let Some(address) = address {
            entries.extend(
                self.db
                    .explorer_miner_finalizer_meta_cf()
                    .zs_range_iter_at(
                        &self.snapshot,
                        ExplorerMinerFinalizerKey::min_for_address(address)
                            ..=ExplorerMinerFinalizerKey::max_for_address(address),
                        false,
                    )
                    .filter_map(|(key, _)| load(key.address(), key.finalizer())),
            );
        } else if let Some(finalizer) = finalizer {
            let minimum = ExplorerFinalizerMinerRankKey::min_for_finalizer(finalizer);
            let maximum = ExplorerFinalizerMinerRankKey::max_for_finalizer(finalizer);
            let range = match cursor {
                Some((address, _, stake)) if reverse => (
                    Included(minimum),
                    Excluded(ExplorerFinalizerMinerRankKey::new(
                        finalizer, address, stake,
                    )),
                ),
                Some((address, _, stake)) => (
                    Excluded(ExplorerFinalizerMinerRankKey::new(
                        finalizer, address, stake,
                    )),
                    Included(maximum),
                ),
                None => (Included(minimum), Included(maximum)),
            };
            entries.extend(
                self.db
                    .explorer_finalizer_miner_order_cf()
                    .zs_range_iter_at(&self.snapshot, range, reverse)
                    .filter_map(|(key, ())| load(key.address(), finalizer))
                    .take(limit),
            );
        } else {
            let range = match cursor {
                Some((address, finalizer, stake)) if reverse => (
                    Unbounded,
                    Excluded(ExplorerMinerFinalizerRankKey::new(
                        address, finalizer, stake,
                    )),
                ),
                Some((address, finalizer, stake)) => (
                    Excluded(ExplorerMinerFinalizerRankKey::new(
                        address, finalizer, stake,
                    )),
                    Unbounded,
                ),
                None => (Unbounded, Unbounded),
            };
            entries.extend(
                self.db
                    .explorer_miner_finalizer_order_cf()
                    .zs_range_iter_at(&self.snapshot, range, reverse)
                    .filter_map(|(key, ())| load(key.address(), key.finalizer()))
                    .take(limit),
            );
        }
        entries.sort_by_key(|entry| {
            ExplorerMinerFinalizerRankKey::new(
                entry.address,
                entry.finalizer,
                entry.record.current_stake_zat,
            )
        });
        if reverse {
            entries.reverse();
        }
        entries.truncate(limit);
        if reverse {
            entries.reverse();
        }
        entries
    }

    pub(crate) fn finalizer_summary(&self, finalizer: [u8; 32]) -> ExplorerFinalizerStakeSummary {
        let totals = self.totals(Some(finalizer));
        ExplorerFinalizerStakeSummary {
            primary_stake_address: self
                .entries(
                    Some(finalizer),
                    None,
                    None,
                    None,
                    ExplorerPageDirection::Older,
                    1,
                )
                .first()
                .map(|entry| entry.address),
            transparent_address_count: totals
                .miner_address_count
                .saturating_add(totals.other_transparent_address_count),
        }
    }
}
