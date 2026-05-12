//! Benchmarks for parsing L1 block-info calldata during EVM block execution.

use std::hint::black_box;

use alloy_primitives::hex_literal::hex;
use base_execution_evm::parse_l1_info;
use criterion::{Criterion, criterion_group, criterion_main};

fn bedrock_input() -> Vec<u8> {
    let mut input = vec![0u8; 4 + 256];
    input[68..100]
        .copy_from_slice(&hex!("000000000000000000000000000000000000000000000000000000000009f352"));
    input[196..228]
        .copy_from_slice(&hex!("0000000000000000000000000000000000000000000000000000000000000834"));
    input[228..260]
        .copy_from_slice(&hex!("00000000000000000000000000000000000000000000000000000000000f4240"));
    input
}

fn ecotone_input() -> Vec<u8> {
    hex!(
        "440a5e200000146b000f79c500000000000000040000000066d052e700000000013ad8a3000000000000000000000000000000000000000000000000000000003ef1278700000000000000000000000000000000000000000000000000000000000000012fdf87b89884a61e74b322bbcf60386f543bfae7827725efaaf0ab1de2294a590000000000000000000000006887246668a3b87f54deb3b94ba47a6f63f32985"
    )
    .to_vec()
}

fn isthmus_input() -> Vec<u8> {
    hex!(
        "098999be00000558000c5fc500000000000000030000000067a9f765000000000000002900000000000000000000000000000000000000000000000000000000006a6d09000000000000000000000000000000000000000000000000000000000000000172fcc8e8886636bdbe96ba0e4baab67ea7e7811633f52b52e8cf7a5123213b6f000000000000000000000000d3f2c5afb2d76f5579f326b0cd7da5f5a4126c3500004e2000000000000001f4"
    )
    .to_vec()
}

fn jovian_input() -> Vec<u8> {
    hex!(
        "3db6be2b00000558000c5fc500000000000000030000000067a9f765000000000000002900000000000000000000000000000000000000000000000000000000006a6d09000000000000000000000000000000000000000000000000000000000000000172fcc8e8886636bdbe96ba0e4baab67ea7e7811633f52b52e8cf7a5123213b6f000000000000000000000000d3f2c5afb2d76f5579f326b0cd7da5f5a4126c3500004e2000000000000001f4dead"
    )
    .to_vec()
}

fn bench_parse_l1_info(c: &mut Criterion) {
    for (name, input) in [
        ("bedrock", bedrock_input()),
        ("ecotone", ecotone_input()),
        ("isthmus", isthmus_input()),
        ("jovian", jovian_input()),
    ] {
        c.bench_function(&format!("parse_l1_info/{name}"), |b| {
            b.iter(|| {
                black_box(parse_l1_info(black_box(&input)).unwrap());
            });
        });
    }
}

criterion_group!(benches, bench_parse_l1_info);
criterion_main!(benches);
