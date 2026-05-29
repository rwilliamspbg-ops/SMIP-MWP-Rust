use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crypto::kex::{HybridKEX, INITIATOR_PUB_LEN, RESPONDER_MSG_LEN, SESSION_SECRET_LEN};
use crypto::session::{HybridSession, prederive_session, TAG_SIZE};

const PAYLOAD_LEN: usize = 128;

fn baseline_no_crypto(b: &mut criterion::Bencher<'_>) {
    b.iter(|| {
        let mut packet = vec![0u8; PAYLOAD_LEN];
        for (index, byte) in packet.iter_mut().enumerate() {
            *byte = (index & 0xFF) as u8;
        }

        black_box(packet)
    });
}

fn hybrid_kex_round_trip(b: &mut criterion::Bencher<'_>) {
    b.iter(|| {
        let initiator = HybridKEX::new().expect("initiator keygen");
        let mut responder = HybridKEX::new().expect("responder keygen");

        let initiator_pub = initiator.public_key();
        assert_eq!(initiator_pub.len(), INITIATOR_PUB_LEN);

        let (responder_msg, responder_secret) = responder
            .respond(&initiator_pub)
            .expect("responder handshake");
        assert_eq!(responder_msg.len(), RESPONDER_MSG_LEN);
        assert_eq!(responder_secret.len(), SESSION_SECRET_LEN);

        let mut initiator = initiator;
        let initiator_secret = initiator
            .finish(&responder_msg)
            .expect("initiator handshake");
        assert_eq!(initiator_secret.len(), SESSION_SECRET_LEN);

        black_box((responder_secret, initiator_secret))
    });
}

fn crypto_overhead_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_overhead");
    group.bench_function("baseline_no_crypto", baseline_no_crypto);
    group.bench_function("worst_case_hybrid_kex", hybrid_kex_round_trip);
    // Symmetric per-packet encrypt/decrypt (realistic dataplane measurement)
    let combined_secret = vec![0x42u8; 64];
    let session_info = b"bench-session-info";
    // ensure pre-derived cache
    let _ = prederive_session(&combined_secret, session_info);
    let sess = HybridSession::new(&combined_secret, session_info).expect("session");

    let payload = vec![0x55u8; PAYLOAD_LEN];

    group.bench_function("symmetric_encrypt_in_place", |b| {
        let mut seq: u64 = 1;
        b.iter(|| {
            let mut dst = Vec::with_capacity(payload.len() + TAG_SIZE);
            sess.encrypt_to(&mut dst, &payload, seq).expect("encrypt");
            seq = seq.wrapping_add(1);
            black_box(&dst);
        })
    });

    group.bench_function("symmetric_decrypt_in_place", |b| {
        // prepare a ciphertext to decrypt; use the same seq so auth succeeds
        let seq: u64 = 1;
        let ct_template = sess.encrypt(&payload, seq).expect("encrypt template");
        b.iter(|| {
            let mut ct = ct_template.clone();
            sess.decrypt_in_place(&mut ct, seq).expect("decrypt");
            black_box(&ct);
        })
    });
    group.finish();
}

criterion_group!(benches, crypto_overhead_benchmark);
criterion_main!(benches);