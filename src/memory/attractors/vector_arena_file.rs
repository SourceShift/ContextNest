//! File-backed row storage for [`super::vector_arena::VectorArena`].
//!
//! Anonymous heap pages under memory pressure go to the compressor and then
//! swap, and every later touch pays decompression — the CPU burst seen on the
//! operator substrate. Pages of a shared file mapping are instead written
//! back to the file and dropped, and re-read from the page cache on demand.
//!
//! The file is derived state: it is created fresh on every boot and, on
//! Unix, unlinked as soon as it is mapped, so a crash never leaves a stale
//! arena behind and no other process can open it.

use memmap2::MmapMut;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// Initial file capacity: 1 Mi floats (4 MB).
const MIN_CAPACITY_FLOATS: usize = 1 << 20;

#[derive(Debug)]
pub struct FileRows {
    file: File,
    map: MmapMut,
    capacity_floats: usize,
}

impl FileRows {
    pub fn create(dir: &Path, name: &str, min_floats: usize) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let path = dir.join(format!("{name}.{}.f32", std::process::id()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        #[cfg(unix)]
        std::fs::remove_file(&path).map_err(|e| format!("unlink {}: {e}", path.display()))?;
        let capacity_floats = min_floats.max(MIN_CAPACITY_FLOATS);
        let map = map_file(&file, capacity_floats)?;
        Ok(Self {
            file,
            map,
            capacity_floats,
        })
    }

    pub fn floats(&self) -> &[f32] {
        // SAFETY: the mapping is page-aligned (so f32-aligned), exactly
        // `capacity_floats * 4` bytes long, and private to this process
        // (unlinked on Unix). `&self` keeps it alive and unaliased-mutably.
        unsafe { std::slice::from_raw_parts(self.map.as_ptr().cast::<f32>(), self.capacity_floats) }
    }

    pub fn floats_mut(&mut self) -> &mut [f32] {
        // SAFETY: as in `floats`; `&mut self` guarantees exclusivity.
        unsafe {
            std::slice::from_raw_parts_mut(
                self.map.as_mut_ptr().cast::<f32>(),
                self.capacity_floats,
            )
        }
    }

    /// Grow the file and remap so at least `floats` values fit.
    pub fn reserve_floats(&mut self, floats: usize) -> Result<(), String> {
        if floats <= self.capacity_floats {
            return Ok(());
        }
        let capacity = floats.max(self.capacity_floats * 2);
        self.map = map_file(&self.file, capacity)?;
        self.capacity_floats = capacity;
        Ok(())
    }
}

fn map_file(file: &File, capacity_floats: usize) -> Result<MmapMut, String> {
    let bytes = capacity_floats
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or("vector arena size overflow")?;
    file.set_len(bytes as u64)
        .map_err(|e| format!("grow vector arena to {bytes} bytes: {e}"))?;
    // SAFETY: the file is private to this process (see module docs), so no
    // other writer can mutate the mapping behind our references.
    unsafe { MmapMut::map_mut(file) }.map_err(|e| format!("map vector arena: {e}"))
}

#[cfg(test)]
mod tests {
    use super::super::vector_arena::{Holder, VectorArena};

    #[test]
    fn file_backed_arena_round_trips_and_grows() {
        let dir = tempfile::tempdir().unwrap();
        let arena = VectorArena::new();
        arena
            .claim("before", Holder::Fragment, &[1.0, 2.0, 3.0])
            .unwrap();
        arena.attach_file(dir.path(), "test-arena").unwrap();
        assert!(arena.stats().file_backed);
        assert_eq!(arena.vector("before").unwrap(), vec![1.0, 2.0, 3.0]);
        // Past the 1 Mi-float initial capacity forces a remap.
        let dim = 3;
        let rows = (super::MIN_CAPACITY_FLOATS / dim) + 10;
        for i in 0..rows {
            arena
                .claim(&format!("r{i}"), Holder::Node, &[i as f32 + 1.0, 1.0, 0.5])
                .unwrap();
        }
        assert_eq!(arena.vector("before").unwrap(), vec![1.0, 2.0, 3.0]);
        let last = format!("r{}", rows - 1);
        assert_eq!(arena.vector(&last).unwrap(), vec![rows as f32, 1.0, 0.5]);
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "arena file is unlinked once mapped"
        );
    }
}
