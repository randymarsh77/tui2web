//! Native-testable application contract and the version 1 JSON WASM adapter.
use crate::{
    fs::{MemoryFilesystem, Snapshot},
    WebBackend,
};
use ratatui::{layout::Rect, Frame, Terminal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;
pub type AppResult<T> = Result<T, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Input {
    Key {
        key: String,
        code: String,
        repeat: bool,
        modifiers: Modifiers,
    },
    Text {
        text: String,
    },
    Paste {
        text: String,
    },
    Mouse {
        kind: MouseKind,
        column: u16,
        row: u16,
        button: i16,
        modifiers: Modifiers,
    },
    Focus {
        focused: bool,
    },
    Tick,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseKind {
    Down,
    Up,
    Move,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Init {
    pub version: u32,
    pub columns: u16,
    pub rows: u16,
    pub now_ms: f64,
    pub random_seed: u32,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    pub snapshot: Option<Snapshot>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    Event {
        input: Input,
        #[serde(rename = "nowMs")]
        now_ms: f64,
    },
    Resize {
        columns: u16,
        rows: u16,
    },
    Snapshot,
    Shutdown,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Output {
    pub version: u32,
    pub frame: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub exited: bool,
    /// Relative delay to one host tick; None cancels a previously requested tick.
    pub wake_after_ms: Option<u32>,
}

/// Return dirty only when the visual state changed. Host timers are opt-in, never a frame loop.
#[derive(Default)]
pub struct Update {
    pub dirty: bool,
    pub files_changed: bool,
    pub exit: bool,
    pub wake_after_ms: Option<u32>,
}
impl Update {
    pub fn render() -> Self {
        Self {
            dirty: true,
            ..Self::default()
        }
    }
}
pub struct Context {
    pub fs: MemoryFilesystem,
    pub now_ms: f64,
    pub columns: u16,
    pub rows: u16,
    pub config: BTreeMap<String, String>,
    random_state: u32,
}
impl Context {
    /// Reproducible non-cryptographic randomness seeded by the browser's crypto API.
    pub fn random_u32(&mut self) -> u32 {
        let mut x = self.random_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random_state = x;
        x
    }
}
pub trait Application: Sized {
    /// Seed fixtures only if the supplied filesystem is empty; restore is applied before this call.
    fn init(context: &mut Context) -> AppResult<Self>;
    fn update(&mut self, input: Input, context: &mut Context) -> AppResult<Update>;
    fn render(&self, frame: &mut Frame, context: &Context);
    /// Optionally schedule the first tick without waiting for user input.
    fn initial_wake_after_ms(&self) -> Option<u32> {
        None
    }
    fn shutdown(&mut self, _context: &mut Context) -> AppResult<()> {
        Ok(())
    }
}

pub struct Runner<A: Application> {
    app: A,
    context: Context,
    terminal: Terminal<WebBackend>,
    exited: bool,
}
fn dimensions(columns: u16, rows: u16) -> AppResult<()> {
    if columns == 0 || rows == 0 || columns > 300 || rows > 120 {
        Err("terminal dimensions must be 1..300 columns and 1..120 rows".into())
    } else {
        Ok(())
    }
}
impl<A: Application> Runner<A> {
    pub fn new(init: Init) -> AppResult<Self> {
        if init.version != PROTOCOL_VERSION {
            return Err("unsupported protocol version".into());
        }
        dimensions(init.columns, init.rows)?;
        if !init.now_ms.is_finite() {
            return Err("invalid host time".into());
        }
        let mut context = Context {
            fs: MemoryFilesystem::new(),
            now_ms: init.now_ms,
            config: init.config,
            columns: init.columns,
            rows: init.rows,
            random_state: init.random_seed.max(1),
        };
        if let Some(snapshot) = init.snapshot {
            context.fs.restore(snapshot).map_err(|e| e.to_string())?;
        }
        let app = A::init(&mut context)?;
        let terminal =
            Terminal::new(WebBackend::new(init.columns, init.rows)).map_err(|e| e.to_string())?;
        Ok(Self {
            app,
            context,
            terminal,
            exited: false,
        })
    }
    pub fn initial_output(&mut self) -> AppResult<Output> {
        self.output(Update {
            dirty: true,
            files_changed: true,
            wake_after_ms: self.app.initial_wake_after_ms(),
            ..Update::default()
        })
    }
    fn output(&mut self, update: Update) -> AppResult<Output> {
        if update.exit {
            self.app.shutdown(&mut self.context)?;
        }
        let frame = if update.dirty {
            let app = &self.app;
            let context = &self.context;
            self.terminal
                .draw(|frame| app.render(frame, context))
                .map_err(|e| e.to_string())?;
            Some(self.terminal.backend().get_ansi_output().to_owned())
        } else {
            None
        };
        self.exited |= update.exit;
        Ok(Output {
            version: PROTOCOL_VERSION,
            frame,
            snapshot: (update.files_changed || update.exit).then(|| self.context.fs.snapshot()),
            exited: self.exited,
            wake_after_ms: update.wake_after_ms,
        })
    }
    pub fn dispatch(&mut self, command: Command) -> AppResult<Output> {
        if self.exited {
            return Err("application has exited".into());
        }
        let update = match command {
            Command::Event { input, now_ms } => {
                if !now_ms.is_finite() {
                    return Err("invalid host time".into());
                }
                if matches!(&input, Input::Text { text } | Input::Paste { text } if text.len() > 65536)
                {
                    return Err("input exceeds 64 KiB".into());
                }
                self.context.now_ms = now_ms;
                self.app.update(input, &mut self.context)?
            }
            Command::Resize { columns, rows } => {
                dimensions(columns, rows)?;
                self.context.columns = columns;
                self.context.rows = rows;
                self.terminal.backend_mut().resize(columns, rows);
                self.terminal
                    .resize(Rect::new(0, 0, columns, rows))
                    .map_err(|e| e.to_string())?;
                Update::render()
            }
            Command::Snapshot => Update {
                files_changed: true,
                ..Update::default()
            },
            Command::Shutdown => Update {
                exit: true,
                files_changed: true,
                ..Update::default()
            },
        };
        self.output(update)
    }
    pub fn from_json(json: &str) -> AppResult<Self> {
        Self::new(serde_json::from_str(json).map_err(|e| e.to_string())?)
    }
    pub fn initial_json(&mut self) -> AppResult<String> {
        serde_json::to_string(&self.initial_output()?).map_err(|e| e.to_string())
    }
    pub fn dispatch_json(&mut self, json: &str) -> AppResult<String> {
        let output = self.dispatch(serde_json::from_str(json).map_err(|e| e.to_string())?)?;
        serde_json::to_string(&output).map_err(|e| e.to_string())
    }
}

/// Export the stable `App` constructor, `start`, `dispatch`, and wasm-bindgen `free`.
/// The consuming crate must depend on wasm-bindgen as required by its attribute macro.
#[cfg(feature = "wasm")]
#[macro_export]
macro_rules! export_app {
    ($application:ty) => {
        #[wasm_bindgen::prelude::wasm_bindgen]
        pub struct App {
            runner: $crate::app::Runner<$application>,
        }
        #[wasm_bindgen::prelude::wasm_bindgen]
        impl App {
            #[wasm_bindgen(constructor)]
            pub fn new(init: &str) -> Result<App, wasm_bindgen::JsValue> {
                $crate::app::Runner::from_json(init)
                    .map(|runner| App { runner })
                    .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
            }
            pub fn start(&mut self) -> Result<String, wasm_bindgen::JsValue> {
                self.runner
                    .initial_json()
                    .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
            }
            pub fn dispatch(&mut self, command: &str) -> Result<String, wasm_bindgen::JsValue> {
                self.runner
                    .dispatch_json(command)
                    .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
            }
        }
    };
}
