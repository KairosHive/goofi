//! Recording: every engine's frames, on one timeline.

use goofi_core::time::{stamp, Time};

#[test]
fn utc_is_anchored_once_and_advances_monotonically() {
    let t = Time::new();
    let a = t.utc();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let b = t.utc();
    let step = b.duration_since(a).expect("utc never goes backwards");
    assert!(step.as_millis() >= 15, "utc advances with the monotonic clock: {step:?}");
    // The anchor is the origin, so an instant read at a patch second is that second past it.
    let at = t.utc_at(1.5);
    let d = at.duration_since(a).expect("later than the first read");
    assert!(d.as_secs_f64() > 1.4 && d.as_secs_f64() < 1.6, "utc_at follows patch seconds: {d:?}");
    assert_eq!(stamp(a).len(), "YYYYMMDD-HHMMSS.mmm".len());
}
