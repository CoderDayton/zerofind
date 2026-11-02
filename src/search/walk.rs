//! A work-stealing directory queue whose worker count grows with the backlog.
//!
//! Each worker owns a queue: it pushes the subdirectories it finds and takes the newest back,
//! so it walks its own subtree depth-first, reading neighbouring directories. An idle worker
//! steals the oldest job from another queue, the one farthest from what its owner is reading.
//!
//! The walk starts on the calling thread. After each directory, a worker holding more jobs than
//! idle workers can take adds workers for the rest, up to the cap: a narrow tree stays on one
//! thread, a wide one fills the cap at once. Idle workers sleep on a condition variable and
//! wake only for new work or the end of the walk.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, Scope};

/// What one walk did, for `ZF_TRACE`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Workers started, the calling thread included.
    pub workers: usize,
    /// Jobs (directories) processed.
    pub dirs: usize,
    /// Entries the jobs reported.
    pub entries: usize,
    /// Times a worker found no job anywhere and slept.
    pub waits: usize,
}

struct Pool<'f, J, W> {
    /// One per worker slot, indexed by worker.
    queues: Box<[Mutex<VecDeque<J>>]>,
    /// Jobs in all queues, counted before they are pushed and after they are taken, so it never
    /// undercounts: with `busy`, it says when the walk is over.
    queued: AtomicUsize,
    /// Workers holding a job, or about to try for one.
    busy: AtomicUsize,
    sleeping: AtomicUsize,
    /// Worker slots claimed. Never decreases, so a slot index belongs to one worker for good.
    workers: AtomicUsize,
    /// Claimed slots whose thread could not be started; their queues stay empty.
    failed: AtomicUsize,
    /// Workers may be added while below this; zero once a spawn fails.
    cap: AtomicUsize,
    done: AtomicBool,
    /// Guards sleeping and the summed stats.
    sleep: Mutex<Stats>,
    ready: Condvar,
    init: &'f (dyn Fn() -> W + Sync),
    work: &'f (dyn Fn(&mut W, J, &mut Vec<J>) -> usize + Sync),
    halted: &'f (dyn Fn() -> bool + Sync),
}

/// Runs `work` on `first` and on every job it finds, with up to `cap` workers. Each worker gets
/// its own context from `init`. `work` pushes the jobs it finds and returns how many entries
/// it saw. The walk ends when no job is left or `halted` returns true. A panic in `work` ends
/// the walk and panics here (with the original payload only if it was on the calling thread).
pub fn run<J: Send, W>(
    first: J,
    cap: usize,
    init: &(dyn Fn() -> W + Sync),
    work: &(dyn Fn(&mut W, J, &mut Vec<J>) -> usize + Sync),
    halted: &(dyn Fn() -> bool + Sync),
) -> Stats {
    let cap = cap.max(1);
    let pool = Pool {
        queues: (0..cap).map(|_| Mutex::new(VecDeque::new())).collect(),
        queued: AtomicUsize::new(1),
        busy: AtomicUsize::new(0),
        sleeping: AtomicUsize::new(0),
        workers: AtomicUsize::new(1),
        failed: AtomicUsize::new(0),
        cap: AtomicUsize::new(cap),
        done: AtomicBool::new(false),
        sleep: Mutex::new(Stats::default()),
        ready: Condvar::new(),
        init,
        work,
        halted,
    };
    lock(&pool.queues[0]).push_back(first);
    thread::scope(|s| worker(s, &pool, 0));
    let stats = *lock(&pool.sleep);
    Stats {
        workers: pool.workers.into_inner() - pool.failed.into_inner(),
        ..stats
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Ends the walk for every worker if `work` unwinds, so none sleeps forever.
struct EndOnPanic<'p, 'f, J, W>(&'p Pool<'f, J, W>);

impl<J, W> Drop for EndOnPanic<'_, '_, J, W> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.end();
        }
    }
}

fn worker<'s, 'e, J: Send, W>(s: &'s Scope<'s, 'e>, pool: &'e Pool<'e, J, W>, me: usize) {
    let mut ctx = (pool.init)();
    let mut found = Vec::new();
    let mut mine = Stats::default();
    while let Some(job) = pool.next(me, &mut mine.waits) {
        mine.entries += {
            let _end = EndOnPanic(pool);
            (pool.work)(&mut ctx, job, &mut found)
        };
        mine.dirs += 1;
        let n = found.len();
        let held = if n > 0 {
            pool.queued.fetch_add(n, SeqCst);
            let mut q = lock(&pool.queues[me]);
            q.extend(found.drain(..));
            q.len()
        } else {
            0
        };
        if pool.busy.fetch_sub(1, SeqCst) == 1 && pool.queued.load(SeqCst) == 0 {
            pool.end();
        }
        if n > 0 {
            pool.wake(n);
            pool.grow(s, held);
        }
    }
    let mut total = lock(&pool.sleep);
    total.dirs += mine.dirs;
    total.entries += mine.entries;
    total.waits += mine.waits;
}

impl<J, W> Pool<'_, J, W> {
    /// The next job for worker `me`: its own newest, else another's oldest, else sleep until
    /// there is one. `None` once the walk is over.
    fn next(&self, me: usize, waits: &mut usize) -> Option<J> {
        loop {
            if self.done.load(SeqCst) {
                return None;
            }
            if (self.halted)() {
                self.end();
                return None;
            }
            self.busy.fetch_add(1, SeqCst);
            if let Some(job) = self.take(me) {
                self.queued.fetch_sub(1, SeqCst);
                return Some(job);
            }
            if self.busy.fetch_sub(1, SeqCst) == 1 && self.queued.load(SeqCst) == 0 {
                self.end();
                return None;
            }
            let mut guard = lock(&self.sleep);
            self.sleeping.fetch_add(1, SeqCst);
            while self.queued.load(SeqCst) == 0
                && self.busy.load(SeqCst) > 0
                && !self.done.load(SeqCst)
            {
                *waits += 1;
                guard = self
                    .ready
                    .wait(guard)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            self.sleeping.fetch_sub(1, SeqCst);
        }
    }

    fn take(&self, me: usize) -> Option<J> {
        if let Some(job) = lock(&self.queues[me]).pop_back() {
            return Some(job);
        }
        // Only claimed slots can hold jobs.
        let n = self.workers.load(SeqCst).min(self.queues.len());
        (1..n).find_map(|k| lock(&self.queues[(me + k) % n]).pop_front())
    }

    fn wake(&self, n: usize) {
        let sleeping = self.sleeping.load(SeqCst);
        if sleeping > 0 {
            // Taking the lock orders this after a sleeper's last check of `queued`.
            let _guard = lock(&self.sleep);
            for _ in 0..n.min(sleeping) {
                self.ready.notify_one();
            }
        }
    }

    fn end(&self) {
        self.done.store(true, SeqCst);
        let _guard = lock(&self.sleep);
        self.ready.notify_all();
    }
}

impl<'e, J: Send, W> Pool<'e, J, W> {
    /// Adds a worker for each job in this worker's queue, beyond the one it takes next, that no
    /// sleeping worker can take.
    fn grow<'s>(&'e self, s: &'s Scope<'s, 'e>, held: usize) {
        let want = held.saturating_sub(1 + self.sleeping.load(SeqCst));
        if want == 0 {
            return;
        }
        let cap = self.cap.load(SeqCst);
        let mut first = self.workers.load(SeqCst);
        let last = loop {
            if first >= cap {
                return;
            }
            let last = first + want.min(cap - first);
            match self
                .workers
                .compare_exchange_weak(first, last, SeqCst, SeqCst)
            {
                Ok(_) => break last,
                Err(now) => first = now,
            }
        };
        let failed = (first..last)
            .filter(|&me| {
                thread::Builder::new()
                    .name("zf-walk".into())
                    .spawn_scoped(s, move || worker(s, self, me))
                    .is_err()
            })
            .count();
        if failed > 0 {
            // Out of threads: walk on with the workers we have.
            self.failed.fetch_add(failed, SeqCst);
            self.cap.store(0, SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A job is a depth; each job below `depth` has `fan` children.
    fn tree(fan: usize, depth: usize, cap: usize) -> Stats {
        let work = |_: &mut (), d: usize, found: &mut Vec<usize>| {
            if d < depth {
                found.extend(std::iter::repeat_n(d + 1, fan));
            }
            if d.is_multiple_of(3) {
                thread::yield_now();
            }
            fan
        };
        run(0, cap, &|| (), &work, &|| false)
    }

    fn nodes(fan: usize, depth: usize) -> usize {
        (0..=depth).map(|d| fan.pow(d as u32)).sum()
    }

    #[test]
    fn every_job_runs_exactly_once() {
        for round in 0..300 {
            let s = tree(3, 6, 8);
            assert_eq!(s.dirs, nodes(3, 6), "round {round}");
            assert!(s.workers <= 8);
        }
    }

    #[test]
    fn a_chain_stays_on_one_worker() {
        assert_eq!(
            tree(1, 500, 16),
            Stats {
                workers: 1,
                dirs: 501,
                entries: 501,
                waits: 0
            }
        );
    }

    #[test]
    fn a_wide_tree_grows_to_the_cap() {
        let s = tree(8, 4, 4);
        assert_eq!(s.dirs, nodes(8, 4));
        assert_eq!(s.workers, 4);
    }

    #[test]
    fn a_backlog_gets_its_workers_at_once() {
        // A slow first read must not be followed by one slow read per added worker.
        assert_eq!(tree(8, 1, 16).workers, 8);
    }

    #[test]
    fn a_cap_of_one_walks_alone() {
        let s = tree(4, 4, 1);
        assert_eq!((s.workers, s.dirs), (1, nodes(4, 4)));
    }

    #[test]
    fn idle_workers_take_jobs_from_a_busy_one() {
        // The root's 32 children sit in its worker's queue while it reads a slow directory.
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let work = |_: &mut (), d: usize, found: &mut Vec<usize>| {
            if d == 0 {
                found.extend([1; 32]);
                return 0;
            }
            let now = running.fetch_add(1, SeqCst) + 1;
            peak.fetch_max(now, SeqCst);
            thread::sleep(Duration::from_millis(if d == 1 { 5 } else { 0 }));
            running.fetch_sub(1, SeqCst);
            0
        };
        let s = run(0, 8, &|| (), &work, &|| false);
        assert_eq!(s.dirs, 33);
        assert!(
            peak.load(SeqCst) >= 4,
            "at most {} jobs ran at once",
            peak.load(SeqCst)
        );
    }

    #[test]
    fn halting_stops_taking_jobs() {
        let stop = AtomicBool::new(false);
        let seen = AtomicUsize::new(0);
        let work = |_: &mut (), d: usize, found: &mut Vec<usize>| {
            if seen.fetch_add(1, SeqCst) == 10 {
                stop.store(true, SeqCst);
            }
            found.extend([d + 1, d + 1]);
            0
        };
        let s = run(0, 4, &|| (), &work, &|| stop.load(SeqCst));
        assert!(s.dirs < 20, "an endless tree stopped after {} jobs", s.dirs);
    }

    #[test]
    fn a_panicking_job_ends_the_walk_and_propagates() {
        let work = |_: &mut (), d: usize, found: &mut Vec<usize>| {
            assert!(d != 3, "boom");
            found.extend([d + 1, d + 1, d + 1]);
            0
        };
        let r = std::panic::catch_unwind(|| run(0, 4, &|| (), &work, &|| false));
        assert!(r.is_err());
    }
}
