//! Terralite's RoseGold host.
//!
//! `.rg` files register with `events.*` (Fabric-style names) or the older
//! `hooks.*` names. While a hook runs, `world`, `player`, and `item` ask the
//! game about cells, the local player, and pack items. The language runtime
//! lives in RoseGold. This crate is the piece the game calls.
//!
//! `events.break` and `events.break_block` are `fn(block: String, x: Int, y: Int, z: Int): Bool`.
//! False cancels the break. `hooks.before_break` is the older form, `fn(block: String): Bool`,
//! and it can cancel the same break. `events.place` and `events.place_block` are
//! `fn(item: String, x: Int, y: Int, z: Int): Bool`.
//! False cancels the place. `events.broken` is
//! `fn(block: String, x: Int, y: Int, z: Int, left: String)`. `left` is the block
//! now in the cell. `events.placed` is `fn(item: String, x: Int, y: Int, z: Int)`.
//! `events.select_slot` is `fn(slot: Int, item: String)`. `events.open_screen` is
//! `fn(screen: String, open: Bool)`. `events.break_speed` is `fn(block: String): Float`
//! (`0.5` breaks in half the time). `events.pickup_item` is `fn(item: String): Bool`.
//! False leaves the drop on the ground. `events.break_drop` is
//! `fn(block: String, left: String): String`, the item id to drop, or `""` for none.
//! `events.loot` is `fn(item: String, x: Int, z: Int, omen: Bool): String`. It runs
//! when a trial key opens a copper chest. `item` is the datapack pick. The
//! returned id replaces it. `""` or an unknown id keeps the datapack pick.
//!
//! `block.register(namespace, path)` and `item.register(namespace, path)` run in
//! `main` and add `namespace:path`. Each returns that id. `block.texture(id, path)`
//! attaches a pack PNG (or another block's texture). `block.model(id, model)`
//! uses a pack shape or model; the default is `cube`. A script block places from
//! the hotbar, and `world.set_block` writes it. A script item is not a block.
//! `events.entity_spawn` is `fn(kind: String, x: Int, y: Int, z: Int)`
//! and runs once for `"player"`, then for the demo `"pig"` and `"zombie"`.
//! `events.entity_hurt` is `fn(kind: String, amount: Int): Bool`.
//! `events.entity_tick` is `fn(kind: String)`. The client and the authority tick
//! `"player"`, `"sheep"`, `"pig"`, and `"zombie"` every 50 ms. `events.tick` is
//! `fn(n: Int)`. It runs on every 20th of those steps, once a second in a steady
//! game, and at most once when a catch-up is capped. `n` starts at 1. A hook may
//! `world.set_block`. The client
//! and the server each count their own steps. `events.dawn` is `fn(day: Int)`.
//! It runs when the sky crosses dawn, a quarter of the way through the day, and
//! when a bed skips to morning. `day` is the day of the four-day year, `0`
//! spring through `3` winter. A step that skips several mornings reports one
//! crossing. `events.dusk` is `fn(day: Int)`. It runs when the sky crosses
//! dusk, three quarters of the way through the day. `day` is the same four-day
//! count. A bed skips the night and does not fire it. `/day` and `/night` set
//! the clock without either hook. `events.jump` is
//! `fn(x: Int, y: Int, z: Int): Bool`. It runs when Space would launch off the
//! ground. `x`, `y`, and `z` are the cell the feet are in. False keeps the
//! player down for that press. A swim and a climb do not ask. One held press
//! asks once, and the next landing asks again. `events.sprint` is
//! `fn(x: Int, y: Int, z: Int): Bool`. It runs when Ctrl and a walk key are
//! both held, which is the fast walk. `x`, `y`, and `z` are the cell the feet
//! are in. False keeps that hold at a normal walk. Standing still with Ctrl
//! does not ask. One hold asks once. `events.land` is
//! `fn(x: Int, y: Int, z: Int, drop: Int)`. It runs when the player touches the
//! ground after leaving it, including a jump. `x`, `y`, and `z` are the cell
//! the feet are in. `drop` is the fall in whole blocks, and a hop under one
//! block is `0`. A swim that never touches the floor does not land, and neither
//! does a respawn. A fall of four blocks or more still calls `entity_hurt` for
//! the player after this hook. `events.use_block` is
//! `fn(item: String, block: String, x: Int, y: Int, z: Int): Bool`. It runs when a
//! right click looks at a block. False consumes the click. `events.use_item` is
//! `fn(item: String): Bool`. It runs when the block hook did not consume the click.
//! False skips eating, placing, and the other built-in uses. `events.use_entity` is
//! `fn(item: String, kind: String): Bool`. It runs first when the look hits a mob
//! (`wolf`, `pig`, `zombie`, `breeze`, `fox`, `frog`, `parrot`, `cat`, `boat`).
//! False skips that mob's feed, saddle, and mace swing, and the block and item
//! uses of the same click. `events.attack` is `fn(item: String, target: String): Bool`.
//! It runs on a left click before mining starts, a dragon egg hops, or an end
//! crystal blasts. `target` is the aimed mob, or the looked-at block id when no
//! mob is on the ray. False consumes the click, so the break and the blast
//! damage do not happen. `events.chat` is `fn(line: String): Bool`. It runs for
//! an ordinary chat line, after slash commands have been tried. False swallows
//! the line. `events.respawn` is `fn(cause: String, x: Int, y: Int, z: Int): Bool`.
//! It runs for a death (`death`), a bed (`bed`), and a charged anchor (`anchor`).
//! `x, y, z` is the cell the player would come back to. False sends a death to
//! the stairs, keeps the night from ending, or leaves the saved spawn unchanged.
//! `events.craft` is `fn(result: String): Bool`. It runs when a result is taken
//! from the crafting grid, before the ingredients are spent. False leaves them
//! in the grid. `events.smelt` is `fn(result: String): Bool`. It runs when output
//! is taken from a furnace, a smoker, or a blast furnace. False leaves the item
//! in the output slot. `events.repair` is `fn(item: String): Bool`. It runs when
//! Enter on an anvil would spend one iron ore to refill a worn pick. `item` is
//! that pick. False leaves the ore and the worn pick alone. `events.enchant` is
//! `fn(item: String): Bool`. It runs when Enter on an enchanting table would
//! spend one bookshelf to put Efficiency on a plain pick. `item` is that pick.
//! False leaves the bookshelf and the plain pick alone. `events.brew` is
//! `fn(ingredient: String): Bool`. It runs when Enter on a brewing stand would
//! spend a water bucket and `nether_wart_block` or `flower` to make a healing
//! potion. False leaves both unspent. `events.smith` is `fn(item: String): Bool`.
//! It runs when Enter on a smithing table would spend iron ore to turn a stone
//! pick into iron or an iron pick into diamond, or a heavy core to set into a
//! mace. `item` is the held id. False leaves that payment unspent. `events.weave`
//! is `fn(wool: String): Bool`. It runs when Enter on a loom would spend one
//! `wool` to make a banner. False leaves that wool in the bar. Any of these hooks may
//! `world.set_block`. The first write is sent as the place when the click is consumed.
//! `events.command` is `fn(args: String): String`. The id is the chat name (`spark`
//! or `wand:spark`). Typing `/spark hi` or picking the row on the command list
//! calls it. The returned string is the chat reply. The same registration adds
//! a `COMMAND` row. `item.places(item, block)` makes that
//! item place the block from the hotbar. `recipe.register(result, ingredient)`
//! records one crafting line. `recipe.shapeless(result, "a+b")` records a pile.
//! `recipe.shaped(result, "a,,,a,,,a,,")` records a 3 by 3, nine cells separated
//! by commas, blanks empty. Both ids must already exist. The crafting table
//! shows the result. `entity.register(namespace, path)` adds a mob. It stands
//! just east of the oak stairs. `entity.spawn(id, x, y, z)` is `Bool`. It places
//! another body of an id this script registered, at that cell. False means the
//! id was not registered here or a body already stands in that cell.
//! `entity.despawn(id, x, y, z)` is `Bool`. It removes that spawned body. False
//! means nothing spawned stood there. The first body from `entity.register` stays.
//! `import ModAPI` is the Fabric-style entry (`builtin/std/TerraLite/ModAPI.rg`).
//! `on_init` runs after `main` when a script defines it. `TLMod.log` and each
//! `register` call are recorded.
//!
//! `world.get_block` / `world.block` is the id in that cell, or `""` outside.
//! `world.set_block` / `world.write` sets a pack block or a `block.register` id.
//! A script item is rejected.
//! `world.get_meta` / `world.set_meta` read and write the 0..15 metadata nibble.
//! `world.is_air` is false outside the world. `world.time_of_day` is `0` at
//! midnight and `-1` when the hook has no sky. `player.held` is the selected
//! item on the client. `item.exists` / `item.hardness` read the pack.
//!
//! ```text
//! fn allow(id: String, x: Int, y: Int, z: Int): Bool {
//!     return world.block(x, y - 1, z) != "stone";
//! }
//!
//! fn broke(id: String, x: Int, y: Int, z: Int, left: String) {
//!     print(id + " left " + left + " replaces " + world.replaces(id));
//! }
//!
//! fn main(): Int {
//!     hooks.before_place("no-place-on-stone", allow);
//!     hooks.after_break("log-break", broke);
//!     return 0;
//! }
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use rosegold_eval::{Session, Value};

pub use rosegold_eval::{GuiElement, GuiScreen, SettingOption};

/// One hook `main` registered. `slot` is the index [`Host::call`] uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub name: String,
    pub id: String,
    pub slot: usize,
}

/// Hooks and registry calls from one script's `main`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptLoad {
    pub hooks: Vec<Loaded>,
    /// `(namespace, path)` from `block.register`.
    pub blocks: Vec<(String, String)>,
    /// `(namespace, path)` from `item.register`.
    pub items: Vec<(String, String)>,
    /// `(namespace, path)` from `entity.register`.
    pub entities: Vec<(String, String)>,
    /// `(block id, png or block id)` from `block.texture`.
    pub block_textures: Vec<(String, String)>,
    /// `(block id, shape or model id)` from `block.model`.
    pub block_models: Vec<(String, String)>,
    /// `(item id, block id)` from `item.places`.
    pub item_places: Vec<(String, String)>,
    /// `(result id, ingredient id)` from `recipe.register`.
    pub recipes: Vec<(String, String)>,
    /// `(result id, "a+b")` from `recipe.shapeless`.
    pub shapeless: Vec<(String, String)>,
    /// `(result id, nine comma-separated cells)` from `recipe.shaped`.
    pub shaped: Vec<(String, String)>,
    /// `("note", message)` and `("registry", kind)` from `ModAPI`.
    pub api: Vec<(String, String)>,
    /// `(registry kind, id)` from `TLRegistry.register` when it is not a block or item.
    pub entries: Vec<(String, String)>,
    /// `(id, x, y, z)` still standing after `entity.spawn` and `entity.despawn`.
    pub spawns: Vec<(String, i32, i32, i32)>,
    /// Screens from `GUIAPI.createScreen`.
    pub screens: Vec<GuiScreen>,
    /// Rows from `SettingsAPI.createOption`.
    pub options: Vec<SettingOption>,
}

/// Arguments the game passes into a hook.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Text(String),
    Int(i32),
    Bool(bool),
}

/// What the hook returned, and the text it printed.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// True only when the function returned `false`.
    pub returned_false: bool,
    /// Set when the function returned a number. Break-speed hooks use this.
    pub number: Option<f64>,
    /// Set when the function returned a string. Break-drop hooks use this.
    pub text: Option<String>,
    pub printed: String,
    /// Screens this script holds after the call. A press may have added or removed a row.
    pub screens: Vec<GuiScreen>,
    /// Chat lines `chat.say` queued during the call. The game fills this after the hook.
    pub lines: Vec<String>,
}

/// A `world.*` question from a script.
#[derive(Clone, Debug, PartialEq)]
pub enum Query {
    Block {
        x: i32,
        y: i32,
        z: i32,
    },
    Hardness {
        id: String,
    },
    Light {
        id: String,
    },
    Replaces {
        id: String,
    },
    /// Place `id` in the cell. `air` clears it. An unknown id does not change the cell.
    Write {
        x: i32,
        y: i32,
        z: i32,
        id: String,
    },
    /// Hotbar slot `1..=10`. Empty is `""`.
    Slot {
        index: i32,
    },
    /// Stack count in hotbar slot `1..=10`.
    Count {
        index: i32,
    },
    IsAir {
        x: i32,
        y: i32,
        z: i32,
    },
    Meta {
        x: i32,
        y: i32,
        z: i32,
    },
    /// Metadata nibble `0..=15` on the block already in the cell.
    SetMeta {
        x: i32,
        y: i32,
        z: i32,
        meta: i32,
    },
    /// A chat line from `chat.say`. It does not change the world.
    Say {
        text: String,
    },
    /// The current value of a script settings row. Empty when the name is unknown.
    Setting {
        name: String,
    },
    Time,
    IsDay,
    Season,
    HasTime,
    Held,
    Selected,
    Present,
    PlayerX,
    PlayerY,
    PlayerZ,
    Exists {
        id: String,
    },
    MaxCount {
        id: String,
    },
}

/// The game's answer to a [`Query`].
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

enum Job {
    Load {
        source: String,
        path: String,
        sandbox: PathBuf,
        reply: Sender<Result<ScriptLoad, String>>,
    },
    Call {
        slot: usize,
        args: Vec<Arg>,
        back: Sender<FromScript>,
    },
    SetRow {
        slot: usize,
        row: String,
        text: String,
        reply: Sender<Result<(), String>>,
    },
}

enum FromScript {
    Query {
        query: Query,
        reply: Sender<Result<Answer, String>>,
    },
    Done {
        returned_false: bool,
        number: Option<f64>,
        text: Option<String>,
        printed: String,
        screens: Vec<GuiScreen>,
        error: Option<String>,
    },
}

/// One script thread. Clone it into each hook; the thread stays up while a clone does.
#[derive(Clone)]
pub struct Host {
    tx: Sender<Job>,
}

impl Host {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut sessions: Vec<Session> = Vec::new();
            let mut slots: Vec<(usize, usize)> = Vec::new();
            while let Ok(job) = rx.recv() {
                match job {
                    Job::Load {
                        source,
                        path,
                        sandbox,
                        reply,
                    } => {
                        let result = match Session::load(&source, &path, Some(&sandbox)) {
                            Ok(session) => {
                                let mut hooks = Vec::new();
                                let session_index = sessions.len();
                                for (hook_index, (name, id)) in
                                    session.hooks().into_iter().enumerate()
                                {
                                    let slot = slots.len();
                                    slots.push((session_index, hook_index));
                                    hooks.push(Loaded { name, id, slot });
                                }
                                let mut blocks = Vec::new();
                                let mut items = Vec::new();
                                let mut entities = Vec::new();
                                for (kind, namespace, path) in session.registrations() {
                                    let pair = (namespace, path);
                                    if kind == "block" {
                                        blocks.push(pair);
                                    } else if kind == "entity" {
                                        entities.push(pair);
                                    } else {
                                        items.push(pair);
                                    }
                                }
                                let mut block_textures = Vec::new();
                                let mut block_models = Vec::new();
                                let mut item_places = Vec::new();
                                let mut recipes = Vec::new();
                                let mut shapeless = Vec::new();
                                let mut shaped = Vec::new();
                                let mut api = Vec::new();
                                let mut entries = Vec::new();
                                let spawns = session.spawns();
                                let screens = session.screens();
                                let options = session.options();
                                for (kind, id, value) in session.attachments() {
                                    let pair = (id, value);
                                    match kind.as_str() {
                                        "texture" => block_textures.push(pair),
                                        "model" => block_models.push(pair),
                                        "places" => item_places.push(pair),
                                        "recipe" => recipes.push(pair),
                                        "shapeless" => shapeless.push(pair),
                                        "shaped" => shaped.push(pair),
                                        "note" | "registry" => api.push((kind, pair.0)),
                                        "entry" => entries.push(pair),
                                        _ => {}
                                    }
                                }
                                sessions.push(session);
                                Ok(ScriptLoad {
                                    hooks,
                                    blocks,
                                    items,
                                    entities,
                                    block_textures,
                                    block_models,
                                    item_places,
                                    recipes,
                                    shapeless,
                                    shaped,
                                    api,
                                    entries,
                                    spawns,
                                    screens,
                                    options,
                                })
                            }
                            Err(err) => Err(err),
                        };
                        let _ = reply.send(result);
                    }
                    Job::Call { slot, args, back } => {
                        let done = invoke(&mut sessions, &slots, slot, args, &back);
                        let _ = back.send(done);
                    }
                    Job::SetRow {
                        slot,
                        row,
                        text,
                        reply,
                    } => {
                        let done = slots
                            .get(slot)
                            .map(|(session_index, hook_index)| {
                                sessions[*session_index].set_row_text(*hook_index, &row, &text);
                            })
                            .ok_or_else(|| format!("hook {slot} is missing"));
                        let _ = reply.send(done);
                    }
                }
            }
        });
        Self { tx }
    }

    /// Check and run `source`. `sandbox` is the root for `std.io` paths.
    pub fn load(&self, source: &str, path: &str, sandbox: &Path) -> Result<ScriptLoad, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Job::Load {
                source: source.to_string(),
                path: path.to_string(),
                sandbox: sandbox.to_path_buf(),
                reply: reply_tx,
            })
            .map_err(|_| "script host stopped".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "script host stopped".to_string())?
    }

    /// Store a typed field line on the screen that owns `slot`, before its press runs.
    pub fn set_row_text(&self, slot: usize, row: &str, text: &str) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Job::SetRow {
                slot,
                row: row.to_string(),
                text: text.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "script host stopped".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "script host stopped".to_string())?
    }

    /// Run the hook at `slot`. `answer` runs on this thread, so it can read the world.
    ///
    /// `answer` must not call [`Host::call`]. The script thread is waiting for it.
    pub fn call(
        &self,
        slot: usize,
        args: &[Arg],
        answer: &mut dyn FnMut(Query) -> Result<Answer, String>,
    ) -> Result<Call, String> {
        let (back_tx, back_rx) = mpsc::channel();
        self.tx
            .send(Job::Call {
                slot,
                args: args.to_vec(),
                back: back_tx,
            })
            .map_err(|_| "script host stopped".to_string())?;
        loop {
            match back_rx
                .recv()
                .map_err(|_| "script host stopped".to_string())?
            {
                FromScript::Query { query, reply } => {
                    let _ = reply.send(answer(query));
                }
                FromScript::Done {
                    returned_false,
                    number,
                    text,
                    printed,
                    screens,
                    error,
                } => {
                    if let Some(error) = error {
                        return Err(error);
                    }
                    return Ok(Call {
                        returned_false,
                        number,
                        text,
                        printed,
                        screens,
                        lines: Vec::new(),
                    });
                }
            }
        }
    }
}

fn invoke(
    sessions: &mut [Session],
    slots: &[(usize, usize)],
    slot: usize,
    args: Vec<Arg>,
    back: &Sender<FromScript>,
) -> FromScript {
    let done = invoke_call(sessions, slots, slot, args, back);
    match done {
        Ok(call) => FromScript::Done {
            returned_false: call.returned_false,
            number: call.number,
            text: call.text,
            printed: call.printed,
            screens: call.screens,
            error: None,
        },
        Err(error) => FromScript::Done {
            returned_false: false,
            number: None,
            text: None,
            printed: String::new(),
            screens: Vec::new(),
            error: Some(error),
        },
    }
}

fn invoke_call(
    sessions: &mut [Session],
    slots: &[(usize, usize)],
    slot: usize,
    args: Vec<Arg>,
    back: &Sender<FromScript>,
) -> Result<Call, String> {
    let (session_index, hook_index) = *slots
        .get(slot)
        .ok_or_else(|| format!("hook {slot} is missing"))?;
    let back = back.clone();
    let mut host = move |name: &str, values: &[Value]| {
        let query = query_of(name, values)?;
        let (reply_tx, reply_rx) = mpsc::channel();
        back.send(FromScript::Query {
            query,
            reply: reply_tx,
        })
        .map_err(|_| "script host stopped".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "script host stopped".to_string())?
            .map(value_of)
    };
    let values = args.into_iter().map(value_of_arg).collect();
    let (value, printed) =
        sessions[session_index].call_hook_hosted(hook_index, values, &mut host)?;
    let screens = sessions[session_index].screens();
    Ok(Call {
        returned_false: matches!(value, Value::Bool(false)),
        number: match &value {
            Value::Float(n) => Some(*n),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        },
        text: match value {
            Value::String(text) => Some(text),
            _ => None,
        },
        printed,
        screens,
        lines: Vec::new(),
    })
}

fn value_of_arg(arg: Arg) -> Value {
    match arg {
        Arg::Text(text) => Value::String(text),
        Arg::Int(n) => Value::Int(i64::from(n)),
        Arg::Bool(value) => Value::Bool(value),
    }
}

fn value_of(answer: Answer) -> Value {
    match answer {
        Answer::Text(text) => Value::String(text),
        Answer::Int(n) => Value::Int(n),
        Answer::Float(n) => Value::Float(n),
        Answer::Bool(value) => Value::Bool(value),
    }
}

fn query_of(name: &str, args: &[Value]) -> Result<Query, String> {
    match name {
        "world.block" | "world.get_block" => Ok(Query::Block {
            x: int_at(args, 0)?,
            y: int_at(args, 1)?,
            z: int_at(args, 2)?,
        }),
        "world.hardness" | "item.hardness" => Ok(Query::Hardness {
            id: text_at(args, 0)?,
        }),
        "world.light" | "item.light" => Ok(Query::Light {
            id: text_at(args, 0)?,
        }),
        "world.replaces" | "item.replaces" => Ok(Query::Replaces {
            id: text_at(args, 0)?,
        }),
        "world.write" | "world.set_block" => Ok(Query::Write {
            x: int_at(args, 0)?,
            y: int_at(args, 1)?,
            z: int_at(args, 2)?,
            id: text_at(args, 3)?,
        }),
        "world.slot" | "player.stack" => Ok(Query::Slot {
            index: int_at(args, 0)?,
        }),
        "world.count" | "player.count" => Ok(Query::Count {
            index: int_at(args, 0)?,
        }),
        "world.is_air" => Ok(Query::IsAir {
            x: int_at(args, 0)?,
            y: int_at(args, 1)?,
            z: int_at(args, 2)?,
        }),
        "world.get_meta" => Ok(Query::Meta {
            x: int_at(args, 0)?,
            y: int_at(args, 1)?,
            z: int_at(args, 2)?,
        }),
        "world.set_meta" => Ok(Query::SetMeta {
            x: int_at(args, 0)?,
            y: int_at(args, 1)?,
            z: int_at(args, 2)?,
            meta: int_at(args, 3)?,
        }),
        "world.time_of_day" => Ok(Query::Time),
        "world.is_day" => Ok(Query::IsDay),
        "world.season" => Ok(Query::Season),
        "world.has_time" => Ok(Query::HasTime),
        "player.held" => Ok(Query::Held),
        "player.selected" => Ok(Query::Selected),
        "player.present" => Ok(Query::Present),
        "player.x" => Ok(Query::PlayerX),
        "player.y" => Ok(Query::PlayerY),
        "player.z" => Ok(Query::PlayerZ),
        "item.exists" => Ok(Query::Exists {
            id: text_at(args, 0)?,
        }),
        "item.max_count" => Ok(Query::MaxCount {
            id: text_at(args, 0)?,
        }),
        "chat.say" => Ok(Query::Say {
            text: text_at(args, 0)?,
        }),
        "settings.value" => Ok(Query::Setting {
            name: text_at(args, 0)?,
        }),
        other => Err(format!("unknown function {other}")),
    }
}

fn text_at(args: &[Value], index: usize) -> Result<String, String> {
    match args.get(index) {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err("host function expects String".into()),
    }
}

fn int_at(args: &[Value], index: usize) -> Result<i32, String> {
    match args.get(index) {
        Some(Value::Int(n)) => i32::try_from(*n).map_err(|_| "coordinate is out of range".into()),
        _ => Err("host function expects Int".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: &str = r#"
fn allow(id: String, x: Int, y: Int, z: Int): Bool {
    var under = world.block(x, y - 1, z);
    if under == "stone" {
        print("refused " + id);
        return false;
    }
    return true;
}

fn main(): Int {
    hooks.before_place("no-place-on-stone", allow);
    return 0;
}
"#;

    #[test]
    fn a_before_place_hook_can_ask_what_is_under_the_cell() {
        let host = Host::start();
        let loaded = host.load(PLACE, "mod.rg", Path::new(".")).unwrap();
        assert_eq!(loaded.hooks[0].name, "before_place");
        assert_eq!(loaded.hooks[0].id, "no-place-on-stone");
        let call = host
            .call(
                loaded.hooks[0].slot,
                &[
                    Arg::Text("dirt".into()),
                    Arg::Int(1),
                    Arg::Int(2),
                    Arg::Int(1),
                ],
                &mut |query| match query {
                    Query::Block { x, y, z } => {
                        assert_eq!((x, y, z), (1, 1, 1));
                        Ok(Answer::Text("stone".into()))
                    }
                    other => Err(format!("unexpected {other:?}")),
                },
            )
            .unwrap();
        assert!(call.returned_false);
        assert!(call.printed.contains("refused dirt"));
    }
}
