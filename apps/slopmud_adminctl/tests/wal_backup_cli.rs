use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use slopmud_walbackup::WalBackupStore;

fn temp_dir(label: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "slopmud_adminctl_wal_{label}_{}_{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn append_wal(path: &Path, entries: &[(u64, u64, &str)]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for (index, ms, body) in entries {
        writeln!(
            f,
            "{{\"index\":{index},\"term\":1,\"ms\":{ms},\"entry\":{{\"body\":\"{body}\"}}}}"
        )
        .unwrap();
    }
}

fn run_adminctl(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_slopmud_adminctl"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "adminctl {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn wal_backup_cli_lists_verifies_and_recovers_local_pitr() {
    let dir = temp_dir("local_pitr");
    let source = dir.join("raft.jsonl");
    let backup = dir.join("backup");
    let restored = dir.join("restored.jsonl");

    append_wal(&source, &[(1, 10, "first")]);
    let mut store = WalBackupStore::new(source.clone(), backup.clone(), "n0".to_string(), 32, 10);
    store.sync_once(100).unwrap();
    append_wal(&source, &[(2, 20, "second")]);
    store.sync_once(200).unwrap();

    let backup_s = backup.to_string_lossy();
    let restored_s = restored.to_string_lossy();

    let list = run_adminctl(&[
        "wal-backup",
        "list",
        "--dir",
        &backup_s,
        "--node-id",
        "n0",
        "--extents",
    ]);
    assert!(list.contains("manifest="));
    assert!(list.contains("index=1.."));

    let verify = run_adminctl(&["wal-backup", "verify", "--dir", &backup_s]);
    assert!(verify.contains("ok manifest="));

    let recover = run_adminctl(&[
        "wal-backup",
        "recover",
        "--dir",
        &backup_s,
        "--node-id",
        "n0",
        "--out",
        &restored_s,
        "--until-index",
        "1",
    ]);
    assert!(recover.contains("recovered bytes="));
    let recovered = std::fs::read_to_string(restored).unwrap();
    assert!(recovered.contains("\"index\":1"));
    assert!(!recovered.contains("\"index\":2"));
}
