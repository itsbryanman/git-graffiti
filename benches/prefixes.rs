use std::{
    hint::black_box,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use git_graffiti::object;

fn main() {
    let raw = b"tree 0123456789012345678901234567890123456789\nauthor bench <bench@example.com> 1 +0000\ncommitter bench <bench@example.com> 1 +0000\n\nbench\n";
    let prepared = Arc::new(object::prepare(raw).expect("prepare benchmark commit"));
    let threads = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let per_thread = 2_000_000_u64;
    let start = Instant::now();
    let mut workers = Vec::with_capacity(threads);
    for worker in 0..threads {
        let prepared = Arc::clone(&prepared);
        workers.push(thread::spawn(move || {
            let begin = worker as u64 * per_thread;
            for nonce in begin..begin + per_thread {
                black_box(prepared.digest_for_nonce(black_box(nonce)));
            }
        }));
    }
    for worker in workers {
        worker.join().expect("benchmark worker panicked");
    }
    let elapsed = start.elapsed();
    let attempts = per_thread * threads as u64;
    let hashes_per_second = attempts as f64 / elapsed.as_secs_f64();

    println!("hardware threads: {threads}");
    println!("measured: {hashes_per_second:.0} hashes/s over {attempts} hashes");
    for prefix_len in [5_u32, 6, 7] {
        let expected_attempts = 16_u64.pow(prefix_len);
        let expected = Duration::from_secs_f64(expected_attempts as f64 / hashes_per_second);
        println!("{prefix_len} chars: {expected:.3?} expected ({expected_attempts} attempts)");
    }
}
