//! Current stake publicly attributable to known transparent miner addresses.

use std::{
    collections::HashMap,
    ops::Bound::{Excluded, Included, Unbounded},
};

use zcash_primitives::transaction::StakingAction;
use zebra_chain::{
    parameters::Network,
    transaction::{primary_value_endpoints, Transaction, TransactionValueEndpoint},
    transparent::{Address, OutPoint, Utxo},
};

use crate::{
    request::FinalizedBlock,
    service::finalized_state::{DiskWriteBatch, ZebraDb},
    ExplorerBondAttributionRecord, ExplorerFinalizerMinerSummary, ExplorerMinerFinalizerRecord,
    ExplorerMinerStakeRankEntry, ExplorerMinerStakeTotals, ExplorerPageDirection,
    ExplorerStakeSource,
};

use super::disk_format::{
    ExplorerFinalizerMinerRankKey, ExplorerMinerFinalizerKey, ExplorerMinerFinalizerRankKey,
};

impl ZebraDb {
    /// Returns the highest-stake recognized miner address and distinct miner count for a finalizer.
    pub fn explorer_finalizer_miner_summary(
        &self,
        finalizer: [u8; 32],
    ) -> ExplorerFinalizerMinerSummary {
        let minimum = ExplorerFinalizerMinerRankKey::min_for_finalizer(finalizer);
        let maximum = ExplorerFinalizerMinerRankKey::max_for_finalizer(finalizer);
        let primary_miner_address = self
            .explorer_finalizer_miner_order_cf()
            .zs_forward_range_iter(minimum..=maximum)
            .next()
            .map(|(key, ())| key.address());
        let miner_address_count = self
            .explorer_miner_stake_totals(Some(finalizer))
            .miner_address_count;

        ExplorerFinalizerMinerSummary {
            primary_miner_address,
            miner_address_count,
        }
    }

    /// Returns current global or per-finalizer miner-attributed stake totals.
    pub fn explorer_miner_stake_totals(
        &self,
        finalizer: Option<[u8; 32]>,
    ) -> ExplorerMinerStakeTotals {
        finalizer
            .map_or_else(
                || self.explorer_miner_stake_totals_cf().zs_get(&()),
                |finalizer| self.explorer_finalizer_miner_totals_cf().zs_get(&finalizer),
            )
            .unwrap_or_default()
    }

    /// Returns whether an exact current miner-finalizer ranking key exists.
    pub fn explorer_contains_miner_stake_entry(
        &self,
        finalizer_filter: Option<[u8; 32]>,
        address: Address,
        finalizer: [u8; 32],
        current_stake_zat: u64,
    ) -> bool {
        match finalizer_filter {
            Some(filter) if filter == finalizer => {
                self.explorer_finalizer_miner_order_cf().zs_contains(
                    &ExplorerFinalizerMinerRankKey::new(finalizer, address, current_stake_zat),
                )
            }
            Some(_) => false,
            None => self.explorer_miner_finalizer_order_cf().zs_contains(
                &ExplorerMinerFinalizerRankKey::new(address, finalizer, current_stake_zat),
            ),
        }
    }

    /// Returns one cursor-adjacent current miner-attributed stake page.
    pub fn explorer_miner_stake_entries(
        &self,
        finalizer_filter: Option<[u8; 32]>,
        cursor: Option<(Address, [u8; 32], u64)>,
        direction: ExplorerPageDirection,
        limit: usize,
    ) -> Vec<ExplorerMinerStakeRankEntry> {
        let pairs = match finalizer_filter {
            None => {
                let cf = self.explorer_miner_finalizer_order_cf();
                let mut keys = match (direction, cursor) {
                    (ExplorerPageDirection::Older, Some((address, finalizer, stake))) => cf
                        .zs_forward_range_iter((
                            Excluded(ExplorerMinerFinalizerRankKey::new(
                                address, finalizer, stake,
                            )),
                            Unbounded,
                        ))
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Older, None) => cf
                        .zs_forward_range_iter(..)
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Newer, Some((address, finalizer, stake))) => cf
                        .zs_reverse_range_iter(
                            ..ExplorerMinerFinalizerRankKey::new(address, finalizer, stake),
                        )
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Newer, None) => Vec::new(),
                };
                if direction == ExplorerPageDirection::Newer {
                    keys.reverse();
                }
                keys.into_iter()
                    .map(|key| (key.address(), key.finalizer()))
                    .collect::<Vec<_>>()
            }
            Some(finalizer) => {
                let cf = self.explorer_finalizer_miner_order_cf();
                let minimum = ExplorerFinalizerMinerRankKey::min_for_finalizer(finalizer);
                let maximum = ExplorerFinalizerMinerRankKey::max_for_finalizer(finalizer);
                let mut keys = match (direction, cursor) {
                    (ExplorerPageDirection::Older, Some((address, _, stake))) => cf
                        .zs_forward_range_iter((
                            Excluded(ExplorerFinalizerMinerRankKey::new(
                                finalizer, address, stake,
                            )),
                            Included(maximum),
                        ))
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Older, None) => cf
                        .zs_forward_range_iter(minimum..=maximum)
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Newer, Some((address, _, stake))) => cf
                        .zs_reverse_range_iter(
                            minimum..ExplorerFinalizerMinerRankKey::new(finalizer, address, stake),
                        )
                        .take(limit)
                        .map(|(key, ())| key)
                        .collect::<Vec<_>>(),
                    (ExplorerPageDirection::Newer, None) => Vec::new(),
                };
                if direction == ExplorerPageDirection::Newer {
                    keys.reverse();
                }
                keys.into_iter()
                    .map(|key| (key.address(), finalizer))
                    .collect::<Vec<_>>()
            }
        };

        pairs
            .into_iter()
            .map(|(miner_address, finalizer)| {
                let record = self
                    .explorer_miner_finalizer_meta_cf()
                    .zs_get(&ExplorerMinerFinalizerKey::new(miner_address, finalizer))
                    .expect("ranked miner-finalizer pairs have metadata");
                let miner_record = self
                    .explorer_miner_record(miner_address)
                    .expect("attributed stake addresses are indexed miners");
                ExplorerMinerStakeRankEntry {
                    miner_address,
                    miner_record,
                    finalizer,
                    finalizer_address: self.finalizer_address(&finalizer),
                    record,
                }
            })
            .collect()
    }
}

impl DiskWriteBatch {
    /// Updates current bond-source attribution in the finalized block batch.
    pub(crate) fn prepare_explorer_miner_stake_batch(
        &mut self,
        db: &ZebraDb,
        network: &Network,
        finalized: &FinalizedBlock,
        spent_utxos: &HashMap<OutPoint, Utxo>,
        current_block_miner: Option<Address>,
        new_miner: bool,
    ) {
        let mut updates = MinerStakeUpdates::new(db, current_block_miner);
        let latest_timestamp = finalized.block.header.time.timestamp();
        if new_miner {
            updates.promote_transparent_source(
                current_block_miner.expect("a new miner has a transparent payout address"),
            );
        }

        for transaction in &finalized.block.transactions {
            let Some(action) = transaction.staking_action() else {
                continue;
            };
            match action {
                StakingAction::CreateNewDelegationBond {
                    amount_zats,
                    unique_pubkey,
                    target_finalizer,
                    ..
                } => {
                    let attribution = ExplorerBondAttributionRecord {
                        source: classify_stake_source(network, transaction, spent_utxos),
                        current_finalizer: target_finalizer.pub_key.0,
                        current_stake_zat: *amount_zats,
                    };
                    updates.set_bond(*unique_pubkey, Some(attribution));
                    updates.add_attribution(attribution, finalized, latest_timestamp, true);
                }
                StakingAction::RetargetDelegationBond {
                    unique_pubkey,
                    to_finalizer,
                    ..
                } => {
                    let Some(mut attribution) = updates.bond(*unique_pubkey) else {
                        continue;
                    };
                    updates.remove_attribution(attribution);
                    attribution.current_finalizer = to_finalizer.pub_key.0;
                    updates.set_bond(*unique_pubkey, Some(attribution));
                    updates.add_attribution(attribution, finalized, latest_timestamp, true);
                }
                StakingAction::BeginDelegationUnbonding { unique_pubkey, .. }
                | StakingAction::WithdrawDelegationBond { unique_pubkey, .. } => {
                    if let Some(attribution) = updates.bond(*unique_pubkey) {
                        updates.remove_attribution(attribution);
                        updates.set_bond(*unique_pubkey, None);
                    }
                }
                StakingAction::ConvertFinalizerRewardToDelegationBond {
                    amount_zats,
                    unique_pubkey,
                    this_finalizer,
                    ..
                } => {
                    let attribution = ExplorerBondAttributionRecord {
                        source: ExplorerStakeSource::Rewards,
                        current_finalizer: *this_finalizer,
                        current_stake_zat: *amount_zats,
                    };
                    updates.set_bond(*unique_pubkey, Some(attribution));
                    updates.add_attribution(attribution, finalized, latest_timestamp, true);
                }
            }
        }

        for (bond_key, reward) in &finalized.bond_rewards {
            let Some(mut attribution) = updates.bond(*bond_key) else {
                continue;
            };
            updates.add_reward(attribution, *reward);
            attribution.current_stake_zat = attribution
                .current_stake_zat
                .checked_add(*reward)
                .expect("verified miner-attributed bond value fits in u64");
            updates.set_bond(*bond_key, Some(attribution));
        }

        for bond_key in &finalized.bond_burns {
            if let Some(attribution) = updates.bond(*bond_key) {
                updates.remove_attribution(attribution);
                updates.set_bond(*bond_key, None);
            }
        }

        updates.write(self);
    }
}

struct MinerStakeUpdates<'db> {
    db: &'db ZebraDb,
    bonds: HashMap<[u8; 32], Option<ExplorerBondAttributionRecord>>,
    pairs: HashMap<ExplorerMinerFinalizerKey, Option<ExplorerMinerFinalizerRecord>>,
    original_pairs: HashMap<ExplorerMinerFinalizerKey, Option<ExplorerMinerFinalizerRecord>>,
    global_totals: ExplorerMinerStakeTotals,
    finalizer_totals: HashMap<[u8; 32], ExplorerMinerStakeTotals>,
    ranked_pairs: HashMap<ExplorerMinerFinalizerKey, bool>,
    current_block_miner: Option<Address>,
    changed: bool,
}

impl<'db> MinerStakeUpdates<'db> {
    fn new(db: &'db ZebraDb, current_block_miner: Option<Address>) -> Self {
        Self {
            db,
            bonds: HashMap::new(),
            pairs: HashMap::new(),
            original_pairs: HashMap::new(),
            global_totals: db.explorer_miner_stake_totals(None),
            finalizer_totals: HashMap::new(),
            ranked_pairs: HashMap::new(),
            current_block_miner,
            changed: false,
        }
    }

    fn bond(&mut self, key: [u8; 32]) -> Option<ExplorerBondAttributionRecord> {
        if let Some(record) = self.bonds.get(&key) {
            return *record;
        }
        let record = self.db.explorer_bond_attribution_cf().zs_get(&key);
        self.bonds.insert(key, record);
        record
    }

    fn set_bond(&mut self, key: [u8; 32], record: Option<ExplorerBondAttributionRecord>) {
        self.bonds.insert(key, record);
        self.changed = true;
    }

    fn pair(&mut self, key: ExplorerMinerFinalizerKey) -> Option<ExplorerMinerFinalizerRecord> {
        if let Some(record) = self.pairs.get(&key) {
            return *record;
        }
        let record = self.db.explorer_miner_finalizer_meta_cf().zs_get(&key);
        self.original_pairs.insert(key, record);
        self.pairs.insert(key, record);
        record
    }

    fn finalizer_totals(&mut self, finalizer: [u8; 32]) -> &mut ExplorerMinerStakeTotals {
        self.finalizer_totals
            .entry(finalizer)
            .or_insert_with(|| self.db.explorer_miner_stake_totals(Some(finalizer)))
    }

    fn is_miner(&self, address: Address) -> bool {
        self.current_block_miner == Some(address)
            || self.db.explorer_miner_record(address).is_some()
    }

    fn promote_transparent_source(&mut self, address: Address) {
        let minimum = ExplorerMinerFinalizerKey::min_for_address(address);
        let maximum = ExplorerMinerFinalizerKey::max_for_address(address);
        let records = self
            .db
            .explorer_miner_finalizer_meta_cf()
            .zs_forward_range_iter(minimum..=maximum)
            .filter(|(_, record)| record.active_bond_count > 0)
            .collect::<Vec<_>>();

        for (key, record) in records {
            self.original_pairs.entry(key).or_insert(Some(record));
            self.pairs.insert(key, Some(record));
            self.ranked_pairs.insert(key, true);
            move_transparent_totals(
                &mut self.global_totals,
                record.current_stake_zat,
                false,
                true,
            );
            move_transparent_totals(
                self.finalizer_totals(key.finalizer()),
                record.current_stake_zat,
                false,
                true,
            );
            self.changed = true;
        }
    }

    fn add_attribution(
        &mut self,
        attribution: ExplorerBondAttributionRecord,
        finalized: &FinalizedBlock,
        latest_timestamp: i64,
        count_action: bool,
    ) {
        match attribution.source {
            ExplorerStakeSource::Transparent(address) => self.add_pair(
                address,
                attribution,
                finalized,
                latest_timestamp,
                count_action,
            ),
            source => self.add_non_transparent(
                source,
                attribution.current_finalizer,
                attribution.current_stake_zat,
                true,
            ),
        }
    }

    fn add_pair(
        &mut self,
        address: Address,
        attribution: ExplorerBondAttributionRecord,
        finalized: &FinalizedBlock,
        latest_timestamp: i64,
        count_action: bool,
    ) {
        let key = ExplorerMinerFinalizerKey::new(address, attribution.current_finalizer);
        let previous = self.pair(key);
        let mut record = previous.unwrap_or(ExplorerMinerFinalizerRecord {
            current_stake_zat: 0,
            active_bond_count: 0,
            stake_action_count: 0,
            latest_height: finalized.height.0,
            latest_block_hash: finalized.hash,
            latest_timestamp,
        });
        let new_pair = record.active_bond_count == 0;
        record.current_stake_zat = record
            .current_stake_zat
            .checked_add(attribution.current_stake_zat)
            .expect("verified miner-finalizer stake fits in u64");
        record.active_bond_count = record
            .active_bond_count
            .checked_add(1)
            .expect("active miner-funded bond count fits in u64");
        if count_action {
            record.stake_action_count = record
                .stake_action_count
                .checked_add(1)
                .expect("transparent stake action count fits in u64");
            record.latest_height = finalized.height.0;
            record.latest_block_hash = finalized.hash;
            record.latest_timestamp = latest_timestamp;
        }
        self.pairs.insert(key, Some(record));
        let is_miner = self.is_miner(address);
        self.ranked_pairs.insert(key, is_miner);
        self.add_transparent_totals(
            attribution.current_finalizer,
            attribution.current_stake_zat,
            new_pair,
            is_miner,
        );
    }

    fn add_reward(&mut self, attribution: ExplorerBondAttributionRecord, reward: u64) {
        match attribution.source {
            ExplorerStakeSource::Transparent(address) => {
                let key = ExplorerMinerFinalizerKey::new(address, attribution.current_finalizer);
                let mut record = self
                    .pair(key)
                    .expect("an attributed transparent bond has source metadata");
                record.current_stake_zat = record
                    .current_stake_zat
                    .checked_add(reward)
                    .expect("rewarded transparent stake fits in u64");
                self.pairs.insert(key, Some(record));
                let is_miner = self.is_miner(address);
                self.ranked_pairs.insert(key, is_miner);
                self.add_transparent_totals(attribution.current_finalizer, reward, false, is_miner);
            }
            source => {
                self.add_non_transparent(source, attribution.current_finalizer, reward, false)
            }
        }
    }

    fn remove_attribution(&mut self, attribution: ExplorerBondAttributionRecord) {
        let ExplorerStakeSource::Transparent(address) = attribution.source else {
            self.remove_non_transparent(
                attribution.source,
                attribution.current_finalizer,
                attribution.current_stake_zat,
                true,
            );
            return;
        };
        let key = ExplorerMinerFinalizerKey::new(address, attribution.current_finalizer);
        let mut record = self
            .pair(key)
            .expect("an attributed transparent bond has source metadata");
        record.current_stake_zat = record
            .current_stake_zat
            .checked_sub(attribution.current_stake_zat)
            .expect("attributed bond stake was included in its pair");
        record.active_bond_count = record
            .active_bond_count
            .checked_sub(1)
            .expect("attributed bond count was included in its pair");
        let pair_removed = record.active_bond_count == 0;
        if pair_removed {
            debug_assert_eq!(record.current_stake_zat, 0);
        }
        self.pairs.insert(key, Some(record));
        let is_miner = self.is_miner(address);
        self.ranked_pairs.insert(key, is_miner && !pair_removed);
        self.remove_transparent_totals(
            attribution.current_finalizer,
            attribution.current_stake_zat,
            pair_removed,
            is_miner,
        );
    }

    fn add_transparent_totals(
        &mut self,
        finalizer: [u8; 32],
        amount: u64,
        new_pair: bool,
        is_miner: bool,
    ) {
        add_transparent_totals(&mut self.global_totals, amount, new_pair, is_miner);
        add_transparent_totals(self.finalizer_totals(finalizer), amount, new_pair, is_miner);
        self.changed = true;
    }

    fn remove_transparent_totals(
        &mut self,
        finalizer: [u8; 32],
        amount: u64,
        removed_pair: bool,
        is_miner: bool,
    ) {
        remove_transparent_totals(&mut self.global_totals, amount, removed_pair, is_miner);
        remove_transparent_totals(
            self.finalizer_totals(finalizer),
            amount,
            removed_pair,
            is_miner,
        );
        self.changed = true;
    }

    fn add_non_transparent(
        &mut self,
        source: ExplorerStakeSource,
        finalizer: [u8; 32],
        amount: u64,
        new_bond: bool,
    ) {
        add_non_transparent(&mut self.global_totals, source, amount, new_bond);
        add_non_transparent(self.finalizer_totals(finalizer), source, amount, new_bond);
        self.changed = true;
    }

    fn remove_non_transparent(
        &mut self,
        source: ExplorerStakeSource,
        finalizer: [u8; 32],
        amount: u64,
        removed_bond: bool,
    ) {
        remove_non_transparent(&mut self.global_totals, source, amount, removed_bond);
        remove_non_transparent(
            self.finalizer_totals(finalizer),
            source,
            amount,
            removed_bond,
        );
        self.changed = true;
    }

    fn write(self, batch: &mut DiskWriteBatch) {
        if !self.changed {
            return;
        }

        for (bond_key, record) in self.bonds {
            let writer = self
                .db
                .explorer_bond_attribution_cf()
                .with_batch_for_writing(batch);
            if let Some(record) = record {
                let _ = writer.zs_insert(&bond_key, &record);
            } else {
                let _ = writer.zs_delete(&bond_key);
            }
        }

        for (key, record) in self.pairs {
            let original = self.original_pairs.get(&key).copied().flatten();
            if let Some(original) = original {
                let _ = self
                    .db
                    .explorer_miner_finalizer_order_cf()
                    .with_batch_for_writing(batch)
                    .zs_delete(&ExplorerMinerFinalizerRankKey::new(
                        key.address(),
                        key.finalizer(),
                        original.current_stake_zat,
                    ));
                let _ = self
                    .db
                    .explorer_finalizer_miner_order_cf()
                    .with_batch_for_writing(batch)
                    .zs_delete(&ExplorerFinalizerMinerRankKey::new(
                        key.finalizer(),
                        key.address(),
                        original.current_stake_zat,
                    ));
            }
            if let Some(record) = record {
                let _ = self
                    .db
                    .explorer_miner_finalizer_meta_cf()
                    .with_batch_for_writing(batch)
                    .zs_insert(&key, &record);
                if record.current_stake_zat > 0
                    && self
                        .ranked_pairs
                        .get(&key)
                        .copied()
                        .unwrap_or_else(|| self.db.explorer_miner_record(key.address()).is_some())
                {
                    let _ = self
                        .db
                        .explorer_miner_finalizer_order_cf()
                        .with_batch_for_writing(batch)
                        .zs_insert(
                            &ExplorerMinerFinalizerRankKey::new(
                                key.address(),
                                key.finalizer(),
                                record.current_stake_zat,
                            ),
                            &(),
                        );
                    let _ = self
                        .db
                        .explorer_finalizer_miner_order_cf()
                        .with_batch_for_writing(batch)
                        .zs_insert(
                            &ExplorerFinalizerMinerRankKey::new(
                                key.finalizer(),
                                key.address(),
                                record.current_stake_zat,
                            ),
                            &(),
                        );
                }
            } else {
                let _ = self
                    .db
                    .explorer_miner_finalizer_meta_cf()
                    .with_batch_for_writing(batch)
                    .zs_delete(&key);
            }
        }

        let _ = self
            .db
            .explorer_miner_stake_totals_cf()
            .with_batch_for_writing(batch)
            .zs_insert(&(), &self.global_totals);
        for (finalizer, totals) in self.finalizer_totals {
            let writer = self
                .db
                .explorer_finalizer_miner_totals_cf()
                .with_batch_for_writing(batch);
            if totals_are_empty(totals) {
                let _ = writer.zs_delete(&finalizer);
            } else {
                let _ = writer.zs_insert(&finalizer, &totals);
            }
        }
    }
}

fn classify_stake_source(
    network: &Network,
    transaction: &Transaction,
    spent_utxos: &HashMap<OutPoint, Utxo>,
) -> ExplorerStakeSource {
    let mut spent_outputs = Vec::new();
    for input in transaction.inputs() {
        let Some(outpoint) = input.outpoint() else {
            return ExplorerStakeSource::Unknown;
        };
        let Some(utxo) = spent_utxos.get(&outpoint) else {
            return ExplorerStakeSource::Unknown;
        };
        spent_outputs.push(&utxo.output);
    }

    let Ok((primary_from, _)) = primary_value_endpoints(transaction, network, spent_outputs) else {
        return ExplorerStakeSource::Unknown;
    };

    classify_primary_stake_source(primary_from)
}

fn classify_primary_stake_source(
    primary_from: Option<TransactionValueEndpoint>,
) -> ExplorerStakeSource {
    match primary_from {
        Some(TransactionValueEndpoint::Transparent(address)) => {
            ExplorerStakeSource::Transparent(address)
        }
        Some(
            TransactionValueEndpoint::Sprout
            | TransactionValueEndpoint::Sapling
            | TransactionValueEndpoint::Orchard
            | TransactionValueEndpoint::Ironwood,
        ) => ExplorerStakeSource::Shielded,
        Some(
            TransactionValueEndpoint::Coinbase
            | TransactionValueEndpoint::StakingBonded
            | TransactionValueEndpoint::StakingUnbonded
            | TransactionValueEndpoint::FinalizerRewards,
        )
        | None => ExplorerStakeSource::Unknown,
    }
}

fn add_transparent_totals(
    totals: &mut ExplorerMinerStakeTotals,
    amount: u64,
    new_pair: bool,
    is_miner: bool,
) {
    let (stake, count) = if is_miner {
        (&mut totals.miner_stake_zat, &mut totals.miner_address_count)
    } else {
        (
            &mut totals.other_transparent_stake_zat,
            &mut totals.other_transparent_address_count,
        )
    };
    *stake = stake
        .checked_add(amount)
        .expect("transparent source stake fits in u64");
    if new_pair {
        *count = count
            .checked_add(1)
            .expect("transparent source pair count fits in u64");
    }
}

fn remove_transparent_totals(
    totals: &mut ExplorerMinerStakeTotals,
    amount: u64,
    removed_pair: bool,
    is_miner: bool,
) {
    let (stake, count) = if is_miner {
        (&mut totals.miner_stake_zat, &mut totals.miner_address_count)
    } else {
        (
            &mut totals.other_transparent_stake_zat,
            &mut totals.other_transparent_address_count,
        )
    };
    *stake = stake
        .checked_sub(amount)
        .expect("removed transparent stake was included in source totals");
    if removed_pair {
        *count = count
            .checked_sub(1)
            .expect("removed transparent pair was included in source totals");
    }
}

fn move_transparent_totals(
    totals: &mut ExplorerMinerStakeTotals,
    amount: u64,
    from_is_miner: bool,
    to_is_miner: bool,
) {
    remove_transparent_totals(totals, amount, true, from_is_miner);
    add_transparent_totals(totals, amount, true, to_is_miner);
}

fn add_non_transparent(
    totals: &mut ExplorerMinerStakeTotals,
    source: ExplorerStakeSource,
    amount: u64,
    new_bond: bool,
) {
    let (stake, count) = non_transparent_totals_mut(totals, source);
    *stake = stake
        .checked_add(amount)
        .expect("non-transparent source stake fits in u64");
    if new_bond {
        *count = count
            .checked_add(1)
            .expect("non-transparent source bond count fits in u64");
    }
}

fn remove_non_transparent(
    totals: &mut ExplorerMinerStakeTotals,
    source: ExplorerStakeSource,
    amount: u64,
    removed_bond: bool,
) {
    let (stake, count) = non_transparent_totals_mut(totals, source);
    *stake = stake
        .checked_sub(amount)
        .expect("removed stake was included in non-transparent source totals");
    if removed_bond {
        *count = count
            .checked_sub(1)
            .expect("removed bond was included in non-transparent source totals");
    }
}

fn non_transparent_totals_mut(
    totals: &mut ExplorerMinerStakeTotals,
    source: ExplorerStakeSource,
) -> (&mut u64, &mut u64) {
    match source {
        ExplorerStakeSource::Shielded => (
            &mut totals.shielded_stake_zat,
            &mut totals.shielded_bond_count,
        ),
        ExplorerStakeSource::Unknown => (
            &mut totals.unknown_stake_zat,
            &mut totals.unknown_bond_count,
        ),
        ExplorerStakeSource::Rewards => (
            &mut totals.reward_bond_stake_zat,
            &mut totals.reward_bond_count,
        ),
        ExplorerStakeSource::Transparent(_) => {
            unreachable!("transparent sources use address-aware totals")
        }
    }
}

fn totals_are_empty(totals: ExplorerMinerStakeTotals) -> bool {
    totals == ExplorerMinerStakeTotals::default()
}

#[cfg(test)]
mod tests {
    use super::{
        add_non_transparent, add_transparent_totals, classify_primary_stake_source,
        move_transparent_totals, remove_non_transparent, remove_transparent_totals,
        ExplorerFinalizerMinerRankKey,
    };
    use crate::{
        constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
        service::finalized_state::{DiskWriteBatch, ZebraDb, STATE_COLUMN_FAMILIES_IN_CODE},
        Config, ExplorerMinerStakeTotals, ExplorerStakeSource,
    };
    use zebra_chain::{
        parameters::{Network, NetworkKind},
        transaction::TransactionValueEndpoint,
        transparent::Address,
    };

    #[test]
    fn finalizer_miner_summary_uses_highest_stake_address_and_total_count() {
        let db = ZebraDb::new(
            &Config::ephemeral(),
            STATE_DATABASE_KIND,
            &state_database_format_version_in_code(),
            &Network::Mainnet,
            true,
            STATE_COLUMN_FAMILIES_IN_CODE
                .iter()
                .map(ToString::to_string),
            false,
        )
        .expect("opening an ephemeral database should succeed");
        let finalizer = [3; 32];
        let primary = Address::from_pub_key_hash(NetworkKind::Mainnet, [2; 20]);
        let secondary = Address::from_pub_key_hash(NetworkKind::Mainnet, [1; 20]);
        let unrelated = Address::from_pub_key_hash(NetworkKind::Mainnet, [9; 20]);

        let mut batch = DiskWriteBatch::new();
        let _ = db
            .explorer_finalizer_miner_order_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &ExplorerFinalizerMinerRankKey::new(finalizer, secondary, 100),
                &(),
            );
        let _ = db
            .explorer_finalizer_miner_order_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &ExplorerFinalizerMinerRankKey::new(finalizer, primary, 200),
                &(),
            );
        let _ = db
            .explorer_finalizer_miner_order_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &ExplorerFinalizerMinerRankKey::new([4; 32], unrelated, 300),
                &(),
            );
        let _ = db
            .explorer_finalizer_miner_totals_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &finalizer,
                &ExplorerMinerStakeTotals {
                    miner_address_count: 2,
                    ..Default::default()
                },
            );
        db.write_batch(batch)
            .expect("writing finalizer miner summary fixtures should succeed");

        let summary = db.explorer_finalizer_miner_summary(finalizer);
        assert_eq!(summary.primary_miner_address, Some(primary));
        assert_eq!(summary.miner_address_count, 2);
    }

    #[test]
    fn primary_source_classification_keeps_transparent_funding() {
        let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);

        assert_eq!(
            classify_primary_stake_source(Some(TransactionValueEndpoint::Transparent(address))),
            ExplorerStakeSource::Transparent(address)
        );
        assert_eq!(
            classify_primary_stake_source(Some(TransactionValueEndpoint::Ironwood)),
            ExplorerStakeSource::Shielded
        );
        assert_eq!(
            classify_primary_stake_source(None),
            ExplorerStakeSource::Unknown
        );
    }

    #[test]
    fn source_totals_track_reclassification_rewards_and_removal() {
        let mut totals = ExplorerMinerStakeTotals::default();

        add_transparent_totals(&mut totals, 100, true, false);
        assert_eq!(totals.other_transparent_stake_zat, 100);
        assert_eq!(totals.other_transparent_address_count, 1);

        move_transparent_totals(&mut totals, 100, false, true);
        assert_eq!(totals.other_transparent_stake_zat, 0);
        assert_eq!(totals.other_transparent_address_count, 0);
        assert_eq!(totals.miner_stake_zat, 100);
        assert_eq!(totals.miner_address_count, 1);

        add_non_transparent(&mut totals, ExplorerStakeSource::Shielded, 200, true);
        add_non_transparent(&mut totals, ExplorerStakeSource::Rewards, 50, true);
        assert_eq!(totals.shielded_stake_zat, 200);
        assert_eq!(totals.shielded_bond_count, 1);
        assert_eq!(totals.reward_bond_stake_zat, 50);
        assert_eq!(totals.reward_bond_count, 1);

        remove_non_transparent(&mut totals, ExplorerStakeSource::Rewards, 50, true);
        remove_non_transparent(&mut totals, ExplorerStakeSource::Shielded, 200, true);
        remove_transparent_totals(&mut totals, 100, true, true);
        assert_eq!(totals, ExplorerMinerStakeTotals::default());
    }
}
