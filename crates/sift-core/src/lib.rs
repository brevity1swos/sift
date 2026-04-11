//! sift-core: session lifecycle, ledger store, snapshots, diff, sweep.

pub mod config;
pub mod entry;
pub mod paths;
pub mod state;

pub use config::Mode;
pub use entry::{new_entry_id, DiffStats, LedgerEntry, Op, Status, Tool};
