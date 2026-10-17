//! Exercise both emitted launchers against real disposable filesystem fixtures.
use super::*;

pub(super) fn mutations(prefix: &str) {
    let t = tempfile::tempdir().unwrap();
    let work = t.path().join("work");
    let outside = t.path().join("outside");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(outside.join("empty")).unwrap();
    std::fs::write(outside.join("victim"), "retain").unwrap();
    std::os::unix::fs::symlink(&outside, work.join("escape")).unwrap();
    let code = format!(
        "__file__='/managed/bridge.py'\n{prefix}\nsys.addaudithook(audit)\n{}",
        include_str!("filesystem_guard_cases.py")
    );
    let text = process::run(
        Path::new("/usr/bin/python3"),
        &["-B".into(), "-I".into(), "-c".into(), code],
        &work,
        5,
        512,
        65536,
    )
    .unwrap();
    assert!(text.contains("filesystem-boundary-ok"));
    assert_eq!(
        std::fs::read_to_string(outside.join("victim")).unwrap(),
        "retain"
    );
    assert!(outside.join("empty").is_dir());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 2);
}
