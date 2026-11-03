//! Sorting for `-n` and `-m`: records are the paths exactly as they would be printed.

use std::cmp::Reverse;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::domain::Order;

/// Sorts one root's records in place, statting on up to `threads` threads for `Modified`.
/// `Found` leaves them as they are.
pub fn sort(records: &mut [&[u8]], order: Order, threads: usize) {
    match order {
        Order::Found => {}
        Order::Nearest => records.sort_by_key(|r| nearest(r)),
        Order::Modified => by_modified(records, threads),
    }
}

type Stamped<'a> = (Reverse<SystemTime>, &'a [u8]);

fn by_modified(records: &mut [&[u8]], threads: usize) {
    let mut keyed: Vec<Stamped> = records
        .iter()
        .map(|&r| (Reverse(SystemTime::UNIX_EPOCH), r))
        .collect();
    // One stat per record, spread over the workers: one by one, 189k records took 2 s.
    let chunk = keyed.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|s| {
        for part in keyed.chunks_mut(chunk) {
            s.spawn(|| stamp(part));
        }
    });
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| nearest(a.1).cmp(&nearest(b.1))));
    for (slot, (_, r)) in records.iter_mut().zip(keyed) {
        *slot = r;
    }
}

/// Fills in modification times. A file that vanished keeps the epoch and sorts as the oldest.
fn stamp(part: &mut [Stamped]) {
    for (time, r) in part {
        if let Ok(t) = std::fs::symlink_metadata(path(r)).and_then(|m| m.modified()) {
            *time = Reverse(t);
        }
    }
}

/// Depth, then bytes. The `./` kept for names starting with `-` counts for neither.
fn nearest(record: &[u8]) -> (usize, &[u8]) {
    let r = record.strip_prefix(b"./").unwrap_or(record);
    let separators = r
        .iter()
        .filter(|&&b| b == b'/' || (cfg!(windows) && b == b'\\'))
        .count();
    (separators, r)
}

#[cfg(unix)]
fn path(record: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(record).into()
}

/// Windows records are WTF-8; an unpaired surrogate cannot be rebuilt without `unsafe`, so such a
/// name sorts as the oldest.
#[cfg(not(unix))]
fn path(record: &[u8]) -> PathBuf {
    std::str::from_utf8(record).map_or_else(|_| PathBuf::new(), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_orders_by_depth_then_bytes() {
        let mut r: Vec<&[u8]> = vec![b"b/x", b"a/b/x", b"./-x", b"x", b"a/x"];
        sort(&mut r, Order::Nearest, 1);
        assert_eq!(r, [&b"./-x"[..], b"x", b"a/x", b"b/x", b"a/b/x"]);
    }
}
