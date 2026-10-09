//! Finalized Crosslink staking-action history.

use std::{
    collections::HashMap,
    ops::Bound::{Excluded, Included},
};

use zcash_primitives::transaction::StakingAction;
use zebra_chain::{
    block,
    parameters::Network,
    transaction::Transaction,
    transparent::{OutPoint, Utxo},
};

use crate::{
    request::FinalizedBlock,
    service::finalized_state::{DiskWriteBatch, TransactionLocation, ZebraDb},
    ExplorerPageDirection, ExplorerStakeAction, ExplorerStakeHistoryFilter,
    ExplorerStakeHistoryRecord, ExplorerStakeSource,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct BondIdentity {
    pub(super) source: ExplorerStakeSource,
    pub(super) finalizer: Option<[u8; 32]>,
    pub(super) amount_zat: Option<u64>,
}

impl ZebraDb {
    /// Returns one finalized staking event at `location`.
    pub fn explorer_stake_history_record(
        &self,
        location: TransactionLocation,
    ) -> Option<ExplorerStakeHistoryRecord> {
        self.explorer_stake_history_cf().zs_get(&location)
    }

    /// Returns cursor-adjacent finalized staking events matching `filter`.
    pub fn explorer_stake_history_records(
        &self,
        filter: ExplorerStakeHistoryFilter,
        cursor: Option<TransactionLocation>,
        direction: ExplorerPageDirection,
        from_height: block::Height,
        to_height: block::Height,
        limit: usize,
    ) -> Vec<(TransactionLocation, ExplorerStakeHistoryRecord)> {
        if limit == 0 || from_height > to_height {
            return Vec::new();
        }

        let minimum = TransactionLocation::min_for_height(from_height);
        let maximum = TransactionLocation::max_for_height(to_height);
        let cf = self.explorer_stake_history_cf();
        let mut records = match (direction, cursor) {
            (ExplorerPageDirection::Older, Some(cursor)) => cf
                .zs_reverse_range_iter((Included(minimum), Excluded(cursor)))
                .filter(|(_, record)| filter.matches(*record))
                .take(limit)
                .collect::<Vec<_>>(),
            (ExplorerPageDirection::Older, None) => cf
                .zs_reverse_range_iter(minimum..=maximum)
                .filter(|(_, record)| filter.matches(*record))
                .take(limit)
                .collect::<Vec<_>>(),
            (ExplorerPageDirection::Newer, Some(cursor)) => cf
                .zs_forward_range_iter((Excluded(cursor), Included(maximum)))
                .filter(|(_, record)| filter.matches(*record))
                .take(limit)
                .collect::<Vec<_>>(),
            (ExplorerPageDirection::Newer, None) => Vec::new(),
        };

        if direction == ExplorerPageDirection::Newer {
            records.reverse();
        }
        records
    }

    fn latest_stake_bond_identity(&self, bond_key: [u8; 32]) -> Option<BondIdentity> {
        if let Some(attribution) = self.explorer_bond_attribution_cf().zs_get(&bond_key) {
            return Some(BondIdentity {
                source: attribution.source,
                finalizer: Some(attribution.current_finalizer),
                amount_zat: Some(attribution.current_stake_zat),
            });
        }

        if let Some((bond, status)) = self.delegation_bond_with_status(&bond_key) {
            if status.is_withdrawn() || status.is_burned() {
                return None;
            }
            let creation = self.explorer_stake_history_record(bond.created_at)?;
            return Some(BondIdentity {
                source: creation.source,
                finalizer: Some(bond.target_finalizer),
                amount_zat: Some(u64::from(bond.amount)),
            });
        }

        self.explorer_stake_history_cf()
            .zs_reverse_range_iter(..)
            .find_map(|(_, record)| {
                (record.bond_key == bond_key).then(|| match record.action {
                    ExplorerStakeAction::Withdraw => None,
                    _ => Some(BondIdentity {
                        source: record.source,
                        finalizer: record.to_finalizer.or(record.from_finalizer),
                        amount_zat: record.amount_zat,
                    }),
                })
            })
            .flatten()
    }
}

impl DiskWriteBatch {
    /// Indexes staking actions while committing one finalized block.
    pub(crate) fn prepare_explorer_stake_history_batch(
        &mut self,
        db: &ZebraDb,
        network: &Network,
        finalized: &FinalizedBlock,
        spent_utxos: &HashMap<OutPoint, Utxo>,
    ) {
        prepare_stake_history_batch(
            self,
            db,
            finalized.height,
            &finalized.block.transactions,
            |_, transaction| {
                super::miner_stake::classify_stake_source(network, transaction, spent_utxos)
            },
        );
    }
}

fn prepare_stake_history_batch<F>(
    batch: &mut DiskWriteBatch,
    db: &ZebraDb,
    height: block::Height,
    transactions: &[std::sync::Arc<Transaction>],
    mut create_source: F,
) where
    F: FnMut(TransactionLocation, &Transaction) -> ExplorerStakeSource,
{
    let records = stake_history_records(
        height,
        transactions,
        &mut HashMap::new(),
        |key| db.latest_stake_bond_identity(key),
        &mut create_source,
    );
    for (location, record) in records {
        let _ = db
            .explorer_stake_history_cf()
            .with_batch_for_writing(batch)
            .zs_insert(&location, &record);
    }
}

pub(super) fn stake_history_records<F, G>(
    height: block::Height,
    transactions: &[std::sync::Arc<Transaction>],
    identities: &mut HashMap<[u8; 32], Option<BondIdentity>>,
    mut load_identity: G,
    mut create_source: F,
) -> Vec<(TransactionLocation, ExplorerStakeHistoryRecord)>
where
    F: FnMut(TransactionLocation, &Transaction) -> ExplorerStakeSource,
    G: FnMut([u8; 32]) -> Option<BondIdentity>,
{
    let mut records = Vec::new();

    for (transaction_index, transaction) in transactions.iter().enumerate() {
        let Some(action) = transaction.staking_action() else {
            continue;
        };
        let location = TransactionLocation::from_usize(height, transaction_index);
        let bond_key = action.bond_key();
        let existing = match action {
            StakingAction::CreateNewDelegationBond { .. }
            | StakingAction::ConvertFinalizerRewardToDelegationBond { .. } => None,
            _ => *identities
                .entry(bond_key)
                .or_insert_with(|| load_identity(bond_key)),
        };

        let (record, next_identity) = match action {
            StakingAction::CreateNewDelegationBond {
                amount_zats,
                target_finalizer,
                ..
            } => {
                let identity = BondIdentity {
                    source: create_source(location, transaction),
                    finalizer: Some(target_finalizer.pub_key.0),
                    amount_zat: Some(*amount_zats),
                };
                (
                    ExplorerStakeHistoryRecord {
                        action: ExplorerStakeAction::Create,
                        bond_key,
                        source: identity.source,
                        from_finalizer: None,
                        to_finalizer: identity.finalizer,
                        amount_zat: Some(*amount_zats),
                    },
                    Some(identity),
                )
            }
            StakingAction::BeginDelegationUnbonding { .. } => {
                let identity = existing.unwrap_or(BondIdentity {
                    source: ExplorerStakeSource::Unknown,
                    finalizer: None,
                    amount_zat: None,
                });
                (
                    ExplorerStakeHistoryRecord {
                        action: ExplorerStakeAction::BeginUnbonding,
                        bond_key,
                        source: identity.source,
                        from_finalizer: identity.finalizer,
                        to_finalizer: None,
                        amount_zat: identity.amount_zat,
                    },
                    Some(identity),
                )
            }
            StakingAction::WithdrawDelegationBond { amount_zats, .. } => {
                let identity = existing.unwrap_or(BondIdentity {
                    source: ExplorerStakeSource::Unknown,
                    finalizer: None,
                    amount_zat: Some(*amount_zats),
                });
                (
                    ExplorerStakeHistoryRecord {
                        action: ExplorerStakeAction::Withdraw,
                        bond_key,
                        source: identity.source,
                        from_finalizer: identity.finalizer,
                        to_finalizer: None,
                        amount_zat: Some(*amount_zats),
                    },
                    None,
                )
            }
            StakingAction::RetargetDelegationBond {
                from_finalizer,
                to_finalizer,
                ..
            } => {
                let source = existing
                    .map(|identity| identity.source)
                    .unwrap_or(ExplorerStakeSource::Unknown);
                let identity = BondIdentity {
                    source,
                    finalizer: Some(to_finalizer.pub_key.0),
                    amount_zat: existing.and_then(|identity| identity.amount_zat),
                };
                (
                    ExplorerStakeHistoryRecord {
                        action: ExplorerStakeAction::Retarget,
                        bond_key,
                        source,
                        from_finalizer: Some(from_finalizer.pub_key.0),
                        to_finalizer: identity.finalizer,
                        amount_zat: identity.amount_zat,
                    },
                    Some(identity),
                )
            }
            StakingAction::ConvertFinalizerRewardToDelegationBond {
                this_finalizer,
                amount_zats,
                ..
            } => {
                let identity = BondIdentity {
                    source: ExplorerStakeSource::Rewards,
                    finalizer: Some(*this_finalizer),
                    amount_zat: Some(*amount_zats),
                };
                (
                    ExplorerStakeHistoryRecord {
                        action: ExplorerStakeAction::ConvertReward,
                        bond_key,
                        source: identity.source,
                        from_finalizer: None,
                        to_finalizer: identity.finalizer,
                        amount_zat: Some(*amount_zats),
                    },
                    Some(identity),
                )
            }
        };

        identities.insert(bond_key, next_identity);
        records.push((location, record));
    }
    records
}

#[cfg(test)]
mod tests {
    use zebra_chain::{
        block::Height,
        parameters::{Network, NetworkKind},
        transparent::Address,
    };

    use crate::{
        constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
        service::finalized_state::{DiskWriteBatch, STATE_COLUMN_FAMILIES_IN_CODE},
        Config,
    };

    use super::*;

    #[test]
    fn history_queries_filter_and_keep_newest_first_order() {
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
        let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
        let finalizer = [8; 32];
        let fixtures = [
            (
                TransactionLocation::from_index(Height(10), 1),
                ExplorerStakeHistoryRecord {
                    action: ExplorerStakeAction::Create,
                    bond_key: [1; 32],
                    source: ExplorerStakeSource::Transparent(address),
                    from_finalizer: None,
                    to_finalizer: Some(finalizer),
                    amount_zat: Some(100),
                },
            ),
            (
                TransactionLocation::from_index(Height(20), 2),
                ExplorerStakeHistoryRecord {
                    action: ExplorerStakeAction::Create,
                    bond_key: [2; 32],
                    source: ExplorerStakeSource::Shielded,
                    from_finalizer: None,
                    to_finalizer: Some([9; 32]),
                    amount_zat: Some(200),
                },
            ),
            (
                TransactionLocation::from_index(Height(30), 3),
                ExplorerStakeHistoryRecord {
                    action: ExplorerStakeAction::Retarget,
                    bond_key: [1; 32],
                    source: ExplorerStakeSource::Transparent(address),
                    from_finalizer: Some(finalizer),
                    to_finalizer: Some([10; 32]),
                    amount_zat: Some(100),
                },
            ),
        ];
        let mut batch = DiskWriteBatch::new();
        for (location, record) in fixtures {
            let _ = db
                .explorer_stake_history_cf()
                .with_batch_for_writing(&mut batch)
                .zs_insert(&location, &record);
        }
        db.write_batch(batch)
            .expect("writing stake history fixtures should succeed");

        let address_rows = db.explorer_stake_history_records(
            ExplorerStakeHistoryFilter {
                address: Some(address),
                ..Default::default()
            },
            None,
            ExplorerPageDirection::Older,
            Height(0),
            Height(100),
            10,
        );
        assert_eq!(
            address_rows
                .iter()
                .map(|(location, _)| location.height.0)
                .collect::<Vec<_>>(),
            vec![30, 10]
        );

        let finalizer_rows = db.explorer_stake_history_records(
            ExplorerStakeHistoryFilter {
                finalizer: Some(finalizer),
                ..Default::default()
            },
            Some(TransactionLocation::from_index(Height(10), 1)),
            ExplorerPageDirection::Newer,
            Height(0),
            Height(100),
            10,
        );
        assert_eq!(finalizer_rows.len(), 1);
        assert_eq!(finalizer_rows[0].0.height, Height(30));
    }

    #[test]
    fn regular_source_survives_retarget_unbond_and_withdraw_in_history() {
        use std::sync::Arc;
        use zcash_primitives::bft::{finalizer_key_from_seed, FinalizerAddress};
        use zebra_chain::{parameters::NetworkUpgrade, transaction::LockTime};
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
        let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
        let (_, key_a, _) = finalizer_key_from_seed(b"history-a");
        let (_, key_b, _) = finalizer_key_from_seed(b"history-b");
        let (a, b) = (
            FinalizerAddress::create(&key_a),
            FinalizerAddress::create(&key_b),
        );
        let bond_key = [1; 32];
        let transaction = |action| {
            Arc::new(Transaction::VCrosslink {
                network_upgrade: NetworkUpgrade::Nu6,
                lock_time: LockTime::unlocked(),
                expiry_height: Height(0),
                inputs: Vec::new(),
                outputs: Vec::new(),
                sapling_shielded_data: None,
                orchard_shielded_data: None,
                ironwood_shielded_data: None,
                staking_action: Some(action),
            })
        };
        let actions = [
            transaction(StakingAction::CreateNewDelegationBond {
                amount_zats: 100,
                unique_pubkey: bond_key,
                bond_salt: [0; 32],
                target_finalizer: a,
                signature: [0; 64],
            }),
            transaction(StakingAction::RetargetDelegationBond {
                unique_pubkey: bond_key,
                signature: [0; 64],
                from_finalizer: a,
                to_finalizer: b,
            }),
            transaction(StakingAction::BeginDelegationUnbonding {
                unique_pubkey: bond_key,
                signature: [0; 64],
            }),
        ];
        let mut batch = DiskWriteBatch::new();
        let mut classifications = 0;
        prepare_stake_history_batch(&mut batch, &db, Height(10), &actions, |_, _| {
            classifications += 1;
            ExplorerStakeSource::Transparent(address)
        });
        db.write_batch(batch).unwrap();
        assert_eq!(classifications, 1);
        let mut batch = DiskWriteBatch::new();
        prepare_stake_history_batch(
            &mut batch,
            &db,
            Height(20),
            &[transaction(StakingAction::WithdrawDelegationBond {
                amount_zats: 100,
                unique_pubkey: bond_key,
                signature: [0; 64],
            })],
            |_, _| panic!("withdrawal fees must not replace the original bond source"),
        );
        db.write_batch(batch).unwrap();
        let rows = db.explorer_stake_history_records(
            ExplorerStakeHistoryFilter {
                address: Some(address),
                finalizer: Some(b.pub_key.0),
                ..Default::default()
            },
            None,
            ExplorerPageDirection::Older,
            Height(0),
            Height(100),
            10,
        );
        assert_eq!(
            rows.iter().map(|(_, row)| row.action).collect::<Vec<_>>(),
            vec![
                ExplorerStakeAction::Withdraw,
                ExplorerStakeAction::BeginUnbonding,
                ExplorerStakeAction::Retarget
            ]
        );
        assert!(rows
            .iter()
            .all(|(_, row)| row.source == ExplorerStakeSource::Transparent(address)));
        assert!(rows.iter().all(|(_, row)| row.amount_zat == Some(100)));
        assert!(db.explorer_miner_record(address).is_none());
    }

    #[test]
    fn unbonding_identity_uses_original_source_and_rewarded_canonical_amount() {
        use crate::service::finalized_state::disk_format::delegation::{
            BondStatus, DelegationBond,
        };
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
        let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
        let bond_key = [1; 32];
        let creation = TransactionLocation::from_index(Height(10), 1);
        let unbonding = TransactionLocation::from_index(Height(20), 1);
        let mut batch = DiskWriteBatch::new();
        let _ = db
            .explorer_stake_history_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &creation,
                &ExplorerStakeHistoryRecord {
                    action: ExplorerStakeAction::Create,
                    bond_key,
                    source: ExplorerStakeSource::Transparent(address),
                    from_finalizer: None,
                    to_finalizer: Some([2; 32]),
                    amount_zat: Some(100),
                },
            );
        let _ = db
            .explorer_stake_history_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &unbonding,
                &ExplorerStakeHistoryRecord {
                    action: ExplorerStakeAction::BeginUnbonding,
                    bond_key,
                    source: ExplorerStakeSource::Transparent(address),
                    from_finalizer: Some([3; 32]),
                    to_finalizer: None,
                    amount_zat: Some(100),
                },
            );
        let _ = db
            .delegation_bond_by_key_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &bond_key,
                &DelegationBond::new(125.try_into().unwrap(), [3; 32], creation),
            );
        let _ = db
            .bond_status_by_key_cf()
            .with_batch_for_writing(&mut batch)
            .zs_insert(
                &bond_key,
                &BondStatus::Unbonding {
                    unbonded_at: unbonding,
                },
            );
        db.write_batch(batch).unwrap();
        let identity = db.latest_stake_bond_identity(bond_key).unwrap();
        assert_eq!(identity.source, ExplorerStakeSource::Transparent(address));
        assert_eq!(identity.finalizer, Some([3; 32]));
        assert_eq!(identity.amount_zat, Some(125));
    }
}
