//! The real `shine` binary on the command line.

use std::process::Command;

fn shine(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_shine")).args(args).output().expect("runs")
}

#[test]
fn simulate_a_saved_project_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("shine-cli-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wdm.shine");
    let mut p = shine::panels::project::Project::new();
    shine::panels::optics::examples::wdm_link(&mut p);
    std::fs::write(&path, p.to_bytes()).unwrap();
    let path = path.to_string_lossy().into_owned();

    let out = shine(&["simulate", &path]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Display 26,9") && text.contains("11"), "{text}");
    assert!(out.stderr.is_empty(), "nothing but the output: {}", String::from_utf8_lossy(&out.stderr));

    let csv = dir.join("spectrum.csv");
    let out = shine(&["simulate", &path, "--wavelength", "1520:1570:51", "--out", &csv.to_string_lossy()]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&csv).unwrap().lines().count(), 52);

    let bad = shine(&["simulate", &path, "--sweep", "Nope.x=0:1"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no part or block called"));

    let version = shine(&["--version"]);
    assert_eq!(String::from_utf8(version.stdout).unwrap().trim(), format!("shine {}", env!("CARGO_PKG_VERSION")));
    std::fs::remove_dir_all(&dir).ok();
}
