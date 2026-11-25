use crate::Error;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Caps on one call. Go had no per-call caps (only a process-wide memory budget, default half of
/// the cgroup limit), so the defaults here are this crate's own: finite, and far above a CZI
/// tile. A zero field is a real cap, not "use the default" (except `max_threads`, where 0 counts
/// as 1).
///
/// ```
/// use jpegxr::Limits;
///
/// // A 2-CPU, 1 GiB pod: two threads and 768 MiB.
/// let limits = Limits { max_threads: 2, max_alloc: 768 << 20, ..Limits::default() };
/// assert_eq!(limits.max_pixels, Limits::default().max_pixels);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Peak bytes of working memory one call may hold, all threads together: the decoder's
    /// estimated peak ([`crate::Decoder::estimate`], checked first, as Go's budget was) and then
    /// every allocation that grows with the input, charged before it is made. For `decode` and
    /// `decode_component` the output image counts. Over it: [`crate::Error::AllocLimit`]. The
    /// input is borrowed and not counted.
    pub max_alloc: u64,
    /// Pixels in the image. Over it: [`crate::Error::LimitExceeded`]`("pixels")`.
    pub max_pixels: u64,
    /// Image width. Over it: [`crate::Error::LimitExceeded`]`("width")`.
    pub max_width: u64,
    /// Image height. Over it: [`crate::Error::LimitExceeded`]`("height")`.
    pub max_height: u64,
    /// Most threads one call uses, the calling thread included. 1 runs on the calling thread and
    /// never touches a pool; 0 counts as 1. The default is
    /// [`std::thread::available_parallelism`]. The decoder is a two-stage pipeline (entropy
    /// decoding on one thread, transform and output on the calling thread), so it never uses
    /// more than 2. It uses threads only when the image is big enough to pay for them and when
    /// their buffers fit in `max_alloc`; otherwise it runs on fewer, so the output and any error
    /// are the same for every value. No error is returned for this field.
    pub max_threads: usize,
}

/// `available_parallelism`, read once: it reads cgroup files on every call.
fn default_threads() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_alloc: 1 << 31,
            max_pixels: 1 << 30,
            max_width: 1 << 20,
            max_height: 1 << 20,
            max_threads: default_threads(),
        }
    }
}

impl Limits {
    /// `max_threads` as the threads a call may use: at least 1.
    pub(crate) fn threads(&self) -> usize {
        self.max_threads.max(1)
    }
}

/// The bytes a call has charged against `Limits::max_alloc`, shared by every thread of the call.
/// A charge stays booked until `release`, `rewind` or, for a [`Held`] one, the guard's drop:
/// `codec::decode` rewinds to where it started when it returns, and the plane `decode_component`
/// gives up is released by hand. Buffers are not charged back one by one as they drop. Bytes are exact (no page rounding),
/// so the total stays under Go's estimate.
pub(crate) struct Ledger {
    max: u64,
    used: AtomicU64,
    peak: AtomicU64,
    /// Capacity beyond need that kept buffers hold: see `reuse` and `settle`.
    kept: AtomicU64,
}

impl Ledger {
    pub(crate) fn new(l: &Limits) -> Self {
        Self {
            max: l.max_alloc,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            kept: AtomicU64::new(0),
        }
    }

    /// The most bytes booked at any one time: an upper bound of the call's heap peak.
    pub(crate) fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    /// Books `n` bytes; fails, booking nothing, when they would pass the budget.
    fn add(&self, n: u64) -> Result<(), Error> {
        let mut cur = self.used.load(Ordering::Relaxed);
        loop {
            let u = cur.saturating_add(n);
            if u > self.max {
                return Err(Error::AllocLimit {
                    need: u,
                    max: self.max,
                });
            }
            match self
                .used
                .compare_exchange_weak(cur, u, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.peak.fetch_max(u, Ordering::Relaxed);
                    return Ok(());
                }
                Err(c) => cur = c,
            }
        }
    }

    /// Books `n` bytes about to be allocated.
    pub(crate) fn charge(&self, n: u64) -> Result<(), Error> {
        self.add(n)
    }

    /// The bytes booked now, to `rewind` to.
    pub(crate) fn mark(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    /// Gives back everything booked since `mark`; the caller has dropped those buffers and every
    /// thread that held a charge has finished.
    pub(crate) fn rewind(&self, mark: u64) {
        self.used.store(mark, Ordering::Relaxed);
        self.kept.store(0, Ordering::Relaxed);
    }

    /// Gives back `n` bytes charged for a buffer that was dropped.
    pub(crate) fn release(&self, n: u64) {
        self.used.fetch_sub(n, Ordering::Relaxed);
    }

    /// Books `n` bytes until the guard drops, or `None` when they do not fit. For thread
    /// scratch: a caller that gets `None` runs serially, so the budget never turns into an error
    /// a single-threaded call would not give.
    pub(crate) fn hold(&self, n: u64) -> Option<Held<'_>> {
        self.add(n).ok()?;
        Some(Held { ledger: self, n })
    }

    /// Charges `n` copies of `init` and puts them in `v`, which may be a buffer kept from an
    /// earlier call: the charge is the one a fresh buffer makes (so a limit gives the same
    /// error), `v` is cleared and refilled as a fresh one, and its capacity is reused. The
    /// capacity beyond `n` is noted for `settle`.
    pub(crate) fn reuse<T: Clone>(&self, v: &mut Vec<T>, n: usize, init: T) -> Result<(), Error> {
        self.charge((n as u64).saturating_mul(std::mem::size_of::<T>() as u64))?;
        refill(v, n, init)?;
        self.keep(v);
        Ok(())
    }

    /// Notes the capacity `v` holds beyond its length, for `settle`.
    pub(crate) fn keep<T>(&self, v: &Vec<T>) {
        let extra =
            ((v.capacity() - v.len()) as u64).saturating_mul(std::mem::size_of::<T>() as u64);
        self.kept.fetch_add(extra, Ordering::Relaxed);
    }

    /// Books the capacity noted by `keep`, once every charge a fresh call would make is made, so
    /// a kept buffer never turns a charge that fits into an error. Returns `false`, booking
    /// nothing, when it does not fit: the caller then shrinks its buffers to their length.
    pub(crate) fn settle(&self) -> bool {
        self.add(self.kept.swap(0, Ordering::Relaxed)).is_ok()
    }
}

/// Bytes booked on a [`Ledger`] until this drops.
pub(crate) struct Held<'a> {
    ledger: &'a Ledger,
    n: u64,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.ledger.used.fetch_sub(self.n, Ordering::Relaxed);
    }
}

/// Makes `v` `n` copies of `init`, reusing its capacity, with the allocation failure as an
/// error.
pub(crate) fn refill<T: Clone>(v: &mut Vec<T>, n: usize, init: T) -> Result<(), Error> {
    v.clear();
    v.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    v.resize(n, init);
    Ok(())
}

/// `n` copies of `v`, with the allocation failure as an error.
pub(crate) fn filled<T: Clone>(n: usize, v: T) -> Result<Vec<T>, Error> {
    let mut out = Vec::new();
    refill(&mut out, n, v)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_alloc: u64) -> Limits {
        Limits {
            max_alloc,
            ..Limits::default()
        }
    }

    #[test]
    fn a_hold_is_released_on_drop() {
        let l = Ledger::new(&limits(100));
        {
            let _h = l.hold(80).unwrap();
            assert!(l.hold(30).is_none());
        }
        assert!(l.hold(100).is_some());
        assert_eq!(l.peak(), 100);
    }

    #[test]
    fn a_failed_hold_books_nothing() {
        let l = Ledger::new(&limits(100));
        assert!(l.hold(101).is_none());
        assert!(l.charge(100).is_ok());
    }

    #[test]
    fn a_charge_over_the_budget_is_alloc_limit() {
        let l = Ledger::new(&limits(10));
        assert!(l.charge(10).is_ok());
        assert_eq!(l.charge(1), Err(Error::AllocLimit { need: 11, max: 10 }));
        let mut v = Vec::new();
        assert_eq!(
            Ledger::new(&limits(10)).reuse(&mut v, 11, 0u8),
            Err(Error::AllocLimit { need: 11, max: 10 })
        );
        let mut v = vec![7u32; 3];
        Ledger::new(&limits(4)).reuse(&mut v, 1, 0u32).unwrap();
        assert_eq!(v, vec![0u32]);
    }

    #[test]
    fn a_kept_capacity_is_booked_after_the_charges() {
        for (max, fits) in [(23, false), (24, true)] {
            let l = Ledger::new(&limits(max));
            let mut v = vec![0u8; 8];
            l.reuse(&mut v, 4, 0).unwrap();
            l.charge(16).unwrap();
            assert_eq!(l.settle(), fits);
            assert_eq!(l.mark(), if fits { 24 } else { 20 });
        }
    }

    #[test]
    fn zero_threads_counts_as_one() {
        let l = Limits {
            max_threads: 0,
            ..Limits::default()
        };
        assert_eq!(l.threads(), 1);
        assert!(Limits::default().threads() >= 1);
    }
}
