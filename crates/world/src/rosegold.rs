//! RoseGold mods on the hook list.
//!
//! Each `*.rg` file in a mods directory, and each `*.rg` file in a mod folder, is real RoseGold. `main` registers
//! hooks with `hooks.before_break`, `hooks.before_place`, `hooks.after_break`,
//! and `hooks.after_place`. A before-break or before-place function that
//! returns false cancels the edit. After-break and after-place run only when
//! the cell changes. Inside a hook, `world.block`, `world.hardness`,
//! `world.light`, and `world.replaces` read the pack and the world. A script
//! that fails to load is an error, and the server does not listen.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::cell::RefCell;

use rebuild_rosegold::{Answer, Arg, Host, Query};

use crate::chunk::Block;
use crate::cluster::Cluster;
use crate::hooks::{AfterBreak, AfterPlace, BeforeBreak, BeforePlace, BreakVerdict, Hooks};
use crate::pack::Pack;

/// One hook a script registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptHook {
    BeforeBreak {
        id: String,
    },
    BreakBlock {
        id: String,
    },
    BeforePlace {
        id: String,
    },
    AfterBreak {
        id: String,
    },
    AfterPlace {
        id: String,
    },
    /// Compound fill was placed into a host (`water` / `ember`).
    FillPlace {
        id: String,
    },
    /// Compound fill was cleared from a host.
    FillRemove {
        id: String,
    },
    OnSelect {
        id: String,
    },
    OnScreen {
        id: String,
    },
    BreakSpeed {
        id: String,
    },
    PickupItem {
        id: String,
    },
    BreakDrop {
        id: String,
    },
    Loot {
        id: String,
    },
    EntitySpawn {
        id: String,
    },
    EntityHurt {
        id: String,
    },
    EntityDeath {
        id: String,
    },
    EntityTick {
        id: String,
    },
    UseItem {
        id: String,
    },
    UseBlock {
        id: String,
    },
    UseEntity {
        id: String,
    },
    Attack {
        id: String,
    },
    Chat {
        id: String,
    },
    Respawn {
        id: String,
    },
    Craft {
        id: String,
    },
    Smelt {
        id: String,
    },
    Repair {
        id: String,
    },
    Enchant {
        id: String,
    },
    Brew {
        id: String,
    },
    Smith {
        id: String,
    },
    Weave {
        id: String,
    },
    Tick {
        id: String,
    },
    Dawn {
        id: String,
    },
    Dusk {
        id: String,
    },
    Jump {
        id: String,
    },
    Sprint {
        id: String,
    },
    Land {
        id: String,
    },
    Command {
        id: String,
    },
    /// Enter or a click on a GUIAPI row. `id` is the screen name.
    ScreenPress {
        id: String,
    },
    /// The player flipped a script settings row. `id` is the option name.
    SettingChange {
        id: String,
    },
}

impl ScriptHook {
    pub fn id(&self) -> &str {
        match self {
            Self::BeforeBreak { id }
            | Self::BreakBlock { id }
            | Self::BeforePlace { id }
            | Self::AfterBreak { id }
            | Self::AfterPlace { id }
            | Self::FillPlace { id }
            | Self::FillRemove { id }
            | Self::OnSelect { id }
            | Self::OnScreen { id }
            | Self::BreakSpeed { id }
            | Self::PickupItem { id }
            | Self::BreakDrop { id }
            | Self::Loot { id }
            | Self::EntitySpawn { id }
            | Self::EntityHurt { id }
            | Self::EntityDeath { id }
            | Self::EntityTick { id }
            | Self::UseItem { id }
            | Self::UseBlock { id }
            | Self::UseEntity { id }
            | Self::Attack { id }
            | Self::Chat { id }
            | Self::Respawn { id }
            | Self::Craft { id }
            | Self::Smelt { id }
            | Self::Repair { id }
            | Self::Enchant { id }
            | Self::Brew { id }
            | Self::Smith { id }
            | Self::Weave { id }
            | Self::Tick { id }
            | Self::Dawn { id }
            | Self::Dusk { id }
            | Self::Jump { id }
            | Self::Sprint { id }
            | Self::Land { id }
            | Self::Command { id }
            | Self::ScreenPress { id }
            | Self::SettingChange { id } => id,
        }
    }

    /// Log label, with a hyphen (`before-break`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::BeforeBreak { .. } => "before-break",
            Self::BreakBlock { .. } => "break-block",
            Self::BeforePlace { .. } => "before-place",
            Self::AfterBreak { .. } => "after-break",
            Self::AfterPlace { .. } => "after-place",
            Self::FillPlace { .. } => "fill-place",
            Self::FillRemove { .. } => "fill-remove",
            Self::OnSelect { .. } => "on-select",
            Self::OnScreen { .. } => "on-screen",
            Self::BreakSpeed { .. } => "break-speed",
            Self::PickupItem { .. } => "pickup-item",
            Self::BreakDrop { .. } => "break-drop",
            Self::Loot { .. } => "loot",
            Self::EntitySpawn { .. } => "entity-spawn",
            Self::EntityHurt { .. } => "entity-hurt",
            Self::EntityDeath { .. } => "entity-death",
            Self::EntityTick { .. } => "entity-tick",
            Self::UseItem { .. } => "use-item",
            Self::UseBlock { .. } => "use-block",
            Self::UseEntity { .. } => "use-entity",
            Self::Attack { .. } => "attack",
            Self::Chat { .. } => "chat",
            Self::Respawn { .. } => "respawn",
            Self::Craft { .. } => "craft",
            Self::Smelt { .. } => "smelt",
            Self::Repair { .. } => "repair",
            Self::Enchant { .. } => "enchant",
            Self::Brew { .. } => "brew",
            Self::Smith { .. } => "smith",
            Self::Weave { .. } => "weave",
            Self::Tick { .. } => "tick",
            Self::Dawn { .. } => "dawn",
            Self::Dusk { .. } => "dusk",
            Self::Jump { .. } => "jump",
            Self::Sprint { .. } => "sprint",
            Self::Land { .. } => "land",
            Self::Command { .. } => "command",
            Self::ScreenPress { .. } => "screen-press",
            Self::SettingChange { .. } => "setting-change",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::BeforeBreak { .. } => "before_break",
            Self::BreakBlock { .. } => "break_block",
            Self::BeforePlace { .. } => "before_place",
            Self::AfterBreak { .. } => "after_break",
            Self::AfterPlace { .. } => "after_place",
            Self::FillPlace { .. } => "fill_place",
            Self::FillRemove { .. } => "fill_remove",
            Self::OnSelect { .. } => "on_select",
            Self::OnScreen { .. } => "on_screen",
            Self::BreakSpeed { .. } => "break_speed",
            Self::PickupItem { .. } => "pickup_item",
            Self::BreakDrop { .. } => "break_drop",
            Self::Loot { .. } => "loot",
            Self::EntitySpawn { .. } => "entity_spawn",
            Self::EntityHurt { .. } => "entity_hurt",
            Self::EntityDeath { .. } => "entity_death",
            Self::EntityTick { .. } => "entity_tick",
            Self::UseItem { .. } => "use_item",
            Self::UseBlock { .. } => "use_block",
            Self::UseEntity { .. } => "use_entity",
            Self::Attack { .. } => "attack",
            Self::Chat { .. } => "chat",
            Self::Respawn { .. } => "respawn",
            Self::Craft { .. } => "craft",
            Self::Smelt { .. } => "smelt",
            Self::Repair { .. } => "repair",
            Self::Enchant { .. } => "enchant",
            Self::Brew { .. } => "brew",
            Self::Smith { .. } => "smith",
            Self::Weave { .. } => "weave",
            Self::Tick { .. } => "tick",
            Self::Dawn { .. } => "dawn",
            Self::Dusk { .. } => "dusk",
            Self::Jump { .. } => "jump",
            Self::Sprint { .. } => "sprint",
            Self::Land { .. } => "land",
            Self::Command { .. } => "command",
            Self::ScreenPress { .. } => "screen_press",
            Self::SettingChange { .. } => "setting_change",
        }
    }

    fn from_name(name: &str, id: String) -> Result<Self, String> {
        match name {
            "before_break" => Ok(Self::BeforeBreak { id }),
            "break_block" => Ok(Self::BreakBlock { id }),
            "before_place" => Ok(Self::BeforePlace { id }),
            "after_break" => Ok(Self::AfterBreak { id }),
            "after_place" => Ok(Self::AfterPlace { id }),
            "fill_place" => Ok(Self::FillPlace { id }),
            "fill_remove" => Ok(Self::FillRemove { id }),
            "on_select" => Ok(Self::OnSelect { id }),
            "on_screen" => Ok(Self::OnScreen { id }),
            "break_speed" => Ok(Self::BreakSpeed { id }),
            "pickup_item" => Ok(Self::PickupItem { id }),
            "break_drop" => Ok(Self::BreakDrop { id }),
            "loot" => Ok(Self::Loot { id }),
            "entity_spawn" => Ok(Self::EntitySpawn { id }),
            "entity_hurt" => Ok(Self::EntityHurt { id }),
            "entity_death" => Ok(Self::EntityDeath { id }),
            "entity_tick" => Ok(Self::EntityTick { id }),
            "use_item" => Ok(Self::UseItem { id }),
            "use_block" => Ok(Self::UseBlock { id }),
            "use_entity" => Ok(Self::UseEntity { id }),
            "attack" => Ok(Self::Attack { id }),
            "chat" => Ok(Self::Chat { id }),
            "respawn" => Ok(Self::Respawn { id }),
            "craft" => Ok(Self::Craft { id }),
            "smelt" => Ok(Self::Smelt { id }),
            "repair" => Ok(Self::Repair { id }),
            "enchant" => Ok(Self::Enchant { id }),
            "brew" => Ok(Self::Brew { id }),
            "smith" => Ok(Self::Smith { id }),
            "weave" => Ok(Self::Weave { id }),
            "tick" => Ok(Self::Tick { id }),
            "dawn" => Ok(Self::Dawn { id }),
            "dusk" => Ok(Self::Dusk { id }),
            "jump" => Ok(Self::Jump { id }),
            "sprint" => Ok(Self::Sprint { id }),
            "land" => Ok(Self::Land { id }),
            "command" => Ok(Self::Command { id }),
            "screen_press" => Ok(Self::ScreenPress { id }),
            "setting_change" => Ok(Self::SettingChange { id }),
            other => Err(format!("unknown hook {other}")),
        }
    }
}

/// What the game knows about the player while one hook runs.
///
/// Authority break, place, and spawn hooks see the hotbar from the last hotbar
/// packet, with no eye and no sky. Client hooks pass the hotbar, the eye, and
/// the clock. [`ScriptView::bare`] is the empty view.
#[derive(Clone, Copy)]
pub struct ScriptView<'a> {
    pub slots: &'a [(String, i32)],
    /// Selected hotbar slot, 1-based. `0` when this hook has no player.
    pub selected: i32,
    pub eye: Option<[f32; 3]>,
    /// `0` midnight, `0.5` noon. `None` when this hook has no sky.
    pub time: Option<f32>,
    /// Season label from the sky, such as `Spring`. Empty when there is no sky.
    pub season: &'a str,
}

impl ScriptView<'static> {
    /// Authority hooks. `player.present` and `world.has_time` are false.
    pub fn bare() -> Self {
        Self {
            slots: &[],
            selected: 0,
            eye: None,
            time: None,
            season: "",
        }
    }
}

/// Script hooks registered on one [`Hooks`].
pub(crate) struct ScriptTable {
    before_break: Vec<ScriptSlot>,
    break_block: Vec<ScriptSlot>,
    before_place: Vec<ScriptSlot>,
    after_break: Vec<ScriptSlot>,
    after_place: Vec<ScriptSlot>,
    fill_place: Vec<ScriptSlot>,
    fill_remove: Vec<ScriptSlot>,
    on_select: Vec<ScriptSlot>,
    on_screen: Vec<ScriptSlot>,
    break_speed: Vec<ScriptSlot>,
    pickup_item: Vec<ScriptSlot>,
    break_drop: Vec<ScriptSlot>,
    loot: Vec<ScriptSlot>,
    entity_spawn: Vec<ScriptSlot>,
    entity_hurt: Vec<ScriptSlot>,
    entity_death: Vec<ScriptSlot>,
    entity_tick: Vec<ScriptSlot>,
    use_item: Vec<ScriptSlot>,
    use_block: Vec<ScriptSlot>,
    use_entity: Vec<ScriptSlot>,
    attack: Vec<ScriptSlot>,
    chat: Vec<ScriptSlot>,
    respawn: Vec<ScriptSlot>,
    craft: Vec<ScriptSlot>,
    smelt: Vec<ScriptSlot>,
    repair: Vec<ScriptSlot>,
    enchant: Vec<ScriptSlot>,
    brew: Vec<ScriptSlot>,
    smith: Vec<ScriptSlot>,
    weave: Vec<ScriptSlot>,
    tick: Vec<ScriptSlot>,
    dawn: Vec<ScriptSlot>,
    dusk: Vec<ScriptSlot>,
    jump: Vec<ScriptSlot>,
    sprint: Vec<ScriptSlot>,
    land: Vec<ScriptSlot>,
    command: Vec<ScriptSlot>,
    screen_press: Vec<ScriptSlot>,
    setting_change: Vec<ScriptSlot>,
    /// `("note", message)` and `("registry", kind)` from `ModAPI`.
    api: Vec<(String, String)>,
    /// `(registry kind, id)` from `TLRegistry.register` besides blocks and items.
    entries: Vec<(String, String)>,
    /// Mob ids from `entity.register` and `ENTITY` registry entries.
    mobs: Vec<String>,
    writes: std::cell::RefCell<Vec<crate::changes::CellChange>>,
    /// Live bodies from hook-time `entity.spawn` / `entity.despawn`.
    /// `Ok` places, `Err` removes.
    live_spawns: std::cell::RefCell<Vec<Result<(String, i32, i32, i32), (String, i32, i32, i32)>>>,
    /// Lines from `chat.say` during the hook that is running.
    lines: std::cell::RefCell<Vec<String>>,
    /// GUIAPI screen names `gui.open` asked for during the hook.
    pending_screens: std::cell::RefCell<Vec<String>>,
    /// Hotbar the server last heard. Empty until a hotbar packet arrives.
    inventory: std::cell::RefCell<SyncedBar>,
}

#[derive(Clone)]
struct SyncedBar {
    selected: i32,
    slots: Vec<(String, i32)>,
}

struct ScriptSlot {
    id: String,
    host: Host,
    slot: usize,
}

impl ScriptTable {
    pub(crate) fn new() -> Self {
        Self {
            before_break: Vec::new(),
            break_block: Vec::new(),
            before_place: Vec::new(),
            after_break: Vec::new(),
            after_place: Vec::new(),
            fill_place: Vec::new(),
            fill_remove: Vec::new(),
            on_select: Vec::new(),
            on_screen: Vec::new(),
            break_speed: Vec::new(),
            pickup_item: Vec::new(),
            break_drop: Vec::new(),
            loot: Vec::new(),
            entity_spawn: Vec::new(),
            entity_hurt: Vec::new(),
            entity_death: Vec::new(),
            entity_tick: Vec::new(),
            use_item: Vec::new(),
            use_block: Vec::new(),
            use_entity: Vec::new(),
            attack: Vec::new(),
            chat: Vec::new(),
            respawn: Vec::new(),
            craft: Vec::new(),
            smelt: Vec::new(),
            repair: Vec::new(),
            enchant: Vec::new(),
            brew: Vec::new(),
            smith: Vec::new(),
            weave: Vec::new(),
            tick: Vec::new(),
            dawn: Vec::new(),
            dusk: Vec::new(),
            jump: Vec::new(),
            sprint: Vec::new(),
            land: Vec::new(),
            command: Vec::new(),
            screen_press: Vec::new(),
            setting_change: Vec::new(),
            api: Vec::new(),
            entries: Vec::new(),
            mobs: Vec::new(),
            writes: std::cell::RefCell::new(Vec::new()),
            live_spawns: std::cell::RefCell::new(Vec::new()),
            lines: std::cell::RefCell::new(Vec::new()),
            pending_screens: std::cell::RefCell::new(Vec::new()),
            inventory: std::cell::RefCell::new(SyncedBar {
                selected: 0,
                slots: Vec::new(),
            }),
        }
    }

    pub(crate) fn content(&self) -> &[(String, String)] {
        &self.entries
    }

    pub(crate) fn mobs(&self) -> &[String] {
        &self.mobs
    }

    pub(crate) fn note_mob(&mut self, id: &str) {
        if !self.mobs.iter().any(|existing| existing == id) {
            self.mobs.push(id.to_string());
        }
    }

    pub(crate) fn drain_writes(&self) -> Vec<crate::changes::CellChange> {
        self.writes.borrow_mut().drain(..).collect()
    }

    pub(crate) fn drain_live_spawns(
        &self,
    ) -> Vec<Result<(String, i32, i32, i32), (String, i32, i32, i32)>> {
        self.live_spawns.borrow_mut().drain(..).collect()
    }

    pub(crate) fn drain_screen_opens(&self) -> Vec<String> {
        self.pending_screens.borrow_mut().drain(..).collect()
    }

    pub(crate) fn has(&self, kind: &str, id: &str) -> bool {
        self.list(kind)
            .is_some_and(|list| list.iter().any(|hook| hook.id == id))
    }

    pub(crate) fn before_break(&self, world: &mut Cluster, event: &BeforeBreak) -> BreakVerdict {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let mut verdict = BreakVerdict::Allow;
        for hook in &self.before_break {
            match call(
                self,
                hook,
                world,
                &[Arg::Text(event.block.id.to_string())],
                &view,
                ScriptWrites::Open,
            ) {
                Ok(false) => verdict = BreakVerdict::Cancel,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    verdict = BreakVerdict::Cancel;
                }
            }
        }
        let args = [
            Arg::Text(event.block.id.to_string()),
            Arg::Int(event.x),
            Arg::Int(event.y),
            Arg::Int(event.z),
        ];
        for hook in &self.break_block {
            match call(self, hook, world, &args, &view, ScriptWrites::Open) {
                Ok(false) => verdict = BreakVerdict::Cancel,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    verdict = BreakVerdict::Cancel;
                }
            }
        }
        verdict
    }

    pub(crate) fn before_place(&self, world: &mut Cluster, event: &BeforePlace) -> BreakVerdict {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let mut verdict = BreakVerdict::Allow;
        let args = [
            Arg::Text(event.block.id.to_string()),
            Arg::Int(event.x),
            Arg::Int(event.y),
            Arg::Int(event.z),
        ];
        for hook in &self.before_place {
            match call(self, hook, world, &args, &view, ScriptWrites::Open) {
                Ok(false) => verdict = BreakVerdict::Cancel,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    verdict = BreakVerdict::Cancel;
                }
            }
        }
        verdict
    }

    pub(crate) fn after_break(&self, world: &mut Cluster, event: &AfterBreak) {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [
            Arg::Text(event.block.id.to_string()),
            Arg::Int(event.x),
            Arg::Int(event.y),
            Arg::Int(event.z),
            Arg::Text(event.left.id.to_string()),
        ];
        for hook in &self.after_break {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    pub(crate) fn on_select(
        &self,
        world: &mut Cluster,
        slot: i32,
        block: &str,
        view: &ScriptView<'_>,
    ) {
        let args = [Arg::Int(slot), Arg::Text(block.to_string())];
        for hook in &self.on_select {
            if let Err(err) = call(self, hook, world, &args, view, ScriptWrites::Closed) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    pub(crate) fn on_screen(
        &self,
        world: &mut Cluster,
        id: &str,
        open: bool,
        view: &ScriptView<'_>,
    ) {
        let args = [Arg::Text(id.to_string()), Arg::Bool(open)];
        for hook in &self.on_screen {
            if let Err(err) = call(self, hook, world, &args, view, ScriptWrites::Closed) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// Enter or a click on `row` of the GUIAPI screen `id`.
    pub(crate) fn screen_press(
        &self,
        world: &mut Cluster,
        id: &str,
        row: &str,
        typed: Option<&str>,
        view: &ScriptView<'_>,
    ) -> Vec<String> {
        let args = [Arg::Text(row.to_string())];
        let mut said = Vec::new();
        for hook in &self.screen_press {
            if hook.id != id {
                continue;
            }
            if let Some(text) = typed {
                if let Err(err) = hook.host.set_row_text(hook.slot, row, text) {
                    eprintln!("rosegold: field failed: {err}");
                }
            }
            match invoke(self, hook, world, &args, view, ScriptWrites::Closed) {
                Ok(answer) => {
                    for screen in answer.screens {
                        Pack::builtin().note_gui_screen(screen);
                    }
                    said.extend(answer.lines);
                }
                Err(err) => eprintln!("rosegold: hook failed: {err}"),
            }
        }
        said
    }

    /// The player flipped the settings row `name`. The script reads the new value itself.
    ///
    /// One nearby light may appear in air, or that light may return to air.
    pub(crate) fn setting_changed(
        &self,
        world: &mut Cluster,
        name: &str,
        view: &ScriptView<'_>,
    ) -> Vec<String> {
        let args = [Arg::Text(name.to_string())];
        let mut said = Vec::new();
        for hook in &self.setting_change {
            if hook.id != name {
                continue;
            }
            match invoke(self, hook, world, &args, view, ScriptWrites::SoftLight) {
                Ok(answer) => said.extend(answer.lines),
                Err(err) => eprintln!("rosegold: hook failed: {err}"),
            }
        }
        said
    }

    pub(crate) fn after_place(&self, world: &mut Cluster, event: &AfterPlace) {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [
            Arg::Text(event.block.id.to_string()),
            Arg::Int(event.x),
            Arg::Int(event.y),
            Arg::Int(event.z),
        ];
        for hook in &self.after_place {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// `events.fill_place` / `hooks.fill_place` — fill kind entered a host.
    pub(crate) fn fill_place(&self, world: &mut Cluster, kind: &str, x: i32, y: i32, z: i32) {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [
            Arg::Text(kind.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.fill_place {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// `events.fill_remove` / `hooks.fill_remove` — fill kind left a host.
    pub(crate) fn fill_remove(&self, world: &mut Cluster, kind: &str, x: i32, y: i32, z: i32) {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [
            Arg::Text(kind.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.fill_remove {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// Fire fill_place / fill_remove when the cell's fill byte changed.
    pub(crate) fn note_fill_change(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        before: u8,
        after: u8,
    ) {
        if before == after {
            return;
        }
        if after != crate::chunk::FILL_NONE {
            if let Some(kind) = crate::chunk::fill_name(after) {
                self.fill_place(world, kind, x, y, z);
            }
        } else if let Some(kind) = crate::chunk::fill_name(before) {
            self.fill_remove(world, kind, x, y, z);
        }
    }

    /// Multiply every break-speed hook. `1` leaves the pack hardness. A hook
    /// that returns `0.5` makes that block take half as long.
    pub(crate) fn break_scale(&self, world: &mut Cluster, id: &str, view: &ScriptView<'_>) -> f32 {
        let mut scale = 1.0f32;
        let args = [Arg::Text(id.to_string())];
        for hook in &self.break_speed {
            let Ok(call) = invoke(self, hook, world, &args, view, ScriptWrites::Closed) else {
                continue;
            };
            if let Some(number) = call.number {
                if number.is_finite() && number > 0.0 {
                    scale *= number as f32;
                }
            }
        }
        scale.clamp(0.05, 8.0)
    }

    /// False when a pickup hook cancels this item. No hooks means the drop can be taken.
    pub(crate) fn allow_pickup(
        &self,
        world: &mut Cluster,
        id: &str,
        view: &ScriptView<'_>,
    ) -> bool {
        let mut allow = true;
        let args = [Arg::Text(id.to_string())];
        for hook in &self.pickup_item {
            match call(self, hook, world, &args, view, ScriptWrites::Closed) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// The item a break should drop. `None` keeps the broken block. `Some("")` drops nothing.
    pub(crate) fn break_drop(
        &self,
        world: &mut Cluster,
        block: &str,
        left: &str,
        view: &ScriptView<'_>,
    ) -> Option<String> {
        let args = [Arg::Text(block.to_string()), Arg::Text(left.to_string())];
        let mut drop = None;
        for hook in &self.break_drop {
            let Ok(call) = invoke(self, hook, world, &args, view, ScriptWrites::Closed) else {
                continue;
            };
            if let Some(text) = call.text {
                drop = Some(text);
            }
        }
        drop
    }

    /// The item a copper chest should pay instead of `item`.
    ///
    /// `None` keeps the datapack pick. A hook returns the replacement id.
    /// `""` and an unknown id are ignored, and the last accepted id wins.
    pub(crate) fn loot(
        &self,
        world: &mut Cluster,
        item: &str,
        x: i32,
        z: i32,
        omen: bool,
        view: &ScriptView<'_>,
    ) -> Option<String> {
        if self.loot.is_empty() {
            return None;
        }
        let args = [
            Arg::Text(item.to_string()),
            Arg::Int(x),
            Arg::Int(z),
            Arg::Bool(omen),
        ];
        let pack = Pack::builtin();
        let mut next = None;
        for hook in &self.loot {
            let Ok(call) = invoke(self, hook, world, &args, view, ScriptWrites::Closed) else {
                continue;
            };
            let Some(text) = call.text else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            if !pack.contains_item(&text) && pack.tool_icon(&text).is_none() {
                continue;
            }
            next = Some(text);
        }
        next
    }

    /// Player join or a demo/script mob spawn. False when a hook cancels.
    /// Returning nothing / true keeps the spawn. Writes are kept for the server.
    pub(crate) fn entity_spawn(&self, world: &mut Cluster, kind: &str, x: i32, y: i32, z: i32) -> bool {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let mut allow = true;
        let args = [
            Arg::Text(kind.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.entity_spawn {
            match call(self, hook, world, &args, &view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// Run hurt hooks. Returns the amount to apply after cancel/rewrite.
    ///
    /// `events.entity_hurt` is `fn(kind: String, amount: Int): Bool|Int`.
    /// `false` cancels (returns 0). An `Int` replaces the amount (`0` cancels).
    /// `true` / void keeps the current amount. Hooks see the rewritten amount
    /// from earlier hooks. A throw cancels.
    pub(crate) fn entity_hurt(&self, world: &mut Cluster, kind: &str, amount: i32) -> i32 {
        if amount <= 0 {
            return 0;
        }
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let mut next = amount;
        for hook in &self.entity_hurt {
            let args = [Arg::Text(kind.to_string()), Arg::Int(next)];
            match invoke(self, hook, world, &args, &view, ScriptWrites::Closed) {
                Ok(call) if call.returned_false => return 0,
                Ok(call) => {
                    if let Some(value) = call.number {
                        if !value.is_finite() {
                            continue;
                        }
                        let rewritten = value.round() as i32;
                        if rewritten <= 0 {
                            return 0;
                        }
                        next = rewritten;
                    }
                }
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    return 0;
                }
            }
        }
        next
    }

    /// False when a death hook cancels despawn/loot side effects.
    pub(crate) fn entity_death(
        &self,
        world: &mut Cluster,
        kind: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> bool {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let mut allow = true;
        let args = [
            Arg::Text(kind.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.entity_death {
            match call(self, hook, world, &args, &view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a use-item hook consumes the right click. No hooks means the click continues.
    ///
    /// `item` is the selected hotbar id, or `""` for an empty hand. A hook may
    /// `world.set_block`. The first write is what the game sends when the click is consumed.
    pub(crate) fn use_item(&self, world: &mut Cluster, item: &str, view: &ScriptView<'_>) -> bool {
        if self.use_item.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string())];
        for hook in &self.use_item {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a use-block hook consumes the right click on `block` at `x, y, z`.
    ///
    /// Runs before [`Self::use_item`] when the player is looking at a block.
    /// A hook may `world.set_block`.
    pub(crate) fn use_block(
        &self,
        world: &mut Cluster,
        item: &str,
        block: &str,
        x: i32,
        y: i32,
        z: i32,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.use_block.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [
            Arg::Text(item.to_string()),
            Arg::Text(block.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.use_block {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a use-entity hook consumes the right click on `kind`.
    ///
    /// Runs before [`Self::use_block`] when the look hits a mob. `kind` is
    /// `wolf`, `pig`, `zombie`, and the other aimed mobs. A hook may
    /// `world.set_block`. False skips feeding, saddling, and a mace swing.
    pub(crate) fn use_entity(
        &self,
        world: &mut Cluster,
        item: &str,
        kind: &str,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.use_entity.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string()), Arg::Text(kind.to_string())];
        for hook in &self.use_entity {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when an attack hook consumes the left click on `target`.
    ///
    /// `target` is the aimed mob (`wolf`, `zombie`, …) or the looked-at block id.
    /// Empty when the swing hits neither. False skips the break, the egg hop,
    /// and the crystal blast. A hook may `world.set_block`.
    pub(crate) fn attack(
        &self,
        world: &mut Cluster,
        item: &str,
        target: &str,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.attack.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string()), Arg::Text(target.to_string())];
        for hook in &self.attack {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a chat hook swallows this ordinary line.
    ///
    /// A leading slash is a command, and the game does not ask this hook.
    /// A hook may `world.set_block`. False means the line is not shown and
    /// not sent.
    pub(crate) fn chat(&self, world: &mut Cluster, line: &str, view: &ScriptView<'_>) -> bool {
        if self.chat.is_empty() || line.starts_with('/') {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(line.to_string())];
        for hook in &self.chat {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a respawn hook refuses this return.
    ///
    /// `cause` is `death`, `bed`, or `anchor`. `x, y, z` is the cell the player
    /// would come back to. False sends a death back to the stairs, keeps the
    /// night from ending, or leaves the saved spawn unchanged. A hook may
    /// `world.set_block`.
    pub(crate) fn respawn(
        &self,
        world: &mut Cluster,
        cause: &str,
        x: i32,
        y: i32,
        z: i32,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.respawn.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [
            Arg::Text(cause.to_string()),
            Arg::Int(x),
            Arg::Int(y),
            Arg::Int(z),
        ];
        for hook in &self.respawn {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a craft hook refuses this result.
    ///
    /// The argument is the result item id from the crafting grid. False leaves
    /// the ingredients in the grid. A hook may `world.set_block`.
    pub(crate) fn craft(&self, world: &mut Cluster, result: &str, view: &ScriptView<'_>) -> bool {
        if self.craft.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(result.to_string())];
        for hook in &self.craft {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a smelt hook refuses this output.
    ///
    /// The argument is the item id in the furnace, smoker, or blast-furnace
    /// output slot. False leaves it there. A hook may `world.set_block`.
    pub(crate) fn smelt(&self, world: &mut Cluster, result: &str, view: &ScriptView<'_>) -> bool {
        if self.smelt.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(result.to_string())];
        for hook in &self.smelt {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a repair hook refuses this anvil spend.
    ///
    /// The argument is the worn tool id. False leaves the iron ore and the
    /// tool alone. A hook may `world.set_block`.
    pub(crate) fn repair(&self, world: &mut Cluster, item: &str, view: &ScriptView<'_>) -> bool {
        if self.repair.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string())];
        for hook in &self.repair {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when an enchant hook refuses this bookshelf spend.
    ///
    /// The argument is the plain pickaxe id. False leaves the bookshelf and
    /// the pick alone. A hook may `world.set_block`.
    pub(crate) fn enchant(&self, world: &mut Cluster, item: &str, view: &ScriptView<'_>) -> bool {
        if self.enchant.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string())];
        for hook in &self.enchant {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a brew hook refuses this mix.
    ///
    /// The argument is `nether_wart_block` or `flower`. False leaves the water
    /// bucket and that ingredient alone. A hook may `world.set_block`.
    pub(crate) fn brew(
        &self,
        world: &mut Cluster,
        ingredient: &str,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.brew.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(ingredient.to_string())];
        for hook in &self.brew {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a smith hook refuses this upgrade.
    ///
    /// The argument is the held item: `stone_pickaxe`, `iron_pickaxe`, or
    /// `mace`. False leaves the iron ore or the heavy core unspent. A hook may
    /// `world.set_block`.
    pub(crate) fn smith(&self, world: &mut Cluster, item: &str, view: &ScriptView<'_>) -> bool {
        if self.smith.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(item.to_string())];
        for hook in &self.smith {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a weave hook refuses this wool.
    ///
    /// The argument is the wool id. False leaves it in the bar. A hook may
    /// `world.set_block`.
    pub(crate) fn weave(&self, world: &mut Cluster, wool: &str, view: &ScriptView<'_>) -> bool {
        if self.weave.is_empty() {
            return true;
        }
        let mut allow = true;
        let args = [Arg::Text(wool.to_string())];
        for hook in &self.weave {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// The reply from command hooks whose id or short name is `id`.
    ///
    /// `None` when nothing is registered under that name. `args` is the text
    /// after the name, or empty when the command list runs it. A hook is
    /// `fn(args: String): String`. The returned string is the reply. A hook may
    /// `world.set_block`. Registering the hook also lists a `COMMAND` entry.
    pub(crate) fn command(
        &self,
        world: &mut Cluster,
        id: &str,
        args: &str,
        view: &ScriptView<'_>,
    ) -> Option<String> {
        if self.command.is_empty() {
            return None;
        }
        let mut reply = String::new();
        let mut ran = false;
        let call_args = [Arg::Text(args.to_string())];
        for hook in &self.command {
            if !command_hit(&hook.id, id) {
                continue;
            }
            ran = true;
            match invoke(self, hook, world, &call_args, view, ScriptWrites::Open) {
                Ok(call) => push_reply(&mut reply, &call.text.unwrap_or_default()),
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    push_reply(&mut reply, &err);
                }
            }
        }
        if ran { Some(reply) } else { None }
    }

    /// One second of play. `n` starts at 1.
    ///
    /// The client and the server each count their own 50 ms steps. This runs
    /// on every 20th step, so a steady game calls it once a second. A stalled
    /// catch-up still calls it at most once. A hook may `world.set_block`.
    pub(crate) fn tick(&self, world: &mut Cluster, n: i32) {
        if self.tick.is_empty() {
            return;
        }
        let _ = self.drain_writes();
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [Arg::Int(n)];
        for hook in &self.tick {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// A sunrise. `day` is the day of the four-day year, `0` through `3`.
    ///
    /// The sky calls this when a step crosses dawn, and a bed calls it when
    /// sleep skips to morning. A hook may `world.set_block`.
    pub(crate) fn dawn(&self, world: &mut Cluster, day: i32, view: &ScriptView<'_>) {
        if self.dawn.is_empty() {
            return;
        }
        let _ = self.drain_writes();
        let args = [Arg::Int(day)];
        for hook in &self.dawn {
            if let Err(err) = call(self, hook, world, &args, view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// A sunset. `day` is the day of the four-day year, `0` through `3`.
    ///
    /// The sky calls this when a step crosses dusk. A bed skips the night, so
    /// it does not call this. A hook may `world.set_block`.
    pub(crate) fn dusk(&self, world: &mut Cluster, day: i32, view: &ScriptView<'_>) {
        if self.dusk.is_empty() {
            return;
        }
        let _ = self.drain_writes();
        let args = [Arg::Int(day)];
        for hook in &self.dusk {
            if let Err(err) = call(self, hook, world, &args, view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// False when a jump hook keeps the player on the ground.
    ///
    /// `x`, `y`, and `z` are the cell the feet are in. False skips the launch.
    /// Swimming and climbing do not ask. A hook may `world.set_block`.
    pub(crate) fn jump(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.jump.is_empty() {
            return true;
        }
        let _ = self.drain_writes();
        let mut allow = true;
        let args = [Arg::Int(x), Arg::Int(y), Arg::Int(z)];
        for hook in &self.jump {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// False when a sprint hook keeps the walk at normal speed.
    ///
    /// `x`, `y`, and `z` are the cell the feet are in. False clears the fast
    /// flag for this hold. Standing still does not ask. A hook may `world.set_block`.
    pub(crate) fn sprint(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        view: &ScriptView<'_>,
    ) -> bool {
        if self.sprint.is_empty() {
            return true;
        }
        let _ = self.drain_writes();
        let mut allow = true;
        let args = [Arg::Int(x), Arg::Int(y), Arg::Int(z)];
        for hook in &self.sprint {
            match call(self, hook, world, &args, view, ScriptWrites::Open) {
                Ok(false) => allow = false,
                Ok(true) => {}
                Err(err) => {
                    eprintln!("rosegold: hook failed: {err}");
                    allow = false;
                }
            }
        }
        allow
    }

    /// A landing. `drop` is how many whole blocks the player fell.
    ///
    /// `x`, `y`, and `z` are the cell the feet are in. The game calls this when
    /// the player touches the ground after leaving it. A hook may `world.set_block`.
    pub(crate) fn land(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        drop: i32,
        view: &ScriptView<'_>,
    ) {
        if self.land.is_empty() {
            return;
        }
        let _ = self.drain_writes();
        let args = [Arg::Int(x), Arg::Int(y), Arg::Int(z), Arg::Int(drop)];
        for hook in &self.land {
            if let Err(err) = call(self, hook, world, &args, view, ScriptWrites::Open) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    /// A mob tick. The engine has no mobs, so this is only called from tests.
    pub(crate) fn entity_tick(&self, world: &mut Cluster, kind: &str) {
        let bar = self.bar();
        let view = Self::view_of(&bar);
        let args = [Arg::Text(kind.to_string())];
        for hook in &self.entity_tick {
            if let Err(err) = call(self, hook, world, &args, &view, ScriptWrites::Closed) {
                eprintln!("rosegold: hook failed: {err}");
            }
        }
    }

    pub(crate) fn listens(&self, kind: &str) -> bool {
        self.list(kind).is_some_and(|list| !list.is_empty())
    }

    pub(crate) fn set_inventory(&self, selected: i32, slots: Vec<(String, i32)>) {
        *self.inventory.borrow_mut() = SyncedBar { selected, slots };
    }

    fn bar(&self) -> SyncedBar {
        self.inventory.borrow().clone()
    }

    fn view_of<'a>(bar: &'a SyncedBar) -> ScriptView<'a> {
        ScriptView {
            slots: &bar.slots,
            selected: bar.selected,
            eye: None,
            time: None,
            season: "",
        }
    }

    fn list(&self, kind: &str) -> Option<&[ScriptSlot]> {
        match kind {
            "before_break" => Some(&self.before_break),
            "break_block" => Some(&self.break_block),
            "before_place" => Some(&self.before_place),
            "after_break" => Some(&self.after_break),
            "after_place" => Some(&self.after_place),
            "fill_place" => Some(&self.fill_place),
            "fill_remove" => Some(&self.fill_remove),
            "on_select" => Some(&self.on_select),
            "on_screen" => Some(&self.on_screen),
            "break_speed" => Some(&self.break_speed),
            "pickup_item" => Some(&self.pickup_item),
            "break_drop" => Some(&self.break_drop),
            "loot" => Some(&self.loot),
            "entity_spawn" => Some(&self.entity_spawn),
            "entity_hurt" => Some(&self.entity_hurt),
            "entity_death" => Some(&self.entity_death),
            "entity_tick" => Some(&self.entity_tick),
            "use_item" => Some(&self.use_item),
            "use_block" => Some(&self.use_block),
            "use_entity" => Some(&self.use_entity),
            "attack" => Some(&self.attack),
            "chat" => Some(&self.chat),
            "respawn" => Some(&self.respawn),
            "craft" => Some(&self.craft),
            "smelt" => Some(&self.smelt),
            "repair" => Some(&self.repair),
            "enchant" => Some(&self.enchant),
            "brew" => Some(&self.brew),
            "smith" => Some(&self.smith),
            "weave" => Some(&self.weave),
            "tick" => Some(&self.tick),
            "dawn" => Some(&self.dawn),
            "dusk" => Some(&self.dusk),
            "jump" => Some(&self.jump),
            "sprint" => Some(&self.sprint),
            "land" => Some(&self.land),
            "command" => Some(&self.command),
            "screen_press" => Some(&self.screen_press),
            "setting_change" => Some(&self.setting_change),
            _ => None,
        }
    }

    fn push(&mut self, hook: &ScriptHook, host: Host, slot: usize) {
        let item = ScriptSlot {
            id: hook.id().to_string(),
            host,
            slot,
        };
        match hook {
            ScriptHook::BeforeBreak { .. } => self.before_break.push(item),
            ScriptHook::BreakBlock { .. } => self.break_block.push(item),
            ScriptHook::BeforePlace { .. } => self.before_place.push(item),
            ScriptHook::AfterBreak { .. } => self.after_break.push(item),
            ScriptHook::AfterPlace { .. } => self.after_place.push(item),
            ScriptHook::FillPlace { .. } => self.fill_place.push(item),
            ScriptHook::FillRemove { .. } => self.fill_remove.push(item),
            ScriptHook::OnSelect { .. } => self.on_select.push(item),
            ScriptHook::OnScreen { .. } => self.on_screen.push(item),
            ScriptHook::BreakSpeed { .. } => self.break_speed.push(item),
            ScriptHook::PickupItem { .. } => self.pickup_item.push(item),
            ScriptHook::BreakDrop { .. } => self.break_drop.push(item),
            ScriptHook::Loot { .. } => self.loot.push(item),
            ScriptHook::EntitySpawn { .. } => self.entity_spawn.push(item),
            ScriptHook::EntityHurt { .. } => self.entity_hurt.push(item),
            ScriptHook::EntityDeath { .. } => self.entity_death.push(item),
            ScriptHook::EntityTick { .. } => self.entity_tick.push(item),
            ScriptHook::UseItem { .. } => self.use_item.push(item),
            ScriptHook::UseBlock { .. } => self.use_block.push(item),
            ScriptHook::UseEntity { .. } => self.use_entity.push(item),
            ScriptHook::Attack { .. } => self.attack.push(item),
            ScriptHook::Chat { .. } => self.chat.push(item),
            ScriptHook::Respawn { .. } => self.respawn.push(item),
            ScriptHook::Craft { .. } => self.craft.push(item),
            ScriptHook::Smelt { .. } => self.smelt.push(item),
            ScriptHook::Repair { .. } => self.repair.push(item),
            ScriptHook::Enchant { .. } => self.enchant.push(item),
            ScriptHook::Brew { .. } => self.brew.push(item),
            ScriptHook::Smith { .. } => self.smith.push(item),
            ScriptHook::Weave { .. } => self.weave.push(item),
            ScriptHook::Tick { .. } => self.tick.push(item),
            ScriptHook::Dawn { .. } => self.dawn.push(item),
            ScriptHook::Dusk { .. } => self.dusk.push(item),
            ScriptHook::Jump { .. } => self.jump.push(item),
            ScriptHook::Sprint { .. } => self.sprint.push(item),
            ScriptHook::Land { .. } => self.land.push(item),
            ScriptHook::ScreenPress { .. } => self.screen_press.push(item),
            ScriptHook::SettingChange { .. } => self.setting_change.push(item),
            ScriptHook::Command { id } => {
                self.entries.push(("COMMAND".to_string(), id.clone()));
                self.command.push(item);
            }
        }
    }
}

/// Load `source` and register each hook it registers on `hooks`.
pub fn load_source(source: &str, hooks: &mut Hooks) -> Result<Vec<ScriptHook>, String> {
    let host = Host::start();
    let found = host.load(source, "mod.rg", Path::new("."))?;
    register(&host, found, hooks, "mod.rg")
}

/// Load every script in `dir`, in path order.
///
/// That is each `*.rg` file, each `*.rg` file one folder down, and each
/// `src/mod.rg`.
///
/// A missing directory registers nothing. A script that fails to parse, check,
/// or run names the file and returns an error.
pub fn load_dir(dir: &Path, hooks: &mut Hooks) -> Result<Vec<(PathBuf, Vec<ScriptHook>)>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_scripts(dir, &mut paths)?;
    paths.sort();
    let host = Host::start();
    let mut loaded = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let sandbox = path.parent().unwrap_or(dir);
        let found = host
            .load(&text, &path.display().to_string(), sandbox)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let hooks_found = register(&host, found, hooks, &path.display().to_string())?;
        loaded.push((path, hooks_found));
    }
    Ok(loaded)
}

fn is_script(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rg"))
}

/// Flat `*.rg` files, `*.rg` files sitting in a mod folder, and `src/mod.rg`.
/// Asset folders are not scripts.
fn collect_scripts(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            let entry_script = path.join("src").join("mod.rg");
            if entry_script.is_file() {
                paths.push(entry_script);
            }
            let nested = fs::read_dir(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            for nested in nested {
                let nested = nested.map_err(|err| format!("{}: {err}", path.display()))?;
                let file = nested.path();
                if file.is_file() && is_script(&file) {
                    paths.push(file);
                }
            }
        } else if is_script(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

/// `src/mod.rg` lives in the mod folder. A flat script lives next to its data.
fn mod_root(source: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(source);
    let parent = path.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    if !parent.is_dir() {
        return None;
    }
    if path.file_name().and_then(|name| name.to_str()) == Some("mod.rg")
        && parent.file_name().and_then(|name| name.to_str()) == Some("src")
    {
        return parent
            .parent()
            .filter(|dir| dir.is_dir())
            .map(|dir| dir.to_path_buf());
    }
    Some(parent.to_path_buf())
}

fn register(
    host: &Host,
    found: rebuild_rosegold::ScriptLoad,
    hooks: &mut Hooks,
    source: &str,
) -> Result<Vec<ScriptHook>, String> {
    let mut seen_blocks = Vec::new();
    for (namespace, path) in &found.blocks {
        let id = format!("{namespace}:{path}");
        if seen_blocks.contains(&id) {
            return Err(format!("block.register: duplicate block {id}"));
        }
        seen_blocks.push(id);
        Pack::builtin()
            .register_block_from(namespace, path, source)
            .map_err(|err| format!("block.register: {err}"))?;
    }
    let mut seen_items = Vec::new();
    for (namespace, path) in &found.items {
        let id = format!("{namespace}:{path}");
        if seen_items.contains(&id) {
            return Err(format!("item.register: duplicate item {id}"));
        }
        seen_items.push(id);
        Pack::builtin()
            .register_item_from(namespace, path, source)
            .map_err(|err| format!("item.register: {err}"))?;
    }
    for (namespace, path) in &found.entities {
        let id = Pack::builtin()
            .register_entity_from(namespace, path, source)
            .map_err(|err| format!("entity.register: {err}"))?;
        hooks.scripts.note_mob(&id);
    }
    for (id, x, y, z) in &found.spawns {
        Pack::builtin()
            .note_spawn(id, *x, *y, *z, source)
            .map_err(|err| format!("entity.spawn: {err}"))?;
    }
    for (id, texture) in &found.block_textures {
        Pack::builtin()
            .set_script_texture(id, texture, source)
            .map_err(|err| format!("block.texture: {err}"))?;
    }
    for (id, model) in &found.block_models {
        Pack::builtin()
            .set_script_model(id, model, source)
            .map_err(|err| format!("block.model: {err}"))?;
    }
    for (id, model) in &found.entity_models {
        Pack::builtin()
            .set_entity_model(id, model, source)
            .map_err(|err| format!("entity.model: {err}"))?;
    }
    for (id, flag) in &found.entity_hostile {
        let hostile = flag == "1" || flag.eq_ignore_ascii_case("true");
        Pack::builtin()
            .set_entity_hostile(id, hostile, source)
            .map_err(|err| format!("entity.hostile: {err}"))?;
    }
    for (id, egg) in &found.entity_spawn_eggs {
        Pack::builtin()
            .set_entity_spawn_egg(id, egg, source)
            .map_err(|err| format!("entity.spawn_egg: {err}"))?;
    }
    for (id, seconds) in &found.block_hardness {
        let hardness: f32 = seconds
            .parse()
            .map_err(|_| format!("block.hardness: bad seconds {seconds}"))?;
        Pack::builtin()
            .set_script_hardness(id, hardness, source)
            .map_err(|err| format!("block.hardness: {err}"))?;
    }
    for (id, level) in &found.block_lights {
        let level: u8 = level
            .parse()
            .map_err(|_| format!("block.light: bad level {level}"))?;
        Pack::builtin()
            .set_script_light(id, level, source)
            .map_err(|err| format!("block.light: {err}"))?;
    }
    for (item, block) in &found.item_places {
        Pack::builtin()
            .set_item_places(item, block, source)
            .map_err(|err| format!("item.places: {err}"))?;
    }
    for (item, count) in &found.item_stacks {
        let count: u16 = count
            .parse()
            .map_err(|_| format!("item.stack: bad count {count}"))?;
        Pack::builtin()
            .set_item_stack(item, count, source)
            .map_err(|err| format!("item.stack: {err}"))?;
    }
    for (result, ingredient) in &found.recipes {
        Pack::builtin()
            .add_script_recipe(result, ingredient, source)
            .map_err(|err| format!("recipe.register: {err}"))?;
    }
    for (result, list) in &found.shapeless {
        let ingredients: Vec<&str> = list
            .split('+')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .collect();
        Pack::builtin()
            .add_script_line(result, &ingredients, source)
            .map_err(|err| format!("recipe.shapeless: {err}"))?;
    }
    for (result, grid) in &found.shaped {
        Pack::builtin()
            .add_script_shaped(result, grid, source)
            .map_err(|err| format!("recipe.shaped: {err}"))?;
    }
    for (kind, value) in &found.api {
        if kind == "note" {
            println!("rosegold: {value}");
        }
        hooks.scripts.api.push((kind.clone(), value.clone()));
    }
    for (kind, id) in &found.entries {
        hooks.scripts.entries.push((kind.clone(), id.clone()));
        if kind == "ENTITY" {
            Pack::builtin()
                .register_entity(id, source)
                .map_err(|err| format!("entity.register: {err}"))?;
            hooks.scripts.note_mob(id);
        }
        if kind == "BLOCK_ENTITY" {
            Pack::builtin()
                .note_block_entity(id)
                .map_err(|err| format!("block_entity.register: {err}"))?;
        }
        if kind == "SCREEN" {
            Pack::builtin()
                .add_script_screen(id, source)
                .map_err(|err| format!("screen.register: {err}"))?;
        }
    }
    for id in &found.block_entity_binds {
        Pack::builtin()
            .note_block_entity(id)
            .map_err(|err| format!("block_entity.bind: {err}"))?;
    }
    for screen in found.screens {
        Pack::builtin()
            .add_gui_screen(screen, source)
            .map_err(|err| format!("gui.screen: {err}"))?;
    }
    for option in found.options {
        Pack::builtin()
            .add_setting(option, source)
            .map_err(|err| format!("settings.option: {err}"))?;
    }
    if let Some(root) = mod_root(source) {
        Pack::builtin()
            .load_mod_data(&root, source)
            .map_err(|err| format!("{source}: {err}"))?;
    }
    let mut loaded = Vec::new();
    for item in found.hooks {
        let hook = ScriptHook::from_name(&item.name, item.id)?;
        if hooks.has_id(hook.key(), hook.id()) {
            return Err(format!("duplicate {} hook {}", hook.kind(), hook.id()));
        }
        hooks.scripts.push(&hook, host.clone(), item.slot);
        loaded.push(hook);
    }
    Ok(loaded)
}

/// How far a hook may change the world.
enum ScriptWrites {
    /// `world.write` returns false.
    Closed,
    /// Any in-bounds block, the same as a place hook.
    Open,
    /// One light, into air near the player, or that light back to air.
    SoftLight,
}

fn call(
    table: &ScriptTable,
    hook: &ScriptSlot,
    world: &mut Cluster,
    args: &[Arg],
    view: &ScriptView<'_>,
    writes: ScriptWrites,
) -> Result<bool, String> {
    Ok(!invoke(table, hook, world, args, view, writes)?.returned_false)
}

fn invoke(
    table: &ScriptTable,
    hook: &ScriptSlot,
    world: &mut Cluster,
    args: &[Arg],
    view: &ScriptView<'_>,
    writes: ScriptWrites,
) -> Result<rebuild_rosegold::Call, String> {
    let result = hook.host.call(hook.slot, args, &mut |query| {
        answer(table, world, view, &writes, query)
    });
    let said: Vec<String> = table.lines.borrow_mut().drain(..).collect();
    let mut call = result?;
    emit_script_output(&call.printed);
    call.lines = said;
    Ok(call)
}

fn answer(
    table: &ScriptTable,
    world: &mut Cluster,
    view: &ScriptView<'_>,
    gate: &ScriptWrites,
    query: Query,
) -> Result<Answer, String> {
    let writes = &table.writes;
    let live_spawns = &table.live_spawns;
    let lines = &table.lines;
    let pending_screens = &table.pending_screens;
    let pack = Pack::builtin();
    match query {
        Query::Block { x, y, z } => {
            if !world.in_bounds(x, y, z) {
                return Ok(Answer::Text(String::new()));
            }
            Ok(Answer::Text(world.block_at(x, y, z).id.to_string()))
        }
        Query::Hardness { id } => {
            let seconds = pack.hardness_of(Block::new(&id, 0)).unwrap_or(0.0);
            Ok(Answer::Float(f64::from(seconds)))
        }
        Query::Light { id } => {
            let level = pack
                .light_of(Block::new(&id, 0))
                .map(|light| i64::from(light.level))
                .unwrap_or(0);
            Ok(Answer::Int(level))
        }
        Query::Replaces { id } => Ok(Answer::Text(
            pack.break_result(Block::new(&id, 0)).id.to_string(),
        )),
        Query::Write { x, y, z, id } => {
            let prev = if world.in_bounds(x, y, z) {
                world.block_at(x, y, z).id.to_string()
            } else {
                String::new()
            };
            let block = if id == "air" {
                Block::AIR
            } else if pack.contains_block(&id) {
                Block::new(&id, 0)
            } else {
                return Ok(Answer::Bool(false));
            };
            let wrote = match gate {
                ScriptWrites::Closed => false,
                ScriptWrites::Open => world.set_block(x, y, z, block),
                ScriptWrites::SoftLight => {
                    if writes.borrow().len() >= 1 {
                        false
                    } else {
                        soft_light(world, view, x, y, z, block)
                    }
                }
            };
            if wrote {
                if prev != block.id {
                    pack.clear_tile_at(x, y, z);
                }
                writes.borrow_mut().push(crate::changes::CellChange {
                    x,
                    y,
                    z,
                    block: world.block_at(x, y, z),
                });
            }
            Ok(Answer::Bool(wrote))
        }
        Query::Slot { index } => Ok(Answer::Text(hotbar_slot(view.slots, index).0)),
        Query::Count { index } => Ok(Answer::Int(i64::from(hotbar_slot(view.slots, index).1))),
        Query::IsAir { x, y, z } => Ok(Answer::Bool(
            world.in_bounds(x, y, z) && world.block_at(x, y, z).is_air(),
        )),
        Query::Meta { x, y, z } => {
            if !world.in_bounds(x, y, z) {
                return Ok(Answer::Int(0));
            }
            Ok(Answer::Int(i64::from(world.block_at(x, y, z).metadata)))
        }
        Query::SetMeta { x, y, z, meta } => {
            if !matches!(gate, ScriptWrites::Open)
                || !(0..=15).contains(&meta)
                || !world.in_bounds(x, y, z)
            {
                return Ok(Answer::Bool(false));
            }
            let current = world.block_at(x, y, z);
            if current.is_air() {
                return Ok(Answer::Bool(false));
            }
            let block = Block::new(current.id, meta as u8);
            let wrote = world.set_block(x, y, z, block);
            if wrote {
                writes.borrow_mut().push(crate::changes::CellChange {
                    x,
                    y,
                    z,
                    block: world.block_at(x, y, z),
                });
            }
            Ok(Answer::Bool(wrote))
        }
        Query::Fill { x, y, z } => {
            if !world.in_bounds(x, y, z) {
                return Ok(Answer::Text(String::new()));
            }
            Ok(Answer::Text(
                crate::chunk::fill_name(world.block_at(x, y, z).fill)
                    .unwrap_or("")
                    .to_string(),
            ))
        }
        Query::SetFill { x, y, z, kind } => {
            if !matches!(gate, ScriptWrites::Open) || !world.in_bounds(x, y, z) {
                return Ok(Answer::Bool(false));
            }
            let current = world.block_at(x, y, z);
            if !crate::chunk::compound_host(current.id) {
                return Ok(Answer::Bool(false));
            }
            let fill = if kind.is_empty() || kind == "none" {
                Some(crate::chunk::FILL_NONE)
            } else {
                crate::chunk::fill_kind(&kind)
            };
            let Some(fill) = fill else {
                return Ok(Answer::Bool(false));
            };
            if fill != crate::chunk::FILL_NONE
                && !crate::chunk::compound_accepts_meta(current.id, current.metadata, fill)
            {
                return Ok(Answer::Bool(false));
            }
            if current.fill == fill && !crate::chunk::is_tank(current.id) {
                return Ok(Answer::Bool(true));
            }
            let before = current.fill;
            let block = if crate::chunk::is_tank(current.id) {
                if fill == crate::chunk::FILL_NONE {
                    crate::chunk::Block::new("tank", 0)
                } else if current.fill == fill {
                    crate::chunk::tank_at(
                        fill,
                        (crate::chunk::tank_level(current) + crate::chunk::TANK_BUCKET)
                            .min(crate::chunk::TANK_MAX),
                    )
                } else {
                    crate::chunk::tank_at(fill, crate::chunk::TANK_BUCKET)
                }
            } else {
                current.with_fill(fill)
            };
            let wrote = world.set_block(x, y, z, block);
            if wrote {
                writes.borrow_mut().push(crate::changes::CellChange {
                    x,
                    y,
                    z,
                    block: world.block_at(x, y, z),
                });
                table.note_fill_change(world, x, y, z, before, fill);
            }
            Ok(Answer::Bool(wrote))
        }
        Query::IsCompound { x, y, z } => Ok(Answer::Bool(
            world.in_bounds(x, y, z) && crate::chunk::compound_host(world.block_at(x, y, z).id),
        )),
        Query::Time => Ok(Answer::Float(view.time.map(f64::from).unwrap_or(-1.0))),
        Query::IsDay => Ok(Answer::Bool(
            view.time.is_some_and(|time| (0.25..0.75).contains(&time)),
        )),
        Query::Season => Ok(Answer::Text(view.season.to_ascii_lowercase())),
        Query::HasTime => Ok(Answer::Bool(view.time.is_some())),
        Query::Held => Ok(Answer::Text(hotbar_slot(view.slots, view.selected).0)),
        Query::Selected => Ok(Answer::Int(i64::from(view.selected))),
        Query::Present => Ok(Answer::Bool(view.eye.is_some())),
        Query::PlayerX => Ok(Answer::Float(axis(view, 0))),
        Query::PlayerY => Ok(Answer::Float(axis(view, 1))),
        Query::PlayerZ => Ok(Answer::Float(axis(view, 2))),
        Query::Exists { id } => Ok(Answer::Bool(pack.contains_item(&id))),
        Query::MaxCount { id } => Ok(Answer::Int(i64::from(pack.max_stack(&id)))),
        Query::Say { text } => {
            let line = crate::net::chat_line(&text);
            if line.is_empty() {
                return Ok(Answer::Bool(false));
            }
            lines.borrow_mut().push(line);
            Ok(Answer::Bool(true))
        }
        Query::Setting { name } => {
            let value = pack
                .settings()
                .into_iter()
                .find(|option| option.name == name)
                .map(|option| option.value)
                .unwrap_or_default();
            Ok(Answer::Text(value))
        }
        Query::SpawnEntity { id, x, y, z } => {
            if !matches!(gate, ScriptWrites::Open) {
                return Ok(Answer::Bool(false));
            }
            if pack.note_spawn(&id, x, y, z, "").is_err() {
                return Ok(Answer::Bool(false));
            }
            live_spawns.borrow_mut().push(Ok((id, x, y, z)));
            Ok(Answer::Bool(true))
        }
        Query::DespawnEntity { id, x, y, z } => {
            if !matches!(gate, ScriptWrites::Open) {
                return Ok(Answer::Bool(false));
            }
            if !pack.take_spawn(&id, x, y, z) {
                return Ok(Answer::Bool(false));
            }
            live_spawns.borrow_mut().push(Err((id, x, y, z)));
            Ok(Answer::Bool(true))
        }
        Query::TileGet { x, y, z, key } => Ok(Answer::Text(pack.tile_get(x, y, z, &key))),
        Query::TileSet {
            x,
            y,
            z,
            key,
            value,
        } => {
            if !world.in_bounds(x, y, z) {
                return Ok(Answer::Bool(false));
            }
            let id = world.block_at(x, y, z).id.to_string();
            Ok(Answer::Bool(pack.tile_set(x, y, z, &id, &key, &value)))
        }
        Query::TileGetInt { x, y, z, key } => {
            Ok(Answer::Int(pack.tile_get_int(x, y, z, &key)))
        }
        Query::TileSetInt {
            x,
            y,
            z,
            key,
            value,
        } => {
            if !world.in_bounds(x, y, z) {
                return Ok(Answer::Bool(false));
            }
            let id = world.block_at(x, y, z).id.to_string();
            Ok(Answer::Bool(pack.tile_set_int(
                x,
                y,
                z,
                &id,
                &key,
                i64::from(value),
            )))
        }
        Query::TileClear { x, y, z } => {
            let had = pack.tile_has(x, y, z);
            pack.clear_tile_at(x, y, z);
            Ok(Answer::Bool(had))
        }
        Query::TileHas { x, y, z } => Ok(Answer::Bool(pack.tile_has(x, y, z))),
        Query::OpenGui { name } => {
            if !pack.has_gui_screen(&name) {
                return Ok(Answer::Bool(false));
            }
            pending_screens.borrow_mut().push(name);
            Ok(Answer::Bool(true))
        }
    }
}

/// A settings flip may place one light into air, or clear a light back to air,
/// within four blocks of the player's eye. Anything else stays.
fn soft_light(
    world: &mut Cluster,
    view: &ScriptView<'_>,
    x: i32,
    y: i32,
    z: i32,
    block: Block,
) -> bool {
    let Some(eye) = view.eye else {
        return false;
    };
    if !eye[0].is_finite() || !eye[1].is_finite() || !eye[2].is_finite() {
        return false;
    }
    let ex = eye[0].floor() as i32;
    let ey = eye[1].floor() as i32;
    let ez = eye[2].floor() as i32;
    if (x - ex).abs() > 4 || (y - ey).abs() > 4 || (z - ez).abs() > 4 {
        return false;
    }
    if !world.in_bounds(x, y, z) {
        return false;
    }
    let pack = Pack::builtin();
    let current = world.block_at(x, y, z);
    let placing = !block.is_air() && pack.light_of(block).is_some();
    let clearing = block.is_air() && pack.light_of(current).is_some();
    if placing && !current.is_air() {
        return false;
    }
    if !placing && !clearing {
        return false;
    }
    world.set_block(x, y, z, block)
}

fn axis(view: &ScriptView<'_>, index: usize) -> f64 {
    view.eye.map(|eye| f64::from(eye[index])).unwrap_or(0.0)
}

fn hotbar_slot(slots: &[(String, i32)], index: i32) -> (String, i32) {
    if !(1..=slots.len() as i32).contains(&index) {
        return (String::new(), 0);
    }
    let (id, count) = &slots[(index - 1) as usize];
    (id.clone(), *count)
}

/// `spark` matches `spark` and `wand:spark`. A leading slash is ignored.
fn command_hit(hook_id: &str, asked: &str) -> bool {
    let asked = asked.trim().trim_start_matches('/');
    let hook_name = command_label(hook_id);
    let asked_name = command_label(asked);
    hook_id == asked || hook_name == asked || hook_name == asked_name
}

fn command_label(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

fn push_reply(reply: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !reply.is_empty() {
        reply.push('\n');
    }
    reply.push_str(text);
}

fn emit_script_output(text: &str) {
    if text.is_empty() {
        return;
    }
    print!("{text}");
    #[cfg(test)]
    PLACE_LOGS.with(|logs| {
        for line in text.lines() {
            if !line.is_empty() {
                logs.borrow_mut().push(line.to_string());
            }
        }
    });
}

#[cfg(test)]
thread_local! {
    static PLACE_LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::Chunk;
    use crate::edit::{Edit, PlayerBox};

    // Hook probes live under testdata so the live mods folder stays demos-only.
    const SHIPPED: &str = include_str!("../testdata/hook_mods/keep-stone.rg");
    const LOG_PLACE: &str = include_str!("../testdata/hook_mods/log-place.rg");
    const LOG_BREAK: &str = include_str!("../testdata/hook_mods/log-break.rg");
    const LOG_PLAYER: &str = include_str!("../testdata/hook_mods/log-player.rg");
    const WOOL_MOD: &str = include_str!("../testdata/hook_mods/red-wool.rg");
    const ROSE_KIT: &str = include_str!("../../../mods/rose-kit.rg");
    const USE_WAND: &str = include_str!("../testdata/hook_mods/use-wand.rg");
    const SPARK: &str = include_str!("../testdata/hook_mods/spark.rg");
    const USE_MOB: &str = include_str!("../testdata/hook_mods/use-mob.rg");
    const ATTACK: &str = include_str!("../testdata/hook_mods/attack.rg");
    const HUSH: &str = include_str!("../testdata/hook_mods/hush.rg");
    const RESPAWN: &str = include_str!("../testdata/hook_mods/respawn.rg");
    const CRAFT: &str = include_str!("../testdata/hook_mods/craft.rg");
    const SMELT: &str = include_str!("../testdata/hook_mods/smelt.rg");
    const REPAIR: &str = include_str!("../testdata/hook_mods/repair.rg");
    const ENCHANT: &str = include_str!("../testdata/hook_mods/enchant.rg");
    const BREW: &str = include_str!("../testdata/hook_mods/brew.rg");
    const SMITH: &str = include_str!("../testdata/hook_mods/smith.rg");
    const WEAVE: &str = include_str!("../testdata/hook_mods/weave.rg");
    const TICK: &str = include_str!("../testdata/hook_mods/tick.rg");
    const DAWN: &str = include_str!("../testdata/hook_mods/dawn.rg");
    const DUSK: &str = include_str!("../testdata/hook_mods/dusk.rg");
    const JUMP: &str = include_str!("../testdata/hook_mods/jump.rg");
    const SPRINT: &str = include_str!("../testdata/hook_mods/sprint.rg");
    const LAND: &str = include_str!("../testdata/hook_mods/land.rg");
    const SEAL: &str = include_str!("../testdata/hook_mods/seal.rg");

    fn hand(slots: &[(String, i32)]) -> ScriptView<'_> {
        ScriptView {
            slots,
            selected: 1,
            eye: None,
            time: None,
            season: "",
        }
    }

    fn sample() -> Cluster {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 1, 1, Block::new("stone", 0)),
                (2, 1, 1, Block::new("grass", 0)),
                (3, 1, 1, Block::new("coal_ore", 0)),
            ],
        )
        .unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    fn far() -> PlayerBox {
        PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        }
    }

    fn lines() -> Vec<String> {
        PLACE_LOGS.with(|logs| logs.borrow().clone())
    }

    #[test]
    fn a_chat_event_can_swallow_one_line() {
        let mut hooks = Hooks::new();
        let loaded = load_source(HUSH, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "hush-secret" && matches!(hook, ScriptHook::Chat { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.chat(&mut world, "hello", &view));
        assert!(hooks.chat(&mut world, "/secret", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.chat(&mut world, "secret", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_respawn_event_marks_a_death_and_can_keep_you_awake() {
        let mut hooks = Hooks::new();
        let loaded = load_source(RESPAWN, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "mark-return" && matches!(hook, ScriptHook::Respawn { .. })
        }));
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.respawn(&mut world, "death", 1, 1, 1, &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        assert!(!hooks.respawn(&mut world, "bed", 2, 1, 1, &view));
        assert!(hooks.respawn(&mut world, "anchor", 3, 1, 1, &view));
        assert!(world.block_at(2, 2, 1).is_air());
    }

    #[test]
    fn a_craft_event_can_refuse_a_stick() {
        let mut hooks = Hooks::new();
        let loaded = load_source(CRAFT, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "no-stick" && matches!(hook, ScriptHook::Craft { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.craft(&mut world, "torch", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.craft(&mut world, "stick", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_smelt_event_can_refuse_an_ingot() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SMELT, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "keep-iron" && matches!(hook, ScriptHook::Smelt { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.smelt(&mut world, "stone", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.smelt(&mut world, "iron_ingot", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_repair_event_can_keep_the_iron_ore() {
        let mut hooks = Hooks::new();
        let loaded = load_source(REPAIR, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "keep-ore" && matches!(hook, ScriptHook::Repair { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.repair(&mut world, "wooden_pickaxe", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.repair(&mut world, "iron_pickaxe", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn an_enchant_event_can_keep_the_bookshelf() {
        let mut hooks = Hooks::new();
        let loaded = load_source(ENCHANT, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "keep-book" && matches!(hook, ScriptHook::Enchant { .. })
        }));
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.enchant(&mut world, "wooden_pickaxe", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.enchant(&mut world, "iron_pickaxe", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_brew_event_can_keep_the_nether_wart() {
        let mut hooks = Hooks::new();
        let loaded = load_source(BREW, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "keep-wart" && matches!(hook, ScriptHook::Brew { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.brew(&mut world, "flower", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.brew(&mut world, "nether_wart_block", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_smith_event_can_keep_an_iron_pick() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SMITH, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "keep-pick" && matches!(hook, ScriptHook::Smith { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.smith(&mut world, "stone_pickaxe", &view));
        assert!(hooks.smith(&mut world, "mace", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.smith(&mut world, "iron_pickaxe", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_weave_event_can_keep_the_wool() {
        let mut hooks = Hooks::new();
        let loaded = load_source(WEAVE, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "keep-wool" && matches!(hook, ScriptHook::Weave { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(hooks.weave(&mut world, "string", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.weave(&mut world, "wool", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_tick_event_marks_the_first_second() {
        let mut hooks = Hooks::new();
        let loaded = load_source(TICK, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "one-mark" && matches!(hook, ScriptHook::Tick { .. }))
        );
        let mut world = sample();
        hooks.drive_entities(&mut world, 19);
        assert!(world.block_at(1, 2, 1).is_air());
        hooks.drive_entities(&mut world, 1);
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        assert_eq!(hooks.drain_writes().len(), 1);
        hooks.drive_entities(&mut world, 20);
        assert!(hooks.drain_writes().is_empty());
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_dawn_event_marks_the_second_morning() {
        let mut hooks = Hooks::new();
        let loaded = load_source(DAWN, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "first-light" && matches!(hook, ScriptHook::Dawn { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        hooks.dawn(&mut world, 0, &view);
        assert!(world.block_at(1, 2, 1).is_air());
        hooks.dawn(&mut world, 1, &view);
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_dusk_event_marks_the_opening_afternoon() {
        let mut hooks = Hooks::new();
        let loaded = load_source(DUSK, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "last-light" && matches!(hook, ScriptHook::Dusk { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        hooks.dusk(&mut world, 1, &view);
        assert!(world.block_at(1, 2, 1).is_air());
        hooks.dusk(&mut world, 0, &view);
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
    }

    #[test]
    fn a_jump_event_can_stay_down_or_mark_the_leap() {
        let mut hooks = Hooks::new();
        let loaded = load_source(JUMP, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "stay-down" && matches!(hook, ScriptHook::Jump { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(!hooks.jump(&mut world, 1, 4, 1, &view));
        assert!(world.block_at(1, 5, 1).is_air());
        assert!(hooks.jump(&mut world, 1, 2, 1, &view));
        assert_eq!(world.block_at(1, 3, 1).id, "dirt");
    }

    #[test]
    fn a_sprint_event_can_stay_slow_or_mark_the_dash() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SPRINT, &mut hooks).unwrap();
        assert!(
            loaded.iter().any(|hook| {
                hook.id() == "stay-slow" && matches!(hook, ScriptHook::Sprint { .. })
            })
        );
        let mut world = sample();
        let view = hand(&[]);
        assert!(!hooks.sprint(&mut world, 1, 4, 1, &view));
        assert!(world.block_at(1, 5, 1).is_air());
        assert!(hooks.sprint(&mut world, 1, 2, 1, &view));
        assert_eq!(world.block_at(1, 3, 1).id, "dirt");
    }

    #[test]
    fn a_land_event_marks_a_fall_of_one_block() {
        let mut hooks = Hooks::new();
        let loaded = load_source(LAND, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "mark-fall" && matches!(hook, ScriptHook::Land { .. }))
        );
        let mut world = sample();
        let view = hand(&[]);
        hooks.land(&mut world, 1, 2, 1, 0, &view);
        assert!(world.block_at(1, 3, 1).is_air());
        hooks.land(&mut world, 1, 2, 1, 1, &view);
        assert_eq!(world.block_at(1, 3, 1).id, "dirt");
    }

    #[test]
    fn an_attack_event_can_consume_a_stone_break_and_a_wolf_hit() {
        let mut hooks = Hooks::new();
        let loaded = load_source(ATTACK, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "glove-guard" && matches!(hook, ScriptHook::Attack { .. })
        }));
        let mut world = sample();
        let glove = [("hit:glove".to_string(), 1)];
        let view = hand(&glove);
        assert!(hooks.attack(&mut world, "apple", "stone", &view));
        assert!(hooks.attack(&mut world, "hit:glove", "grass", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.attack(&mut world, "hit:glove", "stone", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        assert!(!hooks.attack(&mut world, "hit:glove", "wolf", &view));
        assert!(Pack::builtin().contains_item("hit:glove"));
    }

    #[test]
    fn a_use_entity_event_can_consume_a_wolf_click() {
        let mut hooks = Hooks::new();
        let loaded = load_source(USE_MOB, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "calm-wolf" && matches!(hook, ScriptHook::UseEntity { .. })
        }));
        let mut world = sample();
        let calm = [("mob:calm".to_string(), 1)];
        let view = hand(&calm);
        assert!(hooks.use_entity(&mut world, "apple", "wolf", &view));
        assert!(hooks.use_entity(&mut world, "mob:calm", "pig", &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.use_entity(&mut world, "mob:calm", "wolf", &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        assert!(Pack::builtin().contains_item("mob:calm"));
    }

    #[test]
    fn a_command_callback_replies_and_can_write_a_block() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SPARK, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "spark" && matches!(hook, ScriptHook::Command { .. }))
        );
        assert!(
            hooks
                .script_content()
                .iter()
                .any(|(kind, id)| kind == "COMMAND" && id == "spark")
        );
        let mut world = sample();
        let view = hand(&[]);
        assert_eq!(
            hooks.command(&mut world, "spark", "", &view).as_deref(),
            Some("spark")
        );
        assert_eq!(
            hooks.command(&mut world, "/spark", "hi", &view).as_deref(),
            Some("spark hi")
        );
        assert!(hooks.command(&mut world, "home", "", &view).is_none());
        assert!(world.block_at(1, 2, 1).is_air());
        assert_eq!(
            hooks
                .command(&mut world, "wand:spark", "dirt", &view)
                .as_deref(),
            Some("spark dirt")
        );
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        let writes = hooks.drain_writes();
        assert!(writes.iter().any(|change| {
            change.x == 1 && change.y == 2 && change.z == 1 && change.block.id == "dirt"
        }));
    }

    #[test]
    fn a_use_event_plants_on_the_block_and_can_cancel_the_click() {
        let mut hooks = Hooks::new();
        let loaded = load_source(USE_WAND, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "wand-plant" && matches!(hook, ScriptHook::UseBlock { .. })
        }));
        assert!(
            loaded
                .iter()
                .any(|hook| hook.id() == "wand-hold" && matches!(hook, ScriptHook::UseItem { .. }))
        );
        let mut world = sample();
        let rose = [("wand:rose".to_string(), 1)];
        let view = hand(&rose);
        assert!(hooks.use_item(&mut world, "apple", &view));
        assert!(!hooks.use_item(&mut world, "wand:rose", &view));
        assert!(hooks.use_block(&mut world, "apple", "stone", 1, 1, 1, &view));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!hooks.use_block(&mut world, "wand:rose", "stone", 1, 1, 1, &view));
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");
        let writes = hooks.drain_writes();
        assert!(writes.iter().any(|change| {
            change.x == 1 && change.y == 2 && change.z == 1 && change.block.id == "dirt"
        }));
        assert!(Pack::builtin().contains_item("wand:rose"));
    }

    #[test]
    fn the_shipped_script_cancels_stone_and_lets_grass_break() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SHIPPED, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![ScriptHook::BeforeBreak {
                id: "keep-stone".into(),
            }]
        );
        let mut world = sample();
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 1, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(1, 1, 1), Block::new("stone", 0));
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(2, 1, 1).is_air());
    }

    #[test]
    fn the_shipped_seal_consumes_one_dig_and_one_place() {
        let mut hooks = Hooks::new();
        let loaded = load_source(SEAL, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| {
            hook.id() == "seal-dig" && matches!(hook, ScriptHook::BreakBlock { .. })
        }));
        assert!(loaded.iter().any(|hook| {
            hook.id() == "seal-place" && matches!(hook, ScriptHook::BeforePlace { .. })
        }));
        let mut world = sample();
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 1,
                block: Block::new("cobblestone", 0),
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(4, 1, 1), Block::new("cobblestone", 0));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 5,
                y: 1,
                z: 1,
                block: Block::new("seal:wax", 0),
                player: far(),
            },
            &hooks
        ));
        assert!(world.block_at(5, 1, 1).is_air());
        assert_eq!(world.block_at(1, 2, 1).id, "dirt");

        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 6,
                y: 1,
                z: 1,
                block: Block::new("seal:cap", 0),
                player: far(),
            },
            &hooks
        ));
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 6, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(6, 1, 1), Block::new("seal:cap", 0));
        assert_eq!(world.block_at(6, 2, 1).id, "dirt");
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(2, 1, 1).is_air());
    }

    #[test]
    fn a_missing_mods_folder_registers_nothing() {
        let mut hooks = Hooks::new();
        let loaded = load_dir(
            Path::new(r"C:\CODE\TerraLite\mods\no-such-folder"),
            &mut hooks,
        )
        .unwrap();
        assert!(loaded.is_empty());
        let mut world = sample();
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 1, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(1, 1, 1).is_air());
    }

    #[test]
    fn a_bad_script_and_a_duplicate_hook_fail_to_load() {
        let mut hooks = Hooks::new();
        let err = load_source("fn main(: Int { return 0; }\n", &mut hooks).unwrap_err();
        assert!(
            err.contains("parse error") || err.contains("expected"),
            "{err}"
        );

        let err = load_source(
            "fn allow(block: String): Bool { return true; }\nfn main(): Int { hooks.before_break(\"\", allow); return 0; }\n",
            &mut Hooks::new(),
        )
        .unwrap_err();
        assert!(err.contains("empty"), "{err}");

        let err = load_source(
            "fn allow(block: String): Bool { return false; }\nfn main(): Int {\n    hooks.before_break(\"keep-stone\", allow);\n    hooks.before_break(\"keep-stone\", allow);\n    return 0;\n}\n",
            &mut Hooks::new(),
        )
        .unwrap_err();
        assert!(err.contains("duplicate"), "{err}");

        let err = load_source(
            "fn boom(block: String) throws: Bool { throw \"nope\"; }\nfn main(): Int { hooks.before_break(\"keep-stone\", boom); return 0; }\n",
            &mut hooks,
        )
        .unwrap();
        assert_eq!(
            err[0],
            ScriptHook::BeforeBreak {
                id: "keep-stone".into()
            }
        );
        let mut world = sample();
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(2, 1, 1), Block::new("grass", 0));
    }

    #[test]
    fn the_shipped_after_place_script_logs_a_successful_place_only() {
        let mut hooks = Hooks::new();
        let loaded = load_source(LOG_PLACE, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![ScriptHook::AfterPlace {
                id: "log-place".into(),
            }]
        );
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 1,
                block: Block::new("cobblestone", 0),
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(4, 1, 1), Block::new("cobblestone", 0));
        assert_eq!(lines(), vec!["rosegold: placed cobblestone at 4, 1, 1"]);

        let overlapping = PlayerBox {
            min: [4.0, 2.0, 1.0],
            max: [4.6, 3.8, 1.6],
        };
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 4,
                y: 2,
                z: 1,
                block: Block::new("dirt", 0),
                player: overlapping,
            },
            &hooks
        ));
        assert!(lines().is_empty());
        assert!(world.block_at(4, 2, 1).is_air());
    }

    #[test]
    fn the_shipped_after_break_script_logs_the_block_left_behind() {
        let mut hooks = Hooks::new();
        let loaded = load_source(LOG_BREAK, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![ScriptHook::AfterBreak {
                id: "log-break".into(),
            }]
        );
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 3, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(3, 1, 1), Block::new("stone", 0));
        assert_eq!(
            lines(),
            vec!["rosegold: broke coal_ore at 3, 1, 1 left stone"]
        );
    }

    #[test]
    fn a_script_can_refuse_a_place_on_stone_and_read_the_pack() {
        let source = r#"
fn allow(id: String, x: Int, y: Int, z: Int): Bool {
    return world.block(x, y - 1, z) != "stone";
}

fn broke(id: String, x: Int, y: Int, z: Int, left: String) {
    var here = world.block(x, y, z);
    var next = world.replaces(id);
    var hard = world.hardness(id);
    var glow = world.light("lantern");
    var outside = world.block(0, -1, 0);
    print(id + " left " + left + " now " + here + " replaces " + next + " hard " + hard + " glow " + glow + " outside [" + outside + "]");
}

fn main(): Int {
    hooks.before_place("no-place-on-stone", allow);
    hooks.after_break("read-pack", broke);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: Block::new("dirt", 0),
                player: far(),
            },
            &hooks
        ));
        assert!(world.block_at(1, 2, 1).is_air());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 5,
                y: 1,
                z: 1,
                block: Block::new("dirt", 0),
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(5, 1, 1), Block::new("dirt", 0));

        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(
            lines(),
            vec!["grass left air now air replaces air hard 0.25 glow 12 outside []"]
        );
    }

    #[test]
    fn the_shipped_player_script_logs_a_hotbar_pick_and_a_screen() {
        let mut hooks = Hooks::new();
        let loaded = load_source(LOG_PLAYER, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![
                ScriptHook::OnSelect {
                    id: "log-select".into(),
                },
                ScriptHook::OnScreen {
                    id: "log-screen".into(),
                },
            ]
        );
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        let slots = [("cobblestone".to_string(), 64)];
        let view = hand(&slots);
        hooks.select(&mut world, 1, "cobblestone", &view);
        hooks.screen(&mut world, "index", true, &view);
        hooks.screen(&mut world, "index", false, &view);
        assert_eq!(
            lines(),
            vec![
                "rosegold: selected 1 cobblestone",
                "rosegold: opened index",
                "rosegold: closed index",
            ]
        );
    }

    #[test]
    fn a_script_scales_a_break_reads_the_hotbar_and_writes_a_block() {
        let source = r#"
fn speed(id: String): Float {
    if (id == "grass") {
        return 0.5;
    }
    return 1.0;
}

fn picked(slot: Int, id: String) {
    var name = world.slot(slot);
    var n = world.count(slot);
    if (name == "cobblestone" && n == 64 && id == "cobblestone") {
        print("hotbar cobblestone 64");
    }
}

fn broke(id: String, x: Int, y: Int, z: Int, left: String) {
    if (id == "grass" && world.block(x, y + 1, z) == "air") {
        world.write(x, y + 1, z, "dirt");
        world.write(x, y + 1, z, "not-a-block");
    }
}

fn main(): Int {
    hooks.break_speed("field-notes", speed);
    hooks.on_select("field-notes", picked);
    hooks.after_break("field-notes", broke);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        let slots = [("cobblestone".to_string(), 64)];
        let view = hand(&slots);
        assert!((hooks.break_scale(&mut world, "grass", &view) - 0.5).abs() < 0.001);
        assert!((hooks.break_scale(&mut world, "stone", &view) - 1.0).abs() < 0.001);
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        hooks.select(&mut world, 1, "cobblestone", &view);
        assert_eq!(lines(), vec!["hotbar cobblestone 64"]);
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        assert_eq!(world.block_at(2, 2, 1), Block::new("dirt", 0));
        let extra = hooks.drain_writes();
        assert_eq!(extra.len(), 1);
        assert_eq!((extra[0].x, extra[0].y, extra[0].z), (2, 2, 1));
        assert_eq!(extra[0].block, Block::new("dirt", 0));
    }

    #[test]
    fn a_fabric_style_mod_dyes_wool_swaps_a_drop_and_refuses_dirt() {
        let mut hooks = Hooks::new();
        let loaded = load_source(WOOL_MOD, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![
                ScriptHook::AfterPlace {
                    id: "red-wool".into(),
                },
                ScriptHook::BreakDrop {
                    id: "stone-to-cobble".into(),
                },
                ScriptHook::PickupItem {
                    id: "leave-dirt".into(),
                },
            ]
        );
        let mut world = sample();
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 5,
                y: 1,
                z: 1,
                block: Block::new("wool", 0),
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(5, 1, 1), Block::new("wool", 14));
        let dyed = hooks.drain_writes();
        assert_eq!(dyed.len(), 1);
        assert_eq!(dyed[0].block, Block::new("wool", 14));

        let slots = [("cobblestone".to_string(), 64)];
        let player = ScriptView {
            slots: &slots,
            selected: 1,
            eye: Some([2.0, 4.0, 2.0]),
            time: Some(0.5),
            season: "Spring",
        };
        assert_eq!(
            hooks
                .break_drop(&mut world, "stone", "air", &player)
                .as_deref(),
            Some("cobblestone")
        );
        assert_eq!(
            hooks
                .break_drop(&mut world, "grass", "air", &player)
                .as_deref(),
            Some("grass")
        );
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(!hooks.allow_pickup(&mut world, "dirt", &player));
        assert!(lines().is_empty());
        assert!(hooks.allow_pickup(&mut world, "stone", &player));
        assert_eq!(lines(), vec!["day"]);
        assert!(!hooks.allow_pickup(&mut world, "not-a-block", &player));
        assert!(!hooks.allow_pickup(&mut world, "dirt", &ScriptView::bare()));
    }

    #[test]
    fn a_loot_hook_replaces_one_chest_pick_and_skips_an_unknown_id() {
        let source = r#"
fn on_loot(item: String, x: Int, z: Int, omen: Bool): String {
    if (item == "apple" && x == 1 && z == 2 && omen == false) {
        return "dirt";
    }
    if (item == "mace") {
        return "nope";
    }
    return "";
}

fn main(): Int {
    events.loot("rose-for-apple", on_loot);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        let loaded = load_source(source, &mut hooks).unwrap();
        assert_eq!(
            loaded,
            vec![ScriptHook::Loot {
                id: "rose-for-apple".into()
            }]
        );
        let mut world = sample();
        let view = ScriptView::bare();
        assert_eq!(
            hooks
                .loot(&mut world, "apple", 1, 2, false, &view)
                .as_deref(),
            Some("dirt")
        );
        assert_eq!(hooks.loot(&mut world, "apple", 1, 2, true, &view), None);
        assert_eq!(hooks.loot(&mut world, "iron_ore", 1, 2, false, &view), None);
        assert_eq!(hooks.loot(&mut world, "mace", 0, 0, true, &view), None);
    }

    #[test]
    fn break_block_sees_the_cell_and_can_cancel() {
        let source = r#"
fn on_break(block: String, x: Int, y: Int, z: Int): Bool {
    if (block == "stone" && x == 1 && y == 1 && z == 1 && world.get_block(x, y, z) == "stone") {
        return false;
    }
    return true;
}

fn main(): Int {
    events.break_block("keep-this-stone", on_break);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 1, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(1, 1, 1), Block::new("stone", 0));
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(2, 1, 1).is_air());
    }

    #[test]
    fn a_mod_registers_a_block_and_item_and_sees_the_server_hotbar() {
        let mut hooks = Hooks::new();
        let loaded = load_source(ROSE_KIT, &mut hooks).unwrap();
        assert!(loaded.iter().any(|hook| matches!(
            hook,
            ScriptHook::EntitySpawn {
                id
            } if id == "rose-kit-spawn"
        )));
        let pack = Pack::builtin();
        let rose_block = Block::new("rosegold:rose_block", 0);
        assert!(pack.contains_block("rosegold:rose_block"));
        assert!(pack.shape_of(rose_block).is_full_cube());
        assert_eq!(
            pack.texture_of(rose_block),
            Some("assets/rosegold/textures/rose_block.png")
        );
        assert!(crate::asset_png("assets/rosegold/textures/rose_block.png").is_some());
        assert!(crate::asset_png("assets/terralite/textures/water.png").is_some());
        assert_eq!(
            pack.placed_block("rosegold:rose").as_deref(),
            Some("rosegold:rose_block")
        );
        assert_eq!(pack.hardness_of(rose_block), Some(1.5));
        assert!(pack.contains_item("rosegold:rose"));
        assert!(pack.contains_item("rosegold:rose_block"));
        assert!(!pack.contains_block("rosegold:rose"));
        let bud = Block::new("rosegold:rose_bud", 0);
        assert!(pack.contains_block("rosegold:rose_bud"));
        assert!(
            !pack.shape_of(bud).is_full_cube(),
            "the mod-folder model is the small bud, not a full cube"
        );
        assert!(pack.script_recipes().iter().any(|(result, ingredient)| {
            result == "rosegold:rose_block" && ingredient == "rosegold:rose"
        }));
        assert!(
            pack.script_recipe_lines()
                .iter()
                .any(|(result, ingredients)| {
                    result == "rosegold:bouquet"
                        && ingredients
                            == &["rosegold:rose".to_string(), "rosegold:rose".to_string()]
                })
        );
        assert!(
            pack.script_entities()
                .iter()
                .any(|id| id == "rosegold:wisp")
        );
        assert!(hooks.scripts.mobs().iter().any(|id| id == "rosegold:wisp"));
        assert!(
            pack.script_spawns()
                .iter()
                .any(|(id, x, y, z)| { id == "rosegold:wisp" && *x == 12 && *y == 1 && *z == 14 })
        );
        assert!(loaded.iter().any(|hook| matches!(
            hook,
            ScriptHook::Loot { id } if id == "rose-kit-loot"
        )));
        let bare = ScriptView::bare();
        assert_eq!(
            hooks
                .loot(&mut sample(), "apple", 4, 1, false, &bare)
                .as_deref(),
            Some("rosegold:rose")
        );
        assert_eq!(
            hooks
                .loot(&mut sample(), "iron_ore", 4, 1, false, &bare)
                .as_deref(),
            Some("iron_ore")
        );
        assert!(pack.script_patterns().iter().any(|(result, cells)| {
            result == "rosegold:rose_post"
                && cells[0] == "rosegold:rose"
                && cells[3] == "rosegold:rose"
                && cells[6] == "rosegold:rose"
                && cells[1].is_empty()
        }));

        let probe = r#"
fn look(id: String, x: Int, y: Int, z: Int) {
    if (id == "wool" && item.exists("rosegold:rose") && !world.set_block(x, y, z, "rosegold:rose")) {
        print("item rejected");
    }
    if (world.set_block(x, y + 1, z, "rosegold:rose_block")) {
        print("block accepted");
    }
}

fn main(): Int {
    events.placed("rose-kit-probe", look);
    return 0;
}
"#;
        load_source(probe, &mut hooks).unwrap();
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 5,
                y: 1,
                z: 1,
                block: Block::new("wool", 0),
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(5, 1, 1), Block::new("wool", 0));
        assert_eq!(world.block_at(5, 2, 1), rose_block);
        assert_eq!(lines(), vec!["item rejected", "block accepted"]);
        let _ = hooks.drain_writes();

        hooks.set_inventory(
            2,
            vec![
                ("cobblestone".into(), 64),
                ("rosegold:rose_block".into(), 64),
            ],
        );
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        assert!(crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 6,
                y: 1,
                z: 1,
                block: rose_block,
                player: far(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(6, 1, 1), rose_block);
        assert_eq!(lines(), vec!["placed rose", "block accepted"]);
        assert!(!crate::apply_authoritative(
            &mut world,
            Edit::Place {
                x: 7,
                y: 1,
                z: 1,
                block: Block::new("rosegold:rose", 0),
                player: far(),
            },
            &hooks
        ));
        assert!(world.block_at(7, 1, 1).is_air());

        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        hooks.entity_spawn(&mut world, "player", 8, 3, 14);
        assert_eq!(world.block_at(9, 3, 14), rose_block);
        assert!(
            hooks
                .drain_writes()
                .iter()
                .any(|cell| cell.x == 9 && cell.y == 3 && cell.z == 14 && cell.block == rose_block)
        );
        assert_eq!(lines(), vec!["spawned player"]);
        assert_eq!(hooks.entity_hurt(&mut world, "player", 4), 4);
        assert_eq!(hooks.entity_hurt(&mut world, "player", -1), 0);
        assert_eq!(hooks.entity_hurt(&mut world, "sheep", -1), 0);
        hooks.entity_tick(&mut world, "player");
        assert_eq!(lines(), vec!["spawned player"]);
    }

    #[test]
    fn entity_hurt_can_cancel_or_rewrite_the_amount() {
        let source = r#"
fn half(kind: String, amount: Int): Int {
    if (kind == "zombie") {
        return amount / 2;
    }
    if (kind == "player") {
        return 0;
    }
    return amount;
}

fn main(): Int {
    events.entity_hurt("rewrite-hurt", half);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        assert_eq!(hooks.entity_hurt(&mut world, "zombie", 4), 2);
        assert_eq!(hooks.entity_hurt(&mut world, "player", 3), 0);
        assert_eq!(hooks.entity_hurt(&mut world, "pig", 5), 5);
        assert_eq!(hooks.entity_hurt(&mut world, "zombie", 0), 0);
    }

    #[test]
    fn entity_spawn_places_a_body_and_despawn_takes_it_back() {
        let source = r#"
fn main(): Int {
    entity.register("rosegold", "mote");
    entity.register("rosegold", "glint");
    if (!entity.spawn("rosegold:mote", 3, 1, 3)) {
        return 1;
    }
    if (entity.spawn("rosegold:mote", 3, 1, 3)) {
        return 2;
    }
    if (entity.spawn("missing:thing", 1, 1, 1)) {
        return 3;
    }
    if (!entity.spawn("rosegold:glint", 4, 1, 4)) {
        return 4;
    }
    if (!entity.despawn("rosegold:glint", 4, 1, 4)) {
        return 5;
    }
    if (entity.despawn("rosegold:glint", 4, 1, 4)) {
        return 6;
    }
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let spawns = Pack::builtin().script_spawns();
        assert!(
            spawns
                .iter()
                .any(|(id, x, y, z)| id == "rosegold:mote" && *x == 3 && *y == 1 && *z == 3)
        );
        assert!(!spawns.iter().any(|(id, _, _, _)| id == "rosegold:glint"));
        assert!(!spawns.iter().any(|(id, _, _, _)| id == "missing:thing"));
    }

    #[test]
    fn entity_ticks_drive_the_player_and_the_mobs() {
        let source = r#"
fn on_tick(kind: String) {
    print(kind);
}

fn main(): Int {
    events.entity_tick("tick-drive", on_tick);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        hooks.drive_entities(&mut world, 2);
        assert_eq!(
            lines(),
            vec![
                "player".to_string(),
                "sheep".to_string(),
                "pig".to_string(),
                "zombie".to_string(),
                "player".to_string(),
                "sheep".to_string(),
                "pig".to_string(),
                "zombie".to_string()
            ]
        );
        hooks.drive_entities(&mut world, 40);
        assert_eq!(lines().len(), 8 + 80);
    }

    #[test]
    fn a_mod_folder_loads_its_script() {
        let root = std::env::temp_dir().join("terralite-mod-folder-d4ab");
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("kit");
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(
            dir.join("main.rg"),
            r#"
fn main(): Int {
    block.register("modkit", "folder_block");
    return 0;
}
"#,
        )
        .unwrap();
        let mut hooks = Hooks::new();
        let loaded = load_dir(&root, &mut hooks).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(Pack::builtin().contains_block("modkit:folder_block"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_guide_mod_loads_from_src_and_registers() {
        let root = std::env::temp_dir().join("terralite-guide-mod-d4ab");
        let _ = fs::remove_dir_all(&root);
        let from = Path::new(r"C:\CODE\TerraLite\mods\k's-guide");
        for rel in [
            "src/mod.rg",
            "data/guide/blocks/marker.json",
            "data/guide/blocks/post.json",
            "data/guide/blocks/chest.json",
            "data/guide/recipes/marker.json",
            "data/guide/recipes/post.json",
            "data/guide/recipes/chest.json",
            "assets/guide/gui/book.json",
        ] {
            let dest = root.join("ks-guide").join(rel);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::copy(from.join(rel), &dest).unwrap();
        }
        let mut hooks = Hooks::new();
        let loaded = load_dir(&root, &mut hooks).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].0.ends_with("mod.rg"));
        assert!(loaded[0].1.is_empty(), "on_init does not register a hook");
        let kinds: Vec<&str> = hooks
            .scripts
            .api
            .iter()
            .filter(|(kind, _)| kind == "registry")
            .map(|(_, kind)| kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "ITEM",
                "BLOCK",
                "ENTITY",
                "BLOCK_ENTITY",
                "ITEM_GROUP",
                "SCREEN",
                "ITEM_GROUP",
                "EVENT",
                "COMMAND",
                "CONFIG",
                "RESOURCE",
                "TEXTURE",
                "PARTICLE",
            ]
        );
        assert!(
            hooks
                .scripts
                .api
                .iter()
                .any(|(kind, text)| { kind == "note" && text == "k's guide: k's guide loaded" })
        );
        let pack = Pack::builtin();
        assert!(pack.contains_block("guide:marker"));
        assert!(
            pack.shape_of(crate::Block::new("guide:marker", 0))
                .is_full_cube()
        );
        assert_eq!(
            pack.hardness_of(crate::Block::new("guide:marker", 0)),
            Some(2.5)
        );
        assert_eq!(
            pack.placed_block("guide:page").as_deref(),
            Some("guide:marker")
        );
        assert!(pack.script_recipes().iter().any(|(result, ingredient)| {
            result == "guide:marker" && ingredient == "cobblestone"
        }));
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "SCREEN" && id == "guide:book")
        );
        assert!(pack.script_screens().iter().any(|id| id == "guide:book"));
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "COMMAND" && id == "guide:day")
        );
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "COMMAND" && id == "guide:home")
        );
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "COMMAND" && id == "guide:night")
        );
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "PARTICLE" && id == "guide:spark")
        );
        assert!(pack.contains_block("guide:post"));
        assert_eq!(
            pack.hardness_of(crate::Block::new("guide:post", 0)),
            Some(1.25)
        );
        assert_eq!(
            pack.placed_block("guide:slip").as_deref(),
            Some("guide:post")
        );
        assert!(pack.contains_block("guide:chest"));
        assert!(pack.is_block_entity("guide:chest"));
        assert_eq!(
            pack.placed_block("guide:key").as_deref(),
            Some("guide:chest")
        );
        assert!(
            hooks
                .scripts
                .entries
                .iter()
                .any(|(kind, id)| kind == "BLOCK_ENTITY" && id == "guide:chest")
        );
        assert!(
            pack.script_recipe_lines()
                .iter()
                .any(|(result, ingredients)| {
                    result == "guide:post"
                        && ingredients == &["guide:marker".to_string(), "dirt".to_string()]
                })
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_guiapi_press_reaches_the_script() {
        let source = r#"
import GUIAPI;

fn picked(row: String) {
    print(row);
}

fn main(): Int {
    var screen = GUIAPI.createScreen("notes", GUIAPI.vec(320.0, 240.0), GUIAPI.vec(40.0, 48.0));
    GUIAPI.addElement(screen, GUIAPI.text("marker", "marker"));
    GUIAPI.on_press(screen, "n", picked);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        let loaded = load_source(source, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| { matches!(hook, ScriptHook::ScreenPress { id } if id == "notes") })
        );
        let notes = Pack::builtin()
            .gui_screens()
            .into_iter()
            .find(|screen| screen.name == "notes")
            .expect("notes screen");
        assert_eq!(notes.key, "n");
        assert_eq!(notes.elements[0].name, "marker");
        assert_eq!(notes.elements[0].text, "marker");
        let mut world = sample();
        PLACE_LOGS.with(|logs| logs.borrow_mut().clear());
        hooks.screen_row(&mut world, "notes", "marker", &ScriptView::bare());
        assert_eq!(lines(), vec!["marker".to_string()]);
    }

    #[test]
    fn a_guiapi_press_says_in_chat_and_cannot_write() {
        let source = r#"
import GUIAPI;

fn picked(row: String) {
    world.write(1, 1, 1, "dirt");
    GUIAPI.say("oak noted");
}

fn main(): Int {
    var screen = GUIAPI.createScreen("talk", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(16.0, 16.0));
    GUIAPI.addElement(screen, GUIAPI.text("oak", "oak"));
    GUIAPI.on_press(screen, "k", picked);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let mut world = sample();
        let said = hooks.screen_row(&mut world, "talk", "oak", &ScriptView::bare());
        assert_eq!(said, vec!["oak noted".to_string()]);
        assert_eq!(world.block_at(1, 1, 1).id, "stone");
    }

    #[test]
    fn a_settings_option_is_stored_and_a_reserved_name_is_refused() {
        let source = r#"
import SettingsAPI;

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    SettingsAPI.createOption(game, "glow", "soft glow", "on");
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let glow = Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.name == "glow")
            .expect("glow setting");
        assert_eq!(glow.category, "game");
        assert_eq!(glow.category_text, "how it feels");
        assert_eq!(glow.description, "soft glow");
        assert_eq!(glow.value, "on");
        assert!(Pack::builtin().toggle_setting("glow"));
        let glow = Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.name == "glow")
            .expect("glow setting");
        assert_eq!(glow.value, "off");
        let reserved = r#"
import SettingsAPI;

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    SettingsAPI.createOption(game, "volume", "sound", "on");
    return 0;
}
"#;
        let err = load_source(reserved, &mut Hooks::new()).unwrap_err();
        assert!(err.contains("reserved setting"), "{err}");
    }

    #[test]
    fn a_settings_flip_reads_the_value_and_may_place_a_near_light() {
        let source = r#"
import SettingsAPI;
import GUIAPI;

fn flipped(name: String) {
    world.write(1, 1, 1, "dirt");
    world.write(1, 3, 1, "lantern");
    world.write(1, 4, 1, "lantern");
    world.write(8, 2, 1, "lantern");
    GUIAPI.say("glow " + SettingsAPI.value(name));
}

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var ember = SettingsAPI.createOption(game, "ember", "soft glow", "on");
    SettingsAPI.on_change(ember, flipped);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        let loaded = load_source(source, &mut hooks).unwrap();
        assert!(
            loaded
                .iter()
                .any(|hook| { matches!(hook, ScriptHook::SettingChange { id } if id == "ember") })
        );
        let mut world = sample();
        assert!(Pack::builtin().toggle_setting("ember"));
        let said = hooks.setting_changed(&mut world, "ember", &ScriptView::bare());
        assert_eq!(said, vec!["glow off".to_string()]);
        assert_eq!(world.block_at(1, 1, 1).id, "stone");
        assert!(world.block_at(1, 3, 1).is_air());
        let here = ScriptView {
            slots: &[],
            selected: 0,
            eye: Some([1.2, 2.2, 1.2]),
            time: None,
            season: "",
        };
        assert!(Pack::builtin().toggle_setting("ember"));
        let said = hooks.setting_changed(&mut world, "ember", &here);
        assert_eq!(said, vec!["glow on".to_string()]);
        assert_eq!(world.block_at(1, 1, 1).id, "stone");
        assert_eq!(world.block_at(1, 3, 1).id, "lantern");
        assert!(world.block_at(1, 4, 1).is_air());
        assert!(world.block_at(8, 2, 1).is_air());
        let clear = r#"
import SettingsAPI;

fn flipped(name: String) {
    world.write(1, 1, 1, "air");
    world.write(1, 3, 1, "air");
}

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var ember = SettingsAPI.createOption(game, "ember", "soft glow", "on");
    SettingsAPI.on_change(ember, flipped);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(clear, &mut hooks).unwrap();
        assert!(Pack::builtin().toggle_setting("ember"));
        let _ = hooks.setting_changed(&mut world, "ember", &here);
        assert_eq!(world.block_at(1, 1, 1).id, "stone");
        assert!(world.block_at(1, 3, 1).is_air());
    }

    #[test]
    fn a_settings_choice_list_cycles_and_cannot_write() {
        let source = r#"
import SettingsAPI;
import GUIAPI;

fn flipped(name: String) {
    world.write(1, 1, 1, "dirt");
    GUIAPI.say("pace " + SettingsAPI.value(name));
}

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var pace = SettingsAPI.createOption(game, "pace", "walk speed", "mid");
    SettingsAPI.choices(pace, "low mid high");
    SettingsAPI.on_change(pace, flipped);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let pace = Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.name == "pace")
            .expect("pace");
        assert_eq!(
            pace.choices,
            vec!["low".to_string(), "mid".to_string(), "high".to_string()]
        );
        assert_eq!(pace.value, "mid");
        let mut world = sample();
        assert!(Pack::builtin().toggle_setting("pace"));
        let said = hooks.setting_changed(&mut world, "pace", &ScriptView::bare());
        assert_eq!(said, vec!["pace high".to_string()]);
        assert!(Pack::builtin().toggle_setting("pace"));
        assert!(Pack::builtin().toggle_setting("pace"));
        let pace = Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.name == "pace")
            .expect("pace");
        assert_eq!(pace.value, "mid");
        assert_eq!(world.block_at(1, 1, 1).id, "stone");
        let missing = r#"
import SettingsAPI;

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var pace = SettingsAPI.createOption(game, "pace", "walk speed", "on");
    SettingsAPI.choices(pace, "low mid high");
    return 0;
}
"#;
        let err = load_source(missing, &mut Hooks::new()).unwrap_err();
        assert!(err.contains("not in the choice list"), "{err}");
    }

    #[test]
    fn a_guiapi_text_stacks_unless_the_position_is_set() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("pile", GUIAPI.vec(320.0, 240.0), GUIAPI.vec(40.0, 40.0));
    GUIAPI.addElement(screen, GUIAPI.text("one", "one"));
    var two = GUIAPI.text("two", "two");
    two.size = GUIAPI.vec(200.0, 40.0);
    GUIAPI.addElement(screen, two);
    var three = GUIAPI.text("three", "three");
    three.position = GUIAPI.vec(40.0, 12.0);
    GUIAPI.addElement(screen, three);
    return 0;
}
"#;
        let mut hooks = Hooks::new();
        load_source(source, &mut hooks).unwrap();
        let pile = Pack::builtin()
            .gui_screens()
            .into_iter()
            .find(|screen| screen.name == "pile")
            .expect("pile screen");
        assert_eq!(pile.elements[0].y, 48.0);
        assert_eq!(pile.elements[1].y, 88.0);
        assert_eq!(pile.elements[2].x, 40.0);
        assert_eq!(pile.elements[2].y, 12.0);
    }

    #[test]
    fn a_guiapi_unknown_font_is_refused() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("ink", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(8.0, 8.0));
    var row = GUIAPI.text("row", "row");
    row.font = "comic";
    GUIAPI.addElement(screen, row);
    return 0;
}
"#;
        let err = load_source(source, &mut Hooks::new()).unwrap_err();
        assert!(err.contains("unknown font"), "{err}");
    }
}
