//! Modding hooks on the server authority path.
//!
//! Rosegold scripts register these hooks. Rust code can register them too.
//!
//! A before-break hook sees a break and may cancel it. A before-place hook
//! sees a place that would change a cell and may cancel it. After-break and
//! after-place run only when the cell changed. The server calls
//! [`apply_authoritative`]. An empty [`Hooks`] changes nothing.

use std::sync::{Arc, Mutex};

use crate::chunk::Block;
use crate::cluster::Cluster;
use crate::edit::{Edit, apply, would_apply};
use crate::rosegold::ScriptTable;

/// What a before-break hook decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakVerdict {
    Allow,
    Cancel,
}

/// The cell a client asked to break, still holding `block`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeforeBreak {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

/// The cell a place would write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeforePlace {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

/// The cell a break just changed. `block` is what was broken. `left` is what remains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AfterBreak {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
    pub left: Block,
}

/// The cell a place just wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AfterPlace {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

struct BeforeBreakHook {
    id: String,
    hook: Box<dyn Fn(&BeforeBreak) -> BreakVerdict + Send>,
}

struct BeforePlaceHook {
    id: String,
    hook: Box<dyn Fn(&BeforePlace) -> BreakVerdict + Send>,
}

struct AfterBreakHook {
    id: String,
    hook: Box<dyn Fn(&AfterBreak) + Send>,
}

struct AfterPlaceHook {
    id: String,
    hook: Box<dyn Fn(&AfterPlace) + Send>,
}

/// Where the demo player stands, in blocks. Feet are above the grass by the stairs.
pub const PLAYER_SPAWN: (i32, i32, i32) = (8, 3, 14);

/// Hooks for one server. Registration order is call order.
/// Rust hooks for a kind run before script hooks for that kind.
pub struct Hooks {
    before_break: Vec<BeforeBreakHook>,
    before_place: Vec<BeforePlaceHook>,
    after_break: Vec<AfterBreakHook>,
    after_place: Vec<AfterPlaceHook>,
    pub(crate) scripts: ScriptTable,
    /// 50 ms steps since the last `events.tick`. A tick is 20 of these.
    tick_steps: std::cell::Cell<u32>,
    /// How many `events.tick` calls this table has made. The next one is `n`.
    tick_n: std::cell::Cell<i32>,
}

impl Hooks {
    pub fn new() -> Self {
        Self {
            before_break: Vec::new(),
            before_place: Vec::new(),
            after_break: Vec::new(),
            after_place: Vec::new(),
            scripts: ScriptTable::new(),
            tick_steps: std::cell::Cell::new(0),
            tick_n: std::cell::Cell::new(0),
        }
    }

    /// `id` names the hook. A second registration of the same id is an error.
    ///
    /// Every registered hook runs. One [`BreakVerdict::Cancel`] cancels the break.
    pub fn register_before_break<F>(&mut self, id: impl Into<String>, hook: F) -> Result<(), String>
    where
        F: Fn(&BeforeBreak) -> BreakVerdict + Send + 'static,
    {
        let id = check_id(id.into())?;
        if self.has_id("before_break", &id) {
            return Err(format!("duplicate before-break hook {id}"));
        }
        self.before_break.push(BeforeBreakHook {
            id,
            hook: Box::new(hook),
        });
        Ok(())
    }

    /// `id` names the hook. A second registration of the same id is an error.
    ///
    /// Every registered hook runs. One [`BreakVerdict::Cancel`] cancels the place.
    pub fn register_before_place<F>(&mut self, id: impl Into<String>, hook: F) -> Result<(), String>
    where
        F: Fn(&BeforePlace) -> BreakVerdict + Send + 'static,
    {
        let id = check_id(id.into())?;
        if self.has_id("before_place", &id) {
            return Err(format!("duplicate before-place hook {id}"));
        }
        self.before_place.push(BeforePlaceHook {
            id,
            hook: Box::new(hook),
        });
        Ok(())
    }

    /// `id` names the hook. A second registration of the same id is an error.
    ///
    /// The hook runs after the break has changed the cell.
    pub fn register_after_break<F>(&mut self, id: impl Into<String>, hook: F) -> Result<(), String>
    where
        F: Fn(&AfterBreak) + Send + 'static,
    {
        let id = check_id(id.into())?;
        if self.has_id("after_break", &id) {
            return Err(format!("duplicate after-break hook {id}"));
        }
        self.after_break.push(AfterBreakHook {
            id,
            hook: Box::new(hook),
        });
        Ok(())
    }

    /// `id` names the hook. A second registration of the same id is an error.
    ///
    /// The hook runs after the place has changed the cell.
    pub fn register_after_place<F>(&mut self, id: impl Into<String>, hook: F) -> Result<(), String>
    where
        F: Fn(&AfterPlace) + Send + 'static,
    {
        let id = check_id(id.into())?;
        if self.has_id("after_place", &id) {
            return Err(format!("duplicate after-place hook {id}"));
        }
        self.after_place.push(AfterPlaceHook {
            id,
            hook: Box::new(hook),
        });
        Ok(())
    }

    pub(crate) fn has_id(&self, kind: &str, id: &str) -> bool {
        let taken = match kind {
            "before_break" => self.before_break.iter().any(|hook| hook.id == id),
            "before_place" => self.before_place.iter().any(|hook| hook.id == id),
            "after_break" => self.after_break.iter().any(|hook| hook.id == id),
            "after_place" => self.after_place.iter().any(|hook| hook.id == id),
            "on_select" | "on_screen" | "break_speed" | "break_block" | "pickup_item"
            | "break_drop" => false,
            _ => false,
        };
        taken || self.scripts.has(kind, id)
    }

    fn before_break(&self, event: &BeforeBreak) -> BreakVerdict {
        let mut verdict = BreakVerdict::Allow;
        for hook in &self.before_break {
            if (hook.hook)(event) == BreakVerdict::Cancel {
                verdict = BreakVerdict::Cancel;
            }
        }
        verdict
    }

    fn before_place(&self, event: &BeforePlace) -> BreakVerdict {
        let mut verdict = BreakVerdict::Allow;
        for hook in &self.before_place {
            if (hook.hook)(event) == BreakVerdict::Cancel {
                verdict = BreakVerdict::Cancel;
            }
        }
        verdict
    }

    fn after_break(&self, event: &AfterBreak) {
        for hook in &self.after_break {
            (hook.hook)(event);
        }
    }

    /// Tell hotbar scripts the player selected `slot` (`1..=10`) holding `block`.
    /// An empty slot passes `""`.
    pub fn select(
        &self,
        world: &mut Cluster,
        slot: i32,
        block: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) {
        self.scripts.on_select(world, slot, block, view);
    }

    /// Tell the script that watches `name` that the player flipped that settings row.
    pub fn setting_changed(
        &self,
        world: &mut Cluster,
        name: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Vec<String> {
        self.scripts.setting_changed(world, name, view)
    }

    /// Tell the script that built `id` that Enter or a click chose `row`.
    pub fn screen_row(
        &self,
        world: &mut Cluster,
        id: &str,
        row: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Vec<String> {
        self.screen_row_text(world, id, row, None, view)
    }

    /// Like [`Self::screen_row`], and `typed` is the field's line when Enter was on one.
    pub fn screen_row_text(
        &self,
        world: &mut Cluster,
        id: &str,
        row: &str,
        typed: Option<&str>,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Vec<String> {
        self.scripts.screen_press(world, id, row, typed, view)
    }

    /// Tell screen scripts that `id` opened or closed.
    pub fn screen(
        &self,
        world: &mut Cluster,
        id: &str,
        open: bool,
        view: &crate::rosegold::ScriptView<'_>,
    ) {
        self.scripts.on_screen(world, id, open, view);
    }

    /// Seconds multiplier for breaking `id`. `1` keeps the pack hardness.
    pub fn break_scale(
        &self,
        world: &mut Cluster,
        id: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> f32 {
        self.scripts.break_scale(world, id, view)
    }

    /// Item id a break drop should use. `None` keeps the broken block.
    pub fn break_drop(
        &self,
        world: &mut Cluster,
        block: &str,
        left: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Option<String> {
        self.scripts.break_drop(world, block, left, view)
    }

    /// Item id a copper chest should pay instead of `item`. `None` keeps `item`.
    pub fn loot(
        &self,
        world: &mut Cluster,
        item: &str,
        x: i32,
        z: i32,
        omen: bool,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Option<String> {
        self.scripts.loot(world, item, x, z, omen, view)
    }

    /// False when a pickup hook leaves this drop on the ground.
    pub fn allow_pickup(
        &self,
        world: &mut Cluster,
        id: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.allow_pickup(world, id, view)
    }

    pub fn listens_break_drop(&self) -> bool {
        self.scripts.listens("break_drop")
    }

    pub fn listens_pickup(&self) -> bool {
        self.scripts.listens("pickup_item")
    }

    /// `(registry kind, id)` stored by `TLRegistry.register` besides blocks and items.
    pub fn script_content(&self) -> &[(String, String)] {
        self.scripts.content()
    }

    /// The hotbar a client just sent. Later break and place hooks see it.
    pub fn set_inventory(&self, selected: i32, slots: Vec<(String, i32)>) {
        self.scripts.set_inventory(selected, slots);
    }

    /// Player joined, or a demo mob standing on the grass. False cancels that spawn.
    pub fn entity_spawn(&self, world: &mut Cluster, kind: &str, x: i32, y: i32, z: i32) -> bool {
        self.scripts.entity_spawn(world, kind, x, y, z)
    }

    /// The pig and the zombie. Called once, next to the player spawn.
    pub fn spawn_demo_mobs(&self, world: &mut Cluster) {
        let _ = self.entity_spawn(world, "pig", 5, 1, 9);
        let _ = self.entity_spawn(world, "zombie", 11, 1, 13);
    }

    /// Hurt hooks may cancel (`0`) or rewrite the amount. Falls, bites, mace
    /// hits, and other damage paths ask this with the target kind.
    pub fn entity_hurt(&self, world: &mut Cluster, kind: &str, amount: i32) -> i32 {
        self.scripts.entity_hurt(world, kind, amount)
    }

    /// False when a death hook cancels despawn/loot side effects.
    pub fn entity_death(
        &self,
        world: &mut Cluster,
        kind: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> bool {
        self.scripts.entity_death(world, kind, x, y, z)
    }

    pub fn drain_live_spawns(
        &self,
    ) -> Vec<Result<(String, i32, i32, i32), (String, i32, i32, i32)>> {
        self.scripts.drain_live_spawns()
    }

    /// Screen names `gui.open` queued during the last hook.
    pub fn drain_screen_opens(&self) -> Vec<String> {
        self.scripts.drain_screen_opens()
    }

    /// Hotbar edits from `player.take` / `player.give` during the last hook.
    pub fn drain_inv_ops(&self) -> Vec<crate::rosegold::InvOp> {
        self.scripts.drain_inv_ops()
    }

    /// Cell remembered by the last `block_entity.open_at`.
    pub fn machine_focus(&self) -> Option<(i32, i32, i32)> {
        self.scripts.machine_focus()
    }

    /// One mob tick. The game drives `"player"`, `"sheep"`, `"pig"`, and `"zombie"`.
    pub fn entity_tick(&self, world: &mut Cluster, kind: &str) {
        self.scripts.entity_tick(world, kind);
    }

    /// False when a script consumes a right click with the held item.
    ///
    /// `events.use_item` is `fn(item: String): Bool`. False cancels eating, placing,
    /// and the other built-in uses of this click.
    pub fn use_item(
        &self,
        world: &mut Cluster,
        item: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.use_item(world, item, view)
    }

    /// False when a script consumes a right click on a block.
    ///
    /// `events.use_block` is `fn(item: String, block: String, x: Int, y: Int, z: Int): Bool`.
    /// The game asks this before [`Self::use_item`] when the look hits a block.
    pub fn use_block(
        &self,
        world: &mut Cluster,
        item: &str,
        block: &str,
        x: i32,
        y: i32,
        z: i32,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.use_block(world, item, block, x, y, z, view)
    }

    /// False when a script consumes a right click on a mob.
    ///
    /// `events.use_entity` is `fn(item: String, kind: String): Bool`. The game asks
    /// this before a block use, an item use, a feed, a saddle, and a mace swing.
    pub fn use_entity(
        &self,
        world: &mut Cluster,
        item: &str,
        kind: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.use_entity(world, item, kind, view)
    }

    /// False when a script consumes a left click.
    ///
    /// `events.attack` is `fn(item: String, target: String): Bool`. `target` is the
    /// aimed mob or the looked-at block. False skips the break and the punch.
    pub fn attack(
        &self,
        world: &mut Cluster,
        item: &str,
        target: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.attack(world, item, target, view)
    }

    /// False when a script swallows an ordinary chat line.
    ///
    /// `events.chat` is `fn(line: String): Bool`. A line that starts with `/` is a
    /// command and is not passed here. False keeps the line off the screen and
    /// off the server.
    pub fn chat(
        &self,
        world: &mut Cluster,
        line: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.chat(world, line, view)
    }

    /// False when a script refuses this return.
    ///
    /// `events.respawn` is `fn(cause: String, x: Int, y: Int, z: Int): Bool`.
    /// `cause` is `death`, `bed`, or `anchor`. False sends a death to the stairs,
    /// keeps the night going, or leaves the saved spawn where it was.
    pub fn respawn(
        &self,
        world: &mut Cluster,
        cause: &str,
        x: i32,
        y: i32,
        z: i32,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.respawn(world, cause, x, y, z, view)
    }

    /// False when a script refuses a crafting-grid result.
    ///
    /// `events.craft` is `fn(result: String): Bool`. False leaves the ingredients
    /// in the grid.
    pub fn craft(
        &self,
        world: &mut Cluster,
        result: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.craft(world, result, view)
    }

    /// False when a script refuses a furnace output.
    ///
    /// `events.smelt` is `fn(result: String): Bool`. The same hook covers the
    /// furnace, the smoker, and the blast furnace. False leaves the item in
    /// the output slot.
    pub fn smelt(
        &self,
        world: &mut Cluster,
        result: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.smelt(world, result, view)
    }

    /// False when a script refuses an anvil repair.
    ///
    /// `events.repair` is `fn(item: String): Bool`. `item` is the worn tool.
    /// False leaves the iron ore unspent and the tool worn.
    pub fn repair(
        &self,
        world: &mut Cluster,
        item: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.repair(world, item, view)
    }

    /// False when a script refuses an enchanting-table spend.
    ///
    /// `events.enchant` is `fn(item: String): Bool`. `item` is the plain pick.
    /// False leaves the bookshelf unspent and the pick without Efficiency.
    pub fn enchant(
        &self,
        world: &mut Cluster,
        item: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.enchant(world, item, view)
    }

    /// False when a script refuses a brewing-stand mix.
    ///
    /// `events.brew` is `fn(ingredient: String): Bool`. `ingredient` is
    /// `nether_wart_block` or `flower`. False leaves the water bucket and that
    /// ingredient unspent.
    pub fn brew(
        &self,
        world: &mut Cluster,
        ingredient: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.brew(world, ingredient, view)
    }

    /// False when a script refuses a smithing-table upgrade.
    ///
    /// `events.smith` is `fn(item: String): Bool`. `item` is `stone_pickaxe`,
    /// `iron_pickaxe`, or `mace`. False leaves the iron ore or the heavy core
    /// unspent.
    pub fn smith(
        &self,
        world: &mut Cluster,
        item: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.smith(world, item, view)
    }

    /// First approach of a named landmark.
    ///
    /// `events.structure_discovered` is `fn(structure: String): Bool`.
    /// False cancels the built-in tip. Returned lines are from `chat.say`.
    pub fn structure_discovered(
        &self,
        world: &mut Cluster,
        structure: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> (bool, Vec<String>) {
        self.scripts
            .structure_discovered(world, structure, view)
    }

    /// False when a script refuses a loom weave.
    ///
    /// `events.weave` is `fn(wool: String): Bool`. `wool` is the wool id.
    /// False leaves that wool unspent.
    pub fn weave(
        &self,
        world: &mut Cluster,
        wool: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.weave(world, wool, view)
    }

    /// Tell `events.dawn` that morning arrived.
    ///
    /// `events.dawn` is `fn(day: Int)`. `day` is `0` spring through `3` winter.
    /// The sky calls this when time crosses dawn, and a bed calls it when sleep
    /// skips to morning.
    pub fn dawn(&self, world: &mut Cluster, day: i32, view: &crate::rosegold::ScriptView<'_>) {
        self.scripts.dawn(world, day, view);
    }

    /// Tell `events.dusk` that evening arrived.
    ///
    /// `events.dusk` is `fn(day: Int)`. `day` is `0` spring through `3` winter.
    /// The sky calls this when time crosses dusk. A bed does not.
    pub fn dusk(&self, world: &mut Cluster, day: i32, view: &crate::rosegold::ScriptView<'_>) {
        self.scripts.dusk(world, day, view);
    }

    /// False when a script keeps the player on the ground.
    ///
    /// `events.jump` is `fn(x: Int, y: Int, z: Int): Bool`. The cell is where the
    /// feet are. False skips the launch. A swim or a climb does not ask.
    pub fn jump(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.jump(world, x, y, z, view)
    }

    /// False when a script keeps this sprint at a normal walk.
    ///
    /// `events.sprint` is `fn(x: Int, y: Int, z: Int): Bool`. The cell is where
    /// the feet are. It runs when Ctrl and a walk key are both held. False
    /// clears the fast speed for that hold. Standing still does not ask.
    pub fn sprint(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> bool {
        self.scripts.sprint(world, x, y, z, view)
    }

    /// Tell `events.land` that the player touched the ground.
    ///
    /// `events.land` is `fn(x: Int, y: Int, z: Int, drop: Int)`. The cell is
    /// where the feet are. `drop` is the fall in whole blocks, and `0` is a
    /// hop under one block. A swim that never touches the floor does not land.
    pub fn land(
        &self,
        world: &mut Cluster,
        x: i32,
        y: i32,
        z: i32,
        drop: i32,
        view: &crate::rosegold::ScriptView<'_>,
    ) {
        self.scripts.land(world, x, y, z, drop, view)
    }

    /// The reply from `events.command`, or `None` when this name has no callback.
    ///
    /// The callback is `fn(args: String): String`. `id` may be `spark`, `wand:spark`,
    /// or `/spark`. `args` is the rest of the chat line. The command list passes `""`.
    /// Registering the event also adds a `COMMAND` row.
    pub fn command(
        &self,
        world: &mut Cluster,
        id: &str,
        args: &str,
        view: &crate::rosegold::ScriptView<'_>,
    ) -> Option<String> {
        self.scripts.command(world, id, args, view)
    }

    /// Tick the player and the mob stubs `steps` times. A step is 50 ms.
    ///
    /// Entity tick and hurt hooks do not write the world. `events.tick` does.
    /// It runs on every 20th step, once a second in a steady game, and at most
    /// once per catch-up. More than 20 steps in one catch-up are dropped so a
    /// stalled frame cannot run away.
    pub fn drive_entities(&self, world: &mut Cluster, steps: u32) {
        let mut carried = self.tick_steps.get();
        let mut n = self.tick_n.get();
        let mobs: Vec<String> = self.scripts.mobs().to_vec();
        for _ in 0..steps.min(20) {
            self.entity_tick(world, "player");
            self.entity_tick(world, "sheep");
            self.entity_tick(world, "pig");
            self.entity_tick(world, "zombie");
            for id in &mobs {
                self.entity_tick(world, id);
            }
            carried += 1;
            if carried == 20 {
                carried = 0;
                n = n.saturating_add(1);
                self.scripts.tick(world, n);
            }
        }
        self.tick_steps.set(carried);
        self.tick_n.set(n);
    }

    /// Blocks `world.write` changed during the last hook, not counting the edit cell.
    pub fn drain_writes(&self) -> Vec<crate::changes::CellChange> {
        self.scripts.drain_writes()
    }

    fn after_place(&self, event: &AfterPlace) {
        for hook in &self.after_place {
            (hook.hook)(event);
        }
    }
}

impl Default for Hooks {
    fn default() -> Self {
        Self::new()
    }
}

/// Wall clock for [`Hooks::drive_entities`]. One step is 50 ms, capped at 20.
pub struct EntityClock {
    last: std::time::Instant,
}

impl EntityClock {
    pub fn new() -> Self {
        Self {
            last: std::time::Instant::now(),
        }
    }

    /// Tick however many 50 ms steps have passed since the last call.
    pub fn catch_up(&mut self, hooks: &Hooks, world: &mut Cluster) {
        let steps = (self.last.elapsed().as_secs_f32() / 0.05) as u32;
        if steps == 0 {
            return;
        }
        hooks.drive_entities(world, steps);
        self.last = std::time::Instant::now();
    }
}

/// Apply `edit` the way the server does.
///
/// A before-break hook can cancel a break of a real block. A before-place hook
/// can cancel a place that would change a cell. After-break and after-place
/// hooks run only when the cell changes. Air, an out-of-bounds cell, and a
/// refused place do not call hooks.
pub fn apply_authoritative(world: &mut Cluster, edit: Edit, hooks: &Hooks) -> bool {
    match edit {
        Edit::Break { x, y, z } => {
            if !would_apply(world, edit) {
                return false;
            }
            let block = world.block_at(x, y, z);
            let event = BeforeBreak { x, y, z, block };
            if hooks.before_break(&event) == BreakVerdict::Cancel
                || hooks.scripts.before_break(world, &event) == BreakVerdict::Cancel
            {
                return false;
            }
            let changed = apply(world, edit);
            if changed {
                let left = world.block_at(x, y, z);
                let event = AfterBreak {
                    x,
                    y,
                    z,
                    block,
                    left,
                };
                hooks.after_break(&event);
                hooks.scripts.after_break(world, &event);
            }
            changed
        }
        Edit::Place {
            x,
            y,
            z,
            block,
            player,
        } => {
            let edit = Edit::Place {
                x,
                y,
                z,
                block,
                player,
            };
            if !would_apply(world, edit) {
                return false;
            }
            let before = world.block_at(x, y, z);
            let event = BeforePlace { x, y, z, block };
            if hooks.before_place(&event) == BreakVerdict::Cancel
                || hooks.scripts.before_place(world, &event) == BreakVerdict::Cancel
            {
                return false;
            }
            let changed = apply(world, edit);
            if changed {
                let event = AfterPlace { x, y, z, block };
                hooks.after_place(&event);
                hooks.scripts.after_place(world, &event);
                let after = world.block_at(x, y, z);
                hooks
                    .scripts
                    .note_fill_change(world, x, y, z, before.fill, after.fill);
                hooks.scripts.flush_fills(world);
            }
            changed
        }
    }
}

/// One event the built-in test hook recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestHookEvent {
    BeforeBreak {
        x: i32,
        y: i32,
        z: i32,
        block: Block,
    },
    AfterPlace {
        x: i32,
        y: i32,
        z: i32,
        block: Block,
    },
}

/// Records every before-break and every successful place. Cancels breaks of one id.
pub struct TestHook {
    cancel_id: &'static str,
    events: Arc<Mutex<Vec<TestHookEvent>>>,
}

impl TestHook {
    /// Cancel a break when the block id is `cancel_id`. Other breaks are allowed.
    pub fn cancels(cancel_id: &'static str) -> Self {
        Self {
            cancel_id,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register this hook under `test-before-break` and `test-after-place`.
    pub fn register(&self, hooks: &mut Hooks) -> Result<(), String> {
        let events = Arc::clone(&self.events);
        let cancel_id = self.cancel_id;
        hooks.register_before_break("test-before-break", move |event| {
            events
                .lock()
                .expect("test hook")
                .push(TestHookEvent::BeforeBreak {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block: event.block,
                });
            if event.block.id == cancel_id {
                BreakVerdict::Cancel
            } else {
                BreakVerdict::Allow
            }
        })?;
        let events = Arc::clone(&self.events);
        hooks.register_after_place("test-after-place", move |event| {
            events
                .lock()
                .expect("test hook")
                .push(TestHookEvent::AfterPlace {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block: event.block,
                });
        })?;
        Ok(())
    }

    pub fn fired(&self) -> Vec<TestHookEvent> {
        self.events.lock().expect("test hook").clone()
    }
}

fn check_id(id: String) -> Result<String, String> {
    if id.is_empty() {
        Err("hook id is empty".into())
    } else {
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk};
    use crate::edit::PlayerBox;

    fn world() -> Cluster {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 1, 1, Block::new("stone", 0)),
                (2, 1, 1, Block::new("grass", 0)),
            ],
        )
        .unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    fn far_player() -> PlayerBox {
        PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        }
    }

    #[test]
    fn filling_a_grate_applies_compound_fill() {
        use crate::chunk::{FILL_WATER, fill_name};
        let mut world = {
            let chunk =
                Chunk::from_blocks(0, 0, [(2, 1, 2, Block::new("copper_grate", 0))]).unwrap();
            Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
        };
        let hooks = Hooks::new();
        assert_eq!(world.block_at(2, 1, 2).fill, 0);
        assert!(apply_authoritative(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
                player: far_player(),
            },
            &hooks
        ));
        assert_eq!(world.block_at(2, 1, 2).fill, FILL_WATER);
        assert_eq!(fill_name(world.block_at(2, 1, 2).fill), Some("water"));
    }

    #[test]
    fn the_test_hook_cancels_one_break_and_records_a_place() {
        let mut world = world();
        let mut hooks = Hooks::new();
        let test_hook = TestHook::cancels("stone");
        test_hook.register(&mut hooks).unwrap();
        assert!(
            hooks
                .register_before_break("test-before-break", |_| BreakVerdict::Allow)
                .is_err()
        );
        assert!(
            hooks
                .register_before_break("", |_| BreakVerdict::Allow)
                .is_err()
        );

        assert!(!apply_authoritative(
            &mut world,
            Edit::Break { x: 1, y: 1, z: 1 },
            &hooks
        ));
        assert_eq!(world.block_at(1, 1, 1), Block::new("stone", 0));

        assert!(apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        assert!(!apply_authoritative(
            &mut world,
            Edit::Break { x: 2, y: 1, z: 1 },
            &hooks
        ));

        assert!(apply_authoritative(
            &mut world,
            Edit::Place {
                x: 3,
                y: 1,
                z: 1,
                block: Block::new("cobblestone", 0),
                player: far_player(),
            },
            &hooks
        ));
        let overlapping = PlayerBox {
            min: [3.0, 2.0, 1.0],
            max: [3.6, 3.8, 1.6],
        };
        assert!(!apply_authoritative(
            &mut world,
            Edit::Place {
                x: 3,
                y: 2,
                z: 1,
                block: Block::new("dirt", 0),
                player: overlapping,
            },
            &hooks
        ));
        assert!(world.block_at(3, 2, 1).is_air());

        assert_eq!(
            test_hook.fired(),
            vec![
                TestHookEvent::BeforeBreak {
                    x: 1,
                    y: 1,
                    z: 1,
                    block: Block::new("stone", 0),
                },
                TestHookEvent::BeforeBreak {
                    x: 2,
                    y: 1,
                    z: 1,
                    block: Block::new("grass", 0),
                },
                TestHookEvent::AfterPlace {
                    x: 3,
                    y: 1,
                    z: 1,
                    block: Block::new("cobblestone", 0),
                },
            ]
        );
    }
}
