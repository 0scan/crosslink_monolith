//! Finalized all-time miner ranking reads and writes.

use std::{
    collections::HashMap,
    ops::Bound::{Excluded, Unbounded},
};

use zebra_chain::{block::Height, transparent::Address};

use crate::{
    service::finalized_state::{DiskWriteBatch, ZebraDb},
    ExplorerBlockStats, ExplorerMinerRankEntry, ExplorerMinerRecord, ExplorerPageDirection,
};

use super::disk_format::ExplorerMinerRankKey;

impl ZebraDb {
    /// Returns all-time mining totals for one attributed payout address.
    pub fn explorer_miner_record(&self, address: Address) -> Option<ExplorerMinerRecord> {
        self.explorer_miner_meta_cf().zs_get(&address.into())
    }

    /// Returns one cursor-adjacent miner page without scanning the complete miner set.
    pub fn explorer_miner_entries(
        &self,
        cursor: Option<(Address, u64)>,
        direction: ExplorerPageDirection,
        limit: usize,
    ) -> Vec<ExplorerMinerRankEntry> {
        let cf = self.explorer_miner_order_cf();
        let mut keys = match (direction, cursor) {
            (ExplorerPageDirection::Older, Some((address, block_count))) => cf
                .zs_forward_range_iter((
                    Excluded(ExplorerMinerRankKey::new(address, block_count)),
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
            (ExplorerPageDirection::Newer, Some((address, block_count))) => cf
                .zs_reverse_range_iter(..ExplorerMinerRankKey::new(address, block_count))
                .take(limit)
                .map(|(key, ())| key)
                .collect::<Vec<_>>(),
            (ExplorerPageDirection::Newer, None) => Vec::new(),
        };
        if direction == ExplorerPageDirection::Newer {
            keys.reverse();
        }
        keys.into_iter()
            .map(|key| {
                let address = key.address();
                let record = self
                    .explorer_miner_record(address)
                    .expect("ranked miners have all-time metadata");
                ExplorerMinerRankEntry { address, record }
            })
            .collect()
    }

    /// Returns whether this exact miner ranking key still exists.
    pub fn explorer_contains_miner_entry(&self, address: Address, block_count: u64) -> bool {
        self.explorer_miner_order_cf()
            .zs_contains(&ExplorerMinerRankKey::new(address, block_count))
    }

    fn explorer_latest_mined_block(
        &self,
        address: Address,
        maximum_height: Height,
    ) -> Option<ExplorerBlockStats> {
        let network = self.network();
        let mut height = maximum_height;
        loop {
            let block = self.block(height.into())?;
            if crate::explorer::analytics::miner_address(&block, &network) == Some(address) {
                return self.explorer_block_stats(height);
            }
            height = height.previous().ok()?;
        }
    }
}

impl DiskWriteBatch {
    /// Adds one finalized block and updates only its miner's metadata and ranking key.
    ///
    /// Returns true when this is the first indexed block for `address`.
    pub(crate) fn prepare_explorer_miner_batch(
        &mut self,
        zebra_db: &ZebraDb,
        address: Address,
        block: &ExplorerBlockStats,
    ) -> bool {
        let previous = zebra_db.explorer_miner_record(address);
        if let Some(previous) = previous {
            let _ = zebra_db
                .explorer_miner_order_cf()
                .with_batch_for_writing(self)
                .zs_delete(&ExplorerMinerRankKey::new(address, previous.block_count));
        }
        let mut record = previous.unwrap_or(ExplorerMinerRecord {
            block_count: 0,
            mined_zat: 0,
            latest_height: block.height,
            latest_block_hash: block.hash,
            latest_timestamp: block.timestamp,
        });
        record.block_count = record
            .block_count
            .checked_add(1)
            .expect("canonical block count fits in u64");
        record.mined_zat = record
            .mined_zat
            .checked_add(block.interval.miner_subsidy_zat)
            .expect("canonical miner subsidy total fits in u128");
        record.latest_height = block.height;
        record.latest_block_hash = block.hash;
        record.latest_timestamp = block.timestamp;

        let _ = zebra_db
            .explorer_miner_meta_cf()
            .with_batch_for_writing(self)
            .zs_insert(&address.into(), &record);
        let _ = zebra_db
            .explorer_miner_order_cf()
            .with_batch_for_writing(self)
            .zs_insert(&ExplorerMinerRankKey::new(address, record.block_count), &());
        previous.is_none()
    }

    /// Removes finalized blocks from miner totals during an offline rollback.
    ///
    /// Returns `(attributed_blocks_removed, miners_removed)` for chain totals.
    #[allow(dead_code)] // Reserved for a future Crosslink finalized-state rollback path.
    pub(crate) fn prepare_explorer_miner_rollback(
        &mut self,
        zebra_db: &ZebraDb,
        removed: &[(Address, ExplorerBlockStats)],
        target_height: Height,
    ) -> (u64, u64) {
        let mut pending = HashMap::<Address, ExplorerMinerRecord>::new();
        for (address, block) in removed {
            let record = pending.entry(*address).or_insert_with(|| {
                let record = zebra_db
                    .explorer_miner_record(*address)
                    .expect("rolled-back attributed miners have all-time totals");
                let _ = zebra_db
                    .explorer_miner_order_cf()
                    .with_batch_for_writing(self)
                    .zs_delete(&ExplorerMinerRankKey::new(*address, record.block_count));
                record
            });
            record.block_count = record
                .block_count
                .checked_sub(1)
                .expect("rolled-back miner block count is nonzero");
            record.mined_zat = record
                .mined_zat
                .checked_sub(block.interval.miner_subsidy_zat)
                .expect("rolled-back miner subsidy was previously counted");
        }

        let mut miners_removed = 0_u64;
        for (address, mut record) in pending {
            if record.block_count == 0 {
                miners_removed = miners_removed
                    .checked_add(1)
                    .expect("removed miner count fits in u64");
                let _ = zebra_db
                    .explorer_miner_meta_cf()
                    .with_batch_for_writing(self)
                    .zs_delete(&address.into());
                continue;
            }
            let latest = zebra_db
                .explorer_latest_mined_block(address, target_height)
                .expect("a retained miner block exists when its count is nonzero");
            record.latest_height = latest.height;
            record.latest_block_hash = latest.hash;
            record.latest_timestamp = latest.timestamp;
            let _ = zebra_db
                .explorer_miner_meta_cf()
                .with_batch_for_writing(self)
                .zs_insert(&address.into(), &record);
            let _ = zebra_db
                .explorer_miner_order_cf()
                .with_batch_for_writing(self)
                .zs_insert(&ExplorerMinerRankKey::new(address, record.block_count), &());
        }

        (
            u64::try_from(removed.len()).expect("removed block count fits in u64"),
            miners_removed,
        )
    }
}

#[cfg(test)]
mod tests {
    use zebra_chain::{
        block,
        parameters::{Network, NetworkKind},
        transparent::Address,
    };

    use crate::{
        constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
        service::finalized_state::{DiskWriteBatch, ZebraDb, STATE_COLUMN_FAMILIES_IN_CODE},
        Config, ExplorerMinerRecord, ExplorerPageDirection,
    };

    use super::ExplorerMinerRankKey;

    #[test]
    fn miner_entries_page_in_both_ranking_directions() {
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
        let miners = [
            (
                Address::from_pub_key_hash(NetworkKind::Mainnet, [1; 20]),
                30,
            ),
            (
                Address::from_pub_key_hash(NetworkKind::Mainnet, [2; 20]),
                20,
            ),
            (
                Address::from_pub_key_hash(NetworkKind::Mainnet, [3; 20]),
                10,
            ),
        ];
        let mut batch = DiskWriteBatch::new();
        for (address, block_count) in miners {
            let record = ExplorerMinerRecord {
                block_count,
                mined_zat: u128::from(block_count) * 5,
                latest_height: u32::try_from(block_count).expect("test count fits in u32"),
                latest_block_hash: block::Hash([u8::try_from(block_count).unwrap(); 32]),
                latest_timestamp: i64::try_from(block_count).expect("test count fits in i64"),
            };
            let _ = db
                .explorer_miner_meta_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(&address.into(), &record);
            let _ = db
                .explorer_miner_order_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(&ExplorerMinerRankKey::new(address, block_count), &());
        }
        db.write_batch(batch)
            .expect("test miner records should be written atomically");

        let first = db.explorer_miner_entries(None, ExplorerPageDirection::Older, 2);
        assert_eq!(
            first
                .iter()
                .map(|entry| entry.record.block_count)
                .collect::<Vec<_>>(),
            vec![30, 20]
        );

        let next = db.explorer_miner_entries(
            Some((miners[1].0, miners[1].1)),
            ExplorerPageDirection::Older,
            2,
        );
        assert_eq!(next[0].record.block_count, 10);

        let previous = db.explorer_miner_entries(
            Some((miners[2].0, miners[2].1)),
            ExplorerPageDirection::Newer,
            2,
        );
        assert_eq!(
            previous
                .iter()
                .map(|entry| entry.record.block_count)
                .collect::<Vec<_>>(),
            vec![30, 20]
        );
    }
}
