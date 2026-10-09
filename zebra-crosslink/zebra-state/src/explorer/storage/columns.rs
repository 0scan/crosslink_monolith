//! Explorer column-family registration and typed accessors.

use crate::service::finalized_state::{
    DiskWriteBatch, TransactionLocation, TypedColumnFamily, ZebraDb,
};

use super::disk_format::{
    ExplorerAddressKey, ExplorerAddressRecord, ExplorerBalanceKey, ExplorerDayKey,
    ExplorerFinalizerMinerRankKey, ExplorerMinerFinalizerKey, ExplorerMinerFinalizerRankKey,
    ExplorerMinerRankKey, ExplorerSchemaVersion, ExplorerShieldedClassLocation,
    ExplorerTransactionKindLocation, ExplorerTransactionRecord,
};

/// Fixed-width metadata keyed once per finalized transaction.
pub const EXPLORER_TRANSACTION_META_BY_LOC: &str = "explorer_tx_meta_by_loc";
/// Explorer-owned schema marker, independent from the canonical state format version.
pub const EXPLORER_SCHEMA: &str = "explorer_schema";
/// One chain-ordered key per finalized transaction, partitioned by kind.
pub const EXPLORER_TRANSACTION_BY_KIND_LOC: &str = "explorer_tx_by_kind_loc";
/// One exact flow/pool/amount-bucket key per finalized shielded transaction.
pub const EXPLORER_SHIELDED_TRANSACTION_BY_CLASS_LOC: &str = "explorer_shielded_tx_by_class_loc";
/// Compact address activity positions keyed by transparent address.
pub const EXPLORER_ADDRESS_META: &str = "explorer_address_meta";
/// Analytics facts keyed by canonical block height.
pub const EXPLORER_BLOCK_STATS: &str = "explorer_block_stats";
/// One canonical all-time aggregate value.
pub const EXPLORER_CHAIN_STATS: &str = "explorer_chain_stats";
/// UTC daily analytics snapshots.
pub const EXPLORER_DAILY_STATS: &str = "explorer_daily_stats";
/// Funded transparent addresses ordered by descending balance.
pub const EXPLORER_BALANCE_ORDER: &str = "explorer_balance_order";
/// All-time mining totals keyed by attributed transparent payout address.
pub const EXPLORER_MINER_META: &str = "explorer_miner_meta";
/// Attributed miners ordered by descending all-time block count.
pub const EXPLORER_MINER_ORDER: &str = "explorer_miner_order";
/// Active miner-funded bonds keyed by bond public key.
pub const EXPLORER_BOND_ATTRIBUTION: &str = "explorer_bond_attribution";
/// Current miner-finalizer pair aggregates keyed by both identities.
pub const EXPLORER_MINER_FINALIZER_META: &str = "explorer_miner_finalizer_meta";
/// Current transparent address-finalizer pairs ordered by descending stake.
pub const EXPLORER_MINER_FINALIZER_ORDER: &str = "explorer_miner_finalizer_order";
/// Current transparent source addresses ordered within each finalizer by descending stake.
pub const EXPLORER_FINALIZER_MINER_ORDER: &str = "explorer_finalizer_miner_order";
/// Current global miner-attributed stake totals.
pub const EXPLORER_MINER_STAKE_TOTALS: &str = "explorer_miner_stake_totals";
/// Current miner-attributed totals keyed by finalizer public key.
pub const EXPLORER_FINALIZER_MINER_TOTALS: &str = "explorer_finalizer_miner_totals";
/// Finalized staking actions keyed in canonical transaction order.
pub const EXPLORER_STAKE_HISTORY_BY_LOC: &str = "explorer_stake_history_by_loc";

pub(super) type ExplorerTransactionMetaCf<'cf> =
    TypedColumnFamily<'cf, TransactionLocation, ExplorerTransactionRecord>;
type ExplorerSchemaCf<'cf> = TypedColumnFamily<'cf, (), ExplorerSchemaVersion>;
pub(super) type ExplorerTransactionKindCf<'cf> =
    TypedColumnFamily<'cf, ExplorerTransactionKindLocation, ()>;
pub(super) type ExplorerShieldedClassCf<'cf> =
    TypedColumnFamily<'cf, ExplorerShieldedClassLocation, ()>;
pub(super) type ExplorerAddressMetaCf<'cf> =
    TypedColumnFamily<'cf, ExplorerAddressKey, ExplorerAddressRecord>;
pub(super) type ExplorerBlockStatsCf<'cf> =
    TypedColumnFamily<'cf, zebra_chain::block::Height, crate::ExplorerBlockStats>;
pub(super) type ExplorerChainStatsCf<'cf> = TypedColumnFamily<'cf, (), crate::ExplorerChainStats>;
pub(super) type ExplorerDailyStatsCf<'cf> =
    TypedColumnFamily<'cf, ExplorerDayKey, crate::ExplorerDailyStats>;
pub(super) type ExplorerBalanceOrderCf<'cf> = TypedColumnFamily<'cf, ExplorerBalanceKey, ()>;
pub(super) type ExplorerMinerMetaCf<'cf> =
    TypedColumnFamily<'cf, ExplorerAddressKey, crate::ExplorerMinerRecord>;
pub(super) type ExplorerMinerOrderCf<'cf> = TypedColumnFamily<'cf, ExplorerMinerRankKey, ()>;
pub(super) type ExplorerBondAttributionCf<'cf> =
    TypedColumnFamily<'cf, [u8; 32], crate::ExplorerBondAttributionRecord>;
pub(super) type ExplorerMinerFinalizerMetaCf<'cf> =
    TypedColumnFamily<'cf, ExplorerMinerFinalizerKey, crate::ExplorerMinerFinalizerRecord>;
pub(super) type ExplorerMinerFinalizerOrderCf<'cf> =
    TypedColumnFamily<'cf, ExplorerMinerFinalizerRankKey, ()>;
pub(super) type ExplorerFinalizerMinerOrderCf<'cf> =
    TypedColumnFamily<'cf, ExplorerFinalizerMinerRankKey, ()>;
pub(super) type ExplorerMinerStakeTotalsCf<'cf> =
    TypedColumnFamily<'cf, (), crate::ExplorerMinerStakeTotals>;
pub(super) type ExplorerFinalizerMinerTotalsCf<'cf> =
    TypedColumnFamily<'cf, [u8; 32], crate::ExplorerMinerStakeTotals>;
pub(super) type ExplorerStakeHistoryCf<'cf> =
    TypedColumnFamily<'cf, TransactionLocation, crate::ExplorerStakeHistoryRecord>;

impl ZebraDb {
    fn explorer_schema_cf(&self) -> ExplorerSchemaCf<'_> {
        ExplorerSchemaCf::new(self.disk_db(), EXPLORER_SCHEMA)
            .expect("explorer schema column family is registered")
    }

    /// Validates an existing explorer schema or initializes a new empty database.
    pub(crate) fn ensure_explorer_schema(
        &self,
        read_only: bool,
    ) -> Result<(), crate::StateInitError> {
        match self.explorer_schema_cf().zs_get(&()) {
            Some(version) if version == ExplorerSchemaVersion::CURRENT => Ok(()),
            Some(ExplorerSchemaVersion(4)) if !read_only => self.upgrade_address_stake_rankings(),
            Some(version) => Err(crate::StateInitError::ExplorerSchema {
                path: self.path().to_owned(),
                reason: format!(
                    "found schema version {}, but this build requires {}",
                    version.0,
                    ExplorerSchemaVersion::CURRENT.0
                ),
            }),
            None if self.tip().is_some() => Err(crate::StateInitError::ExplorerSchema {
                path: self.path().to_owned(),
                reason: "canonical state already contains blocks but has no explorer indexes; use a fresh state database and resync"
                    .to_string(),
            }),
            None if read_only => Err(crate::StateInitError::ExplorerSchema {
                path: self.path().to_owned(),
                reason: "the read-only primary database has no explorer schema marker".to_string(),
            }),
            None => {
                let mut batch = DiskWriteBatch::new();
                let _ = self
                    .explorer_schema_cf()
                    .with_batch_for_writing(&mut batch)
                    .zs_insert(&(), &ExplorerSchemaVersion::CURRENT);
                self.write_batch(batch)
                    .map_err(|error| crate::StateInitError::ExplorerSchema {
                        path: self.path().to_owned(),
                        reason: format!("could not initialize the explorer schema: {error}"),
                    })
            }
        }
    }

    fn upgrade_address_stake_rankings(&self) -> Result<(), crate::StateInitError> {
        const PAIRS_PER_BATCH: usize = 4_096;
        let mut batch = DiskWriteBatch::new();
        let mut pair_count = 0;
        let write = |batch| {
            self.write_batch(batch)
                .map_err(|error| crate::StateInitError::ExplorerSchema {
                    path: self.path().to_owned(),
                    reason: format!("could not upgrade transparent stake rankings: {error}"),
                })
        };

        for (key, record) in self
            .explorer_miner_finalizer_meta_cf()
            .zs_forward_range_iter(..)
        {
            if record.current_stake_zat == 0 || record.active_bond_count == 0 {
                continue;
            }
            let _ = self
                .explorer_miner_finalizer_order_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(
                    &ExplorerMinerFinalizerRankKey::new(
                        key.address(),
                        key.finalizer(),
                        record.current_stake_zat,
                    ),
                    &(),
                );
            let _ = self
                .explorer_finalizer_miner_order_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(
                    &ExplorerFinalizerMinerRankKey::new(
                        key.finalizer(),
                        key.address(),
                        record.current_stake_zat,
                    ),
                    &(),
                );
            pair_count += 1;
            if pair_count == PAIRS_PER_BATCH {
                write(batch)?;
                batch = DiskWriteBatch::new();
                pair_count = 0;
            }
        }
        // Until the marker is committed, a restart repeats the idempotent backfill.
        let _ = self
            .explorer_schema_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(&(), &ExplorerSchemaVersion::CURRENT);
        write(batch)
    }

    pub(super) fn explorer_transaction_meta_cf(&self) -> ExplorerTransactionMetaCf<'_> {
        ExplorerTransactionMetaCf::new(self.disk_db(), EXPLORER_TRANSACTION_META_BY_LOC)
            .expect("explorer transaction metadata column family is registered")
    }

    pub(super) fn explorer_transaction_kind_cf(&self) -> ExplorerTransactionKindCf<'_> {
        ExplorerTransactionKindCf::new(self.disk_db(), EXPLORER_TRANSACTION_BY_KIND_LOC)
            .expect("explorer transaction kind column family is registered")
    }

    pub(super) fn explorer_shielded_class_cf(&self) -> ExplorerShieldedClassCf<'_> {
        ExplorerShieldedClassCf::new(self.disk_db(), EXPLORER_SHIELDED_TRANSACTION_BY_CLASS_LOC)
            .expect("explorer shielded classification column family is registered")
    }

    pub(super) fn explorer_address_meta_cf(&self) -> ExplorerAddressMetaCf<'_> {
        ExplorerAddressMetaCf::new(self.disk_db(), EXPLORER_ADDRESS_META)
            .expect("explorer address metadata column family is registered")
    }

    pub(super) fn explorer_block_stats_cf(&self) -> ExplorerBlockStatsCf<'_> {
        ExplorerBlockStatsCf::new(self.disk_db(), EXPLORER_BLOCK_STATS)
            .expect("explorer block analytics column family is registered")
    }

    pub(super) fn explorer_chain_stats_cf(&self) -> ExplorerChainStatsCf<'_> {
        ExplorerChainStatsCf::new(self.disk_db(), EXPLORER_CHAIN_STATS)
            .expect("explorer chain analytics column family is registered")
    }

    pub(super) fn explorer_daily_stats_cf(&self) -> ExplorerDailyStatsCf<'_> {
        ExplorerDailyStatsCf::new(self.disk_db(), EXPLORER_DAILY_STATS)
            .expect("explorer daily analytics column family is registered")
    }

    pub(super) fn explorer_balance_order_cf(&self) -> ExplorerBalanceOrderCf<'_> {
        ExplorerBalanceOrderCf::new(self.disk_db(), EXPLORER_BALANCE_ORDER)
            .expect("explorer balance order column family is registered")
    }

    pub(super) fn explorer_miner_meta_cf(&self) -> ExplorerMinerMetaCf<'_> {
        ExplorerMinerMetaCf::new(self.disk_db(), EXPLORER_MINER_META)
            .expect("explorer miner metadata column family is registered")
    }

    pub(super) fn explorer_miner_order_cf(&self) -> ExplorerMinerOrderCf<'_> {
        ExplorerMinerOrderCf::new(self.disk_db(), EXPLORER_MINER_ORDER)
            .expect("explorer miner ranking column family is registered")
    }

    pub(super) fn explorer_bond_attribution_cf(&self) -> ExplorerBondAttributionCf<'_> {
        ExplorerBondAttributionCf::new(self.disk_db(), EXPLORER_BOND_ATTRIBUTION)
            .expect("explorer bond attribution column family is registered")
    }

    pub(super) fn explorer_miner_finalizer_meta_cf(&self) -> ExplorerMinerFinalizerMetaCf<'_> {
        ExplorerMinerFinalizerMetaCf::new(self.disk_db(), EXPLORER_MINER_FINALIZER_META)
            .expect("explorer miner-finalizer metadata column family is registered")
    }

    pub(super) fn explorer_miner_finalizer_order_cf(&self) -> ExplorerMinerFinalizerOrderCf<'_> {
        ExplorerMinerFinalizerOrderCf::new(self.disk_db(), EXPLORER_MINER_FINALIZER_ORDER)
            .expect("explorer miner-finalizer ranking column family is registered")
    }

    pub(super) fn explorer_finalizer_miner_order_cf(&self) -> ExplorerFinalizerMinerOrderCf<'_> {
        ExplorerFinalizerMinerOrderCf::new(self.disk_db(), EXPLORER_FINALIZER_MINER_ORDER)
            .expect("explorer finalizer-miner ranking column family is registered")
    }

    pub(super) fn explorer_miner_stake_totals_cf(&self) -> ExplorerMinerStakeTotalsCf<'_> {
        ExplorerMinerStakeTotalsCf::new(self.disk_db(), EXPLORER_MINER_STAKE_TOTALS)
            .expect("explorer miner stake totals column family is registered")
    }

    pub(super) fn explorer_finalizer_miner_totals_cf(&self) -> ExplorerFinalizerMinerTotalsCf<'_> {
        ExplorerFinalizerMinerTotalsCf::new(self.disk_db(), EXPLORER_FINALIZER_MINER_TOTALS)
            .expect("explorer finalizer miner totals column family is registered")
    }

    pub(super) fn explorer_stake_history_cf(&self) -> ExplorerStakeHistoryCf<'_> {
        ExplorerStakeHistoryCf::new(self.disk_db(), EXPLORER_STAKE_HISTORY_BY_LOC)
            .expect("explorer stake history column family is registered")
    }
}

#[cfg(test)]
mod tests {
    use zebra_chain::parameters::Network;

    use crate::{
        constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
        service::finalized_state::STATE_COLUMN_FAMILIES_IN_CODE,
        Config,
    };

    use super::*;

    #[test]
    fn explorer_schema_version_is_initialized_and_validated() {
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

        assert_eq!(
            db.explorer_schema_cf().zs_get(&()),
            Some(ExplorerSchemaVersion::CURRENT)
        );

        let mut batch = DiskWriteBatch::new();
        let _ = db
            .explorer_schema_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &(),
                &ExplorerSchemaVersion(ExplorerSchemaVersion::CURRENT.0 + 1),
            );
        db.write_batch(batch)
            .expect("writing an unsupported test schema should succeed");

        let error = db
            .ensure_explorer_schema(false)
            .expect_err("an unsupported explorer schema must be rejected");
        assert!(matches!(
            error,
            crate::StateInitError::ExplorerSchema { .. }
        ));
    }

    #[test]
    fn schema_four_backfills_all_address_rankings_and_supports_filtered_pagination() {
        use crate::{ExplorerMinerFinalizerRecord, ExplorerPageDirection};
        use zebra_chain::{block::Hash, parameters::NetworkKind, transparent::Address};
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
        .unwrap();
        let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [1; 20]);
        let other = Address::from_pub_key_hash(NetworkKind::Mainnet, [2; 20]);
        let mut batch = DiskWriteBatch::new();
        let _ = db
            .explorer_schema_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(&(), &ExplorerSchemaVersion(4));
        for (address, finalizer, stake) in [
            (address, [1; 32], 300),
            (address, [2; 32], 200),
            (other, [1; 32], 100),
            (address, [3; 32], 0),
        ] {
            let record = ExplorerMinerFinalizerRecord {
                current_stake_zat: stake,
                active_bond_count: u64::from(stake > 0),
                stake_action_count: 1,
                latest_height: 1,
                latest_block_hash: Hash([1; 32]),
                latest_timestamp: 1,
            };
            let _ = db
                .explorer_miner_finalizer_meta_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(&ExplorerMinerFinalizerKey::new(address, finalizer), &record);
        }
        db.write_batch(batch).unwrap();
        assert!(db.ensure_explorer_schema(true).is_err());
        assert_eq!(
            db.explorer_schema_cf().zs_get(&()),
            Some(ExplorerSchemaVersion(4))
        );
        db.ensure_explorer_schema(false).unwrap();
        assert_eq!(
            db.explorer_schema_cf().zs_get(&()),
            Some(ExplorerSchemaVersion::CURRENT)
        );
        for _ in 0..2 {
            let rows = db.explorer_miner_stake_entries(
                None,
                None,
                None,
                None,
                ExplorerPageDirection::Older,
                10,
            );
            assert_eq!(
                rows.iter()
                    .map(|row| row.record.current_stake_zat)
                    .collect::<Vec<_>>(),
                vec![300, 200, 100]
            );
            assert!(rows.iter().all(|row| row.miner_record.is_none()));
            assert!(db
                .explorer_miner_stake_entries(
                    None,
                    None,
                    Some(true),
                    None,
                    ExplorerPageDirection::Older,
                    10,
                )
                .is_empty());
            assert_eq!(
                db.explorer_miner_stake_entries(
                    None,
                    None,
                    Some(false),
                    None,
                    ExplorerPageDirection::Older,
                    10,
                )
                .len(),
                3
            );
            let scoped = db.explorer_miner_stake_entries(
                Some([1; 32]),
                None,
                None,
                None,
                ExplorerPageDirection::Older,
                10,
            );
            assert_eq!(scoped.len(), 2);
            let filtered = db.explorer_miner_stake_entries(
                None,
                Some(address),
                None,
                None,
                ExplorerPageDirection::Older,
                1,
            );
            assert_eq!(filtered[0].record.current_stake_zat, 300);
            let next = db.explorer_miner_stake_entries(
                None,
                Some(address),
                None,
                Some((address, [1; 32], 300)),
                ExplorerPageDirection::Older,
                1,
            );
            assert_eq!(next[0].record.current_stake_zat, 200);
            let prev = db.explorer_miner_stake_entries(
                None,
                Some(address),
                None,
                Some((address, [2; 32], 200)),
                ExplorerPageDirection::Newer,
                1,
            );
            assert_eq!(prev[0].record.current_stake_zat, 300);
            assert_eq!(db.explorer_address_stake_pair_count(address, None, None), 2);
            assert_eq!(
                db.explorer_address_stake_pair_count(address, None, Some(true)),
                0
            );
            assert_eq!(
                db.explorer_address_stake_pair_count(address, None, Some(false)),
                2
            );
            assert_eq!(
                db.explorer_address_stake_pair_count(address, Some([1; 32]), None),
                1
            );
            assert!(!db.explorer_contains_miner_stake_entry(
                None,
                Some(other),
                None,
                address,
                [1; 32],
                300
            ));
            db.ensure_explorer_schema(false).unwrap();
        }
    }
}
