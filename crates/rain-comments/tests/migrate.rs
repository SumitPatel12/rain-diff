use std::fs;
use std::path::PathBuf;

use rain_comments::store::migrate_legacy;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join("rain-comments-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn copies_the_old_folder_and_keeps_it() {
    let base = scratch("migrate-copy");
    let old = base.join("redline");
    fs::create_dir_all(old.join("sessions")).unwrap();
    fs::write(old.join("settings.json"), "{\"theme\":\"dark\"}").unwrap();
    fs::write(old.join("sessions").join("a.json"), "{}").unwrap();

    assert!(migrate_legacy(&base).unwrap());

    let new = base.join("Rain");
    assert_eq!(fs::read_to_string(new.join("settings.json")).unwrap(), "{\"theme\":\"dark\"}");
    assert_eq!(fs::read_to_string(new.join("sessions").join("a.json")).unwrap(), "{}");
    assert!(old.join("settings.json").exists());
    let leftovers: Vec<_> = fs::read_dir(&base)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("migrating"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn leaves_an_existing_rain_folder_alone() {
    let base = scratch("migrate-existing");
    fs::create_dir_all(base.join("redline")).unwrap();
    fs::write(base.join("redline").join("settings.json"), "old").unwrap();
    fs::create_dir_all(base.join("Rain")).unwrap();
    fs::write(base.join("Rain").join("settings.json"), "new").unwrap();

    assert!(!migrate_legacy(&base).unwrap());
    assert_eq!(fs::read_to_string(base.join("Rain").join("settings.json")).unwrap(), "new");
}

#[test]
fn does_nothing_without_an_old_folder() {
    let base = scratch("migrate-none");
    assert!(!migrate_legacy(&base).unwrap());
    assert!(!base.join("Rain").exists());
}
