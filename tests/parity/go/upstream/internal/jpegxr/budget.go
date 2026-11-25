package jpegxr

import (
	"context"
	"errors"
	"fmt"
	"math"
	"os"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
)

// ErrMemoryLimit reports a decode gave up waiting for other decodes to
// release memory budget. Transient: retry later.
var ErrMemoryLimit = errors.New("jpegxr: memory limit reached")

var budget struct {
	mu       sync.Mutex
	cond     *sync.Cond
	explicit bool
	limit    int64 // valid only when explicit; <= 0 means no limit
	used     int64
}

func init() { budget.cond = sync.NewCond(&budget.mu) }

// SetMemoryLimit sets the bytes all running decodes may reserve
// together and returns the previous value. n <= 0 means no limit.
func SetMemoryLimit(n int64) int64 {
	budget.mu.Lock()
	old := currentLimit()
	budget.explicit, budget.limit = true, n
	budget.mu.Unlock()
	budget.cond.Broadcast() // a raised limit may free waiters
	return old
}

// currentLimit is the explicit limit, or the default, recomputed each
// call since GOMEMLIMIT and the cgroup limit can change. Caller holds
// budget.mu.
func currentLimit() int64 {
	if budget.explicit {
		return budget.limit
	}
	return defaultMemoryLimit()
}

// defaultMemoryLimit is half of GOMEMLIMIT, else half the cgroup v2
// memory.max, else no limit.
func defaultMemoryLimit() int64 {
	if gm := debug.SetMemoryLimit(-1); gm > 0 && gm < math.MaxInt64 {
		return gm / 2
	}
	if v := cgroupMemMax(); v > 0 {
		return v / 2
	}
	return -1
}

// cgroupMemMax reads /sys/fs/cgroup/memory.max once; the file doesn't
// change while the process runs, so a warm decode need not reread it.
var (
	cgroupOnce  sync.Once
	cgroupLimit int64 = -1
)

func cgroupMemMax() int64 {
	cgroupOnce.Do(func() {
		b, err := os.ReadFile("/sys/fs/cgroup/memory.max")
		if err != nil {
			return
		}
		if v, err := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64); err == nil {
			cgroupLimit = v
		}
	})
	return cgroupLimit
}

// errBudgetExceeded reports a single decode's estimate is over the
// whole limit; it never wraps ErrMemoryLimit, since waiting can't help.
func errBudgetExceeded(est, limit int64) error {
	return fmt.Errorf("jpegxr: decode needs %d bytes, over the %d byte memory limit", est, limit)
}

// reserve charges est bytes against the budget, blocking while other
// decodes hold enough of it. It returns ErrMemoryLimit if ctx is done
// first, or errBudgetExceeded if est alone is over the whole limit.
func reserve(ctx context.Context, est int64) error {
	budget.mu.Lock()
	defer budget.mu.Unlock()
	limit := currentLimit()
	if limit <= 0 {
		budget.used += est
		return nil
	}
	if est > limit {
		return errBudgetExceeded(est, limit)
	}
	if budget.used+est <= limit {
		budget.used += est
		return nil
	}
	// Contended: only now pay for a ctx watcher.
	stop := context.AfterFunc(ctx, budget.cond.Broadcast)
	defer stop()
	for {
		limit = currentLimit()
		if budget.used+est <= limit {
			budget.used += est
			return nil
		}
		if err := ctx.Err(); err != nil {
			return fmt.Errorf("%w: %w", ErrMemoryLimit, err)
		}
		budget.cond.Wait()
	}
}

// release returns est bytes to the budget.
func release(est int64) {
	budget.mu.Lock()
	budget.used -= est
	budget.mu.Unlock()
	budget.cond.Broadcast()
}
