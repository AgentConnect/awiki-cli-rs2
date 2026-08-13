use super::publish_noreplace;
use std::fs;
use std::sync::{Arc, Barrier};

#[test]
fn publishes_complete_file_without_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("candidate.tmp");
    let destination = root.path().join("record.json");
    fs::write(&source, b"first-complete-record").unwrap();

    assert!(publish_noreplace(&source, &destination).unwrap());
    assert_eq!(fs::read(&destination).unwrap(), b"first-complete-record");
}

#[test]
fn existing_destination_is_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("candidate.tmp");
    let destination = root.path().join("record.json");
    fs::write(&source, b"new-candidate").unwrap();
    fs::write(&destination, b"existing-record").unwrap();

    assert!(!publish_noreplace(&source, &destination).unwrap());
    assert_eq!(fs::read(&destination).unwrap(), b"existing-record");
}

#[test]
fn concurrent_publish_has_exactly_one_winner() {
    let root = tempfile::tempdir().unwrap();
    let destination = Arc::new(root.path().join("record.json"));
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();

    for index in 0..2 {
        let source = root.path().join(format!("candidate-{index}.tmp"));
        fs::write(&source, format!("candidate-{index}")).unwrap();
        let destination = destination.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            publish_noreplace(&source, &destination)
        }));
    }

    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|created| **created).count(), 1);
    let published = fs::read_to_string(destination.as_ref()).unwrap();
    assert!(published == "candidate-0" || published == "candidate-1");
}
