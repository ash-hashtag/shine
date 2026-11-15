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

#[test]
fn an_external_program_runs_only_with_allow_processes() {
    use shine::panels::optics::model::{BlockRef, Builtin, Item, Layout, Terminal};
    let Some(python) = ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success())) else {
        if cfg!(target_os = "linux") {
            panic!("Python is needed for this test");
        }
        return;
    };
    // The bits of a + b, from Python.
    let code = "import sys\nsys.stdin.readline()\nprint('ok', flush=True)\nfor line in sys.stdin:\n    a, b = (round(float(v)) for v in line.split())\n    s = a + b\n    print(s // 2, s % 2, flush=True)\n";
    let mut l = Layout::default();
    let value = |l: &mut Layout, cell| l.add_instance(BlockRef::Builtin(Builtin::Value { name: String::new(), value: 1.0 }), cell);
    let (a, b) = (value(&mut l, (0, 0)), value(&mut l, (3, 0)));
    let program = l.add_instance(
        BlockRef::Builtin(Builtin::Process {
            name: "Adder in Python".into(),
            command: python.into(),
            args: vec!["-c".into(), code.into()],
            inputs: vec!["a".into(), "b".into()],
            outputs: vec!["s1".into(), "s0".into()],
            light_inputs: Vec::new(),
            light_outputs: Vec::new(),
            wavelength_nm: 1550.0,
        }),
        (0, 3),
    );
    let display = l.add_instance(BlockRef::Builtin(Builtin::Display { bits: 2 }), (0, 7));
    let wire = |l: &mut Layout, from, fp, to, tp| l.connect_signal(Terminal { item: Item::Box(from), port: fp }, Terminal { item: Item::Box(to), port: tp });
    wire(&mut l, a, 0, program, 0);
    wire(&mut l, b, 0, program, 1);
    wire(&mut l, program, 0, display, 0);
    wire(&mut l, program, 1, display, 1);
    let mut p = shine::panels::project::Project::new();
    p.add_layout("Python adds", l, 10.0);
    let dir = std::env::temp_dir().join(format!("shine-cli-process-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("python.shine");
    std::fs::write(&path, p.to_bytes()).unwrap();
    let path = path.to_string_lossy().into_owned();

    let refused = shine(&["simulate", &path]);
    assert!(!refused.status.success());
    let says = String::from_utf8_lossy(&refused.stderr);
    assert!(says.contains("--allow-processes") && says.contains("Adder in Python"), "{says}");

    let allowed = shine(&["simulate", &path, "--allow-processes"]);
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
    let text = String::from_utf8(allowed.stdout).unwrap();
    assert!(text.lines().any(|l| l.trim_start().starts_with("Display") && l.trim_end().ends_with(" 2")), "1 + 1 = 2: {text}");
    std::fs::remove_dir_all(&dir).ok();
}
