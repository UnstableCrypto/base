//! Benchmarks for loading L1 block-info values from EVM state.

use base_common_consensus::Predeploys;
use base_common_evm::{BaseSpecId, BaseUpgrade, L1BlockInfo};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use revm::{database::InMemoryDB, primitives::U256, state::AccountInfo};

fn packed_ecotone_scalars(base_fee_scalar: u32, blob_base_fee_scalar: u32) -> U256 {
    let mut slot = [0u8; 32];
    slot[L1BlockInfo::BASE_FEE_SCALAR_OFFSET..L1BlockInfo::BASE_FEE_SCALAR_OFFSET + 4]
        .copy_from_slice(&base_fee_scalar.to_be_bytes());
    slot[L1BlockInfo::BLOB_BASE_FEE_SCALAR_OFFSET..L1BlockInfo::BLOB_BASE_FEE_SCALAR_OFFSET + 4]
        .copy_from_slice(&blob_base_fee_scalar.to_be_bytes());
    U256::from_be_bytes(slot)
}

fn packed_operator_fee_and_da_footprint(
    da_footprint_gas_scalar: u16,
    operator_fee_scalar: u32,
    operator_fee_constant: u64,
) -> U256 {
    let mut slot = [0u8; 32];
    slot[L1BlockInfo::DA_FOOTPRINT_GAS_SCALAR_OFFSET
        ..L1BlockInfo::DA_FOOTPRINT_GAS_SCALAR_OFFSET + 2]
        .copy_from_slice(&da_footprint_gas_scalar.to_be_bytes());
    slot[L1BlockInfo::OPERATOR_FEE_SCALAR_OFFSET..L1BlockInfo::OPERATOR_FEE_SCALAR_OFFSET + 4]
        .copy_from_slice(&operator_fee_scalar.to_be_bytes());
    slot[L1BlockInfo::OPERATOR_FEE_CONSTANT_OFFSET..L1BlockInfo::OPERATOR_FEE_CONSTANT_OFFSET + 8]
        .copy_from_slice(&operator_fee_constant.to_be_bytes());
    U256::from_be_bytes(slot)
}

fn l1_block_info_db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(Predeploys::L1_BLOCK_INFO, AccountInfo::default());

    let l1_block_contract = db.load_account(Predeploys::L1_BLOCK_INFO).unwrap();
    l1_block_contract.storage.insert(L1BlockInfo::L1_BASE_FEE_SLOT, U256::from(1));
    l1_block_contract.storage.insert(L1BlockInfo::ECOTONE_L1_BLOB_BASE_FEE_SLOT, U256::from(2));
    l1_block_contract
        .storage
        .insert(L1BlockInfo::ECOTONE_L1_FEE_SCALARS_SLOT, packed_ecotone_scalars(3, 4));
    l1_block_contract.storage.insert(
        L1BlockInfo::OPERATOR_FEE_SCALARS_SLOT,
        packed_operator_fee_and_da_footprint(5, 6, 7),
    );

    db
}

fn bench_try_fetch(c: &mut Criterion) {
    let mut group = c.benchmark_group("l1_block_info_try_fetch");

    for upgrade in [BaseUpgrade::Isthmus, BaseUpgrade::Jovian] {
        group.bench_function(format!("{upgrade:?}"), |b| {
            b.iter_batched(
                l1_block_info_db,
                |mut db| {
                    L1BlockInfo::try_fetch(&mut db, U256::from(100), BaseSpecId::new(upgrade))
                        .unwrap()
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_try_fetch);
criterion_main!(benches);
