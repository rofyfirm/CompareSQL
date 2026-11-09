use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Error, Result};

/// Laporan progres + flag batal. Dipakai oleh scan, diff, dan eksekusi.
pub trait Progress: Sync {
    fn report(&self, phase: &str, detail: &str, done: u64, total: u64);
    fn is_cancelled(&self) -> bool {
        false
    }
    fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&self, _: &str, _: &str, _: u64, _: u64) {}
}

/// Progress yang hanya menyimpan flag batal; berguna di test/CLI.
pub struct CancelFlag(pub AtomicBool);

impl Progress for CancelFlag {
    fn report(&self, _: &str, _: &str, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
