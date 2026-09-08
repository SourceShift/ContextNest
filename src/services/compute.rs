//! Shared, bounded CPU admission. Permits are held by the blocking closure,
//! including when the HTTP future times out or is dropped.
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

/// Cumulative user + system CPU seconds for this process, across its threads.
/// Operator diagnostics only: this cannot be attributed to one tenant job.
pub fn process_cpu_seconds() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<nix::libc::rusage>::uninit();
        // SAFETY: getrusage initializes the output on success; failure is
        // returned before reading it. RUSAGE_SELF includes all process threads.
        if unsafe { nix::libc::getrusage(nix::libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        let usage = unsafe { usage.assume_init() };
        Some(
            (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
                + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.0,
        )
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn budget() -> &'static Arc<Semaphore> {
    static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
    BUDGET.get_or_init(|| {
        let workers = std::env::var("CONTEXTNEST_CPU_WORKERS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 16);
        Arc::new(Semaphore::new(workers))
    })
}

pub async fn run<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = budget()
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Callers at an untrusted request boundary must also limit admission before
/// awaiting `run`; this prevents a queue of unbounded waiting tasks.
pub async fn process(
    manager: Arc<crate::memory::attractors::MemoryAttractorManager>,
    request: crate::memory::attractors::memory_attractor_manager::MemoryProcessingRequest,
) -> crate::error::ContextNestResult<
    crate::memory::attractors::memory_attractor_manager::MemoryProcessingResult,
> {
    let runtime = tokio::runtime::Handle::current();
    run(move || runtime.block_on(manager.process_memories(request)))
        .await
        .map_err(crate::error::ContextNestError::Validation)?
}
