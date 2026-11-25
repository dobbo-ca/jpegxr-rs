# Threading

- **`max_threads` = 1 spawns nothing** and never touches a pool. Above 1 the decoder is a two-stage
  pipeline over macroblock rows: a pool worker entropy-decodes row r + 1 while the calling thread
  transforms row r and writes its pixels. It is used only when the image has at least 16
  macroblock columns and 256 macroblocks (256 x 256 pixels) and the pipeline's three row buffers
  fit in `max_alloc`; otherwise the decode runs serially. The output and the error (and the
  partial output of a failed `decode_into`) are byte-identical for every thread count, so a thread
  count never causes an error that `max_threads = 1` would not.
- **Pools.** A call without a caller's pool uses a lazy shared pool inside the crate: created on
  the first call that uses threads, never more than `available_parallelism` threads, no other
  state. `Decoder::with_pool(data, &limits, &pool)` runs on the caller's `ThreadPool` instead, and
  a caller's pool always wins (the shared pool is then never started). A call takes at most
  `max_threads` workers, and never more than one more than the pool has threads. The pool's own
  threads are not charged to `max_alloc`, and `ThreadPool::new` can fail with `OutOfMemory`. The
  calling thread always does the rows itself when no pool thread is free, so a busy pool never
  costs a call its result, but a call returns only after a pool thread has picked up its queued
  worker job (which then exits at once if the caller already did the rows): with every pool thread
  busy on other calls, a call waits until one frees up.
- **Only the two-stage pipeline uses threads**, so `max_threads` above 2 gains nothing, and
  `decode_into` for one tile gains at most about what the entropy stage costs. Many tiles
  decode best in parallel by the caller, one `Decoder` and `max_threads = 1` each.
