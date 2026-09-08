//! Reproducible CPU-kernel comparison, no providers, sockets or live memory.
use contextnest::{memory::attractors::utils, services::exact};
use std::time::Instant;
fn main() {
    let dimensions = 256;
    let count = 10_000;
    let scope_count = 500;
    let k = 32;
    let vectors: Vec<_> = (0..count)
        .map(|i| {
            (0..dimensions)
                .map(|d| ((i * 31 + d * 17 + 7) % 997) as f32 / 997.0)
                .collect::<Vec<_>>()
        })
        .collect();
    let ids: Vec<_> = (0..count).map(|i| format!("record-{i:05}")).collect();
    let query = &vectors[123];
    let norms: Vec<_> = vectors.iter().map(|v| exact::norm(v).unwrap()).collect();
    let qn = exact::norm(query).unwrap();
    let start = Instant::now();
    let mut reference: Vec<_> = ids
        .iter()
        .zip(&vectors)
        .map(|(id, v)| (id.clone(), utils::cosine_similarity(query, v)))
        .collect();
    reference.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    reference.truncate(k);
    let reference_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let optimized = exact::top_k(
        ids.iter()
            .zip(&vectors)
            .zip(&norms)
            .map(|((id, v), n)| (id.as_str(), exact::cosine(query, qn, v, *n).unwrap())),
        k,
    );
    let optimized_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        reference.iter().map(|r| &r.0).collect::<Vec<_>>(),
        optimized.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    for (a, b) in reference.iter().zip(&optimized) {
        assert!((a.1 - b.1).abs() < 1e-6);
    }
    let start = Instant::now();
    let scoped = exact::top_k(
        ids.iter()
            .zip(&vectors)
            .zip(&norms)
            .take(scope_count)
            .map(|((id, v), n)| (id.as_str(), exact::cosine(query, qn, v, *n).unwrap())),
        k,
    );
    std::hint::black_box(scoped);
    println!(
        "{}",
        serde_json::json!({"profile":env!("CONTEXTNEST_BUILD_PROFILE"),"dimensions":dimensions,"global_candidates":count,"session_candidates":scope_count,"k":k,"reference_ms":reference_ms,"cached_norm_top_k_ms":optimized_ms,"session_exact_ms":start.elapsed().as_secs_f64()*1000.0,"global_reference_parity":true})
    );
}
