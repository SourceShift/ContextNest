//! Whole-process resident-memory ceiling.
//!
//! The substrate is one long-lived process on a machine that also runs an
//! agent herd (dozens of `claude` + MCP processes). On 2026-09-30 a pre-v0.2
//! build booted here with the old inline-JSON heap profile, peaked at 20.15 GB
//! RSS, and the host's compressor ran out of swap **segments** — a fixed kernel
//! limit, independent of free disk. `watchdogd` missed its 93 s checkin and
//! macOS hard-rebooted. See `docs/upgrading/v0.2.0.md`.
//!
//! The lesson is not "make the substrate smaller" — it is that a process which
//! is about to become the straw should exit itself, loudly, while the kernel is
//! still healthy enough to log. Hence a plain sampler thread that never touches
//! the manager lock and needs no async runtime.

use std::time::Duration;

/// Ceiling in mebibytes. Unset → half of physical memory. `0` → disabled.
pub const MAX_RSS_ENV: &str = "CONTEXTNEST_MAX_RSS_MB";

/// Exit code used when the ceiling is crossed (`EX_SOFTWARE`).
pub const EXIT_RSS_CEILING: i32 = 70;

/// Default ceiling as a fraction of physical memory. A single substrate
/// process taking more than half the machine means every other process on it
/// is already in the compressor's way.
const DEFAULT_PHYSICAL_FRACTION: f64 = 0.5;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const WARN_FRACTION: f64 = 0.8;

/// Resident set size of this process, in bytes.
///
/// `None` when the platform query fails — callers treat that as "unknown",
/// never as "small".
#[cfg(target_os = "macos")]
pub fn resident_bytes() -> Option<usize> {
    let mut info: nix::libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<nix::libc::proc_taskinfo>() as nix::libc::c_int;
    // SAFETY: `info` is a correctly sized `proc_taskinfo`; `proc_pidinfo`
    // writes at most `size` bytes into it and returns the byte count written,
    // so a short result is rejected rather than read as a partial struct.
    let written = unsafe {
        nix::libc::proc_pidinfo(
            nix::libc::getpid(),
            nix::libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut nix::libc::c_void,
            size,
        )
    };
    (written == size).then_some(info.pti_resident_size as usize)
}

/// Resident set size of this process, in bytes (Linux: `/proc/self/statm`).
#[cfg(target_os = "linux")]
pub fn resident_bytes() -> Option<usize> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: usize = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page = page_size()?;
    Some(pages * page)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn resident_bytes() -> Option<usize> {
    None
}

#[cfg(target_os = "linux")]
fn page_size() -> Option<usize> {
    // SAFETY: `sysconf` is a pure query with no failure mode beyond -1.
    let page = unsafe { nix::libc::sysconf(nix::libc::_SC_PAGESIZE) };
    (page > 0).then_some(page as usize)
}

/// Total physical memory, in bytes.
pub fn physical_bytes() -> Option<usize> {
    // SAFETY: both `sysconf` calls are pure queries.
    let (pages, page) = unsafe {
        (
            nix::libc::sysconf(nix::libc::_SC_PHYS_PAGES),
            nix::libc::sysconf(nix::libc::_SC_PAGESIZE),
        )
    };
    (pages > 0 && page > 0).then(|| pages as usize * page as usize)
}

/// The configured ceiling in bytes, or `None` when the guard is disabled or
/// no ceiling can be derived.
pub fn configured_ceiling() -> Option<usize> {
    if let Ok(raw) = std::env::var(MAX_RSS_ENV) {
        let raw = raw.trim();
        return match raw.parse::<u64>() {
            Ok(0) => None,
            Ok(mb) => Some((mb as usize).saturating_mul(1024 * 1024)),
            Err(_) => {
                tracing::warn!(
                    value = raw,
                    "{MAX_RSS_ENV} is not an integer number of MiB; using the default"
                );
                default_ceiling()
            }
        };
    }
    default_ceiling()
}

fn default_ceiling() -> Option<usize> {
    physical_bytes().map(|total| (total as f64 * DEFAULT_PHYSICAL_FRACTION) as usize)
}

/// Spawn the sampler thread. Returns the ceiling in bytes, or `None` when the
/// guard is off (`CONTEXTNEST_MAX_RSS_MB=0`, or physical memory is unknown).
///
/// On crossing the ceiling the process logs at `error!` and exits with
/// [`EXIT_RSS_CEILING`]; it deliberately does not unwind, because the point of
/// the guard is to stop allocating immediately.
pub fn spawn_guard() -> Option<usize> {
    let ceiling = configured_ceiling()?;
    let started = std::thread::Builder::new()
        .name("rss-guard".into())
        .spawn(move || {
            let mut warned = false;
            loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                let Some(rss) = resident_bytes() else {
                    continue;
                };
                if rss >= ceiling {
                    tracing::error!(
                        resident_mib = rss / (1024 * 1024),
                        ceiling_mib = ceiling / (1024 * 1024),
                        "memory guard: resident memory reached the ceiling — exiting so the \
                         host does not swap to death. Raise or unset {MAX_RSS_ENV} to allow more."
                    );
                    std::process::exit(EXIT_RSS_CEILING);
                }
                if !warned && (rss as f64) >= ceiling as f64 * WARN_FRACTION {
                    warned = true;
                    tracing::warn!(
                        resident_mib = rss / (1024 * 1024),
                        ceiling_mib = ceiling / (1024 * 1024),
                        "memory guard: resident memory is over {}% of the ceiling",
                        (WARN_FRACTION * 100.0) as u32
                    );
                }
            }
        });
    match started {
        Ok(_) => Some(ceiling),
        Err(err) => {
            tracing::warn!("memory guard: sampler thread could not start: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_memory_is_plausible_on_this_platform() {
        let rss = resident_bytes().expect("this platform should report RSS");
        assert!(rss > 1024 * 1024, "a running process is more than 1 MiB");
        assert!(
            rss < 1024usize.pow(4),
            "and less than 1 TiB — a bytes/KiB units slip would blow this"
        );
    }

    #[test]
    fn ceiling_follows_the_env_knob() {
        std::env::set_var(MAX_RSS_ENV, "512");
        assert_eq!(configured_ceiling(), Some(512 * 1024 * 1024));
        std::env::set_var(MAX_RSS_ENV, "0");
        assert_eq!(configured_ceiling(), None, "0 disables the guard");
        std::env::set_var(MAX_RSS_ENV, "nonsense");
        assert!(
            configured_ceiling().is_some(),
            "an unparseable value falls back to the default"
        );
        std::env::remove_var(MAX_RSS_ENV);
        // Unset → half of physical memory, when the platform reports it.
        if let Some(total) = physical_bytes() {
            assert_eq!(configured_ceiling(), Some(total / 2));
        }
    }
}
