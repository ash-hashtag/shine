//! Scripted components: a component's behaviour written in JavaScript and
//! run by QuickJS (through `rquickjs`) in a sandbox.
//!
//! The sandbox is QuickJS with only the language itself: no modules, no
//! files, network, processes or timers (QuickJS's `std` and `os` aren't
//! there at all), no `Date`, and `Math.random` repeats the same numbers
//! every run, so a design simulates the same each time. Every call has a
//! time limit and the engine a memory and stack limit; a script that runs
//! over is stopped with an error, not left to hang the editor.
//!
//! A script defines `component`:
//!
//! ```js
//! const component = {
//!   name: "Variable splitter",
//!   light: { inputs: ["in"], outputs: ["out1", "out2"] },
//!   signals: { inputs: ["split"], outputs: ["ticks"] },
//!   state: 0,
//!   // Field from each light input to each output at a wavelength (nm): a
//!   // row per output, each entry a number or [re, im]. `signals` holds
//!   // the wired signal inputs by name.
//!   transmission(wavelength, signals) { const k = signals.split ?? 0;
//!     return [[Math.sqrt(1 - k)], [Math.sqrt(k)]]; },
//!   // Signal outputs, by name or in order.
//!   outputs(signals, state) { return { ticks: state }; },
//!   // On each clock tick: the next state.
//!   tick(signals, state) { return state + 1; },
//! };
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::photonics::{C64, Matrix};

/// How long one call into a script may run.
pub const TIME_LIMIT: Duration = Duration::from_millis(250);
/// Memory a script's engine may use.
pub const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const STACK_LIMIT: usize = 512 * 1024;

/// What a script declares.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declared {
    pub name: String,
    pub light_inputs: Vec<String>,
    pub light_outputs: Vec<String>,
    pub signal_inputs: Vec<String>,
    pub signal_outputs: Vec<String>,
    /// The initial state, as JSON.
    pub state: String,
}

/// The template a new scripted component starts from.
pub const TEMPLATE: &str = r#"// A component written in JavaScript. It runs in a sandbox: no files,
// network or clock. Declare its ports here; Apply updates its box.
const component = {
  name: "Variable splitter",
  light: { inputs: ["in"], outputs: ["out1", "out2"] },
  signals: { inputs: ["split"], outputs: ["ticks"] },
  state: 0,

  // The field from each light input to each output at a wavelength in nm:
  // one row per output, each entry a number or [re, im]. `signals` holds
  // the signal inputs by name (a component's unconnected input reads 0).
  // Here `split` is the share of the power sent to out2.
  transmission(wavelength, signals) {
    const k = Math.min(Math.max(signals.split ?? 0, 0), 1);
    return [[Math.sqrt(1 - k)], [Math.sqrt(k)]];
  },

  // The signal outputs, by name (or as a list in order).
  outputs(signals, state) {
    return { ticks: state };
  },

  // On each clock tick: the next state.
  tick(signals, state) {
    return state + 1;
  },
};
"#;

#[cfg(not(target_arch = "wasm32"))]
mod engine {
    use super::*;
    use rquickjs::context::intrinsic;
    use rquickjs::{Array, Context, Ctx, Function, Object, Runtime, Value};

    /// A loaded script, ready to call.
    pub struct Engine {
        _runtime: Runtime,
        context: Context,
        /// When the current call must stop.
        deadline: Arc<Mutex<Option<Instant>>>,
        stopped: Arc<AtomicBool>,
    }

    fn message(ctx: &Ctx<'_>, e: rquickjs::Error) -> String {
        if matches!(e, rquickjs::Error::Exception) {
            let caught = ctx.catch();
            if let Some(ex) = caught.as_exception() {
                let msg = ex.message().unwrap_or_default();
                // The stack names the line, as "file:12:5": the first number
                // after a colon, in the user's numbering (after the prelude).
                let line = ex.stack().and_then(|s| {
                    s.split(':').skip(1).find_map(|part| part.split(|c: char| !c.is_ascii_digit()).next().filter(|d| !d.is_empty())?.parse::<i32>().ok())
                });
                return match line {
                    Some(line) if line > PRELUDE_LINES => format!("line {}: {msg}", line - PRELUDE_LINES),
                    _ => msg,
                };
            }
            if let Ok(s) = caught.get::<String>() {
                return s;
            }
            return "the script threw an error".into();
        }
        e.to_string()
    }

    const PRELUDE: &str = r#""use strict";
(() => { let s = 0x2545F491; Math.random = () => { s ^= s << 13; s ^= s >>> 17; s ^= s << 5; return (s >>> 0) / 4294967296; }; })();
"#;
    const PRELUDE_LINES: i32 = 2;

    impl Engine {
        /// Load `code` and read what it declares.
        pub fn load(code: &str) -> Result<(Self, Declared), String> {
            let runtime = Runtime::new().map_err(|e| e.to_string())?;
            runtime.set_memory_limit(MEMORY_LIMIT);
            runtime.set_max_stack_size(STACK_LIMIT);
            let deadline: Arc<Mutex<Option<Instant>>> = Arc::default();
            let stopped = Arc::new(AtomicBool::new(false));
            let (d, s) = (deadline.clone(), stopped.clone());
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let over = d.lock().ok().and_then(|g| *g).is_some_and(|t| Instant::now() > t);
                if over {
                    s.store(true, Ordering::Relaxed);
                }
                over
            })));
            // The language only: no Date, Promise, timers, modules or I/O.
            let context = Context::custom::<(intrinsic::Eval, intrinsic::RegExpCompiler, intrinsic::RegExp, intrinsic::Json, intrinsic::MapSet, intrinsic::TypedArrays)>(&runtime)
                .map_err(|e| e.to_string())?;
            let engine = Self { _runtime: runtime, context, deadline, stopped };
            let wrapped = format!("{PRELUDE}{code}\n;globalThis.component = (typeof component === 'undefined') ? undefined : component;");
            let declared = engine.call(|ctx| {
                ctx.eval::<(), _>(wrapped.as_bytes()).map_err(|e| message(&ctx, e))?;
                let c: Value = ctx.globals().get("component").map_err(|e| message(&ctx, e))?;
                let Some(c) = c.as_object() else {
                    return Err("define `const component = { … }` (see the template)".into());
                };
                let names = |group: &str, side: &str| -> Result<Vec<String>, String> {
                    let g: Option<Object> = c.get(group).map_err(|e| message(&ctx, e))?;
                    match g {
                        Some(g) => {
                            let list: Option<Vec<String>> = g.get(side).map_err(|e| format!("{group}.{side} must be a list of names: {}", message(&ctx, e)))?;
                            Ok(list.unwrap_or_default())
                        }
                        None => Ok(Vec::new()),
                    }
                };
                let state: Value = c.get("state").map_err(|e| message(&ctx, e))?;
                let state = ctx.json_stringify(state).map_err(|e| message(&ctx, e))?.map(|s| s.to_string().unwrap_or_default()).unwrap_or_else(|| "null".into());
                Ok(Declared {
                    name: c.get::<_, Option<String>>("name").ok().flatten().unwrap_or_else(|| "Scripted component".into()),
                    light_inputs: names("light", "inputs")?,
                    light_outputs: names("light", "outputs")?,
                    signal_inputs: names("signals", "inputs")?,
                    signal_outputs: names("signals", "outputs")?,
                    state,
                })
            })?;
            if !declared.light_outputs.is_empty() && !declared.light_inputs.is_empty() {
                let has: bool = engine.call(|ctx| {
                    let c: Object = ctx.globals().get("component").map_err(|e| message(&ctx, e))?;
                    Ok(c.get::<_, Option<Function>>("transmission").ok().flatten().is_some())
                })?;
                if !has {
                    return Err("it has light ports but no transmission(wavelength, signals)".into());
                }
            }
            Ok((engine, declared))
        }

        /// Run `f` with the time limit.
        fn call<R>(&self, f: impl FnOnce(Ctx<'_>) -> Result<R, String>) -> Result<R, String> {
            self.stopped.store(false, Ordering::Relaxed);
            *self.deadline.lock().map_err(|_| "script engine poisoned")? = Some(Instant::now() + TIME_LIMIT);
            let result = self.context.with(f);
            *self.deadline.lock().map_err(|_| "script engine poisoned")? = None;
            if self.stopped.load(Ordering::Relaxed) {
                return Err(format!("it ran longer than {} ms and was stopped", TIME_LIMIT.as_millis()));
            }
            result
        }

        fn signals<'js>(ctx: &Ctx<'js>, names: &[String], values: &[Option<f64>]) -> Result<Object<'js>, String> {
            let o = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
            for (n, v) in names.iter().zip(values) {
                if let Some(v) = v {
                    o.set(n.as_str(), *v).map_err(|e| e.to_string())?;
                }
            }
            Ok(o)
        }

        /// The `outputs × inputs` field transmission at `wavelength_nm`.
        pub fn transmission(&self, wavelength_nm: f64, d: &Declared, signals: &[Option<f64>]) -> Result<Matrix, String> {
            let (o, i) = (d.light_outputs.len(), d.light_inputs.len());
            self.call(|ctx| {
                let c: Object = ctx.globals().get("component").map_err(|e| message(&ctx, e))?;
                let f: Function = c.get("transmission").map_err(|e| message(&ctx, e))?;
                let s = Self::signals(&ctx, &d.signal_inputs, signals)?;
                let rows: Array = f.call((rquickjs::function::This(c), wavelength_nm, s)).map_err(|e| message(&ctx, e))?;
                if rows.len() != o {
                    return Err(format!("transmission returned {} rows; it has {o} light outputs", rows.len()));
                }
                let mut m = Matrix::zeros(o, i);
                for r in 0..o {
                    let row: Array = rows.get(r).map_err(|_| format!("row {} isn't a list", r + 1))?;
                    if row.len() != i {
                        return Err(format!("row {} has {} entries; it has {i} light inputs", r + 1, row.len()));
                    }
                    for k in 0..i {
                        let v: Value = row.get(k).map_err(|e| message(&ctx, e))?;
                        m[(r, k)] = if let Some(n) = v.as_number() {
                            C64::new(n, 0.0)
                        } else if let Some(pair) = v.as_array() {
                            C64::new(pair.get::<f64>(0).map_err(|e| message(&ctx, e))?, pair.get::<f64>(1).map_err(|e| message(&ctx, e))?)
                        } else {
                            return Err(format!("entry {},{} must be a number or [re, im]", r + 1, k + 1));
                        };
                    }
                }
                Ok(m)
            })
        }

        /// The signal outputs for these inputs and state (JSON).
        pub fn outputs(&self, d: &Declared, signals: &[Option<f64>], state: &str) -> Result<Vec<f64>, String> {
            self.call(|ctx| {
                let c: Object = ctx.globals().get("component").map_err(|e| message(&ctx, e))?;
                let Some(f) = c.get::<_, Option<Function>>("outputs").ok().flatten() else {
                    return Ok(vec![0.0; d.signal_outputs.len()]);
                };
                let s = Self::signals(&ctx, &d.signal_inputs, signals)?;
                let state = ctx.json_parse(state).map_err(|e| message(&ctx, e))?;
                let out: Value = f.call((rquickjs::function::This(c), s, state)).map_err(|e| message(&ctx, e))?;
                let number = |v: Value| v.as_number().or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 })).unwrap_or(0.0);
                if let Some(list) = out.as_array() {
                    Ok((0..d.signal_outputs.len()).map(|k| list.get::<Value>(k).map(number).unwrap_or(0.0)).collect())
                } else if let Some(obj) = out.as_object() {
                    Ok(d.signal_outputs.iter().map(|n| obj.get::<_, Value>(n.as_str()).map(number).unwrap_or(0.0)).collect())
                } else {
                    Err("outputs must return an object of named values or a list".into())
                }
            })
        }

        /// The next state (JSON) on a clock tick.
        pub fn tick(&self, d: &Declared, signals: &[Option<f64>], state: &str) -> Result<String, String> {
            self.call(|ctx| {
                let c: Object = ctx.globals().get("component").map_err(|e| message(&ctx, e))?;
                let Some(f) = c.get::<_, Option<Function>>("tick").ok().flatten() else {
                    return Ok(state.to_owned());
                };
                let s = Self::signals(&ctx, &d.signal_inputs, signals)?;
                let state = ctx.json_parse(state).map_err(|e| message(&ctx, e))?;
                let next: Value = f.call((rquickjs::function::This(c), s, state)).map_err(|e| message(&ctx, e))?;
                Ok(ctx.json_stringify(next).map_err(|e| message(&ctx, e))?.map(|s| s.to_string().unwrap_or_default()).unwrap_or_else(|| "null".into()))
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use engine::Engine;

/// A script's engine, loaded once per distinct script and shared.
#[cfg(not(target_arch = "wasm32"))]
pub fn engine(code: &str) -> Result<(Arc<Mutex<Engine>>, Declared), String> {
    use std::hash::{Hash, Hasher};
    type Cache = Mutex<HashMap<u64, (Arc<Mutex<Engine>>, Declared)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut h);
    let key = h.finish();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().map_err(|_| "script cache poisoned")?.get(&key) {
        return Ok(hit.clone());
    }
    let (engine, declared) = Engine::load(code)?;
    let entry = (Arc::new(Mutex::new(engine)), declared);
    let mut c = cache.lock().map_err(|_| "script cache poisoned")?;
    if c.len() > 256 {
        c.clear();
    }
    c.insert(key, entry.clone());
    Ok(entry)
}

#[cfg(target_arch = "wasm32")]
pub fn declared(_code: &str) -> Result<Declared, String> {
    Err("scripted components run in the desktop app".into())
}

/// What a script declares, or why it can't be loaded.
#[cfg(not(target_arch = "wasm32"))]
pub fn declared(code: &str) -> Result<Declared, String> {
    engine(code).map(|e| e.1)
}

/// A scripted component's light, as an engine component: the script is
/// asked for its transmission at each wavelength, with the signals that
/// drive it at the time.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct Scripted {
    pub name: String,
    pub engine: Arc<Mutex<Engine>>,
    pub declared: Declared,
    pub signals: Vec<Option<f64>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scripted({})", self.name)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Scripted {
    pub fn transmission(&self, wavelength_nm: f64) -> Result<Matrix, String> {
        self.engine.lock().map_err(|_| "script engine poisoned")?.transmission(wavelength_nm, &self.declared, &self.signals)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::photonics::Component for Scripted {
    fn name(&self) -> &str {
        &self.name
    }
    fn num_inputs(&self) -> usize {
        self.declared.light_inputs.len()
    }
    fn num_outputs(&self) -> usize {
        self.declared.light_outputs.len()
    }
    fn s_matrix(&self, wavelength_nm: f64) -> Matrix {
        // Errors are found before the circuit is built (see the simulator);
        // one here (a script that fails only at some wavelengths) blocks the light.
        self.transmission(wavelength_nm).unwrap_or_else(|e| {
            tracing::warn!("Script “{}” at {wavelength_nm} nm: {e}", self.name);
            Matrix::zeros(self.num_outputs(), self.num_inputs())
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn the_template_declares_its_ports_and_runs() {
        let (e, d) = Engine::load(TEMPLATE).unwrap();
        assert_eq!(d.name, "Variable splitter");
        assert_eq!((d.light_inputs.len(), d.light_outputs.len(), d.signal_inputs.clone(), d.signal_outputs.clone()), (1, 2, vec!["split".into()], vec!["ticks".into()]));
        assert_eq!(d.state, "0");
        // Not driven: all to out1. Driven to 0.2: 80/20.
        let m = e.transmission(1550.0, &d, &[None]).unwrap();
        assert!((m[(0, 0)].norm_sqr() - 1.0).abs() < 1e-12);
        let m = e.transmission(1550.0, &d, &[Some(0.2)]).unwrap();
        assert!((m[(1, 0)].norm_sqr() - 0.2).abs() < 1e-12);
        // State through the clock.
        let next = e.tick(&d, &[None], "0").unwrap();
        assert_eq!(next, "1");
        assert_eq!(e.outputs(&d, &[None], &next).unwrap(), vec![1.0]);
    }

    #[test]
    fn the_sandbox_has_no_way_out() {
        // No modules, files, network, processes, timers or clock.
        for probe in [
            "typeof require",
            "typeof process",
            "typeof std",
            "typeof os",
            "typeof fetch",
            "typeof XMLHttpRequest",
            "typeof setTimeout",
            "typeof Date",
            "typeof Promise",
            "typeof performance",
        ] {
            let code = format!("const component = {{ name: {probe} }};");
            let (_, d) = Engine::load(&code).unwrap();
            assert_eq!(d.name, "undefined", "{probe}");
        }
        // `import` can't load anything.
        assert!(Engine::load("import * as os from 'os'; const component = {};").is_err());
        // Math.random repeats: the same numbers every run.
        let code = "const component = { name: String(Math.random()) };";
        assert_eq!(Engine::load(code).unwrap().1.name, Engine::load(code).unwrap().1.name);
    }

    #[test]
    fn runaway_scripts_are_stopped() {
        let started = Instant::now();
        let err = Engine::load("while (true) {} const component = {};").err().unwrap();
        assert!(err.contains("stopped"), "{err}");
        assert!(started.elapsed() < TIME_LIMIT * 4, "{:?}", started.elapsed());
        // Too much memory: an error, not a crash.
        let err = Engine::load("const a = []; for (;;) a.push(new Array(1e6).fill(1)); const component = {};").err().unwrap();
        assert!(!err.is_empty());
        // Endless recursion: stopped by the stack limit.
        assert!(Engine::load("function f() { return f(); } f(); const component = {};").is_err());
    }

    #[test]
    fn mistakes_are_explained() {
        let syntax = Engine::load("const component = ;").err().unwrap();
        assert!(syntax.starts_with("line 1:"), "syntax errors say where: {syntax}");
        let thrown = Engine::load("const x = 1;\nnull.y;\nconst component = {};").err().unwrap();
        assert!(thrown.starts_with("line 2:"), "{thrown}");
        assert!(Engine::load("let x = 1;").err().unwrap().contains("component"));
        let (e, d) = Engine::load("const component = { light: { inputs: ['a'], outputs: ['b'] }, transmission() { return [[1], [2]]; } };").unwrap();
        assert!(e.transmission(1550.0, &d, &[]).err().unwrap().contains("rows"));
        assert!(Engine::load("const component = { light: { inputs: ['a'], outputs: ['b'] } };").err().unwrap().contains("transmission"));
    }
}
