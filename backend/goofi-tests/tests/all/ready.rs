//! How a component is built: on a thread of its own, taken whole, a panic a refusal.

use goofi_supervisor::ready::{prepare, Ready};

#[test]
fn a_result_is_read_once_it_is_in_and_wakes_who_asked() {
    let (tx, rx) = std::sync::mpsc::channel();
    let ready = prepare("test", || Ok::<_, String>(42));
    ready.on_ready(move || tx.send(()).unwrap());
    rx.recv().unwrap();
    assert!(ready.is_ready());
    assert_eq!(ready.peek(), Some(Ok(42)));
    assert_eq!(ready.take(), Some(Ok(42)));
    assert_eq!(ready.take(), None, "taken once");
    let failed: Ready<u8> = Ready::done(Err("no".into()));
    assert_eq!(failed.error(), Some("no".into()));
    let panicked = prepare("test", || -> Result<u8, String> { panic!("mid-build") });
    let (tx, rx) = std::sync::mpsc::channel();
    panicked.on_ready(move || tx.send(()).unwrap());
    rx.recv().unwrap();
    assert_eq!(panicked.error(), Some("panicked: mid-build".into()));
}
