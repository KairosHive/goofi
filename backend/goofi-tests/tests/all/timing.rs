//! The one time module: how anything waits, paces or measures.

use std::time::{Duration, Instant};

use goofi_supervisor::timing::{Deadline, Event, Grid, Pace, RateMeter};

#[test]
fn ids_coalesce_and_each_wakes_once() {
    let event = Event::new();
    event.ring(3);
    event.ring(3);
    event.ring(200);
    let woken = event.wait();
    assert_eq!(woken.ids().collect::<Vec<_>>(), vec![3, 200]);
    assert!(event.take().is_empty());
}

#[test]
fn a_deadline_wait_returns_empty_on_time() {
    let event = Event::new();
    let t = Instant::now();
    let woken = event.wait_for(Duration::from_millis(5));
    assert!(woken.is_empty());
    assert!(t.elapsed() >= Duration::from_millis(5));
}

#[test]
fn a_grid_is_shared_and_a_rate_is_the_inverse_interval() {
    let interval = Duration::from_millis(10);
    let (mut a, mut b) = (Grid::new(), Grid::new());
    let now = Instant::now();
    assert_eq!(a.due(interval, now), b.due(interval, now), "two duties at one rate land together");
    a.take(interval, now);
    assert!(a.due(interval, now) > now, "a taken tick is past");
    let mut meter = RateMeter::default();
    assert_eq!(meter.note(1.0, 0.5), None);
    assert_eq!(meter.note(1.5, 0.5), Some(2.0));
    assert_eq!(meter.note(1.5, 0.5), Some(2.0), "a clock that did not move keeps the estimate");
    let d = Deadline::within(Duration::from_secs(1));
    assert!(!d.passed() && d.left() <= Duration::from_secs(1));
    assert!(Deadline::at(Instant::now()).passed());
}

#[test]
fn a_pace_that_fell_behind_restarts_from_now() {
    let mut pace = Pace::new(Duration::from_millis(10));
    let first = pace.next();
    // Held past three ticks: a pace is tested by what it answers after a stall, not by sleeping
    // through one, so the stall is a deadline waited out.
    Event::new().wait_for(Duration::from_millis(30));
    let next = pace.advance();
    assert!(next > first + Duration::from_millis(10), "not a burst of catch-up ticks");
    assert!(next <= Instant::now() + Duration::from_millis(10));
}
