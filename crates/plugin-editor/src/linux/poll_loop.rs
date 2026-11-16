//! A main loop for tools that open plug-in editors without GTK (the examples): the [`EventLoop`]
//! the editors and plug-ins register their file descriptors and timers with, and [`run_once`], one
//! pass over them. Sources belong to the thread that added them, which must run them.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{EventLoop, FdEvents};

enum Src {
    Fd(i32, FdEvents, Box<dyn FnMut(FdEvents)>),
    Timer(Duration, Instant, Box<dyn FnMut()>),
}

thread_local! {
    static SOURCES: RefCell<Vec<(u64, Option<Src>)>> = const { RefCell::new(Vec::new()) };
}
static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct PollLoop;

impl EventLoop for PollLoop {
    fn add_fd(&self, fd: i32, watch: FdEvents, cb: Box<dyn FnMut(FdEvents)>) -> Option<u64> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        SOURCES.with(|s| s.borrow_mut().push((id, Some(Src::Fd(fd, watch, cb)))));
        Some(id)
    }

    fn add_timer(&self, ms: u64, cb: Box<dyn FnMut()>) -> Option<u64> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let d = Duration::from_millis(ms);
        SOURCES.with(|s| s.borrow_mut().push((id, Some(Src::Timer(d, Instant::now() + d, cb)))));
        Some(id)
    }

    fn remove(&self, id: u64) {
        SOURCES.with(|s| s.borrow_mut().retain(|(i, _)| *i != id));
    }
}

/// Runs the file descriptors that are ready and the timers that are due, once each.
pub fn run_once() {
    // Each source is taken out while it runs, so its callback may add and remove sources.
    let ids: Vec<u64> = SOURCES.with(|s| s.borrow().iter().map(|(i, _)| *i).collect());
    for id in ids {
        let src = SOURCES.with(|s| s.borrow_mut().iter_mut().find(|(i, _)| *i == id).and_then(|(_, x)| x.take()));
        let Some(mut src) = src else { continue };
        match &mut src {
            Src::Fd(fd, watch, cb) => {
                if let Some(ready) = ready(*fd, *watch) {
                    cb(ready);
                }
            }
            Src::Timer(d, next, cb) => {
                if Instant::now() >= *next {
                    *next = Instant::now() + *d;
                    cb();
                }
            }
        }
        SOURCES.with(|s| {
            if let Some((_, slot)) = s.borrow_mut().iter_mut().find(|(i, _)| *i == id) {
                *slot = Some(src);
            }
        });
    }
}

/// What `fd` is ready for of `watch` (and errors), without waiting; None if nothing.
fn ready(fd: i32, watch: FdEvents) -> Option<FdEvents> {
    let mut events = 0;
    if watch.read {
        events |= libc::POLLIN | libc::POLLPRI;
    }
    if watch.write {
        events |= libc::POLLOUT;
    }
    let mut p = libc::pollfd { fd, events, revents: 0 };
    if unsafe { libc::poll(&mut p, 1, 0) } <= 0 {
        return None;
    }
    let r = p.revents;
    Some(FdEvents {
        read: r & (libc::POLLIN | libc::POLLPRI) != 0,
        write: r & libc::POLLOUT != 0,
        error: r & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0,
    })
}
