use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

use anyhow::{Context, Result, bail};

use crate::{object::PreparedCommit, sha1mid::PrefixMask};

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub nonce: u64,
    pub digest: [u8; 20],
}

pub fn search(prepared: &PreparedCommit, mask: &PrefixMask, threads: usize) -> Result<Hit> {
    if threads == 0 {
        bail!("thread count is zero. use at least one thread");
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None));
    let stride = threads as u64;

    thread::scope(|scope| {
        for worker in 0..threads {
            let stopped = Arc::clone(&stopped);
            let result = Arc::clone(&result);
            scope.spawn(move || {
                let mut nonce = worker as u64;
                loop {
                    if stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    let digest = prepared.digest_for_nonce(nonce);
                    if mask.matches(&digest) {
                        if stopped
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
                            .is_ok()
                        {
                            *result.lock().expect("result lock poisoned") =
                                Some(Hit { nonce, digest });
                        }
                        break;
                    }
                    let Some(next) = nonce.checked_add(stride) else {
                        break;
                    };
                    nonce = next;
                }
            });
        }
    });

    result
        .lock()
        .expect("result lock poisoned")
        .take()
        .context("searched the entire nonce space without a hit")
}
