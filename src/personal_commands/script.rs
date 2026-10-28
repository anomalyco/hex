//! One loaded config in an embedded QuickJS runtime: the module graph from
//! `hex.config.ts` (types stripped by [`super::typescript`]), compiled once,
//! and the handler and transformation calls Hex makes into it. Scripts get
//! plain JavaScript and the `@hex/commands` module: no filesystem, network,
//! process, or timer API. Every load and call runs under a time limit and the
//! runtime under a memory limit.

use std::cell::Cell;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Error, Exception, Function, Module, Object,
    Persistent, Promise, Runtime,
};

const MEMORY_LIMIT: usize = 64 << 20;
const STACK_LIMIT: usize = 1 << 20;
/// The stack of the thread that owns a script: QuickJS's own limit plus room
/// for Hex's frames.
pub const THREAD_STACK_SIZE: usize = 4 << 20;
const LOAD_TIME_LIMIT: Duration = Duration::from_secs(2);
pub const CALL_TIME_LIMIT: Duration = Duration::from_secs(1);
const MAX_SOURCE_BYTES: u64 = 1 << 20;
const MAX_LOG_BYTES: usize = 4096;
const ENTRY: &str = "hex:entry";
const HOST: &str = "hex:host";
const COMMANDS: &str = "@hex/commands";
const COMMANDS_JS: &str = include_str!("commands.js");
const HOST_JS: &str = include_str!("host.js");
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
    // Dropped before the context and runtime that own its value.
    host: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    deadline: Rc<Cell<Option<Instant>>>,
    registration: String,
}

impl Script {
    /// Loads `config` (a file name in `workspace`) and evaluates it.
    pub fn load(workspace: &Path, config: &str) -> Result<Self, String> {
        let root = workspace
            .canonicalize()
            .map_err(|error| format!("{}: {error}", workspace.display()))?;
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
        runtime.set_loader(
            WorkspaceLoader { root: root.clone() },
            WorkspaceLoader { root },
        );
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;
        deadline.set(Some(Instant::now() + LOAD_TIME_LIMIT));
        let loaded = context.with(|ctx| {
            evaluate(&ctx, config)
                .catch(&ctx)
                .map_err(|error| describe(error, &deadline, LOAD_TIME_LIMIT))
        });
        deadline.set(None);
        let (host, registration) = loaded?;
        Ok(Self {
            host,
            context,
            _runtime: runtime,
            deadline,
            registration,
        })
    }

    /// The config's registration as JSON.
    pub fn registration(&self) -> &str {
        &self.registration
    }

    /// Runs command `id`'s handler and returns the actions it asked for as JSON.
    pub fn invoke(&self, id: &str, context: &str, captures: &str) -> Result<String, String> {
        self.call(
            "invoke",
            (id.to_owned(), context.to_owned(), captures.to_owned()),
        )
    }

    /// Runs the transformations `ids` (a JSON array) over `text` in order.
    pub fn transform(&self, ids: &str, text: &str, context: &str) -> Result<String, String> {
        self.call(
            "transform",
            (ids.to_owned(), text.to_owned(), context.to_owned()),
        )
    }

    fn call(&self, method: &str, arguments: (String, String, String)) -> Result<String, String> {
        self.deadline.set(Some(Instant::now() + CALL_TIME_LIMIT));
        let result = self.context.with(|ctx| {
            let outcome = (|| {
                let host = self.host.clone().restore(&ctx)?;
                let function: Function = host.get(method)?;
                let promise: Promise = function.call(arguments)?;
                let output = promise.finish::<String>();
                // Leave no stray work from this call for the next one.
                while ctx.execute_pending_job() {}
                output
            })();
            outcome
                .catch(&ctx)
                .map_err(|error| describe(error, &self.deadline, CALL_TIME_LIMIT))
        });
        self.deadline.set(None);
        result
    }
}

fn evaluate<'js>(
    ctx: &Ctx<'js>,
    config: &str,
) -> rquickjs::Result<(Persistent<Object<'static>>, String)> {
    ctx.globals().set(
        "__hexLog",
        Function::new(ctx.clone(), |level: String, message: String| {
            log(&level, &message);
        })?,
    )?;
    ctx.eval::<(), _>(CONSOLE_JS)?;
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

/// A script failure as the Commands and Modes panes show it: the error and
/// where it happened, or the time limit it ran past.
fn describe(error: CaughtError<'_>, deadline: &Cell<Option<Instant>>, limit: Duration) -> String {
    if deadline
        .get()
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return format!("timed out after {} ms", limit.as_millis());
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
        CaughtError::Error(Error::WouldBlock) => {
            "a promise never settled; scripts have no timers or I/O to wait for".into()
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

/// Resolves and loads modules for one workspace: `@hex/commands`, and files
/// inside the workspace by relative path. Module names are paths relative to
/// the workspace, so errors read `hex.config.ts:12:5`.
struct WorkspaceLoader {
    root: PathBuf,
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
        match name {
            COMMANDS => Ok(name.into()),
            HOST if base == ENTRY => Ok(name.into()),
            "@hex/commands/effect" => {
                refuse("Effect handlers are not supported; use plain functions")
            }
            _ if name.starts_with("./") || name.starts_with("../") => self
                .resolve_file(base, name)
                .or_else(|message| refuse(&message)),
            _ => refuse("only @hex/commands and relative imports are available"),
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
