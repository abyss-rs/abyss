use abyss_core::sync::{apply_delta, compute_delta, compute_signature};
use std::time::Instant;

#[test]
fn test_large_10mb_file_delta_sync_real_stress_test() {
    println!("\n=== Starting 10MB BLAKE3 SIMD Delta Real-World Stress Test ===");

    // 1. Generate 10MB base file with pseudo-random content
    let file_size = 10 * 1024 * 1024; // 10MB
    let mut base_data = vec![0u8; file_size];
    for (i, byte) in base_data.iter_mut().enumerate() {
        *byte = ((i * 31 + 17) % 256) as u8;
    }

    // 2. Create modified target data:
    // - Insert 50KB at offset 1MB
    // - Modify 100 scattered single bytes across the 10MB
    // - Delete 20KB at offset 5MB
    // - Append 100KB at the end
    let mut target_data = base_data.clone();

    let insert_data = vec![0xEE_u8; 50 * 1024];
    target_data.splice(1024 * 1024..1024 * 1024, insert_data);

    for i in 0..100 {
        let offset = (i * 97 * 1024) % target_data.len();
        target_data[offset] = 0xAA;
    }

    target_data.drain(5 * 1024 * 1024..5 * 1024 * 1024 + 20 * 1024);
    target_data.extend(vec![0xCC_u8; 100 * 1024]);

    println!("Base data size:   {} bytes", base_data.len());
    println!("Target data size: {} bytes", target_data.len());

    // 3. Measure Signature Generation time
    let start_sig = Instant::now();
    let signature = compute_signature(&base_data, 2048);
    let sig_duration = start_sig.elapsed();
    println!(
        "Signature generation (10MB, 2048 block): {:?}",
        sig_duration
    );

    // 4. Measure Delta Computation time
    let start_delta = Instant::now();
    let delta = compute_delta(&signature, &target_data);
    let delta_duration = start_delta.elapsed();
    println!("Delta computation (10MB target): {:?}", delta_duration);
    println!(
        "Delta payload size: {} bytes ({:.2}% of target)",
        delta.len(),
        (delta.len() as f64 / target_data.len() as f64) * 100.0
    );

    assert!(
        delta.len() < target_data.len() / 10,
        "Delta should be under 10% of total size for modified files"
    );

    // 5. Measure Delta Application & Verification
    let start_apply = Instant::now();
    let reconstructed = apply_delta(&base_data, &delta).expect("apply delta must succeed");
    let apply_duration = start_apply.elapsed();
    println!("Delta application (reconstruction): {:?}", apply_duration);

    // 6. Assert byte-for-byte equality
    assert_eq!(reconstructed.len(), target_data.len());
    assert_eq!(
        reconstructed, target_data,
        "Reconstructed data MUST match target byte-for-byte!"
    );
    println!("=== Byte-for-byte match verified successfully! ===\n");
}
