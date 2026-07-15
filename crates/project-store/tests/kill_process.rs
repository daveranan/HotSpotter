use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use hot_trimmer_domain::SourceId;
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use uuid::Uuid;

fn owned_source(bytes: usize) -> SourceInput {
    SourceInput {
        id: SourceId::new(),
        ownership: SourceOwnership::OwnedCopy,
        external_path: None,
        sha256: "b".repeat(64),
        width: 64,
        height: 64,
        format: "PNG".into(),
        color_type: "Rgba8".into(),
        has_alpha: true,
        exif_orientation: 1,
        has_embedded_icc_profile: false,
        encoded_bytes: u64::try_from(bytes).expect("fixture byte count"),
        owned_bytes: Some(vec![7; bytes]),
    }
}

fn fixture_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("hot-trimmer-kill-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).expect("create fixture directory");
    path
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() && started.elapsed() < Duration::from_secs(20) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "child process did not reach failpoint");
}

#[test]
fn crash_child() {
    let Ok(mode) = std::env::var("HOT_TRIMMER_CRASH_MODE") else {
        return;
    };
    let project = PathBuf::from(std::env::var_os("HOT_TRIMMER_PROJECT").expect("project path"));
    let signal = PathBuf::from(std::env::var_os("HOT_TRIMMER_SIGNAL").expect("signal path"));
    let mut store = ProjectStore::open(&project).expect("child opens project");
    match mode.as_str() {
        "autosave" => {
            store
                .replace_source(SourceChannel::BaseColor, &owned_source(4 * 1024 * 1024))
                .expect("child autosave");
            fs::write(&signal, b"committed").expect("signal autosave commit");
        }
        "save" => {
            fs::write(&signal, b"starting").expect("signal save start");
            store
                .backup_atomic(&project.with_extension("saving"))
                .expect("child save backup");
        }
        other => panic!("unknown crash mode {other}"),
    }
    thread::sleep(Duration::from_secs(60));
}

fn spawn_child(mode: &str, project: &Path, signal: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "crash_child", "--nocapture"])
        .env("HOT_TRIMMER_CRASH_MODE", mode)
        .env("HOT_TRIMMER_PROJECT", project)
        .env("HOT_TRIMMER_SIGNAL", signal)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn crash child")
}

#[test]
fn kill_after_autosave_commit_leaves_a_valid_reopenable_project() {
    let fixture = fixture_dir();
    let project = fixture.join("autosave.hottrimmer");
    let signal = fixture.join("autosave.signal");
    drop(ProjectStore::create(&project, "Autosave").expect("create project"));
    let mut child = spawn_child("autosave", &project, &signal);
    wait_for(&signal);
    child.kill().expect("kill child");
    child.wait().expect("wait for child");
    let reopened = ProjectStore::open(&project).expect("reopen after kill");
    assert_eq!(reopened.summary().expect("summary").sources.len(), 1);
    assert_eq!(reopened.autosave_journal().expect("journal").len(), 1);
    drop(reopened);
    fs::remove_dir_all(fixture).expect("remove fixture");
}

#[test]
fn kill_during_save_never_damages_the_previous_project() {
    let fixture = fixture_dir();
    let project = fixture.join("save.hottrimmer");
    let signal = fixture.join("save.signal");
    let mut store = ProjectStore::create(&project, "Save").expect("create project");
    store
        .replace_source(SourceChannel::BaseColor, &owned_source(32 * 1024 * 1024))
        .expect("seed large project");
    drop(store);
    let mut child = spawn_child("save", &project, &signal);
    wait_for(&signal);
    child.kill().expect("kill child");
    child.wait().expect("wait for child");
    let reopened = ProjectStore::open(&project).expect("previous project remains valid");
    assert_eq!(reopened.summary().expect("summary").sources.len(), 1);
    drop(reopened);
    fs::remove_dir_all(fixture).expect("remove fixture");
}
