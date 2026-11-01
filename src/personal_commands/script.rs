//! One loaded config in an embedded QuickJS runtime: the module graph from
//! `hex.config.ts` (types stripped by [`super::typescript`]), compiled once,
//! and the transformation calls Hex makes into it. Scripts get plain
//! JavaScript, the `@hex/commands` module, and two host APIs behind the trust
//! prompt: `fetch` and `node:child_process` (with `node:util`'s `promisify`
//! and timers to wait on them, [`super::io`]). No filesystem or environment
//! API. Every load and call runs under a time limit and the runtime under a
//! memory limit.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::promise::PromiseState;
use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Exception, Function, Module, Object, Persistent,
    Promise, Runtime,
};

use super::io::{self, Capabilities, Io, Wake};
use super::trust::Permissions;

const MEMORY_LIMIT: usize = 64 << 20;
const STACK_LIMIT: usize = 1 << 20;
/// The stack of the thread that owns a script: QuickJS's own limit plus room
/// for Hex's frames.
pub const THREAD_STACK_SIZE: usize = 4 << 20;
const LOAD_TIME_LIMIT: Duration = Duration::from_secs(2);
const MAX_SOURCE_BYTES: u64 = 1 << 20;
const MAX_LOG_BYTES: usize = 4096;
const ENTRY: &str = "hex:entry";
const HOST: &str = "hex:host";
const COMMANDS: &str = "@hex/commands";
const IO: &str = "hex:io";
const WEB: &str = "hex:web";
const CHILD_PROCESS: &str = "node:child_process";
const UTIL: &str = "node:util";
const COMMANDS_JS: &str = include_str!("commands.js");
const HOST_JS: &str = include_str!("host.js");
const WEB_JS: &str = include_str!("web.js");
const CHILD_PROCESS_JS: &str = include_str!("child_process.js");
const UTIL_JS: &str = include_str!("util.js");
/// `hex:io` hands the native bindings to the built-in modules, then drops
/// its global.
const IO_JS: &str = "const io = globalThis.__hexIo\ndelete globalThis.__hexIo\nexport default io\n";
const CONSOLE_JS: &str = r#"(() => {
  const log = globalThis.__hexLog
  delete globalThis.__hexLog
  const format = (values) => values.map((value) => {
    if (typeof value === "string") return value
    try { return JSON.stringify(value) ?? String(value) } catch { return String(value) }
  }).join(" ")
  const method = (level) => (...values) => log(level, format(values))
  globalThis.console = Object.freeze({
    log: method("info"), info: method("info"), debug: method("debug"),
    warn: method("warn"), error: method("error"),
  })
})()"#;

pub struct Script {
    // The operations and host hold JavaScript values, so they go before the
    // context and runtime that own them.
    io: io::Shared,
    host: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    deadline: Rc<Cell<Option<Instant>>>,
    registration: String,
    permissions: Permissions,
}

impl Drop for Script {
    fn drop(&mut self) {
        let io = &self.io;
        self.context.with(|_| io.borrow_mut().end());
    }
}

impl Script {
    /// Loads `config` (a file name in `workspace`) and evaluates it.
    pub fn load(workspace: &Path, config: &str) -> Result<Self, String> {
        let root = workspace
            .canonicalize()
            .map_err(|error| format!("{}: {error}", workspace.display()))?;
        let io = Io::new(root.clone());
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);
        let deadline = Rc::new(Cell::new(None::<Instant>));
        let interrupt = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupt
                .get()
                .is_some_and(|deadline| Instant::now() >= deadline)
        })));
        let record = Rc::new(RefCell::new(Record::default()));
        runtime.set_loader(
            WorkspaceLoader {
                root: root.clone(),
                record: record.clone(),
            },
            WorkspaceLoader {
                root,
                record: record.clone(),
            },
        );
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;
        deadline.set(Some(Instant::now() + LOAD_TIME_LIMIT));
        let loaded = context.with(|ctx| {
            let loaded = evaluate(&ctx, config, &io)
                .catch(&ctx)
                .map_err(|error| describe(error, &deadline, LOAD_TIME_LIMIT));
            // Nothing a config starts while loading outlives the load.
            io.borrow_mut().end();
            loaded
        });
        deadline.set(None);
        let mut record = record.borrow_mut();
        // Every module is in; the hash below covers all the code that can run.
        record.sealed = true;
        let (host, registration) = loaded?;
        let permissions = super::trust::scan(&record.sources, record.child_process);
        Ok(Self {
            io,
            host,
            context,
            _runtime: runtime,
            deadline,
            registration,
            permissions,
        })
    }

    /// What the config can reach, for the trust prompt.
    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    /// The config's registration as JSON.
    pub fn registration(&self) -> &str {
        &self.registration
    }

    /// Runs the transformation `id` over `text` within `limit`, with the host
    /// APIs in `allowed`. Hex stopping (`stop`) ends it early. When it
    /// returns, every request, timer, and program it started is over.
    pub fn transform(
        &self,
        id: &str,
        text: &str,
        context: &str,
        limit: Duration,
        allowed: Capabilities,
        stop: &AtomicBool,
    ) -> Result<String, String> {
        let deadline = Instant::now() + limit;
        self.deadline.set(Some(deadline));
        self.io.borrow_mut().begin(deadline, allowed);
        let result = self.context.with(|ctx| {
            let outcome = (|| {
                let host = self.host.clone().restore(&ctx)?;
                let function: Function = host.get("transform")?;
                let promise: Promise = function.call((id, text, context))?;
                self.settle(&ctx, &promise, deadline, limit, stop)
            })();
            let result = match outcome.catch(&ctx) {
                Ok(settled) => settled,
                Err(error) => Err(describe(error, &self.deadline, limit)),
            };
            // Leave no stray work from this call for the next one.
            while ctx.execute_pending_job() {}
            self.io.borrow_mut().end();
            result
        });
        self.deadline.set(None);
        result
    }

    /// Drives QuickJS's job queue and the host operations until `promise`
    /// settles, the deadline passes, or Hex stops.
    fn settle<'js>(
        &self,
        ctx: &Ctx<'js>,
        promise: &Promise<'js>,
        deadline: Instant,
        limit: Duration,
        stop: &AtomicBool,
    ) -> rquickjs::Result<Result<String, String>> {
        loop {
            while ctx.execute_pending_job() {}
            if promise.state() != PromiseState::Pending {
                return promise.finish::<String>().map(Ok);
            }
            if Instant::now() >= deadline {
                return Ok(Err(timed_out(limit)));
            }
            if stop.load(Ordering::Acquire) {
                return Ok(Err("Hex stopped before the transformation finished".into()));
            }
            let wake = self.io.borrow_mut().wait(deadline);
            match wake {
                Wake::Idle => {
                    return Ok(Err(
                        "a promise never settled, and no request, program, or timer was left to settle it"
                            .into(),
                    ));
                }
                Wake::Settled(settled) => {
                    for operation in settled {
                        operation.deliver(ctx)?;
                    }
                }
            }
        }
    }
}

/// The modules a load read, for the trust prompt's hash and scan.
#[derive(Default)]
struct Record {
    /// Each workspace file's name and text, in load order.
    sources: Vec<(String, String)>,
    /// A module imported `node:child_process`.
    child_process: bool,
    /// The load finished; later imports are refused.
    sealed: bool,
}

fn evaluate<'js>(
    ctx: &Ctx<'js>,
    config: &str,
    io: &io::Shared,
) -> rquickjs::Result<(Persistent<Object<'static>>, String)> {
    ctx.globals().set(
        "__hexLog",
        Function::new(ctx.clone(), |level: String, message: String| {
            log(&level, &message);
        })?,
    )?;
    ctx.eval::<(), _>(CONSOLE_JS)?;
    ctx.globals().set("__hexIo", io::bindings(ctx, io)?)?;
    let (_, promise) = Module::declare(ctx.clone(), IO, IO_JS)?.eval()?;
    promise.finish::<()>()?;
    let (_, promise) = Module::declare(ctx.clone(), WEB, WEB_JS)?.eval()?;
    promise.finish::<()>()?;
    let entry = format!(
        "import config from {config:?}\nimport {{ install }} from {HOST:?}\nexport const host = install(config)\n",
        config = format!("./{config}"),
    );
    let (module, promise) = Module::declare(ctx.clone(), ENTRY, entry)?.eval()?;
    promise.finish::<()>()?;
    let host: Object = module.get("host")?;
    let registration: String = host.get("registration")?;
    Ok((Persistent::save(ctx, host), registration))
}

fn log(level: &str, message: &str) {
    let message = &message[..message.floor_char_boundary(MAX_LOG_BYTES)];
    match level {
        "error" => tracing::error!(target: "hex_config", "{message}"),
        "warn" => tracing::warn!(target: "hex_config", "{message}"),
        "debug" => tracing::debug!(target: "hex_config", "{message}"),
        _ => tracing::info!(target: "hex_config", "{message}"),
    }
}

/// A script failure as the Modes pane shows it: the error and where it
/// happened, or the time limit it ran past.
fn describe(error: CaughtError<'_>, deadline: &Cell<Option<Instant>>, limit: Duration) -> String {
    if deadline
        .get()
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return timed_out(limit);
    }
    match error {
        CaughtError::Exception(exception) => {
            let name = exception
                .as_object()
                .get::<_, String>("name")
                .unwrap_or_else(|_| "Error".into());
            let mut text = format!("{name}: {}", exception.message().unwrap_or_default());
            if let Some(stack) = exception.stack() {
                for frame in stack.lines().map(str::trim).filter(|frame| {
                    !frame.is_empty() && !frame.contains(HOST) && !frame.contains(ENTRY)
                }) {
                    text.push_str("\n  ");
                    text.push_str(frame);
                }
            }
            text
        }
        // QuickJS throws null when it cannot even allocate its out-of-memory
        // error.
        CaughtError::Value(value) if value.is_null() => "ran out of memory (or threw null)".into(),
        CaughtError::Value(value) => format!(
            "threw {}",
            value
                .as_string()
                .and_then(|text| text.to_string().ok())
                .unwrap_or_else(|| format!("a {}", value.type_name()))
        ),
        CaughtError::Error(error) => error.to_string(),
    }
}

fn timed_out(limit: Duration) -> String {
    format!("timed out after {} ms", limit.as_millis())
}

/// Resolves and loads modules for one workspace: `@hex/commands`, and files
/// inside the workspace by relative path. Module names are paths relative to
/// the workspace, so errors read `hex.config.ts:12:5`.
struct WorkspaceLoader {
    root: PathBuf,
    record: Rc<RefCell<Record>>,
}

impl WorkspaceLoader {
    fn resolve_file(&self, base: &str, name: &str) -> Result<String, String> {
        let parent = if base == ENTRY {
            Path::new("")
        } else {
            Path::new(base).parent().unwrap_or(Path::new(""))
        };
        let mut relative = PathBuf::new();
        for component in parent.join(name).components() {
            match component {
                Component::Normal(part) => relative.push(part),
                Component::CurDir => {}
                Component::ParentDir if relative.pop() => {}
                _ => return Err("only files inside the config folder can be imported".into()),
            }
        }
        let candidates = ["", ".ts", ".js", ".mjs"].map(|extension| {
            let mut candidate = relative.clone().into_os_string();
            candidate.push(extension);
            PathBuf::from(candidate)
        });
        let found = candidates
            .into_iter()
            .find(|candidate| self.root.join(candidate).is_file())
            .ok_or_else(|| "file not found".to_string())?;
        let real = self
            .root
            .join(&found)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !real.starts_with(&self.root) {
            return Err("only files inside the config folder can be imported".into());
        }
        let parts = found
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>();
        Ok(parts.join("/"))
    }

    fn source(&self, name: &str) -> Result<String, SourceError> {
        let path = self.root.join(name);
        let read = |error: std::io::Error| SourceError::Read(error.to_string());
        if fs::metadata(&path).map_err(read)?.len() > MAX_SOURCE_BYTES {
            return Err(SourceError::Read(format!(
                "larger than {MAX_SOURCE_BYTES} bytes"
            )));
        }
        let source = fs::read_to_string(&path).map_err(read)?;
        self.record
            .borrow_mut()
            .sources
            .push((name.to_owned(), source.clone()));
        if name.ends_with(".json") {
            serde_json::from_str::<serde_json::Value>(&source).map_err(|error| {
                SourceError::Syntax(format!("{}:{}: {error}", error.line(), error.column()))
            })?;
            Ok(format!("export default {source}"))
        } else if name.ends_with(".ts") || name.ends_with(".mts") {
            super::typescript::strip(&source).map_err(SourceError::Syntax)
        } else {
            Ok(source)
        }
    }
}

enum SourceError {
    Read(String),
    /// `line:column: message`.
    Syntax(String),
}

impl Resolver for WorkspaceLoader {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let refuse = |message: &str| {
            Err(Exception::throw_message(
                ctx,
                &format!("{base}: cannot import \"{name}\": {message}"),
            ))
        };
        if self.record.borrow().sealed {
            return refuse("modules load only with the config; import it at the top of a file");
        }
        match name {
            COMMANDS => Ok(name.into()),
            HOST if base == ENTRY => Ok(name.into()),
            IO if base == WEB || base == CHILD_PROCESS => Ok(name.into()),
            CHILD_PROCESS | "child_process" => {
                self.record.borrow_mut().child_process = true;
                Ok(CHILD_PROCESS.into())
            }
            UTIL | "util" => Ok(UTIL.into()),
            "@hex/commands/effect" => {
                refuse("Effect handlers are not supported; use plain functions")
            }
            _ if name.starts_with("./") || name.starts_with("../") => self
                .resolve_file(base, name)
                .or_else(|message| refuse(&message)),
            _ if name.starts_with("node:") => refuse(
                "Hex provides only node:child_process (exec, execFile) and node:util (promisify)",
            ),
            _ => refuse(
                "only @hex/commands, node:child_process, node:util, and relative imports are available",
            ),
        }
    }
}

impl Loader for WorkspaceLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let source = match name {
            COMMANDS => COMMANDS_JS.to_owned(),
            HOST => HOST_JS.to_owned(),
            CHILD_PROCESS => CHILD_PROCESS_JS.to_owned(),
            UTIL => UTIL_JS.to_owned(),
            _ => self.source(name).map_err(|error| match error {
                SourceError::Read(message) => {
                    Exception::throw_message(ctx, &format!("{name}: {message}"))
                }
                SourceError::Syntax(message) => {
                    Exception::throw_syntax(ctx, &format!("{name}:{message}"))
                }
            })?,
        };
        Module::declare(ctx.clone(), name, source)
    }
}
