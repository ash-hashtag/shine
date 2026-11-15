//! External programs: a block ([`Builtin::Process`]) whose signal outputs
//! come from a program on this computer, for logic written in Python,
//! TypeScript or anything else, when Shine can't do it itself.
//!
//! **Risky**: the program runs with the user's permissions. So it runs only
//! once allowed (by command and arguments), for this session only: the
//! editor asks, with a warning; `shine simulate` needs
//! `--allow-processes`; the MCP server and the browser never run one.
//!
//! The protocol, over the program's standard input and output, a line each
//! way, numbers separated by spaces:
//!
//! 1. Shine writes `shine 1 <inputs> <outputs>`; the program answers `ok`.
//! 2. For each evaluation, Shine writes the input values; the program
//!    answers the output values, as many as it was told.
//! 3. When Shine is done, it closes the program's input: the program should
//!    exit.
//!
//! The program is started once and kept running; Shine may ask for the same
//! inputs more than once, or not again, so it should give the same outputs
//! for the same inputs. What it writes to its standard error goes to Shine's
//! log. An answer that takes longer than [`ANSWER_TIME`], or isn't numbers,
//! stops it (it's started again next time).
//!
//! [`Builtin::Process`]: crate::panels::optics::model::Builtin::Process

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

/// The protocol's first line, before the port counts.
pub const HELLO: &str = "shine 1";
/// How long a program may take to answer an evaluation.
pub const ANSWER_TIME: Duration = Duration::from_secs(5);
/// How long it may take to start and answer `ok`.
pub const START_TIME: Duration = Duration::from_secs(15);

type Key = (String, Vec<String>);

pub use crate::panels::circuit::sim::{ProgramAsk as Ask, ProgramRefusal as Refusal};

#[derive(Default)]
struct State {
    allowed: HashSet<Key>,
    /// Every program, as `shine simulate --allow-processes` says.
    all: bool,
    denied: HashSet<Key>,
    running: HashMap<Key, Running>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(State::default))
}

fn key(command: &str, args: &[String]) -> Key {
    (command.to_owned(), args.to_vec())
}

/// Let `command` with `args` run, for this session.
pub fn allow(command: &str, args: &[String]) {
    with(|s| {
        let k = key(command, args);
        s.denied.remove(&k);
        s.allowed.insert(k);
    });
}

/// Don't run `command` with `args`, nor ask again, this session.
pub fn deny(command: &str, args: &[String]) {
    with(|s| {
        let k = key(command, args);
        s.allowed.remove(&k);
        s.running.remove(&k);
        s.denied.insert(k);
    });
}

/// Let every program run (`shine simulate --allow-processes`).
pub fn allow_all() {
    with(|s| s.all = true);
}

pub fn is_allowed(command: &str, args: &[String]) -> bool {
    with(|s| s.all || s.allowed.contains(&key(command, args)))
}

/// Whether the user said not to run `command` with `args`, this session.
pub fn is_denied(command: &str, args: &[String]) -> bool {
    with(|s| s.denied.contains(&key(command, args)))
}

/// Stop every running program and forget every permission.
pub fn reset() {
    with(|s| *s = State::default());
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    inputs: usize,
    outputs: usize,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(command: &str, args: &[String], inputs: usize, outputs: usize) -> Result<Running, String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't start {command}: {e}"))?;
    let stdin = child.stdin.take().ok_or("no input to the program")?;
    let stdout = child.stdout.take().ok_or("no output from the program")?;
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    if let Some(stderr) = child.stderr.take() {
        let name = command.to_owned();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::warn!("{name}: {line}");
            }
        });
    }
    let mut running = Running { child, stdin, lines, inputs, outputs };
    writeln!(running.stdin, "{HELLO} {inputs} {outputs}").and_then(|()| running.stdin.flush()).map_err(|e| format!("{command} closed its input: {e}"))?;
    match running.lines.recv_timeout(START_TIME) {
        Ok(line) if line.trim() == "ok" => Ok(running),
        Ok(line) => Err(format!("{command} doesn't speak Shine's protocol: it answered “{}” to “{HELLO} {inputs} {outputs}”, not “ok”", line.trim())),
        Err(RecvTimeoutError::Timeout) => Err(format!("{command} didn't answer “ok” within {} s", START_TIME.as_secs())),
        Err(RecvTimeoutError::Disconnected) => Err(format!("{command} ended without answering “ok”")),
    }
}

fn ask_once(running: &mut Running, command: &str, inputs: &[f64]) -> Result<Vec<f64>, String> {
    let line = inputs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
    writeln!(running.stdin, "{line}").and_then(|()| running.stdin.flush()).map_err(|e| format!("{command} stopped reading: {e}"))?;
    let answer = match running.lines.recv_timeout(ANSWER_TIME) {
        Ok(a) => a,
        Err(RecvTimeoutError::Timeout) => return Err(format!("{command} didn't answer within {} s", ANSWER_TIME.as_secs())),
        Err(RecvTimeoutError::Disconnected) => return Err(format!("{command} ended")),
    };
    let values: Vec<f64> = answer
        .split_whitespace()
        .map(|v| v.parse::<f64>().map_err(|_| format!("{command} answered “{v}”, not a number")))
        .collect::<Result<_, _>>()?;
    if values.len() != running.outputs {
        return Err(format!("{command} answered {} numbers for {} outputs: “{}”", values.len(), running.outputs, answer.trim()));
    }
    Ok(values)
}

/// The outputs block `name`'s program (`command` with `args`) gives for
/// `inputs`, `outputs` of them; started if it isn't running. Unless the
/// program is allowed, nothing runs: the refusal says to ask the user.
pub fn call(name: &str, command: &str, args: &[String], inputs: &[f64], outputs: usize) -> Result<Vec<f64>, Refusal> {
    let k = key(command, args);
    let permitted = with(|s| {
        if s.all || s.allowed.contains(&k) {
            return Ok(());
        }
        if s.denied.contains(&k) {
            return Err(Refusal::Error(format!("“{name}” runs {command}, which you chose not to run")));
        }
        Err(Refusal::Ask(Ask { name: name.to_owned(), command: command.to_owned(), args: args.to_vec() }))
    });
    permitted?;
    with(|s| {
        // Started again if its ports changed.
        if s.running.get(&k).is_some_and(|r| r.inputs != inputs.len() || r.outputs != outputs) {
            s.running.remove(&k);
        }
        if !s.running.contains_key(&k) {
            let running = start(command, args, inputs.len(), outputs).map_err(|e| Refusal::Error(format!("“{name}”: {e}")))?;
            s.running.insert(k.clone(), running);
        }
        let running = s.running.get_mut(&k).expect("just started");
        ask_once(running, command, inputs).map_err(|e| {
            // Stopped: started again next time.
            s.running.remove(&k);
            Refusal::Error(format!("“{name}”: {e}"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Python program that adds its two inputs and outputs their sum and
    /// product, if Python is here.
    fn adder() -> Option<(String, Vec<String>)> {
        let python = ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()));
        // Linux (here and in CI) has Python: these tests must run there.
        if python.is_none() && cfg!(target_os = "linux") {
            panic!("Python is needed for these tests");
        }
        let python = python?;
        let code = r#"
import sys
first = sys.stdin.readline().split()
assert first[:2] == ["shine", "1"]
print("ok", flush=True)
for line in sys.stdin:
    a, b = map(float, line.split())
    print(a + b, a * b, flush=True)
"#;
        Some((python.to_owned(), vec!["-c".into(), code.into()]))
    }

    #[test]
    fn a_program_runs_only_once_allowed_and_then_answers() {
        let Some((python, args)) = adder() else { return };
        // A key of this test's own (an extra argument), so tests running
        // alongside don't share its permission.
        let args: Vec<String> = args.into_iter().chain(["first".into()]).collect();
        let Err(Refusal::Ask(ask)) = call("Adder", &python, &args, &[2.0, 3.0], 2) else { panic!("asks first") };
        assert_eq!((ask.name.as_str(), &ask.args), ("Adder", &args));
        allow(&python, &args);
        assert_eq!(call("Adder", &python, &args, &[2.0, 3.0], 2).unwrap(), vec![5.0, 6.0]);
        assert_eq!(call("Adder", &python, &args, &[0.5, 4.0], 2).unwrap(), vec![4.5, 2.0], "the same program, kept running");
        // The wrong number of outputs is an error, and it starts again after.
        let error = |r: Result<Vec<f64>, Refusal>| match r {
            Err(Refusal::Error(e)) => e,
            other => panic!("{other:?}"),
        };
        assert!(error(call("Adder", &python, &args, &[1.0, 1.0], 3)).contains("numbers"));
        assert_eq!(call("Adder", &python, &args, &[1.0, 1.0], 2).unwrap(), vec![2.0, 1.0]);
        deny(&python, &args);
        assert!(is_denied(&python, &args));
        assert!(error(call("Adder", &python, &args, &[1.0, 1.0], 2)).contains("chose not to run"));
    }

    #[test]
    fn a_program_that_doesnt_speak_the_protocol_is_refused() {
        let Some((python, _)) = adder() else { return };
        let args = vec!["-c".to_owned(), "print('hello')".to_owned()];
        allow(&python, &args);
        let Err(Refusal::Error(e)) = call("Greeter", &python, &args, &[1.0], 1) else { panic!() };
        assert!(e.contains("doesn't speak Shine's protocol"), "{e}");
        let missing = vec!["no such program".to_owned()];
        allow("shine-no-such-program", &missing);
        let Err(Refusal::Error(e)) = call("Missing", "shine-no-such-program", &missing, &[], 1) else { panic!() };
        assert!(e.contains("can't start"), "{e}");
    }
}
