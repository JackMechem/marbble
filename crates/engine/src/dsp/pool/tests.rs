//! The pool against real threads: blocks, panics, plug-ins that never return, refused priority.

#[cfg(target_os = "linux")]
use super::thread_setup::SCHED_RESET_ON_FORK;
use super::*;
use std::sync::atomic::AtomicUsize;

fn pool(threads: usize) -> Pool {
    Pool::new(PoolOptions { threads, ..Default::default() })
}

/// Records the order tasks finished in, and which thread ran each.
struct Log {
    graph: TaskGraph,
    done: Vec<AtomicU32>,
    clock: AtomicU32,
    thread: Vec<AtomicUsize>,
    early: AtomicU32,
}

impl Log {
    fn new(graph: TaskGraph) -> Log {
        let n = graph.len();
        Log {
            graph,
            done: (0..n).map(|_| AtomicU32::new(0)).collect(),
            clock: AtomicU32::new(1),
            thread: (0..n).map(|_| AtomicUsize::new(0)).collect(),
            early: AtomicU32::new(0),
        }
    }
}

impl Job for Log {
    fn run(&self, task: usize, thread: usize, _: &Call) {
        if self.graph.deps(task).iter().any(|&d| self.done[d as usize].load(Ordering::SeqCst) == 0) {
            self.early.fetch_add(1, Ordering::SeqCst);
        }
        self.thread[task].store(thread, Ordering::Relaxed);
        std::hint::black_box((0..200).sum::<u32>());
        self.done[task].store(self.clock.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
    }
}

fn wide_graph(n: u64) -> TaskGraph {
    // Everything feeds a bus and the bus feeds the last one, with a chain on the side.
    let ids: Vec<u64> = (0..n).collect();
    let mut edges: Vec<(u64, u64)> = (0..n - 2).map(|i| (i, n - 2)).collect();
    edges.push((n - 2, n - 1));
    edges.extend((0..8).map(|i| (i, i + 1)));
    TaskGraph::new(ids, &edges, |_| true).unwrap()
}

fn run_blocks(p: &Pool, g: &TaskGraph, job: &Arc<Log>, blocks: usize) {
    for _ in 0..blocks {
        for d in &job.done {
            d.store(0, Ordering::SeqCst);
        }
        let ptr: *const Log = Arc::as_ptr(job);
        unsafe { p.start(g, ptr) };
        assert!(p.finish(None));
        assert!(job.done.iter().all(|d| d.load(Ordering::SeqCst) != 0), "every task ran");
    }
}

#[test]
fn every_task_runs_after_its_dependencies_whatever_the_thread_count() {
    for threads in [1, 2, 3, 8, 16] {
        let g = wide_graph(40);
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(threads);
        run_blocks(&p, &g, &job, 300);
        assert_eq!(job.early.load(Ordering::SeqCst), 0, "{threads} threads");
    }
}

#[test]
fn a_pool_with_only_the_caller_does_it_all() {
    let g = wide_graph(20);
    let job = Arc::new(Log::new(g.clone()));
    let p = pool(1);
    run_blocks(&p, &g, &job, 5);
    assert!(job.thread.iter().all(|t| t.load(Ordering::Relaxed) == 0));
}

#[test]
fn large_graphs_use_every_ready_word() {
    let g = TaskGraph::new((0..MAX_TASKS as u64).collect(), &[], |_| true).unwrap();
    let job = Arc::new(Log::new(g.clone()));
    let p = pool(4);
    run_blocks(&p, &g, &job, 3);
}

struct Panicky {
    at: usize,
    ran: AtomicUsize,
}

impl Job for Panicky {
    fn run(&self, task: usize, _thread: usize, _: &Call) {
        if task == self.at {
            panic!("expected: a plug-in blowing up in a pool test");
        }
        self.ran.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_panic_is_counted_and_the_pool_carries_on() {
    let g = wide_graph(12);
    for threads in [1, 4] {
        let p = pool(threads);
        for at in [0, 5, 11] {
            let job = Arc::new(Panicky { at, ran: AtomicUsize::new(0) });
            let ptr: *const Panicky = Arc::as_ptr(&job);
            unsafe { p.start(&g, ptr) };
            assert!(p.finish(None), "dependants of the panicked task still run");
            assert_eq!(job.ran.load(Ordering::SeqCst), 11);
            assert_eq!(p.take_panics(), 1);
            assert_eq!(p.take_panics(), 0);
        }
    }
}

struct Sleepy {
    slow: AtomicUsize,
    ran: AtomicUsize,
}

impl Job for Sleepy {
    fn run(&self, _task: usize, thread: usize, _: &Call) {
        // Whichever worker gets a task first holds on to it, as if preempted. The caller gives
        // it the chance, as on a single core it might otherwise never run.
        if thread == 0 {
            thread::sleep(Duration::from_micros(200));
        } else if self.slow.swap(0, Ordering::SeqCst) == 1 {
            thread::sleep(Duration::from_millis(300));
        }
        self.ran.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_stuck_worker_cannot_hang_the_caller_and_everything_else_gets_done() {
    let ids: Vec<u64> = (0..64).collect();
    let g = TaskGraph::new(ids, &[], |_| true).unwrap();
    let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
    let p = pool(2);
    let ptr: *const Sleepy = Arc::as_ptr(&job);
    // Until the worker gets started and takes a task, the caller could do them all; tries go on
    // until it does.
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        let t = Instant::now();
        let done = p.finish(Some(Duration::from_millis(20)));
        assert!(t.elapsed() < Duration::from_millis(280), "only the time limit and a nap past it");
        if !done {
            assert!(!p.is_done());
            assert!(job.ran.load(Ordering::SeqCst) >= 63, "the caller took everything that was ready");
            while !p.is_done() {
                thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("the worker never got a task in 200 blocks");
}

#[test]
fn workers_really_block_between_blocks() {
    let g = wide_graph(10);
    let job = Arc::new(Log::new(g.clone()));
    let p = pool(3);
    run_blocks(&p, &g, &job, 10);
    thread::sleep(Duration::from_millis(50));
    assert!(p.shared.workers.iter().all(|w| w.parked.load(Ordering::SeqCst)), "both parked");
    // And they wake for the next one.
    for _ in 0..100 {
        run_blocks(&p, &g, &job, 1);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(job.early.load(Ordering::SeqCst), 0);
}

#[test]
fn workers_flush_denormals() {
    struct Denormal(AtomicU32);
    impl Job for Denormal {
        fn run(&self, _task: usize, thread: usize, _: &Call) {
            if thread != 0 && std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5f32) > 0.0 {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
    let job = Arc::new(Denormal(AtomicU32::new(0)));
    let p = pool(4);
    let ptr: *const Denormal = Arc::as_ptr(&job);
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        assert!(p.finish(None));
    }
    assert_eq!(job.0.load(Ordering::SeqCst), 0);
}

#[test]
fn stopping_a_pool_returns_at_once() {
    let t = Instant::now();
    drop(pool(8));
    assert!(t.elapsed() < Duration::from_secs(5));
}

fn started(p: &Pool) -> HealthReport {
    let t = Instant::now();
    loop {
        let r = p.health().report();
        if r.starting == 0 {
            return r;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "the workers never started");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn workers_refused_real_time_priority_play_nothing_and_the_rest_play_on() {
    let g = wide_graph(40);
    // Workers 1 and 3 of 4 are refused.
    let p = Pool::new(PoolOptions { threads: 5, refuse_for_checks: 0b0101, ..Default::default() });
    let r = started(&p);
    assert_eq!((r.threads, r.playing, r.refused, r.stuck), (5, 3, 2, 0));
    assert_eq!(p.playing(), 3);
    assert!(p.has_workers());
    assert_eq!(p.health().refusal(), Some("refused for a check"));
    let job = Arc::new(Log::new(g.clone()));
    run_blocks(&p, &g, &job, 300);
    assert_eq!(job.early.load(Ordering::SeqCst), 0);
    let mut ran = [false; 5];
    for t in &job.thread {
        ran[t.load(Ordering::Relaxed)] = true;
    }
    assert!(!ran[1] && !ran[3], "a refused worker took a task");
    // The refused threads have ended.
    assert!(p.joins.iter().filter(|(_, j)| j.is_finished()).count() >= 2);
}

#[test]
fn with_every_worker_refused_the_pool_has_none() {
    let p = Pool::new(PoolOptions { threads: 3, refuse_for_checks: 0b11, ..Default::default() });
    let r = started(&p);
    assert_eq!((r.playing, r.refused), (1, 2));
    assert!(!p.has_workers());
    // It still works, the caller doing it all.
    let g = wide_graph(12);
    let job = Arc::new(Log::new(g.clone()));
    run_blocks(&p, &g, &job, 3);
    assert!(job.thread.iter().all(|t| t.load(Ordering::Relaxed) == 0));
}

#[test]
fn a_worker_given_up_on_is_retired_and_the_others_play_on() {
    let ids: Vec<u64> = (0..64).collect();
    let g = TaskGraph::new(ids, &[], |_| true).unwrap();
    let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
    let p = pool(3);
    started(&p);
    let ptr: *const Sleepy = Arc::as_ptr(&job);
    let mut stuck = false;
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        if !p.finish(Some(Duration::from_millis(20))) {
            stuck = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(stuck, "no worker ever got a task in 200 blocks");
    assert_eq!(p.retire_stuck(), 1, "the worker in the slow task");
    assert_eq!(p.retire_stuck(), 0, "and only once");
    let r = p.health().report();
    assert_eq!((r.playing, r.stuck), (2, 1));
    assert!(p.has_workers());
    let gone = p.shared.workers.iter().position(|w| w.state.load(Ordering::SeqCst) == RETIRED).unwrap() + 1;
    while !p.is_done() {
        thread::sleep(Duration::from_millis(1));
    }
    // Out of its task, it ends, and the pool plays on without it.
    let t = Instant::now();
    while !p.joins.iter().find(|(i, _)| *i == gone).unwrap().1.is_finished() {
        assert!(t.elapsed() < Duration::from_secs(5), "the retired worker didn't end");
        thread::sleep(Duration::from_millis(1));
    }
    let g = wide_graph(40);
    let log = Arc::new(Log::new(g.clone()));
    run_blocks(&p, &g, &log, 300);
    assert!(log.thread.iter().all(|t| t.load(Ordering::Relaxed) != gone));
    assert_eq!(log.early.load(Ordering::SeqCst), 0);
}

/// Holds up the first task a worker takes (of the first `to_hold` workers): first in the task
/// itself while `in_task` is set, then out in a "plug-in" for `nap` and while `hold` is set.
///
/// Unlike a real job, it counts a worker that comes back out of the "plug-in" to find it was left
/// there (`done`, `back`), so it touches the job after the pool has let go of it: a test keeps the
/// job until every worker left in it is gone (`let_go`), or leaks it. A job freed under such a
/// worker is written to after it's freed, which corrupted the heap on Windows (issue #232).
struct Hang {
    to_hold: AtomicU32,
    in_task: AtomicBool,
    hold: AtomicBool,
    nap: Duration,
    /// How long the caller takes over each task.
    caller_nap: Duration,
    /// The worker held up last, and its task.
    on: AtomicUsize,
    task: AtomicUsize,
    ran: Vec<AtomicU32>,
    /// Workers in the call now, out of it either way, and out of it to find they were left.
    in_call: AtomicU32,
    done: AtomicU32,
    back: AtomicU32,
}

impl Hang {
    fn new(n: usize) -> Hang {
        Hang {
            to_hold: AtomicU32::new(1),
            in_task: AtomicBool::new(false),
            hold: AtomicBool::new(true),
            nap: Duration::ZERO,
            caller_nap: Duration::from_micros(200),
            on: AtomicUsize::new(0),
            task: AtomicUsize::new(0),
            ran: (0..n).map(|_| AtomicU32::new(0)).collect(),
            in_call: AtomicU32::new(0),
            done: AtomicU32::new(0),
            back: AtomicU32::new(0),
        }
    }
}

impl Job for Hang {
    fn run(&self, task: usize, thread: usize, call: &Call) {
        if thread == 0 {
            thread::sleep(self.caller_nap);
        } else if self.to_hold.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            self.on.store(thread, Ordering::SeqCst);
            self.task.store(task, Ordering::SeqCst);
            while self.in_task.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1));
            }
            call.enter(77);
            self.in_call.fetch_add(1, Ordering::SeqCst);
            thread::sleep(self.nap);
            while self.hold.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1));
            }
            let ours = call.leave();
            // `back` before `done`: a test that has seen `done` reads `back` as final.
            if !ours {
                self.back.fetch_add(1, Ordering::SeqCst);
                self.done.fetch_add(1, Ordering::SeqCst);
                return;
            }
            self.done.fetch_add(1, Ordering::SeqCst);
        }
        self.ran[task].fetch_add(1, Ordering::SeqCst);
    }
}

/// Lets the workers out of the "plug-in" and waits until each one left there is gone: on its way
/// out it touches the job, which must outlive that (see `Hang`).
fn let_go(p: &Pool, job: &Hang) {
    job.hold.store(false, Ordering::SeqCst);
    until("a thread left in the job didn't let go", || (1..p.threads()).all(|t| !p.is_left(t) || p.gone(t)));
}

fn until(what: &str, ok: impl Fn() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < Duration::from_secs(10), "{what}");
        thread::sleep(Duration::from_millis(1));
    }
}

/// Half the tasks wait for one of the other half.
fn pairs() -> TaskGraph {
    let edges: Vec<(u64, u64)> = (0..32).map(|i| (i, i + 32)).collect();
    TaskGraph::new((0..64).collect(), &edges, |_| true).unwrap()
}

/// Tries blocks of `job` until one is given up on with a worker in it.
fn hang(p: &Pool, g: &TaskGraph, job: *const Hang) {
    for _ in 0..200 {
        // SAFETY: the test keeps the job alive for longer than the pool.
        for r in unsafe { &(*job).ran } {
            r.store(0, Ordering::SeqCst);
        }
        unsafe { p.start(g, job) };
        if !p.finish(Some(Duration::from_millis(20))) {
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("no worker ever got a task in 200 blocks");
}

#[test]
fn a_worker_left_in_a_plug_in_costs_its_task_and_what_waits_for_it_and_nothing_else() {
    let g = pairs();
    let job = Arc::new(Hang::new(64));
    let p = pool(3);
    started(&p);
    hang(&p, &g, Arc::as_ptr(&job));
    let mut left = Vec::new();
    assert_eq!(p.abandon(&g, Duration::ZERO, |thread, task, what| left.push((thread, task, what))), 1);
    let (on, s) = (job.on.load(Ordering::SeqCst), job.task.load(Ordering::SeqCst));
    assert_eq!(left, [(on, s, 77)]);
    assert!(p.is_left(on) && !p.gone(on));
    assert!(p.finish(Some(Duration::from_secs(5))), "the rest of the block ends without it");
    let out = |t: usize| t == s || (s < 32 && t == s + 32);
    for t in 0..64 {
        assert_eq!(p.dropped(t), out(t), "task {t}");
        assert_eq!(job.ran[t].load(Ordering::SeqCst), !out(t) as u32, "task {t} ran");
    }
    assert_eq!(p.retire_stuck(), 0, "it's retired already");
    let r = p.health().report();
    assert_eq!((r.playing, r.stuck), (2, 1));
    // The pool plays on without it.
    let log = Arc::new(Log::new(g.clone()));
    run_blocks(&p, &g, &log, 300);
    assert_eq!(log.early.load(Ordering::SeqCst), 0);
    assert!(log.thread.iter().all(|t| t.load(Ordering::Relaxed) != on));
    assert!((0..64).all(|t| !p.dropped(t)), "a new block has all its tasks");
    // The plug-in comes back: the thread lets go and ends, and the blocks after don't notice.
    job.hold.store(false, Ordering::SeqCst);
    let t = Instant::now();
    while !p.gone(on) || !p.joins.iter().find(|(i, _)| *i == on).unwrap().1.is_finished() {
        assert!(t.elapsed() < Duration::from_secs(5), "the thread didn't end");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(job.back.load(Ordering::SeqCst), 1);
    run_blocks(&p, &g, &log, 300);
    assert_eq!(log.early.load(Ordering::SeqCst), 0);
    assert_eq!(p.take_panics(), 0);
}

#[test]
fn a_pool_with_a_worker_left_in_a_plug_in_stops_at_once() {
    let g = pairs();
    // The thread never comes back, so what it reads must outlive the test.
    let job: &'static Hang = Box::leak(Box::new(Hang::new(64)));
    let p = pool(2);
    started(&p);
    hang(&p, &g, job);
    assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| {}), 1);
    assert!(p.finish(Some(Duration::from_secs(5))));
    let t = Instant::now();
    drop(p);
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(job.back.load(Ordering::SeqCst), 0);
}

/// Issue #8 again, but from inside the task: given up on before it went into the plug-in, the
/// worker is retired, and once in a plug-in that doesn't return it's left there after all.
#[test]
fn a_worker_retired_in_its_task_is_left_once_it_is_out_in_a_plug_in() {
    let g = pairs();
    let mut h = Hang::new(64);
    h.in_task = AtomicBool::new(true);
    let job = Arc::new(h);
    let p = pool(2);
    started(&p);
    hang(&p, &g, Arc::as_ptr(&job));
    assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| panic!("left")), 0, "not in a plug-in yet");
    assert_eq!(p.retire_stuck(), 1);
    let (on, s) = (job.on.load(Ordering::SeqCst), job.task.load(Ordering::SeqCst));
    job.in_task.store(false, Ordering::SeqCst);
    until("it never went into the plug-in", || job.in_call.load(Ordering::SeqCst) == 1);
    assert_eq!(p.abandon(&g, Duration::from_secs(60), |_, _, _| panic!("left")), 0, "not in it for long enough");
    let mut left = Vec::new();
    assert_eq!(p.abandon(&g, Duration::ZERO, |thread, task, what| left.push((thread, task, what))), 1);
    assert_eq!(left, [(on, s, 77)]);
    assert!(p.finish(Some(Duration::from_secs(5))), "the rest of the block ends without it");
    let out = |t: usize| t == s || (s < 32 && t == s + 32);
    for t in 0..64 {
        assert_eq!((p.dropped(t), job.ran[t].load(Ordering::SeqCst)), (out(t), !out(t) as u32), "task {t}");
    }
    let r = p.health().report();
    assert_eq!((r.playing, r.stuck), (1, 1), "retired once");
    job.hold.store(false, Ordering::SeqCst);
    until("the thread didn't let go", || p.gone(on));
    assert_eq!(job.back.load(Ordering::SeqCst), 1);
    run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
}

/// The plug-in returns at about the moment the caller gives up: whichever wins, the counts hold.
#[test]
fn a_plug_in_returning_as_the_caller_gives_up_is_either_left_or_not() {
    let g = pairs();
    let limit = Duration::from_millis(5);
    let mut left = 0;
    for k in 0..60u64 {
        let mut h = Hang::new(64);
        h.hold = AtomicBool::new(false);
        h.caller_nap = Duration::from_micros(20);
        // Either side of the limit. Which way each try goes depends on the scheduler, so the
        // counts are checked whichever it is; the two outcomes are each made to happen in tests
        // of their own (`a_plug_in_that_returns_before_the_caller_looks_is_not_left` and
        // `a_worker_left_in_a_plug_in_costs_its_task_…`), and the exchange that decides between
        // them in `the_caller_leaves_only_the_call_it_looked_at`.
        h.nap = Duration::from_micros(4000 + (k % 9) * 250);
        let job = Arc::new(h);
        let p = pool(2);
        started(&p);
        let ptr: *const Hang = Arc::as_ptr(&job);
        let mut done = true;
        for tries in 0.. {
            assert!(tries < 200, "the worker never got a task");
            for r in &job.ran {
                r.store(0, Ordering::SeqCst);
            }
            unsafe { p.start(&g, ptr) };
            done = p.finish(Some(limit));
            if job.on.load(Ordering::SeqCst) != 0 {
                break;
            }
            assert!(p.finish(None));
        }
        let n = if done { 0 } else { p.abandon(&g, Duration::ZERO, |_, _, _| {}) };
        assert!(done || p.finish(Some(Duration::from_secs(5))));
        until("the worker never came out of its call", || job.done.load(Ordering::SeqCst) == 1);
        let s = job.task.load(Ordering::SeqCst);
        let out = |t: usize| n == 1 && (t == s || (s < 32 && t == s + 32));
        for t in 0..64 {
            assert_eq!((p.dropped(t), job.ran[t].load(Ordering::SeqCst)), (out(t), !out(t) as u32), "try {k}, task {t}");
        }
        assert_eq!(job.back.load(Ordering::SeqCst), n, "try {k}");
        let on = job.on.load(Ordering::SeqCst);
        if n == 1 {
            until("the thread didn't let go", || p.gone(on));
        }
        left += n;
        run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 3);
    }
    eprintln!("{left} of 60 left");
}

/// The plug-in returns after the caller gave the block up but before it looks: the worker isn't
/// left, and its task and what waits for it run as ever.
#[test]
fn a_plug_in_that_returns_before_the_caller_looks_is_not_left() {
    let g = pairs();
    let job = Arc::new(Hang::new(64));
    let p = pool(2);
    started(&p);
    hang(&p, &g, Arc::as_ptr(&job));
    until("it never went into the plug-in", || job.in_call.load(Ordering::SeqCst) == 1);
    job.hold.store(false, Ordering::SeqCst);
    until("it never came out", || job.done.load(Ordering::SeqCst) == 1);
    let on = job.on.load(Ordering::SeqCst);
    assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| panic!("left")), 0);
    assert!(p.finish(Some(Duration::from_secs(30))), "the block ends with every task");
    for t in 0..64 {
        assert_eq!((p.dropped(t), job.ran[t].load(Ordering::SeqCst)), (false, 1), "task {t}");
    }
    assert!(!p.is_left(on) && !p.gone(on));
    assert_eq!(job.back.load(Ordering::SeqCst), 0);
    let r = p.health().report();
    assert_eq!((r.playing, r.stuck), (2, 0), "and it plays on");
    run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
}

/// The caller leaves only the call it looked at: not one the thread made since, back to back.
#[test]
fn the_caller_leaves_only_the_call_it_looked_at() {
    let c = Call::new(Instant::now());
    assert_eq!(c.look(Duration::ZERO), None, "in no call");
    // Out of the call before the caller acts: nothing to leave.
    c.enter(1);
    let s = c.look(Duration::ZERO).unwrap();
    assert!(c.leave());
    assert!(!c.leave_at(s));
    // Out of it and into the next: that one is still the thread's.
    c.enter(1);
    let s = c.look(Duration::ZERO).unwrap();
    assert!(c.leave());
    c.enter(2);
    assert!(!c.leave_at(s), "a later call isn't the one looked at");
    assert!(c.leave() && !c.left());
    // Not in it for long enough.
    c.enter(3);
    assert_eq!(c.look(Duration::from_secs(60)), None);
    // Looked at and left while still in it: the thread's own leave fails.
    let s = c.look(Duration::ZERO).unwrap();
    assert!(c.leave_at(s));
    assert!(c.left() && !c.leave());
}

/// The same with a thread making calls back to back while the caller looks: the call the caller
/// leaves the thread in is the one whose `leave` fails, and every call before it was the
/// thread's.
#[test]
fn a_thread_making_calls_back_to_back_is_left_in_the_call_the_caller_saw() {
    for _ in 0..20 {
        let c = Arc::new(Call::new(Instant::now()));
        let worker = {
            let c = c.clone();
            thread::spawn(move || {
                for k in 1u64.. {
                    c.enter(k);
                    std::hint::black_box((0..k % 50).sum::<u64>());
                    if !c.leave() {
                        return k;
                    }
                }
                unreachable!()
            })
        };
        let at = loop {
            if let Some(s) = c.look(Duration::ZERO) {
                if c.leave_at(s) {
                    break s >> 8;
                }
            }
        };
        assert_eq!(worker.join().unwrap(), at, "left in call {at}");
    }
}

/// For a block given up on, the caller only counts tasks out: it runs none, and the block ends
/// once the workers are through.
#[test]
fn passing_over_a_block_runs_nothing_on_the_caller() {
    let g = pairs();
    let job = Arc::new(Hang::new(64));
    let p = pool(2);
    started(&p);
    let ptr: *const Hang = Arc::as_ptr(&job);
    unsafe { p.start(&g, ptr) };
    if !p.pass_over(Duration::from_millis(50)) {
        // The worker took a task first and is held in its plug-in: left, that's counted out too.
        until("it never went into the plug-in", || job.in_call.load(Ordering::SeqCst) == 1);
        assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| {}), 1);
        assert!(p.pass_over(Duration::ZERO));
    }
    assert!(p.is_done());
    assert!(job.ran.iter().all(|r| r.load(Ordering::SeqCst) == 0), "no task ran");
    let_go(&p, &job);
    run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
}

#[test]
fn two_workers_can_be_left_in_one_block_or_one_after_the_other() {
    let g = pairs();
    let mut h = Hang::new(64);
    h.to_hold = AtomicU32::new(2);
    let job = Arc::new(h);
    let p = pool(3);
    started(&p);
    let ptr: *const Hang = Arc::as_ptr(&job);
    let mut left = 0;
    for tries in 0.. {
        if left == 2 {
            break;
        }
        assert!(tries < 400, "only {left} left");
        unsafe { p.start(&g, ptr) };
        if p.finish(Some(Duration::from_millis(20))) {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let n = p.abandon(&g, Duration::ZERO, |_, _, _| {});
        assert!(n >= 1);
        left += n;
        assert!(p.finish(Some(Duration::from_secs(5))));
    }
    let r = p.health().report();
    assert_eq!((r.playing, r.stuck), (1, 2));
    // The caller plays on alone.
    run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
    job.hold.store(false, Ordering::SeqCst);
    until("a thread didn't let go", || p.gone(1) && p.gone(2));
    assert_eq!(job.back.load(Ordering::SeqCst), 2);
}

/// A worker held up in a task but not in a plug-in can't be left: that's for `retire_stuck`.
#[test]
fn a_worker_held_up_outside_a_plug_in_is_not_left() {
    let ids: Vec<u64> = (0..64).collect();
    let g = TaskGraph::new(ids, &[], |_| true).unwrap();
    let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
    let p = pool(2);
    started(&p);
    let ptr: *const Sleepy = Arc::as_ptr(&job);
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        if !p.finish(Some(Duration::from_millis(20))) {
            assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| panic!("left")), 0);
            assert_eq!(p.retire_stuck(), 1);
            while !p.is_done() {
                thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("the worker never got a task in 200 blocks");
}

/// Spins without sleeping on the first worker that gets a task, as a plug-in in an endless loop
/// would, until it's told to stop.
struct Spinner {
    spin: AtomicBool,
    on: AtomicUsize,
}

impl Job for Spinner {
    fn run(&self, _task: usize, thread: usize, _: &Call) {
        if thread == 0 {
            thread::sleep(Duration::from_micros(200));
            return;
        }
        if self.on.compare_exchange(0, thread, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            let t = Instant::now();
            while self.spin.load(Ordering::Relaxed) && t.elapsed() < Duration::from_secs(2) {
                std::hint::spin_loop();
            }
        }
    }
}

/// The real thing, where the system grants real-time priority: a real-time worker that never
/// sleeps is killed with the whole process at RLIMIT_RTTIME's hard limit (200 ms), unless it's
/// demoted first. A failure here ends the test binary with SIGKILL.
#[cfg(target_os = "linux")]
#[test]
fn a_real_time_worker_given_up_on_is_demoted_before_the_kernel_kills_it() {
    let p = Pool::new(PoolOptions { threads: 2, realtime: Some((512, 48000)), ..Default::default() });
    let r = started(&p);
    if r.realtime == 0 {
        eprintln!("skipped: this system doesn't grant real-time priority ({:?})", p.health().refusal());
        return;
    }
    let tid = p.shared.workers[0].os_thread.load(Ordering::SeqCst) as i32;
    // SAFETY: a plain query about a thread of this process.
    let policy = || unsafe { libc::sched_getscheduler(tid) } & !SCHED_RESET_ON_FORK;
    assert_ne!(policy(), libc::SCHED_OTHER, "promoted");
    let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
    let job = Arc::new(Spinner { spin: AtomicBool::new(true), on: AtomicUsize::new(0) });
    let ptr: *const Spinner = Arc::as_ptr(&job);
    let mut stuck = false;
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        if !p.finish(Some(Duration::from_millis(40))) {
            stuck = true;
            break;
        }
    }
    assert!(stuck, "the worker never got a task");
    assert_eq!(p.retire_stuck(), 1);
    assert_eq!(policy(), libc::SCHED_OTHER, "demoted");
    // Past the hard limit, still spinning, and alive.
    thread::sleep(Duration::from_millis(400));
    job.spin.store(false, Ordering::Relaxed);
    while !p.is_done() {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(p.health().report().stuck, 1);
}

/// The macOS twin: the worker gets a Mach time-constraint policy, and loses it when the caller
/// gives up on it. (A time-constraint thread that never yields isn't killed there, but it would
/// hold a core at real-time priority until the scheduler's fail-safe caught it.)
#[cfg(target_os = "macos")]
#[test]
fn a_real_time_worker_given_up_on_is_demoted() {
    use mach2::thread_policy::{
        thread_policy_get, thread_policy_t, thread_time_constraint_policy_data_t, THREAD_TIME_CONSTRAINT_POLICY, THREAD_TIME_CONSTRAINT_POLICY_COUNT,
    };
    let p = Pool::new(PoolOptions { threads: 2, realtime: Some((512, 48000)), ..Default::default() });
    let r = started(&p);
    assert_eq!(r.realtime, 1, "refused: {:?}", p.health().refusal());
    let port = p.shared.workers[0].os_thread.load(Ordering::SeqCst);
    assert_ne!(port, 0);
    // The thread's time-constraint policy, or None while it has the default (none).
    let policy = || {
        let mut info = thread_time_constraint_policy_data_t { period: 0, computation: 0, constraint: 0, preemptible: 0 };
        let mut count = THREAD_TIME_CONSTRAINT_POLICY_COUNT;
        let mut default = 0;
        // SAFETY: a query about a thread of this process, into a struct of the size given.
        let r = unsafe { thread_policy_get(port, THREAD_TIME_CONSTRAINT_POLICY, &mut info as *mut _ as thread_policy_t, &mut count, &mut default) };
        assert_eq!(r, 0);
        (default == 0).then_some(info)
    };
    let want = super::thread_setup::macos::constraint_for(512, 48000).unwrap();
    let got = policy().expect("promoted");
    assert_eq!((got.period, got.computation, got.constraint), (want.period, want.computation, want.constraint));
    let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
    let job = Arc::new(Spinner { spin: AtomicBool::new(true), on: AtomicUsize::new(0) });
    let ptr: *const Spinner = Arc::as_ptr(&job);
    let mut stuck = false;
    for _ in 0..200 {
        unsafe { p.start(&g, ptr) };
        if !p.finish(Some(Duration::from_millis(40))) {
            stuck = true;
            break;
        }
    }
    assert!(stuck, "the worker never got a task");
    assert_eq!(p.retire_stuck(), 1);
    assert!(policy().is_none(), "demoted");
    job.spin.store(false, Ordering::Relaxed);
    while !p.is_done() {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(p.health().report().stuck, 1);
}

#[cfg(target_os = "macos")]
#[test]
fn the_scheduler_accepts_the_time_constraint_for_every_buffer() {
    use super::thread_setup::macos::constraint_for;
    for (frames, rate) in [(16, 192000), (32, 44100), (512, 48000), (2048, 44100), (8192, 8000), (0, 0)] {
        let c = constraint_for(frames, rate).unwrap();
        assert!(c.computation > 0 && c.computation <= c.constraint && c.constraint >= c.period, "{frames}@{rate}: {c:?}");
        assert_eq!(c.preemptible, 1);
        // And the scheduler takes it, on a thread of its own.
        let granted = thread::spawn(move || super::thread_setup::macos::promote(frames, rate, None)).join().unwrap();
        assert_eq!(granted, Ok(()), "{frames}@{rate}");
    }
}

/// A worker copies the IO thread's time-constraint policy when the device gave one, and works one
/// out from the buffer when the scheduler won't take it.
#[cfg(target_os = "macos")]
#[test]
fn a_worker_copies_the_io_threads_policy() {
    use crate::audio_io::coreaudio::{own_policy, IoThread, TimeConstraint};
    let constraint_for = |frames, rate| {
        let c = super::thread_setup::macos::constraint_for(frames, rate).unwrap();
        TimeConstraint { period: c.period, computation: c.computation, constraint: c.constraint }
    };
    // The policy for another buffer than the pool's, so it can only have come from the IO thread.
    let io = constraint_for(256, 44100);
    let promoted = |policy| {
        thread::spawn(move || {
            let opts = PoolOptions { realtime: Some((512, 48000)), io_thread: IoThread { workgroup: None, policy }, ..Default::default() };
            let sh = Pool::new(PoolOptions { threads: 2, ..Default::default() }).shared.clone();
            assert!(super::thread_setup::promote(&sh, 1, &opts));
            own_policy()
        })
        .join()
        .unwrap()
    };
    assert_eq!(promoted(Some(io)), Some(io));
    // Computation longer than the constraint: refused, so the buffer's own.
    let bad = TimeConstraint { computation: io.constraint * 2, ..io };
    assert_eq!(promoted(Some(bad)), Some(constraint_for(512, 48000)));
    assert_eq!(promoted(None), Some(constraint_for(512, 48000)));
}
