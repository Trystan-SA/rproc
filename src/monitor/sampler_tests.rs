use super::*;

#[test]
fn push_capped_grows_until_cap() {
    let mut q: VecDeque<i32> = VecDeque::new();
    for v in 0..5 {
        push_capped(&mut q, v, 5);
    }
    assert_eq!(q.len(), 5);
    assert_eq!(q.front(), Some(&0));
    assert_eq!(q.back(), Some(&4));
}

#[test]
fn push_capped_drops_oldest_when_full() {
    // Once the cap is reached, the front (oldest) drops on every push so
    // the queue keeps a fixed-size rolling window of the most recent
    // samples — this is the invariant the plots rely on.
    let mut q: VecDeque<i32> = VecDeque::new();
    for v in 0..7 {
        push_capped(&mut q, v, 5);
    }
    assert_eq!(q.len(), 5);
    assert_eq!(q.front(), Some(&2));
    assert_eq!(q.back(), Some(&6));
}

#[test]
fn push_capped_holds_cap_under_burst() {
    // After many pushes the queue size should plateau at exactly `cap`,
    // never grow past it — this is the safety net the plot widgets
    // depend on for their fixed-width X axis.
    let mut q: VecDeque<i32> = VecDeque::new();
    for v in 0..1000 {
        push_capped(&mut q, v, 60);
    }
    assert_eq!(q.len(), 60);
    // And the contents are the last 60 values.
    assert_eq!(q.front(), Some(&940));
    assert_eq!(q.back(), Some(&999));
}

#[test]
fn arc_snapshot_publication_smoke() {
    // We can't easily exercise the sampler thread without running the
    // full pipeline, but we CAN verify the Arc<Snapshot> publication
    // surface keeps the cheap-clone invariant: cloning the published
    // value must return the same pointer rather than reallocating.
    let inner: Arc<Mutex<Arc<Snapshot>>> = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
    let a = inner.lock().unwrap().clone();
    let b = inner.lock().unwrap().clone();
    assert!(
        Arc::ptr_eq(&a, &b),
        "cloned Arcs must share the same allocation"
    );

    // Swap publishes a new Snapshot — the previous handles keep pointing
    // at the old data, the new lock yields the new one.
    {
        let mut guard = inner.lock().unwrap();
        *guard = Arc::new(Snapshot {
            ready: true,
            ..Snapshot::default()
        });
    }
    let c = inner.lock().unwrap().clone();
    assert!(c.ready);
    assert!(!a.ready);
    assert!(!Arc::ptr_eq(&a, &c));
}
