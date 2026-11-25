//! The only user of threads: a two-stage pipeline over macroblock rows. One stage (a pool worker)
//! entropy-decodes row r + 1 while the other (the calling thread) transforms and outputs row r.
//! The pool is the caller's ([`ThreadPool`], on [`crate::Decoder::with_pool`]) or the crate's lazy
//! shared one; its threads outlive the call, so a small image pays a wake-up, not a thread start.
//! The rows move between the stages in a ring of buffers owned by the call, and rayon's scope lets
//! the worker borrow the decoder state, so nothing here is `unsafe`.
//!
//! Memory: a pool's thread stacks belong to the pool, not to a call, so the ledger books a call's
//! own buffers only (the ring slots).

use crate::Error;
use crate::limits::{Ledger, Limits};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

/// The stack of a pool thread. The deepest frame is the macroblock entropy decode.
const THREAD_STACK: usize = 1 << 20;

/// Worker threads that decodes share. Create one, keep it, and pass it to
/// [`crate::Decoder::with_pool`]: each call then runs its pipeline on these threads. A call
/// without one (`decode`, `decode_component`, `Decoder::new`) uses the crate's lazy shared pool,
/// which starts on the first call with `max_threads` above 1 that has an image worth threading,
/// and never has more than [`std::thread::available_parallelism`] threads; a caller's pool always
/// wins. `Limits::max_threads` still caps the workers of one call, the calling thread included,
/// and a call uses at most one more than the pool has threads. Clones share the threads, which
/// stop when the last clone drops. A pool is not charged to `Limits::max_alloc`; its threads'
/// stacks and bookkeeping are its owner's.
///
/// A `ThreadPool` offers no way to run caller code on its threads, so the calling thread of a
/// decode is never one of them: the pipeline's last stage always runs on the caller, so a busy
/// pool never costs a call its result. It can cost it time: the call returns only after a pool
/// thread has picked up its queued worker job (rayon's scope waits for every job it spawned),
/// and that job exits at once if the caller already did the rows. With every pool thread busy
/// on other calls, a call waits until one of them frees up.
#[derive(Clone, Debug)]
pub struct ThreadPool(Arc<rayon::ThreadPool>);

impl ThreadPool {
    /// A pool of `threads` workers (at least 1), started now. It takes no [`Limits`] and is not
    /// charged to `Limits::max_alloc`.
    ///
    /// # Errors
    /// [`Error::OutOfMemory`] if the operating system will not start the threads; no other error.
    pub fn new(threads: usize) -> Result<ThreadPool, Error> {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .stack_size(THREAD_STACK)
            .thread_name(|i| format!("jpegxr-worker-{i}"))
            .build()
            .map(|p| ThreadPool(Arc::new(p)))
            .map_err(|_| Error::OutOfMemory)
    }

    /// How many worker threads the pool has.
    pub fn threads(&self) -> usize {
        self.0.current_num_threads()
    }
}

/// The crate's shared pool: built on first use, `available_parallelism` threads, no other state.
/// `None` if the threads could not be started (calls then run serially).
static SHARED: OnceLock<Option<ThreadPool>> = OnceLock::new();

fn shared_pool() -> Option<&'static ThreadPool> {
    SHARED
        .get_or_init(|| {
            ThreadPool::new(std::thread::available_parallelism().map_or(1, |n| n.get())).ok()
        })
        .as_ref()
}

/// Whether the shared pool has been started (for tests: `max_threads` 1, and a caller's pool,
/// never start it).
#[doc(hidden)]
pub fn shared_pool_started() -> bool {
    SHARED.get().is_some()
}

/// What a call charged and used, for the tests of `Limits`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// The most bytes the call's ledger had booked at once.
    pub charged_peak: u64,
    /// The most threads the call had running at once, the calling thread included.
    pub threads_peak: usize,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            charged_peak: 0,
            threads_peak: 1,
        }
    }
}

/// Called with the output buffer and the number of image rows that are final, as a decode
/// finishes each macroblock row (see `codec::decode`).
pub(crate) type OnRows<'a> = &'a mut dyn FnMut(&mut [u8], usize) -> Result<(), Error>;

/// What one call carries: its ledger, the thread budget, the pool and the cancel flag.
pub(crate) struct Ctx<'a> {
    pub ledger: Ledger,
    /// `Limits::threads()`.
    pub threads: usize,
    pub pool: Option<&'a ThreadPool>,
    pub cancel: &'a AtomicBool,
    pub gauge: Gauge,
    /// Thread every image, however small (tests).
    pub uncapped: bool,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(
        limits: &Limits,
        pool: Option<&'a ThreadPool>,
        cancel: &'a AtomicBool,
        uncapped: bool,
    ) -> Self {
        Self {
            ledger: Ledger::new(limits),
            threads: limits.threads(),
            pool,
            cancel,
            gauge: Gauge::new(),
            uncapped,
        }
    }

    pub(crate) fn probe(&self) -> Probe {
        Probe {
            charged_peak: self.ledger.peak(),
            threads_peak: self.gauge.peak(),
        }
    }
}

/// The pool a pipeline of this call runs on and how many workers it may use, the calling thread
/// included; `None` means run serially. The shared pool starts here, so a caller asks only once
/// it has an image worth threading.
pub(crate) fn stage<'a>(cx: &Ctx<'a>) -> Option<&'a ThreadPool> {
    // Two stages, so never more than two workers.
    // ponytail: the pipeline has two stages; a third (output) stage needs a copy of each row.
    if cx.threads.min(2) < 2 {
        return None;
    }
    let pool = match cx.pool {
        Some(p) => p,
        None => shared_pool()?,
    };
    if cx.threads.min(2).min(pool.threads() + 1) < 2 {
        return None;
    }
    Some(pool)
}

/// The threads of one call that are running now and the most at once, the calling thread
/// included. Per call, so a test can read it without global state.
pub(crate) struct Gauge {
    live: AtomicUsize,
    peak: AtomicUsize,
}

impl Gauge {
    pub(crate) fn new() -> Self {
        Self {
            live: AtomicUsize::new(1),
            peak: AtomicUsize::new(1),
        }
    }

    pub(crate) fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }
}

/// A pool worker counted in a [`Gauge`] while it runs rows.
struct Live<'a>(&'a Gauge);

impl<'a> Live<'a> {
    fn enter(g: &'a Gauge) -> Self {
        let n = g.live.fetch_add(1, Ordering::Relaxed) + 1;
        g.peak.fetch_max(n, Ordering::Relaxed);
        Live(g)
    }
}

impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A mutex's data even if a stage panicked while holding it (the panic is re-raised by the scope).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Rows made by the first stage and not yet taken by the second, and the buffers free for more.
struct Ring<S> {
    free: Vec<S>,
    done: Vec<(usize, S, Result<(), Error>)>,
    stop: bool,
}

/// The first stage's function and the next row it makes.
struct Engine<E> {
    f: E,
    next: usize,
}

struct Shared<S, E> {
    engine: Mutex<Engine<E>>,
    ring: Mutex<Ring<S>>,
    cv: Condvar,
}

impl<S, E> Shared<S, E> {
    fn stop(&self) {
        lock(&self.ring).stop = true;
        self.cv.notify_all();
    }
}

/// Stops the pipeline when a stage leaves for any reason, so a panic in one cannot strand the
/// other: `only_if_panicking` for the worker (it also ends by running out of rows).
struct StopOnDrop<'a, S, E> {
    shared: &'a Shared<S, E>,
    only_if_panicking: bool,
}

impl<S, E> Drop for StopOnDrop<'_, S, E> {
    fn drop(&mut self) {
        if !self.only_if_panicking || std::thread::panicking() {
            self.shared.stop();
        }
    }
}

/// Runs a two-stage pipeline with a ring of `slots` buffers. `e(r, slot)` fills `slot` with row
/// `r` (r in 0..n_e), in order; `t(r, Some(slot))` consumes it, in order, and `t(r, None)` runs
/// for the rows from `n_e` to `n_t - 1` that have nothing to consume. `e` runs on a worker of
/// `pool` and, when the worker has not got to a row the caller needs and is not running, on the
/// calling thread too, so a busy or unstarted pool leaves the whole job to the caller and this
/// never fails. It does wait, after the rows are done, until a pool thread has picked up the
/// queued worker job (`in_place_scope` waits for every job it spawned); that job then returns at
/// once.
///
/// An error from `e` at row `k` is returned after `t` has run rows 0 to k - 1 and before it runs
/// row `k`; an error from `t` is returned at once. A panic in either stage is re-raised here,
/// after the other has returned.
///
/// `slots` is the ring, taken for the call; the slots come back to it when the call ends (all of
/// them unless `t` failed on one), so the next call needs no allocation for them.
pub(crate) fn pipe<S: Send, E, T>(
    pool: &ThreadPool,
    gauge: &Gauge,
    slots: &mut Vec<S>,
    (n_e, n_t): (usize, usize),
    e: E,
    mut t: T,
) -> Result<(), Error>
where
    E: FnMut(usize, &mut S) -> Result<(), Error> + Send,
    T: FnMut(usize, Option<&S>) -> Result<(), Error>,
{
    let shared = Shared {
        engine: Mutex::new(Engine { f: e, next: 0 }),
        ring: Mutex::new(Ring {
            free: std::mem::take(slots),
            done: Vec::new(),
            stop: false,
        }),
        cv: Condvar::new(),
    };
    let r = pool.0.in_place_scope(|scope| {
        let shared = &shared;
        scope.spawn(move |_| worker(shared, n_e, gauge));
        let _stop = StopOnDrop {
            shared,
            only_if_panicking: false,
        };
        consume(shared, (n_e, n_t), &mut t)
    });
    let ring = shared.ring.into_inner().unwrap_or_else(|e| e.into_inner());
    // `free` is the vector taken above, and holds every slot there was.
    *slots = ring.free;
    slots.extend(ring.done.into_iter().map(|(_, s, _)| s));
    r
}

fn worker<S, E>(sh: &Shared<S, E>, n: usize, gauge: &Gauge)
where
    E: FnMut(usize, &mut S) -> Result<(), Error>,
{
    let _stop = StopOnDrop {
        shared: sh,
        only_if_panicking: true,
    };
    // Counted from the first row, so a task that wakes after the caller did everything is not a
    // worker the call used.
    let mut live = None;
    loop {
        let mut eng = lock(&sh.engine);
        let mut slot = {
            let mut g = lock(&sh.ring);
            loop {
                if g.stop || eng.next >= n {
                    return;
                }
                if let Some(s) = g.free.pop() {
                    break s;
                }
                g = sh.cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        };
        if live.is_none() {
            live = Some(Live::enter(gauge));
        }
        let r = eng.next;
        eng.next += 1;
        let res = (eng.f)(r, &mut slot);
        drop(eng);
        let failed = res.is_err();
        lock(&sh.ring).done.push((r, slot, res));
        sh.cv.notify_all();
        if failed {
            return;
        }
    }
}

fn consume<S, E, T>(sh: &Shared<S, E>, (n_e, n_t): (usize, usize), t: &mut T) -> Result<(), Error>
where
    E: FnMut(usize, &mut S) -> Result<(), Error>,
    T: FnMut(usize, Option<&S>) -> Result<(), Error>,
{
    for r in 0..n_t {
        if r >= n_e {
            t(r, None)?;
            continue;
        }
        let (slot, res) = loop {
            let mut g = lock(&sh.ring);
            if let Some(i) = g.done.iter().position(|d| d.0 == r) {
                let (_, s, res) = g.done.swap_remove(i);
                break (s, res);
            }
            if g.stop {
                // The worker panicked; the scope re-raises it.
                return Err(Error::Internal("pipeline worker stopped".into()));
            }
            // The caller makes the row itself when the worker is not on it. try_lock, never lock:
            // the worker takes the engine and then the ring, the caller holds the ring here.
            if let Ok(mut eng) = sh.engine.try_lock()
                && eng.next == r
                && let Some(mut slot) = g.free.pop()
            {
                drop(g);
                eng.next += 1;
                let res = (eng.f)(r, &mut slot);
                if res.is_err() {
                    // No row after a failed one: the worker checks next >= n under this lock.
                    eng.next = usize::MAX;
                }
                break (slot, res);
            }
            drop(sh.cv.wait(g).unwrap_or_else(|e| e.into_inner()));
        };
        res?;
        t(r, Some(&slot))?;
        lock(&sh.ring).free.push(slot);
        sh.cv.notify_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> Vec<Vec<usize>> {
        vec![Vec::new(); 3]
    }

    #[test]
    fn rows_come_out_in_order() {
        let pool = ThreadPool::new(2).unwrap();
        let g = Gauge::new();
        let mut seen = Vec::new();
        pipe(
            &pool,
            &g,
            &mut slots(),
            (40, 41),
            |r, s| {
                s.clear();
                s.push(r * 2);
                Ok(())
            },
            |r, s| {
                if let Some(s) = s {
                    assert_eq!(s[0], r * 2);
                }
                seen.push(r);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, (0..=40).collect::<Vec<_>>());
        assert!(g.peak() <= 2);
    }

    #[test]
    fn an_error_at_row_k_comes_after_the_rows_below_it() {
        let pool = ThreadPool::new(2).unwrap();
        let mut seen = Vec::new();
        let r = pipe(
            &pool,
            &Gauge::new(),
            &mut slots(),
            (20, 21),
            |r, _| {
                if r == 7 {
                    Err(Error::Codestream)
                } else {
                    Ok(())
                }
            },
            |r, _| {
                seen.push(r);
                Ok(())
            },
        );
        assert_eq!(r, Err(Error::Codestream));
        assert_eq!(seen, (0..7).collect::<Vec<_>>());
    }

    #[test]
    fn rows_stop_after_a_failed_one() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let pool = ThreadPool::new(2).unwrap();
        for _ in 0..2000 {
            let after = AtomicUsize::new(0);
            let r = pipe(
                &pool,
                &Gauge::new(),
                &mut slots(),
                (8, 9),
                |r, s| {
                    if r == 0 {
                        s.clear();
                        s.resize(70_000, 1);
                        return Err(Error::Codestream);
                    }
                    after.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                |_, _| Ok(()),
            );
            assert_eq!(r, Err(Error::Codestream));
            assert_eq!(after.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn an_error_from_the_second_stage_stops_the_first() {
        let pool = ThreadPool::new(2).unwrap();
        let r = pipe(
            &pool,
            &Gauge::new(),
            &mut slots(),
            (1000, 1001),
            |_, _| Ok(()),
            |r, _| {
                if r == 3 {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(r, Err(Error::Cancelled));
    }

    #[test]
    #[should_panic(expected = "boom e")]
    fn a_panic_in_the_first_stage_is_raised() {
        let pool = ThreadPool::new(2).unwrap();
        let _ = pipe(
            &pool,
            &Gauge::new(),
            &mut slots(),
            (50, 51),
            |r, _| {
                assert!(r != 5, "boom e");
                Ok(())
            },
            |_, _| Ok(()),
        );
    }

    #[test]
    #[should_panic(expected = "boom t")]
    fn a_panic_in_the_second_stage_is_raised() {
        let pool = ThreadPool::new(2).unwrap();
        let _ = pipe(
            &pool,
            &Gauge::new(),
            &mut slots(),
            (50, 51),
            |_, _| Ok(()),
            |r, _| {
                assert!(r != 5, "boom t");
                Ok(())
            },
        );
    }

    #[test]
    fn one_thread_spawns_nothing() {
        let cancel = AtomicBool::new(false);
        let l = Limits {
            max_threads: 1,
            ..Limits::default()
        };
        let cx = Ctx::new(&l, None, &cancel, true);
        assert!(stage(&cx).is_none());
        assert_eq!(cx.probe().threads_peak, 1);
        // Whether the shared pool started is checked in tests/pool.rs, its own process: other
        // unit tests here decode with the default max_threads and may start it at any time.
    }

    #[test]
    fn a_busy_pool_leaves_the_job_to_the_caller() {
        let pool = ThreadPool::new(1).unwrap();
        let (started, release) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let (s, r) = (started.clone(), release.clone());
        pool.0.spawn(move || {
            s.store(true, Ordering::SeqCst);
            while !r.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
        });
        while !started.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        let g = Gauge::new();
        let mut rows = 0;
        pipe(
            &pool,
            &g,
            &mut slots(),
            (30, 31),
            |_, _| Ok(()),
            |r, _| {
                rows += 1;
                if r == 30 {
                    // The pool's only thread is free again after the caller has finished.
                    release.store(true, Ordering::SeqCst);
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(rows, 31);
        assert_eq!(g.peak(), 1);
    }
}
