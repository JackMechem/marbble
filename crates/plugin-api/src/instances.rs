//! Numbering plug-in instances, across formats: stuck-plug-in handling compares the numbers.

use std::sync::atomic::{AtomicU64, Ordering};

/// Instances made so far in this process.
static INSTANCES: AtomicU64 = AtomicU64::new(0);

/// How many plug-in instances have been made so far: every instance made from now on has a higher
/// number (`Controller::instance`). Allocates nothing and takes no lock.
pub fn instances_made() -> u64 {
    INSTANCES.load(Ordering::Acquire)
}

/// The number of a new instance, from 1 up.
pub fn next_instance() -> u64 {
    INSTANCES.fetch_add(1, Ordering::AcqRel) + 1
}
