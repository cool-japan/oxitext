// OXITEXT MODIFICATION (oxitext 0.2.2): 2 `.unwrap()` sites converted (D13 -- a
// font parser reads untrusted bytes and must not panic on them).
// See ../PROVENANCE.md.
use super::FontRef;
use alloc::vec::Vec;

/// Uniquely generated value for identifying and caching fonts.
#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Debug)]
pub struct CacheKey(pub(crate) u64);

impl CacheKey {
    /// Generates a new cache key.
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static KEY: AtomicUsize = AtomicUsize::new(1);
        // `usize` is at most 64 bits on every Rust target, so this widening is
        // lossless; upstream's `try_into().unwrap()` could only panic on a
        // hypothetical 128-bit `usize`.
        Self(KEY.fetch_add(1, Ordering::Relaxed) as u64)
    }

    /// Returns the underlying value of the key.
    pub fn value(self) -> u64 {
        self.0
    }
}

impl Default for CacheKey {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FontCache<T> {
    entries: Vec<Entry<T>>,
    max_entries: usize,
    epoch: u64,
}

impl<T> FontCache<T> {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            epoch: 0,
            max_entries,
        }
    }

    pub fn get<'a>(
        &'a mut self,
        font: &FontRef,
        id_override: Option<[u64; 2]>,
        mut f: impl FnMut(&FontRef) -> T,
    ) -> ([u64; 2], &'a T) {
        let id = id_override.unwrap_or([font.key.value(), u64::MAX]);
        let (found, index) = self.find(id);
        if found {
            let entry = &mut self.entries[index];
            entry.epoch = self.epoch;
            (entry.id, &entry.data)
        } else {
            self.epoch += 1;
            let data = f(font);
            if index == self.entries.len() {
                self.entries.push(Entry {
                    epoch: self.epoch,
                    id,
                    data,
                });
                // The push above guarantees a last element, so this index is
                // in range and cannot underflow.
                let last = self.entries.len() - 1;
                (id, &self.entries[last].data)
            } else {
                let entry = &mut self.entries[index];
                entry.epoch = self.epoch;
                entry.id = id;
                entry.data = data;
                (id, &entry.data)
            }
        }
    }

    fn find(&self, id: [u64; 2]) -> (bool, usize) {
        let mut lowest = 0;
        let mut lowest_epoch = self.epoch;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.id == id {
                return (true, i);
            }
            if entry.epoch < lowest_epoch {
                lowest_epoch = entry.epoch;
                lowest = i;
            }
        }
        if self.entries.len() < self.max_entries {
            (false, self.entries.len())
        } else {
            (false, lowest)
        }
    }
}

struct Entry<T> {
    epoch: u64,
    id: [u64; 2],
    data: T,
}
