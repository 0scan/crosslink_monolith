use std::sync::Arc;

use zcash_primitives::{
    bft::{finalizer_key_from_seed, FinalizerAddress},
    transaction::StakingAction,
};
use zebra_chain::{
    block::{Block, Hash, Height},
    parameters::{NetworkKind, NetworkUpgrade},
    serialization::ZcashDeserializeInto,
    transaction::{LockTime, Transaction},
    transparent::{self, Address},
};

use super::*;
use crate::{
    constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
    service::{
        finalized_state::{TypedColumnFamily, STATE_COLUMN_FAMILIES_IN_CODE},
        non_finalized_state::Chain,
    },
    Config, ContextuallyVerifiedBlock, ExplorerStakeHistoryFilter, TransactionLocation,
};

fn database(network: &Network) -> ZebraDb {
    ZebraDb::new(
        &Config::ephemeral(),
        STATE_DATABASE_KIND,
        &state_database_format_version_in_code(),
        network,
        true,
        STATE_COLUMN_FAMILIES_IN_CODE
            .iter()
            .map(ToString::to_string),
        false,
    )
    .unwrap()
}

fn finalizer(seed: &[u8]) -> FinalizerAddress {
    FinalizerAddress::create(&finalizer_key_from_seed(seed).1)
}

fn append(
    chain: &mut Chain,
    height: u32,
    transaction: Transaction,
    address: Address,
    input_zat: u64,
    rewards: Vec<([u8; 32], u64)>,
    burns: Vec<[u8; 32]>,
) {
    let height = Height(height);
    let mut block: Block = zebra_test::vectors::BLOCK_MAINNET_419200_BYTES
        .zcash_deserialize_into()
        .unwrap();
    block.transactions.truncate(1);
    let spent_outputs = transaction
        .inputs()
        .iter()
        .filter_map(|input| input.outpoint())
        .map(|outpoint| {
            (
                outpoint,
                transparent::OrderedUtxo::new(
                    transparent::Output {
                        value: input_zat.try_into().unwrap(),
                        lock_script: address.script(),
                    },
                    Height(1),
                    1,
                ),
            )
        })
        .collect();
    block.transactions.push(Arc::new(transaction));
    let hash = Hash([u8::try_from(chain.blocks.len() + 1).unwrap(); 32]);
    let hashes = block
        .transactions
        .iter()
        .map(|transaction| transaction.hash())
        .collect::<Vec<_>>();
    chain.height_by_hash.insert(hash, height);
    chain.blocks.insert(
        height,
        ContextuallyVerifiedBlock {
            block: Arc::new(block),
            hash,
            height,
            new_outputs: Default::default(),
            spent_outputs,
            transaction_hashes: hashes.into(),
            chain_value_pool_change: Default::default(),
            pos_payout: false,
        },
    );
    chain.bond_rewards.push(rewards);
    chain.bond_burns.push(
        burns
            .into_iter()
            .map(|key| {
                (
                    key,
                    crate::service::non_finalized_state::BondStatusInChain::Active,
                )
            })
            .collect(),
    );
    chain.aggregated_stakes.push(Vec::new());
}

fn action_transaction(action: StakingAction) -> Transaction {
    Transaction::VCrosslink {
        network_upgrade: NetworkUpgrade::Nu6,
        lock_time: LockTime::unlocked(),
        expiry_height: Height(0),
        inputs: vec![transparent::Input::PrevOut {
            outpoint: OutPoint {
                hash: zebra_chain::transaction::Hash(action.bond_key()),
                index: 0,
            },
            unlock_script: transparent::Script::new(&[]),
            sequence: 0,
        }],
        outputs: Vec::new(),
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        ironwood_shielded_data: None,
        staking_action: Some(action),
    }
}

fn create(key: [u8; 32], target: FinalizerAddress, amount: u64) -> Transaction {
    action_transaction(StakingAction::CreateNewDelegationBond {
        amount_zats: amount,
        unique_pubkey: key,
        bond_salt: [0; 32],
        target_finalizer: target,
        signature: [0; 64],
    })
}

#[test]
fn two_transparent_bonds_and_history_are_visible_at_the_best_tip() {
    let db = database(&Network::Mainnet);
    let mut chain = Chain::default();
    let target = finalizer(b"transparent-sources");
    let key = target.pub_key.0;
    let first_address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
    let second_address = Address::from_pub_key_hash(NetworkKind::Mainnet, [8; 20]);
    let mut first = create([1; 32], target, 30_000_000);
    let mut second = create([2; 32], target, 10_000_000);
    for (transaction, change) in [(&mut first, 39_955_000_u64), (&mut second, 29_940_000_u64)] {
        if let Transaction::VCrosslink { outputs, .. } = transaction {
            outputs.push(transparent::Output {
                value: change.try_into().unwrap(),
                lock_script: second_address.script(),
            });
        }
    }
    append(
        &mut chain,
        10,
        first,
        first_address,
        69_970_000,
        Vec::new(),
        Vec::new(),
    );
    append(
        &mut chain,
        11,
        second,
        second_address,
        39_955_000,
        Vec::new(),
        Vec::new(),
    );
    *chain.aggregated_stakes.last_mut().unwrap() = vec![(key, 40_000_000)];
    let snapshot = MinerStakeUpdates::for_chain(&db, Some(&chain));
    let summary = snapshot.finalizer_summary(key);
    assert_eq!(summary.primary_stake_address, Some(first_address));
    assert_eq!(summary.transparent_address_count, 2);
    let rows = snapshot.entries(
        Some(key),
        None,
        None,
        None,
        ExplorerPageDirection::Older,
        100,
    );
    assert_eq!(
        rows.iter()
            .map(|row| (row.address, row.record.current_stake_zat))
            .collect::<Vec<_>>(),
        vec![(first_address, 30_000_000), (second_address, 10_000_000)]
    );
    let page = crate::explorer::read::explorer_miner_stake_page(
        Some(Arc::new(chain.clone())),
        &db,
        Some(key),
        None,
        None,
        100,
        None,
        ExplorerPageDirection::Older,
    );
    assert_eq!(page.total_current_stake_zat, 40_000_000);
    assert_eq!(page.totals.other_transparent_stake_zat, 40_000_000);
    assert_eq!(page.pair_count, 2);
    assert_eq!(page.best_tip.unwrap().0, Height(11));
    let history = snapshot.history_entries(
        ExplorerStakeHistoryFilter {
            finalizer: Some(key),
            ..Default::default()
        },
        None,
        ExplorerPageDirection::Older,
        Height(0),
        Height::MAX,
        100,
    );
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[0].record.source,
        ExplorerStakeSource::Transparent(second_address)
    );
    assert_eq!(
        history[1].record.source,
        ExplorerStakeSource::Transparent(first_address)
    );
    let chain = Arc::new(chain);
    let filter = ExplorerStakeHistoryFilter {
        finalizer: Some(key),
        ..Default::default()
    };
    let recent = crate::explorer::read::explorer_stake_history_page(
        Some(chain.clone()),
        &db,
        filter,
        1,
        None,
        ExplorerPageDirection::Older,
        Height(0)..=Height::MAX,
    );
    assert!(recent.has_more);
    assert_eq!(recent.best_tip, page.best_tip);
    assert_eq!(recent.entries[0].location.height, Height(11));
    let cursor = crate::ExplorerStakeHistoryCursor {
        location: recent.entries[0].location,
        block_hash: recent.entries[0].block_hash,
    };
    let older = crate::explorer::read::explorer_stake_history_page(
        Some(chain.clone()),
        &db,
        filter,
        1,
        Some(cursor),
        ExplorerPageDirection::Older,
        Height(0)..=Height::MAX,
    );
    assert_eq!(older.entries[0].location.height, Height(10));
    assert!(!older.has_more);
    let cursor = crate::ExplorerStakeHistoryCursor {
        location: older.entries[0].location,
        block_hash: older.entries[0].block_hash,
    };
    let back = crate::explorer::read::explorer_stake_history_page(
        Some(chain.clone()),
        &db,
        filter,
        1,
        Some(cursor),
        ExplorerPageDirection::Newer,
        Height(0)..=Height::MAX,
    );
    assert_eq!(back.entries, recent.entries);
    let mut fork = (*chain).clone();
    fork.blocks.get_mut(&Height(10)).unwrap().hash = Hash([99; 32]);
    let rejected = crate::explorer::read::explorer_stake_history_page(
        Some(Arc::new(fork)),
        &db,
        filter,
        1,
        Some(cursor),
        ExplorerPageDirection::Older,
        Height(0)..=Height::MAX,
    );
    assert!(!rejected.cursor_valid);
    assert!(snapshot.finalizer_address(key).is_some());
    // A provisional read never writes its result back into canonical storage.
    assert!(db
        .explorer_miner_stake_entries(None, None, None, None, ExplorerPageDirection::Older, 100)
        .is_empty());
}

#[test]
fn best_chain_sources_track_rewards_retarget_unbond_withdraw_and_burn() {
    let db = database(&Network::Mainnet);
    let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
    let (a, b) = (finalizer(b"a"), finalizer(b"b"));
    let mut chain = Chain::default();
    append(
        &mut chain,
        10,
        create([1; 32], a, 100),
        address,
        1000,
        vec![([1; 32], 25)],
        Vec::new(),
    );
    append(
        &mut chain,
        11,
        create([2; 32], a, 200),
        address,
        1000,
        Vec::new(),
        Vec::new(),
    );
    let initial = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(
        initial
            .totals(Some(a.pub_key.0))
            .other_transparent_stake_zat,
        325
    );
    assert_eq!(
        initial
            .finalizer_summary(a.pub_key.0)
            .transparent_address_count,
        1
    );
    append(
        &mut chain,
        12,
        action_transaction(StakingAction::RetargetDelegationBond {
            unique_pubkey: [1; 32],
            signature: [0; 64],
            from_finalizer: a,
            to_finalizer: b,
        }),
        address,
        1000,
        vec![([1; 32], 5)],
        Vec::new(),
    );
    let moved = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(
        moved.totals(Some(a.pub_key.0)).other_transparent_stake_zat,
        200
    );
    assert_eq!(
        moved.totals(Some(b.pub_key.0)).other_transparent_stake_zat,
        130
    );
    append(
        &mut chain,
        13,
        action_transaction(StakingAction::BeginDelegationUnbonding {
            unique_pubkey: [1; 32],
            signature: [0; 64],
        }),
        address,
        1000,
        Vec::new(),
        vec![[2; 32]],
    );
    let unbonded = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(unbonded.totals(None), ExplorerMinerStakeTotals::default());
    assert_eq!(
        unbonded
            .finalizer_summary(a.pub_key.0)
            .primary_stake_address,
        None
    );
    assert_eq!(
        unbonded
            .history_record(TransactionLocation::from_usize(Height(13), 1))
            .unwrap()
            .amount_zat,
        Some(130)
    );
    append(
        &mut chain,
        14,
        action_transaction(StakingAction::WithdrawDelegationBond {
            unique_pubkey: [1; 32],
            signature: [0; 64],
            amount_zats: 130,
        }),
        address,
        1000,
        Vec::new(),
        Vec::new(),
    );
    let withdrawn = MinerStakeUpdates::for_chain(&db, Some(&chain));
    let history = withdrawn
        .history_record(TransactionLocation::from_usize(Height(14), 1))
        .unwrap();
    assert_eq!(history.source, ExplorerStakeSource::Transparent(address));
    assert_eq!(history.from_finalizer, Some(b.pub_key.0));
    assert_eq!(history.amount_zat, Some(130));
    // Reading the earlier fork restores the earlier sources and rewards.
    assert_eq!(initial.totals(None).other_transparent_stake_zat, 325);
    assert_eq!(
        initial.finalizer_summary(a.pub_key.0).primary_stake_address,
        Some(address)
    );
}

#[test]
fn fixed_database_view_survives_writes_and_finalized_prefix_is_not_replayed() {
    let db = database(&Network::Mainnet);
    let target = finalizer(b"snapshot");
    let address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
    let mut chain = Chain::default();
    append(
        &mut chain,
        10,
        create([1; 32], target, 100),
        address,
        1000,
        vec![([1; 32], 25)],
        Vec::new(),
    );
    let before = MinerStakeUpdates::for_chain(&db, None);
    let provisional = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(provisional.totals(None).other_transparent_stake_zat, 125);
    let mut batch = DiskWriteBatch::new();
    // Commit exactly the facts previously provided by the provisional view.
    MinerStakeUpdates::for_chain(&db, Some(&chain)).write(&mut batch);
    let _ = TypedColumnFamily::<Height, Hash>::new(db.disk_db(), "hash_by_height")
        .unwrap()
        .with_batch_for_writing(&mut batch)
        .zs_insert(&Height(10), &chain.blocks[&Height(10)].hash);
    db.write_batch(batch).unwrap();
    // Existing readers still see their original finalized base.
    assert_eq!(before.totals(None), ExplorerMinerStakeTotals::default());
    assert!(before
        .entries(None, None, None, None, ExplorerPageDirection::Older, 100)
        .is_empty());
    assert_eq!(provisional.totals(None).other_transparent_stake_zat, 125);
    // An old chain snapshot can retain a block that has since been finalized.
    let after = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(after.totals(None).other_transparent_stake_zat, 125);
    assert_eq!(
        after.entries(None, None, None, None, ExplorerPageDirection::Older, 100)[0]
            .record
            .active_bond_count,
        1
    );
}

#[test]
fn provisional_rankings_merge_persisted_rows_and_paginate_both_directions() {
    let db = database(&Network::Mainnet);
    let target = finalizer(b"pagination");
    let addresses =
        [1, 2, 3, 4].map(|byte| Address::from_pub_key_hash(NetworkKind::Mainnet, [byte; 20]));
    let mut base = Chain::default();
    append(
        &mut base,
        10,
        create([1; 32], target, 100),
        addresses[0],
        1000,
        Vec::new(),
        Vec::new(),
    );
    append(
        &mut base,
        11,
        create([2; 32], target, 200),
        addresses[1],
        1000,
        Vec::new(),
        Vec::new(),
    );
    let mut batch = DiskWriteBatch::new();
    MinerStakeUpdates::for_chain(&db, Some(&base)).write(&mut batch);
    db.write_batch(batch).unwrap();
    let mut suffix = Chain::default();
    // Reward the existing low-ranked pair past all other rows and add two new rows.
    append(
        &mut suffix,
        12,
        create([3; 32], target, 300),
        addresses[2],
        1000,
        vec![([1; 32], 400)],
        Vec::new(),
    );
    append(
        &mut suffix,
        13,
        create([4; 32], target, 400),
        addresses[3],
        1000,
        Vec::new(),
        Vec::new(),
    );
    let snapshot = MinerStakeUpdates::for_chain(&db, Some(&suffix));
    let scope = Some(target.pub_key.0);
    let first = snapshot.entries(scope, None, None, None, ExplorerPageDirection::Older, 2);
    assert_eq!(
        first
            .iter()
            .map(|row| row.record.current_stake_zat)
            .collect::<Vec<_>>(),
        vec![500, 400]
    );
    let cursor = Some((
        first[1].address,
        first[1].finalizer,
        first[1].record.current_stake_zat,
    ));
    let second = snapshot.entries(scope, None, None, cursor, ExplorerPageDirection::Older, 2);
    assert_eq!(
        second
            .iter()
            .map(|row| row.record.current_stake_zat)
            .collect::<Vec<_>>(),
        vec![300, 200]
    );
    let back = Some((
        second[0].address,
        second[0].finalizer,
        second[0].record.current_stake_zat,
    ));
    assert_eq!(
        snapshot.entries(scope, None, None, back, ExplorerPageDirection::Newer, 2),
        first
    );
    assert_eq!(
        snapshot
            .finalizer_summary(target.pub_key.0)
            .transparent_address_count,
        4
    );
    assert_eq!(
        snapshot
            .entries(
                scope,
                Some(addresses[0]),
                Some(false),
                None,
                ExplorerPageDirection::Older,
                10
            )
            .len(),
        1
    );
}

#[test]
fn source_promotion_in_the_suffix_moves_all_current_pairs_to_miners() {
    let db = database(&Network::Mainnet);
    let (a, b) = (finalizer(b"miner-a"), finalizer(b"miner-b"));
    let mut chain = Chain::default();
    let source = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
    append(
        &mut chain,
        10,
        create([1; 32], a, 100),
        source,
        1000,
        Vec::new(),
        Vec::new(),
    );
    append(
        &mut chain,
        11,
        create([2; 32], b, 200),
        source,
        1000,
        Vec::new(),
        Vec::new(),
    );
    let before = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(before.totals(None).other_transparent_stake_zat, 300);
    // The source mines its first block after both bonds were created.
    append(
        &mut chain,
        12,
        create([3; 32], a, 50),
        source,
        1000,
        Vec::new(),
        Vec::new(),
    );
    let block = Arc::make_mut(&mut chain.blocks.get_mut(&Height(12)).unwrap().block);
    let Transaction::V4 { outputs, .. } = Arc::make_mut(&mut block.transactions[0]) else {
        panic!("fixture is a v4 coinbase")
    };
    outputs[0].lock_script = source.script();
    let snapshot = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(snapshot.totals(None).miner_stake_zat, 350);
    assert_eq!(snapshot.totals(None).other_transparent_stake_zat, 0);
    assert_eq!(snapshot.totals(None).miner_address_count, 2);
    assert_eq!(
        snapshot
            .entries(
                None,
                None,
                Some(true),
                None,
                ExplorerPageDirection::Older,
                10
            )
            .len(),
        2
    );
    assert!(snapshot
        .entries(
            None,
            None,
            Some(false),
            None,
            ExplorerPageDirection::Older,
            10
        )
        .is_empty());
}

#[test]
fn shielded_bonds_and_reward_conversions_keep_their_original_sources() {
    let db = database(&Network::Mainnet);
    let (a, b) = (finalizer(b"shielded"), finalizer(b"rewards"));
    let key = [9; 32];
    let attribution = ExplorerBondAttributionRecord {
        source: ExplorerStakeSource::Shielded,
        current_finalizer: a.pub_key.0,
        current_stake_zat: 100,
    };
    let totals = ExplorerMinerStakeTotals {
        shielded_stake_zat: 100,
        shielded_bond_count: 1,
        ..Default::default()
    };
    let mut batch = DiskWriteBatch::new();
    let _ = db
        .explorer_bond_attribution_cf()
        .with_batch_for_writing(&mut batch)
        .zs_insert(&key, &attribution);
    let _ = db
        .explorer_miner_stake_totals_cf()
        .with_batch_for_writing(&mut batch)
        .zs_insert(&(), &totals);
    let _ = db
        .explorer_finalizer_miner_totals_cf()
        .with_batch_for_writing(&mut batch)
        .zs_insert(&a.pub_key.0, &totals);
    db.write_batch(batch).unwrap();
    let fee_address = Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]);
    let mut chain = Chain::default();
    append(
        &mut chain,
        10,
        action_transaction(StakingAction::ConvertFinalizerRewardToDelegationBond {
            amount_zats: 50,
            unique_pubkey: [8; 32],
            bond_salt: [0; 32],
            this_finalizer: a.pub_key.0,
            signature: [0; 64],
            finalizer_signature: [0; 64],
        }),
        fee_address,
        1000,
        vec![(key, 25), ([8; 32], 5)],
        Vec::new(),
    );
    let converted = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(converted.totals(None).shielded_stake_zat, 125);
    assert_eq!(converted.totals(None).reward_bond_stake_zat, 55);
    assert_eq!(
        converted
            .finalizer_summary(a.pub_key.0)
            .transparent_address_count,
        0
    );
    assert_eq!(converted.finalizer_address(a.pub_key.0), None);
    append(
        &mut chain,
        11,
        action_transaction(StakingAction::RetargetDelegationBond {
            unique_pubkey: key,
            from_finalizer: a,
            to_finalizer: b,
            signature: [0; 64],
        }),
        fee_address,
        1000,
        Vec::new(),
        Vec::new(),
    );
    let moved = MinerStakeUpdates::for_chain(&db, Some(&chain));
    assert_eq!(moved.totals(Some(a.pub_key.0)).shielded_stake_zat, 0);
    assert_eq!(moved.totals(Some(b.pub_key.0)).shielded_stake_zat, 125);
    assert_eq!(moved.totals(Some(a.pub_key.0)).reward_bond_stake_zat, 55);
    assert_eq!(
        moved
            .history_record(TransactionLocation::from_usize(Height(11), 1))
            .unwrap()
            .source,
        ExplorerStakeSource::Shielded
    );
    assert!(moved
        .entries(None, None, None, None, ExplorerPageDirection::Older, 100)
        .is_empty());
}
