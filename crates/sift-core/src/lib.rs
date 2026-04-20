//! sift-core: session lifecycle, ledger store, snapshots, diff, sweep.

pub mod agx;
pub mod config;
pub mod correlation;
pub mod diff;
pub mod entry;
pub mod export;
pub mod fsck;
pub mod gc;
pub mod paths;
pub mod policy;
pub mod session;
pub mod snapshot;
pub mod state;
pub mod store;
pub mod sweep;
pub mod world;

pub use config::Mode;
pub use entry::{new_entry_id, DiffStats, LedgerEntry, Op, Status, Tool};
pub use session::Session;
