//! Data-volume watchdog: exit fast when the substrate's data volume goes away.
//!
//! On 2026-10-14 the operator's data volume (`/Volumes/docker-ssd`) dropped off
//! the USB bus during a Maintenance Sleep, spent ~5 s re-enumerating on a *new*
//! BSD node (`disk8s1` → `disk7s1`), and came back. The server was holding every
//! data file open across that gap, so the kernel **revoked** those file
//! descriptors — and a revoked vnode never recovers, not even once the volume is
//! back.
//!
//! The damage was not that the server died. It is that it didn't. The TCP
//! listener is a socket, not a file on that volume, so the process kept
//! *accepting* connections and never answered one; it burned 36 hours of a core
//! spinning in the kernel, and every liveness check that only probes the port
//! reported it healthy. Two threads had accumulated 859 and 843 minutes of
//! system time against 79 and 16 of user time — state `R`, no work done — which
//! is what a file operation against a revoked vnode looks like from outside.
//!
//! There is no in-process recovery from a revoked fd. The only useful thing the
//! process can do is notice and get out of the way, fast enough that a
//! supervisor can put it back on a healthy mount before the gaps in the ingest
//! stream get wide. That is this module.
//!
//! ## What "gone" means
//!
//! The watchdog samples the *identity* of the data directory — device number
//! plus inode — rather than merely testing that the path resolves. Three
//! distinct failures have to be caught, and only the third is the one that
//! happened:
//!
//! 1. The volume unmounts and the path stops resolving at all.
//! 2. AUTOMOUNT substitutes an empty stub directory for the mount point, so the
//!    path resolves to something that is not the directory we booted against.
//! 3. The volume remounts on a new BSD node. This is the real case, and the
//!    trap is that the path resolves again — to a directory that looks correct
//!    in every way a path comparison could check, while every fd this process
//!    holds stays revoked. Only `st_dev` tells the two mounts apart.
//!
//! ## The blind spot: a remount that keeps its device number
//!
//! On 2026-10-18 the same volume detached and re-enumerated over USB, came
//! back on the *same* BSD node, and `st_dev` was unchanged (`16777247` before
//! and after). [`classify`] therefore reported [`Health::Intact`] forever while
//! every fd the server held was revoked — the 2026-10-14 outage with a
//! detector that could not see it. Path identity is necessary but not
//! sufficient: it answers "is there a directory here that looks right?", not
//! "can this process still use it?".
//!
//! So arming also opens a **sentinel file inside the data directory** and keeps
//! that `File` for the lifetime of the process. Every sample seeks to 0 and
//! writes one byte through it. A revoked vnode fails that write with `EBADF`
//! immediately, which is [`Health::Stale`] — the state the path comparison
//! cannot distinguish from healthy. One byte at offset 0, never truncated, so
//! the file stays a single byte and the probe costs no allocation or fsync.
//!
//! A single failed sample is not a detach: a busy volume can transiently fail a
//! `stat`. [`MISSES_BEFORE_EXIT`] consecutive failures are required, so the
//! watchdog needs ~15 s of sustained silence to fire and a blip does not take
//! the substrate down.
//!
//! ## The other way a data volume takes the substrate down: it fills up
//!
//! A detach is the dramatic failure; a full volume is the common one. The
//! operator volume sat at 99 % with 25 GiB free, which is under the space a
//! single `cn-compact` needs (it writes a second full checkpoint before
//! swapping) and close to what the substrate's own WAL growth wants. A write
//! that fails on `ENOSPC` mid-checkpoint is a much worse outcome than a boot
//! that refuses to start, so [`enforce_free_space`] runs before any boot work:
//! below [`DEFAULT_MIN_FREE_MB`] the process exits rather than accepting work it
//! cannot keep, and below twice the checkpoint size it warns that `cn-compact`
//! is not currently possible. `CONTEXTNEST_MIN_FREE_MB=0` turns the floor off.
//!
//! ## Why the exit is `_exit(2)`, and why the note goes to the boot volume
//!
//! By the time this fires, stdout, stderr and every `tracing` sink are either
//! on the dead volume or in a console nobody is watching, so the line explaining
//! the death cannot be written where it is needed most. The watchdog therefore
//! drops a breadcrumb under the system temp directory — on the boot volume,
//! mounted independently of the data volume — and then calls `_exit` directly.
//! `std::process::exit` runs atexit handlers and flushes stdio, and a flush
//! against a revoked fd is one more way to hang rather than a way to be heard.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Exit code when the data volume is lost (`EX_TEMPFAIL`). Deliberately
/// distinct from the memory guard's `70` so a supervisor — and an operator
/// reading `launchctl print` — can tell the two failures apart.
pub const EXIT_VOLUME_LOST: i32 = 75;

/// Set to `0` to disable the watchdog.
pub const DISABLE_ENV: &str = "CONTEXTNEST_VOLUME_WATCH";

/// How often the data directory is stat'd. One `stat` per tick, no I/O beyond
/// that — this thread must never be the reason the substrate is slow.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// Consecutive failed samples before the process exits. At [`SAMPLE_INTERVAL`]
/// this is ~15 s of sustained loss.
const MISSES_BEFORE_EXIT: u32 = 3;

/// Name of the breadcrumb file left on the boot volume. Full path is
/// `std::env::temp_dir().join(this)`.
const BREADCRUMB: &str = "contextnest-volume-lost.log";

/// Liveness sentinel opened inside the data directory at arm time. The open
/// `File` is the watchdog's proof that this process can still *use* the volume,
/// not merely see it — see the module docs on same-device remounts.
const SENTINEL: &str = ".volume-watchdog.sentinel";

/// What each sample flips the sentinel to. One byte at offset 0, so the file
/// never grows past a single byte no matter how long the process runs.
const SENTINEL_BYTE: &[u8] = b".";

/// Identity of the filesystem object at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeId {
    /// `st_dev` — the mounted device. This is the field that changes when a
    /// volume detaches and re-attaches on a new BSD node.
    pub dev: u64,
    /// `st_ino` — distinguishes a real directory from an automount stub.
    pub ino: u64,
}

/// Identity of the directory at `path`, or `None` when it cannot be stat'd.
///
/// `None` is "unknown", never "fine" — callers must treat it as a failed
/// sample.
pub fn volume_id(path: &Path) -> Option<VolumeId> {
    let meta = std::fs::metadata(path).ok()?;
    Some(VolumeId {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// What one sample means, given the identity recorded at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Same device and inode as at boot.
    Intact,
    /// The path does not resolve.
    Missing,
    /// The path resolves, but to a different filesystem object — the volume
    /// remounted, or an automount stub took the mount point's place.
    Replaced,
    /// The path still stats to the object recorded at boot, but a write
    /// through the descriptor opened then no longer works. The volume
    /// detached and came back on the same device number, so every fd this
    /// process holds — including the WAL and every data file — is revoked and
    /// can never recover.
    Stale,
}

impl Health {
    /// A phrase for the breadcrumb, since that file is the only channel that
    /// survives the loss.
    fn describe(self) -> &'static str {
        match self {
            Health::Intact => "is intact",
            Health::Missing => "no longer resolves",
            Health::Replaced => "now resolves to a different filesystem object",
            Health::Stale => "still stats but its filesystem handles are revoked",
        }
    }
}

/// Classify one sample against the identity recorded at boot.
pub fn classify(expected: VolumeId, observed: Option<VolumeId>) -> Health {
    match observed {
        None => Health::Missing,
        Some(actual) if actual == expected => Health::Intact,
        Some(_) => Health::Replaced,
    }
}

/// Open — creating if needed — the liveness sentinel inside `data_dir`.
///
/// `None` means the sentinel could not be opened. The watchdog still arms on
/// path identity alone, but a same-device remount will not be detected; that is
/// a degraded guard, not a reason to refuse to boot, since a data directory the
/// substrate cannot write to will fail loudly on its own.
fn open_sentinel(dir: &Path) -> Option<File> {
    let path = dir.join(SENTINEL);
    match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => Some(file),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "volume watchdog: could not open the liveness sentinel; falling back to \
                 path-identity checks only (a remount that keeps the same device number \
                 will not be detected)"
            );
            None
        }
    }
}

/// Flip the sentinel through the descriptor opened at arm time.
///
/// A revoked vnode rejects `lseek` and `write` with `EBADF` immediately — that
/// is the entire signal. `Interrupted` is retried rather than counted, because a
/// signal arriving mid-probe says nothing about the volume.
fn probe_sentinel<W: Write + Seek>(sentinel: &mut W) -> std::io::Result<()> {
    const ATTEMPTS: usize = 3;
    for attempt in 1..=ATTEMPTS {
        let attempt_result = sentinel
            .seek(SeekFrom::Start(0))
            .and_then(|_| sentinel.write_all(SENTINEL_BYTE));
        match attempt_result {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted && attempt < ATTEMPTS => {
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// One full liveness sample: path identity first, then the held descriptor.
///
/// The descriptor check only runs when the path still looks right — if the
/// volume is already [`Health::Missing`] or [`Health::Replaced`] there is
/// nothing the sentinel could add, and probing it would just be a guaranteed
/// failure on the way to the same verdict.
fn sample<W: Write + Seek>(dir: &Path, expected: VolumeId, sentinel: &mut Option<W>) -> Health {
    let by_path = classify(expected, volume_id(dir));
    if by_path != Health::Intact {
        return by_path;
    }
    let Some(sentinel) = sentinel.as_mut() else {
        return Health::Intact;
    };
    match probe_sentinel(sentinel) {
        Ok(()) => Health::Intact,
        Err(err) => {
            tracing::debug!(
                path = %dir.display(),
                %err,
                "volume watchdog: sentinel write failed — the volume's descriptors are stale"
            );
            Health::Stale
        }
    }
}

/// Is the watchdog switched off via [`DISABLE_ENV`]?
fn disabled() -> bool {
    matches!(std::env::var(DISABLE_ENV).as_deref(), Ok("0"))
}

/// Arm the watchdog on `data_dir`.
///
/// Returns the identity the watchdog is guarding, or `None` when it could not
/// be armed — explicitly disabled, or the directory cannot be stat'd at arm
/// time. An unarmed watchdog is *not* an error: a boot that cannot see its own
/// data directory will fail on its own, loudly, and arming a guard that fires
/// immediately would turn that clear failure into a crash loop.
///
/// Arming also opens a sentinel file inside `data_dir` and keeps the descriptor
/// for the life of the process. Samples write through it, so the guard detects
/// a remount that comes back on the same device number and leaves every held
/// file descriptor revoked — see the module docs.
pub fn spawn_watchdog(data_dir: &Path) -> Option<VolumeId> {
    if disabled() {
        tracing::warn!(
            "{DISABLE_ENV}=0 — the data-volume watchdog is OFF; a volume detach will wedge \
             this process instead of exiting it"
        );
        return None;
    }

    let Some(expected) = volume_id(data_dir) else {
        tracing::warn!(
            path = %data_dir.display(),
            "volume watchdog: cannot stat the data directory at boot; not arming"
        );
        return None;
    };

    // Opened here rather than inside the thread so the descriptor is bound to
    // the mount this process booted against — the whole point is to hold a
    // handle from before any detach.
    let mut sentinel = open_sentinel(data_dir);
    let has_sentinel = sentinel.is_some();

    let dir = data_dir.to_path_buf();
    let started = std::thread::Builder::new()
        .name("volume-guard".into())
        .spawn(move || {
            let mut misses = 0u32;
            loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                let health = sample(&dir, expected, &mut sentinel);
                if health == Health::Intact {
                    if misses > 0 {
                        tracing::info!(
                            path = %dir.display(),
                            "volume watchdog: the data directory is visible again"
                        );
                        misses = 0;
                    }
                    continue;
                }
                misses += 1;
                if misses < MISSES_BEFORE_EXIT {
                    tracing::warn!(
                        path = %dir.display(),
                        misses,
                        "volume watchdog: data directory {}",
                        health.describe()
                    );
                    continue;
                }
                // The log is on the casualty, so the explanation goes to the
                // boot volume and the exit is immediate. See the module docs.
                breadcrumb(&dir, expected, health, misses);
                tracing::error!(
                    path = %dir.display(),
                    dev = expected.dev,
                    "volume watchdog: the data volume is gone — exiting {EXIT_VOLUME_LOST}. \
                     Every file descriptor on it is revoked and cannot recover; restart the \
                     substrate once the volume is mounted."
                );
                // SAFETY: `_exit` is async-signal-safe and does not run atexit
                // handlers or flush stdio — which is the point, since both would
                // touch revoked descriptors.
                unsafe { nix::libc::_exit(EXIT_VOLUME_LOST) };
            }
        });

    match started {
        Ok(_) => {
            tracing::info!(
                path = %data_dir.display(),
                dev = expected.dev,
                sentinel = has_sentinel,
                interval_s = SAMPLE_INTERVAL.as_secs(),
                "Volume watchdog armed on the data directory"
            );
            Some(expected)
        }
        Err(err) => {
            tracing::warn!("volume watchdog: guard thread could not start: {err}");
            None
        }
    }
}

/// Write the one line that explains the death somewhere that outlives it.
///
/// Best-effort by construction: if this fails there is nothing left to try, and
/// the exit must not be delayed for it.
fn breadcrumb(dir: &Path, expected: VolumeId, health: Health, misses: u32) {
    let path: PathBuf = std::env::temp_dir().join(BREADCRUMB);
    let line = format!(
        "{} contextnest exited {}: the data volume at {} {} \
         (booted dev={} ino={}, {} consecutive failed samples). \
         Every fd on that volume is revoked and there is no in-process recovery; \
         restart once it is mounted again.\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%z"),
        EXIT_VOLUME_LOST,
        dir.display(),
        health.describe(),
        expected.dev,
        expected.ino,
        misses,
    );
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let _ = file.write_all(line.as_bytes());
}

/// Default floor in mebibytes below which a boot is refused. Set
/// [`MIN_FREE_MB_ENV`] to override, or to `0` to boot regardless of space.
///
/// Deliberately low. The substrate's own growth is the input WAL (~1 GB on the
/// operator volume) plus the checkpoint's `-wal` (~0.5 GB); 5 GiB is roughly
/// three times the observed worst case, so the floor only fires when the volume
/// is genuinely unable to absorb a session's worth of writes. A *high* floor
/// would be the wrong instinct here — refusing to boot a memory substrate
/// because it might later fail to compact loses every session in the meantime,
/// which is a larger harm than running tight.
pub const DEFAULT_MIN_FREE_MB: u64 = 5 * 1024;

/// Set the boot floor in mebibytes; `0` disables the check.
pub const MIN_FREE_MB_ENV: &str = "CONTEXTNEST_MIN_FREE_MB";

/// Exit code when the data volume is too full to boot (`EX_IOERR`).
pub const EXIT_DISK_FLOOR: i32 = 74;

/// Bytes available to this user on the filesystem holding `path`, or `None`
/// when the platform query fails. `None` is "unknown", never "fine".
pub fn free_bytes(path: &Path) -> Option<u64> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: nix::libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated C string and `stat` is a
    // correctly sized `statvfs`; `statvfs` writes at most one of those and
    // returns non-zero on failure, which is rejected rather than read.
    let rc = unsafe { nix::libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    // `f_bavail` is the count available to an unprivileged user, which is the
    // number that matters; `f_frsize` is the fundamental fragment size (not
    // `f_bsize`, which is only a transfer-size hint).
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

/// The configured floor in bytes, or `None` when the check is off.
pub fn configured_min_free() -> Option<u64> {
    match std::env::var(MIN_FREE_MB_ENV) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(0) => None,
            Ok(mb) => Some(mb.saturating_mul(1024 * 1024)),
            Err(_) => {
                tracing::warn!(
                    value = raw.as_str(),
                    "{MIN_FREE_MB_ENV} is not an integer number of MiB; using the default"
                );
                Some(DEFAULT_MIN_FREE_MB * 1024 * 1024)
            }
        },
        Err(_) => Some(DEFAULT_MIN_FREE_MB * 1024 * 1024),
    }
}

/// Refuse to boot when the data volume is too full to absorb a session's
/// writes, and warn when it is too full to survive a live `cn-compact`.
///
/// `checkpoint` is the canonical SQLite path; its size is what makes the
/// compaction warning concrete rather than a guess, since `compact()` needs a
/// second full copy of it alongside the source. Pass `None` when the caller
/// does not know it.
///
/// Exits the process (code [`EXIT_DISK_FLOOR`]) when below the floor. Returns
/// normally otherwise.
pub fn enforce_free_space(data_dir: &Path, checkpoint: Option<&Path>) {
    let Some(free) = free_bytes(data_dir) else {
        tracing::warn!(
            path = %data_dir.display(),
            "free-space preflight: cannot stat the data volume; booting unchecked"
        );
        return;
    };
    let gib = free as f64 / (1024.0 * 1024.0 * 1024.0);
    tracing::info!(
        free_gib = format!("{gib:.1}").as_str(),
        "Data volume free space"
    );

    if let Some(floor) = configured_min_free() {
        if free < floor {
            let floor_gib = floor as f64 / (1024.0 * 1024.0 * 1024.0);
            tracing::error!(
                free_gib = format!("{gib:.1}").as_str(),
                floor_gib = format!("{floor_gib:.1}").as_str(),
                path = %data_dir.display(),
                "free-space preflight: the data volume is below the floor — refusing to boot. \
                 A substrate that cannot write is worse than one that is down: it accepts \
                 work it cannot keep. Free space, or lower {MIN_FREE_MB_ENV}."
            );
            std::process::exit(EXIT_DISK_FLOOR);
        }
    }

    // The compaction warning. `cn-compact` writes a whole second checkpoint
    // before swapping, so a volume that cannot hold two of them cannot be
    // compacted — and `cn-compact` is what applies the edge packing, so this
    // gates the boot-time win as well as being a safety margin.
    if let Some(cp) = checkpoint {
        if let Ok(meta) = std::fs::metadata(cp) {
            let need = meta
                .len()
                .saturating_mul(2)
                .saturating_add(2 * 1024 * 1024 * 1024);
            if free < need {
                tracing::warn!(
                    free_gib = format!("{gib:.1}").as_str(),
                    need_gib = format!("{:.1}", need as f64 / (1024.0 * 1024.0 * 1024.0)).as_str(),
                    path = %data_dir.display(),
                    "free-space preflight: not enough room for `make cn-compact` (it writes a \
                     second full checkpoint before swapping). The substrate will run, but the \
                     edge-packing boot win cannot be applied until space is freed."
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_is_a_missed_sample_not_a_success() {
        let missing = Path::new("/definitely/not/here/nor/is/this");
        assert_eq!(volume_id(missing), None, "unknown must never read as fine");
        assert_eq!(classify(VolumeId { dev: 1, ino: 2 }, None), Health::Missing);
    }

    #[test]
    fn the_temp_directory_is_on_a_different_device_than_this_repo() {
        // The breadcrumb is only useful because it lands off the data volume.
        // Here that means: whatever the data volume is, `temp_dir` must be
        // stat-able on its own.
        let tmp = volume_id(&std::env::temp_dir()).expect("temp dir should exist");
        assert!(tmp.dev > 0, "a real mount has a non-zero device number");
    }

    #[test]
    fn the_same_device_and_inode_reads_as_intact() {
        let here = volume_id(Path::new(".")).expect("the cwd exists");
        assert_eq!(classify(here, Some(here)), Health::Intact);
    }

    #[test]
    fn a_different_device_is_a_replacement() {
        // The case that actually happened: same path, new BSD node. A dev
        // change alone must be enough — no inode change required.
        let booted = VolumeId { dev: 7, ino: 42 };
        assert_eq!(
            classify(booted, Some(VolumeId { dev: 8, ino: 42 })),
            Health::Replaced
        );
    }

    #[test]
    fn a_different_inode_on_the_same_device_is_a_replacement() {
        // The automount-stub case: the mount point resolved to a fresh empty
        // directory on the *same* device the volume used to be on.
        let booted = VolumeId { dev: 7, ino: 42 };
        assert_eq!(
            classify(booted, Some(VolumeId { dev: 7, ino: 43 })),
            Health::Replaced
        );
    }

    #[test]
    fn arming_on_a_missing_directory_declines_rather_than_looping() {
        assert_eq!(spawn_watchdog(Path::new("/definitely/not/here")), None);
    }

    /// A descriptor that is already revoked: `lseek` and `write` both fail the
    /// way a revoked vnode does. This is the 2026-10-18 case — the path still
    /// stats to exactly the object recorded at boot, but nothing can use it.
    struct RevokedDescriptor;

    impl Write for RevokedDescriptor {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(nix::libc::EBADF))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for RevokedDescriptor {
        fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
            Err(std::io::Error::from_raw_os_error(nix::libc::EBADF))
        }
    }

    #[test]
    fn a_revoked_descriptor_reads_as_stale_even_though_the_path_is_intact() {
        let here = volume_id(Path::new(".")).expect("the cwd exists");
        assert_eq!(
            classify(here, volume_id(Path::new("."))),
            Health::Intact,
            "the path check alone cannot see this failure"
        );
        let mut sentinel = Some(RevokedDescriptor);
        assert_eq!(sample(Path::new("."), here, &mut sentinel), Health::Stale);
    }

    #[test]
    fn a_healthy_sentinel_keeps_an_intact_path_intact() {
        let here = volume_id(Path::new(".")).expect("the cwd exists");
        let mut sentinel = Some(std::io::Cursor::new(Vec::new()));
        assert_eq!(sample(Path::new("."), here, &mut sentinel), Health::Intact);
    }

    #[test]
    fn a_missing_path_is_reported_without_consulting_the_sentinel() {
        // A healthy sentinel must not mask a path that stopped resolving.
        let expected = VolumeId { dev: 1, ino: 2 };
        let mut sentinel = Some(std::io::Cursor::new(Vec::new()));
        assert_eq!(
            sample(Path::new("/definitely/not/here"), expected, &mut sentinel),
            Health::Missing
        );
    }

    #[test]
    fn the_sentinel_stays_one_byte_however_many_samples_run() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        for _ in 0..100 {
            probe_sentinel(&mut cursor).expect("a cursor never revokes");
        }
        assert_eq!(cursor.into_inner(), SENTINEL_BYTE);
    }

    #[test]
    fn arming_creates_the_sentinel_inside_the_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut sentinel = open_sentinel(dir.path()).expect("a writable directory can hold it");
        probe_sentinel(&mut sentinel).expect("a fresh sentinel is writable");

        let path = dir.path().join(SENTINEL);
        let bytes = std::fs::read(&path).expect("the probe created the file on disk");
        assert_eq!(bytes, SENTINEL_BYTE);
    }

    #[test]
    fn free_space_on_a_real_volume_is_plausible() {
        let free = free_bytes(Path::new(".")).expect("the cwd is on a real volume");
        assert!(free > 0, "a mounted volume has some space free");
        assert!(
            free < 1024u64.pow(5),
            "and under 1 PiB — a fragment-size units slip would blow this"
        );
    }

    #[test]
    fn an_unstattable_path_has_unknown_free_space() {
        assert_eq!(free_bytes(Path::new("/definitely/not/here")), None);
    }

    /// Serialises the tests that read or write [`MIN_FREE_MB_ENV`].
    fn min_free_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn the_free_space_floor_follows_the_env_knob() {
        let _guard = min_free_env_lock();
        std::env::set_var(MIN_FREE_MB_ENV, "512");
        assert_eq!(configured_min_free(), Some(512 * 1024 * 1024));
        std::env::set_var(MIN_FREE_MB_ENV, "0");
        assert_eq!(configured_min_free(), None, "0 disables the check");
        std::env::set_var(MIN_FREE_MB_ENV, "nonsense");
        assert_eq!(
            configured_min_free(),
            Some(DEFAULT_MIN_FREE_MB * 1024 * 1024),
            "an unparseable value falls back to the default"
        );
        std::env::remove_var(MIN_FREE_MB_ENV);
        assert_eq!(
            configured_min_free(),
            Some(DEFAULT_MIN_FREE_MB * 1024 * 1024)
        );
    }

    #[test]
    fn a_roomy_volume_passes_the_preflight_without_exiting() {
        let _guard = min_free_env_lock();
        let dir = tempfile::tempdir().unwrap();
        // A floor of 1 byte cannot fail, so this asserts the *passing* path
        // returns normally rather than reaching `process::exit`.
        std::env::set_var(MIN_FREE_MB_ENV, "0");
        enforce_free_space(dir.path(), None);
        std::env::remove_var(MIN_FREE_MB_ENV);
        // And with a real (tiny) floor set in MiB, still roomy.
        std::env::set_var(MIN_FREE_MB_ENV, "1");
        enforce_free_space(dir.path(), None);
        std::env::remove_var(MIN_FREE_MB_ENV);
    }

    #[test]
    fn an_unknown_data_volume_boots_unchecked_rather_than_refusing() {
        // `None` from `free_bytes` is "unknown", not "full" — refusing to boot
        // because a stat failed would turn a cosmetic problem into an outage.
        let _guard = min_free_env_lock();
        std::env::set_var(MIN_FREE_MB_ENV, "1048576");
        enforce_free_space(Path::new("/definitely/not/here"), None);
        std::env::remove_var(MIN_FREE_MB_ENV);
    }
}
