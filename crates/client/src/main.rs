mod audio;
mod blockbench;
mod boat;
mod body;
mod camera;
mod commands;
mod daylight;
mod dig;
mod drops;
mod egui_ui;
mod gui;
mod hotbar;
mod item_icon;
mod jobs;
mod keybinds;
mod look;
mod loot;
mod map;
mod mesh;
mod minecart;
mod mobs;
mod net;
mod outline;
mod palette;
mod peers;
mod player;
mod recipes;
mod render;
mod shade;
mod shape_pick;
mod sheep;
mod texture;
mod trace;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use rebuild_world::{Cluster, DEFAULT_PORT};
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use net::Link;
use render::State;

const USAGE: &str = "\
Usage: rebuild-client [--port <port>]

Terralite client. Connect to the Terralite server (rebuild-server) on
127.0.0.1 (default port 17610) when it is already running. If nothing is
listening, the client opens the same local synthetic world and save, and
answers breaks and places itself. The mesh and collision update when that
authority replies. This is a private frame protocol, version 1, not protocol 47.

The sun and moon move through a two-minute day, starting at spring noon.
The sun rises in the east. Summer is higher and the day is longer; winter
is lower and the day is shorter; spring and autumn sit between them. Cast
shadows follow that path from a shadow map, so the edge is not painted
on the block corners. When the sun or moon steps, bounce is baked
a little at a time so the frame does not stop. Nearby faces also pick up one bounce of that
light, so a shaded side takes a little color from the ground it faces.
Corners where blocks meet are a little darker when trace is off. Settings has a trace row, on by default. On, the shade is read in the air in front of the face. A closed sky and a crease stay a little darker, and the two-block probes fade across the grass instead of painting black squares. The sun and moon are a gentle wrap, the sky eases off under a roof, and a crease eases darker. Those shades come from the ray grid, and the corners stay undarkened. Off keeps the raster shadow map and the corner darkening.
The moon stays opposite the sun. The window title
shows the season and the phase (dawn, day, dusk, night). Night is a dark
blue sky, a moon, and dim moonlight, and the fog pulls in close. Late
afternoon is a separate gray spell. A lantern still lights the faces it
reaches and is not covered by that shadow. The season changes every day
and repeats every four days.
The window needs a display. A menu opens first. Up and down move the row.
Enter or Space picks it. Play starts the solo world. Join connects to 127.0.0.1 and stays on the menu when nothing is listening. Settings chooses
the world size, a southern or northern sun, sound, view bob (full, low, or off), and trace (on or off). Worlds loads a named
save. Quit closes the window. Click to capture the mouse.
WASD walks, mouse looks, Space jumps, Ctrl is faster, Shift crouches. F5 toggles a third-person look so you can see your Blockbench body walk and dig. A small cross marks the center, and the block under it is named just below. Stepping into water splashes, and swimming is a stroke. An axolotl pauses, then swims the marsh just south of the stairs, at (8, 32). Right-click it and it pathfinds through water to stay near you as a companion. A chicken pecks the grass west of the oak, at (3, 8). It stands and pecks, walks one side of the yard, and steps back when you stand close. Right-click the egg and it hands you raw food. A Blockbench zombie idles by the ore line at (11, 13), walks in to bite, and flashes red when the mace hits. Esc releases the mouse,
and Esc again opens pause: resume, settings, or quit to the title. E opens the Items screen. Up and down arrows
select a recipe. Enter crafts it when the hotbar holds the ingredients. Esc or E closes it.
I opens the inventory, nine slots past the hotbar. Letters search that grid by name, and a space splits words. A search that fits nothing says nothing matches. Enter moves one into the hotbar.
Keys 1 through 9 and 0 while it is open stash one from that hotbar slot. Those nine
slots, the hotbar, and where you stand are written into the same save. G opens the guide.
C lists registered commands. Enter runs the selected one.
T opens chat. Enter sends a line of letters. On a joined server the line comes back.
Joined players show as another body. A long fall spends hearts, and at none you
respawn by the stairs. F opens a chest, a crafting table, or a furnace. Settings world size also changes a
dedicated server.
The hotbar stays.
Keys 1 through 9 and 0 select a hotbar slot. Slot 1 starts as a stack of 64
cobblestone and the other nine slots start empty. White digits on the slot
show the count. A white wire outline marks the block you look at. Hold left
click to break it. Grass is fast. Stone and ores are slower. The block
darkens while you hold. Releasing or looking away resets the progress.
Looking at air draws no outline. Breaking leaves a small colored cube.
Walking into it adds one to a stack of that id, or the first empty slot.
A full hotbar puts it in the inventory. It stays on the ground when both are full.
Right click places one and the count drops. At 0
the slot is empty. Breaks and places are written to
C:\\CODE\\TerraLite\\saves\\blocks.json for the synthetic world, by the
server when it is running and by the client when it is not.
A one-block rise is walked
up when the body fits above it. A taller wall or a ceiling stops the walk.
Space still jumps. Gravity pulls the player down.
";

struct Args {
    port: u16,
    /// After Play, look at the zombie and write one window PNG, then quit.
    shot: Option<std::path::PathBuf>,
    /// Open settings and write one window PNG, then quit.
    shot_settings: Option<std::path::PathBuf>,
    /// Midnight cave with a held torch, then quit.
    shot_held_light: Option<std::path::PathBuf>,
    /// Midnight outdoor zombie horde, then quit.
    shot_night_spawn: Option<std::path::PathBuf>,
    /// Night bed sleep → morning, then quit.
    shot_bed_sleep: Option<std::path::PathBuf>,
    /// Sneak at a cliff edge, then quit.
    shot_crouch: Option<std::path::PathBuf>,
    /// Walk FOV vs sprint FOV stitch, then quit.
    shot_sprint: Option<std::path::PathBuf>,
    /// Dry shore vs underwater FOV/tint stitch, then quit.
    shot_swim: Option<std::path::PathBuf>,
    /// Idle boat vs rowing FOV stitch, then quit.
    shot_boat: Option<std::path::PathBuf>,
    /// Cooling vs ready attack crosshair stitch, then quit.
    shot_attack: Option<std::path::PathBuf>,
    /// Fed vs starving hunger veil stitch, then quit.
    shot_hunger: Option<std::path::PathBuf>,
    /// Idle vs chewing FOV stitch, then quit.
    shot_eat: Option<std::path::PathBuf>,
    /// Grounded vs jump FOV stitch, then quit.
    shot_jump: Option<std::path::PathBuf>,
    /// Idle vs place FOV stitch, then quit.
    shot_place: Option<std::path::PathBuf>,
    /// Idle vs hotbar-swap FOV stitch, then quit.
    shot_swap: Option<std::path::PathBuf>,
    /// Idle vs break FOV stitch, then quit.
    shot_break: Option<std::path::PathBuf>,
    /// Campfire smoke puffs, then quit.
    shot_campfire_smoke: Option<std::path::PathBuf>,
    /// Campfire cook raw→cooked, then quit.
    shot_cook: Option<std::path::PathBuf>,
    /// Smoker cook raw→cooked, then quit.
    shot_smoker_cook: Option<std::path::PathBuf>,
    /// Camp smoker beside the fire, then quit.
    shot_camp_smoker: Option<std::path::PathBuf>,
    /// Blast furnace one-step, then quit.
    shot_blast: Option<std::path::PathBuf>,
    /// Shrine furnace clay→bricks, then quit.
    shot_furnace: Option<std::path::PathBuf>,
    /// Marsh clay bank, then quit.
    shot_clay: Option<std::path::PathBuf>,
    /// Decorated pot craft at shrine table, then quit.
    shot_pot: Option<std::path::PathBuf>,
    /// Compound water/ember grate fill, then quit.
    shot_compound: Option<std::path::PathBuf>,
    /// Creative inventory flow tab, then quit.
    shot_creative: Option<std::path::PathBuf>,
    /// Southeast shrine ruin, then quit.
    shot_shrine: Option<std::path::PathBuf>,
    /// West mine-mouth chest, then quit.
    shot_mine: Option<std::path::PathBuf>,
    /// Keybinds screen, then quit.
    shot_keybinds: Option<std::path::PathBuf>,
    /// Axolotl companion swim, then quit.
    shot_axolotl: Option<std::path::PathBuf>,
    /// Frog on a marsh lily pad, then quit.
    shot_frog: Option<std::path::PathBuf>,
    /// Campfire ember sparks, then quit.
    shot_embers: Option<std::path::PathBuf>,
    /// Night lantern warm flicker, then quit.
    shot_lantern: Option<std::path::PathBuf>,
    /// Camp scarecrow straw sway, then quit.
    shot_sway: Option<std::path::PathBuf>,
    /// Camp wheat crop rustle, then quit.
    shot_rustle: Option<std::path::PathBuf>,
    /// Night lantern soft hiss, then quit.
    shot_hiss: Option<std::path::PathBuf>,
    /// Camp farm fence-gate creak, then quit.
    shot_creak: Option<std::path::PathBuf>,
    /// Dawn shore gull cry, then quit.
    shot_gull: Option<std::path::PathBuf>,
    /// Lever / button wood thunk, then quit.
    shot_thunk: Option<std::path::PathBuf>,
    /// Wooden door creak, then quit.
    shot_door: Option<std::path::PathBuf>,
    /// Chest lid scrape, then quit.
    shot_scrape: Option<std::path::PathBuf>,
    /// Torch place pop, then quit.
    shot_pop: Option<std::path::PathBuf>,
    /// Rain on leaves under canopy, then quit.
    shot_patter: Option<std::path::PathBuf>,
    /// Furnace / smoker heat crackle, then quit.
    shot_heat: Option<std::path::PathBuf>,
    /// Beehive buzz, then quit.
    shot_buzz: Option<std::path::PathBuf>,
    /// Dusk fireflies over water, then quit.
    shot_firefly: Option<std::path::PathBuf>,
    /// Wolf companion sit/follow, then quit.
    shot_wolf: Option<std::path::PathBuf>,
    /// Cat companion sit/follow, then quit.
    shot_cat: Option<std::path::PathBuf>,
    /// Parrot shoulder perch, then quit.
    shot_parrot: Option<std::path::PathBuf>,
    /// Open map with clay bank mark, then quit.
    shot_map_clay: Option<std::path::PathBuf>,
    /// Camp note-block pitch mote, then quit.
    shot_note: Option<std::path::PathBuf>,
    /// Camp hanging-sign path tip, then quit.
    shot_hang: Option<std::path::PathBuf>,
    /// Camp jukebox disc play, then quit.
    shot_jukebox: Option<std::path::PathBuf>,
    /// Shrine bell ring, then quit.
    shot_bell: Option<std::path::PathBuf>,
    /// Shrine lectern page turn, then quit.
    shot_lectern: Option<std::path::PathBuf>,
    /// Camp compost fill mote, then quit.
    shot_compost: Option<std::path::PathBuf>,
    /// Camp beehive honey fill, then quit.
    shot_hive: Option<std::path::PathBuf>,
    /// Camp cauldron water pour, then quit.
    shot_cauldron: Option<std::path::PathBuf>,
    /// Shrine enchanting table book swirl, then quit.
    shot_enchant: Option<std::path::PathBuf>,
    /// Shaft grindstone clear Efficiency, then quit.
    shot_grind: Option<std::path::PathBuf>,
    /// Shrine brewing stand bubble potion, then quit.
    shot_brew: Option<std::path::PathBuf>,
    /// Shaft smithing table upgrade, then quit.
    shot_smith: Option<std::path::PathBuf>,
    /// Shaft anvil repair clang, then quit.
    shot_anvil: Option<std::path::PathBuf>,
    /// Shaft ladder climb into the pit, then quit.
    shot_ladder: Option<std::path::PathBuf>,
    /// Fishing bobber cast wait mote, then quit.
    shot_fish: Option<std::path::PathBuf>,
    /// Sugar cane bone-meal grow, then quit.
    shot_grow: Option<std::path::PathBuf>,
    /// Ripe wheat harvest field, then quit.
    shot_wheat: Option<std::path::PathBuf>,
    /// Farmland till with a hoe, then quit.
    shot_farmland: Option<std::path::PathBuf>,
    /// Bread crafted from wheat, then quit.
    shot_bread: Option<std::path::PathBuf>,
    /// Camp fenced farm plot, then quit.
    shot_plot: Option<std::path::PathBuf>,
    /// Camp scarecrow south of the farm, then quit.
    shot_scarecrow: Option<std::path::PathBuf>,
    /// Minecart idle vs ride FOV stitch, then quit.
    shot_cart: Option<std::path::PathBuf>,
    /// Powered-rail boost ride, then quit.
    shot_boost: Option<std::path::PathBuf>,
    /// Horse idle vs gallop FOV stitch, then quit.
    shot_horse: Option<std::path::PathBuf>,
    /// Horse hitched to the meadow fence, then quit.
    shot_hitch: Option<std::path::PathBuf>,
    /// Meadow horse stable fence ring, then quit.
    shot_stable: Option<std::path::PathBuf>,
    /// Bee pollen trail mid-flight, then quit.
    shot_pollen: Option<std::path::PathBuf>,
}

enum Parsed {
    Help,
    Run(Args),
}

fn main() -> ExitCode {
    env_logger::init();
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Run(args)) => args,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let (cluster, link, source) = match net::connect_or_local(args.port) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("error: {err}");
            tell_user(&err);
            return ExitCode::from(1);
        }
    };
    println!("{}", cluster.summary(source));
    println!(
        "resource pack: {}",
        rebuild_world::resource_pack_root().display()
    );
    println!(
        "block definitions: {}",
        rebuild_world::block_definitions_file().display()
    );
    println!("screen: {}", gui::screen_file().display());
    match shade::load_frame_hook() {
        Ok(hook) if hook.enabled => println!(
            "shader: {} (tint and brightness)",
            shade::frame_shader_file().display()
        ),
        Ok(_) => println!("shader: {} disabled", shade::frame_hook_file().display()),
        Err(err) => eprintln!("shader: {err}"),
    }
    println!(
        "the menu is up. Up and down move the row. Enter or Space picks it. Play starts the solo world. Join connects to 127.0.0.1 and stays on the menu when nothing is listening. Settings chooses
the world size, a southern or northern sun, sound, view bob (full, low, or off), and trace (on or off). Worlds loads a named save. Quit closes the window. Then click the window to look around. WASD walks, Space jumps, Ctrl is faster, Shift crouches. Esc releases the mouse, and Esc again opens pause: resume, settings, or quit to the title. E opens the Items screen. Up and down arrows select a recipe. Esc or E closes it. The hotbar stays. The world is small, a 31x31, and walking into its edge stops you. Keys 1 through 9 and 0 select a hotbar slot. Slot 1 starts as cobblestone; the other nine slots start empty. Left click breaks and keeps a new block id in the first empty slot. Right click places the selected slot. A one-block rise steps you up when there is room above. Gravity pulls you down. Steps, breaks, and places make a sound unless volume is muted. Breaks and places go to the server when it is running, and to the local world otherwise. Either way they are written to the chosen save. The default world is C:\\CODE\\TerraLite\\saves\\blocks.json."
    );

    let event_loop = match EventLoop::new() {
        Ok(loop_) => loop_,
        Err(err) => {
            eprintln!("failed to open a window: {err}");
            eprintln!("the client needs a display and does not run headless.");
            tell_user(&format!("failed to open a window: {err}"));
            return ExitCode::from(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        cluster: Some(cluster),
        link: Some(link),
        local: source == "local",
        port: args.port,
        shot: args.shot,
        shot_settings: args.shot_settings,
        shot_held_light: args.shot_held_light,
        shot_night_spawn: args.shot_night_spawn,
        shot_bed_sleep: args.shot_bed_sleep,
        shot_crouch: args.shot_crouch,
        shot_sprint: args.shot_sprint,
        shot_swim: args.shot_swim,
        shot_boat: args.shot_boat,
        shot_attack: args.shot_attack,
        shot_hunger: args.shot_hunger,
        shot_eat: args.shot_eat,
        shot_jump: args.shot_jump,
        shot_place: args.shot_place,
        shot_swap: args.shot_swap,
        shot_break: args.shot_break,
        shot_campfire_smoke: args.shot_campfire_smoke,
        shot_cook: args.shot_cook,
        shot_smoker_cook: args.shot_smoker_cook,
        shot_camp_smoker: args.shot_camp_smoker,
        shot_blast: args.shot_blast,
        shot_furnace: args.shot_furnace,
        shot_clay: args.shot_clay,
        shot_pot: args.shot_pot,
        shot_compound: args.shot_compound,
        shot_creative: args.shot_creative,
        shot_shrine: args.shot_shrine,
        shot_mine: args.shot_mine,
        shot_keybinds: args.shot_keybinds,
        shot_axolotl: args.shot_axolotl,
        shot_frog: args.shot_frog,
        shot_embers: args.shot_embers,
        shot_lantern: args.shot_lantern,
        shot_sway: args.shot_sway,
        shot_rustle: args.shot_rustle,
        shot_hiss: args.shot_hiss,
        shot_creak: args.shot_creak,
        shot_gull: args.shot_gull,
        shot_thunk: args.shot_thunk,
        shot_door: args.shot_door,
        shot_scrape: args.shot_scrape,
        shot_pop: args.shot_pop,
        shot_patter: args.shot_patter,
        shot_heat: args.shot_heat,
        shot_buzz: args.shot_buzz,
        shot_firefly: args.shot_firefly,
        shot_wolf: args.shot_wolf,
        shot_cat: args.shot_cat,
        shot_parrot: args.shot_parrot,
        shot_map_clay: args.shot_map_clay,
        shot_note: args.shot_note,
        shot_hang: args.shot_hang,
        shot_jukebox: args.shot_jukebox,
        shot_bell: args.shot_bell,
        shot_lectern: args.shot_lectern,
        shot_compost: args.shot_compost,
        shot_hive: args.shot_hive,
        shot_cauldron: args.shot_cauldron,
        shot_enchant: args.shot_enchant,
        shot_grind: args.shot_grind,
        shot_brew: args.shot_brew,
        shot_smith: args.shot_smith,
        shot_anvil: args.shot_anvil,
        shot_ladder: args.shot_ladder,
        shot_fish: args.shot_fish,
        shot_grow: args.shot_grow,
        shot_wheat: args.shot_wheat,
        shot_farmland: args.shot_farmland,
        shot_bread: args.shot_bread,
        shot_plot: args.shot_plot,
        shot_scarecrow: args.shot_scarecrow,
        shot_cart: args.shot_cart,
        shot_boost: args.shot_boost,
        shot_horse: args.shot_horse,
        shot_hitch: args.shot_hitch,
        shot_stable: args.shot_stable,
        shot_pollen: args.shot_pollen,
        state: None,
        last_frame: Instant::now(),
    };
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("window loop stopped: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

struct App {
    cluster: Option<Cluster>,
    link: Option<Link>,
    local: bool,
    port: u16,
    shot: Option<std::path::PathBuf>,
    shot_settings: Option<std::path::PathBuf>,
    shot_held_light: Option<std::path::PathBuf>,
    shot_night_spawn: Option<std::path::PathBuf>,
    shot_bed_sleep: Option<std::path::PathBuf>,
    shot_crouch: Option<std::path::PathBuf>,
    shot_sprint: Option<std::path::PathBuf>,
    shot_swim: Option<std::path::PathBuf>,
    shot_boat: Option<std::path::PathBuf>,
    shot_attack: Option<std::path::PathBuf>,
    shot_hunger: Option<std::path::PathBuf>,
    shot_eat: Option<std::path::PathBuf>,
    shot_jump: Option<std::path::PathBuf>,
    shot_place: Option<std::path::PathBuf>,
    shot_swap: Option<std::path::PathBuf>,
    shot_break: Option<std::path::PathBuf>,
    shot_campfire_smoke: Option<std::path::PathBuf>,
    shot_cook: Option<std::path::PathBuf>,
    shot_smoker_cook: Option<std::path::PathBuf>,
    shot_camp_smoker: Option<std::path::PathBuf>,
    shot_blast: Option<std::path::PathBuf>,
    shot_furnace: Option<std::path::PathBuf>,
    shot_clay: Option<std::path::PathBuf>,
    shot_pot: Option<std::path::PathBuf>,
    shot_compound: Option<std::path::PathBuf>,
    shot_creative: Option<std::path::PathBuf>,
    shot_shrine: Option<std::path::PathBuf>,
    shot_mine: Option<std::path::PathBuf>,
    shot_keybinds: Option<std::path::PathBuf>,
    shot_axolotl: Option<std::path::PathBuf>,
    shot_frog: Option<std::path::PathBuf>,
    shot_embers: Option<std::path::PathBuf>,
    shot_lantern: Option<std::path::PathBuf>,
    shot_sway: Option<std::path::PathBuf>,
    shot_rustle: Option<std::path::PathBuf>,
    shot_hiss: Option<std::path::PathBuf>,
    shot_creak: Option<std::path::PathBuf>,
    shot_gull: Option<std::path::PathBuf>,
    shot_thunk: Option<std::path::PathBuf>,
    shot_door: Option<std::path::PathBuf>,
    shot_scrape: Option<std::path::PathBuf>,
    shot_pop: Option<std::path::PathBuf>,
    shot_patter: Option<std::path::PathBuf>,
    shot_heat: Option<std::path::PathBuf>,
    shot_buzz: Option<std::path::PathBuf>,
    shot_firefly: Option<std::path::PathBuf>,
    shot_wolf: Option<std::path::PathBuf>,
    shot_cat: Option<std::path::PathBuf>,
    shot_parrot: Option<std::path::PathBuf>,
    shot_map_clay: Option<std::path::PathBuf>,
    shot_note: Option<std::path::PathBuf>,
    shot_hang: Option<std::path::PathBuf>,
    shot_jukebox: Option<std::path::PathBuf>,
    shot_bell: Option<std::path::PathBuf>,
    shot_lectern: Option<std::path::PathBuf>,
    shot_compost: Option<std::path::PathBuf>,
    shot_hive: Option<std::path::PathBuf>,
    shot_cauldron: Option<std::path::PathBuf>,
    shot_enchant: Option<std::path::PathBuf>,
    shot_grind: Option<std::path::PathBuf>,
    shot_brew: Option<std::path::PathBuf>,
    shot_smith: Option<std::path::PathBuf>,
    shot_anvil: Option<std::path::PathBuf>,
    shot_ladder: Option<std::path::PathBuf>,
    shot_fish: Option<std::path::PathBuf>,
    shot_grow: Option<std::path::PathBuf>,
    shot_wheat: Option<std::path::PathBuf>,
    shot_farmland: Option<std::path::PathBuf>,
    shot_bread: Option<std::path::PathBuf>,
    shot_plot: Option<std::path::PathBuf>,
    shot_scarecrow: Option<std::path::PathBuf>,
    shot_cart: Option<std::path::PathBuf>,
    shot_boost: Option<std::path::PathBuf>,
    shot_horse: Option<std::path::PathBuf>,
    shot_hitch: Option<std::path::PathBuf>,
    shot_stable: Option<std::path::PathBuf>,
    shot_pollen: Option<std::path::PathBuf>,
    state: Option<State>,
    last_frame: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let Some(cluster) = self.cluster.take() else {
            return;
        };
        let Some(link) = self.link.take() else {
            return;
        };
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Terralite")
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to open a window: {err}");
                eprintln!("the client needs a display and does not run headless.");
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(State::new(
            event_loop.owned_display_handle(),
            window,
            cluster,
            link,
            self.local,
            self.port,
        )) {
            Ok(mut state) => {
                if let Some(path) = self.shot.take() {
                    state.arm_zombie_shot(path);
                }
                if let Some(path) = self.shot_settings.take() {
                    state.arm_settings_shot(path);
                }
                if let Some(path) = self.shot_held_light.take() {
                    state.arm_held_light_shot(path);
                }
                if let Some(path) = self.shot_night_spawn.take() {
                    state.arm_night_spawn_shot(path);
                }
                if let Some(path) = self.shot_bed_sleep.take() {
                    state.arm_bed_sleep_shot(path);
                }
                if let Some(path) = self.shot_crouch.take() {
                    state.arm_crouch_shot(path);
                }
                if let Some(path) = self.shot_sprint.take() {
                    state.arm_sprint_shot(path);
                }
                if let Some(path) = self.shot_swim.take() {
                    state.arm_swim_shot(path);
                }
                if let Some(path) = self.shot_boat.take() {
                    state.arm_boat_shot(path);
                }
                if let Some(path) = self.shot_attack.take() {
                    state.arm_attack_shot(path);
                }
                if let Some(path) = self.shot_hunger.take() {
                    state.arm_hunger_shot(path);
                }
                if let Some(path) = self.shot_eat.take() {
                    state.arm_eat_shot(path);
                }
                if let Some(path) = self.shot_jump.take() {
                    state.arm_jump_shot(path);
                }
                if let Some(path) = self.shot_place.take() {
                    state.arm_place_shot(path);
                }
                if let Some(path) = self.shot_swap.take() {
                    state.arm_swap_shot(path);
                }
                if let Some(path) = self.shot_break.take() {
                    state.arm_break_shot(path);
                }
                if let Some(path) = self.shot_campfire_smoke.take() {
                    state.arm_campfire_smoke_shot(path);
                }
                if let Some(path) = self.shot_cook.take() {
                    state.arm_cook_shot(path);
                }
                if let Some(path) = self.shot_smoker_cook.take() {
                    state.arm_smoker_cook_shot(path);
                }
                if let Some(path) = self.shot_camp_smoker.take() {
                    state.arm_camp_smoker_shot(path);
                }
                if let Some(path) = self.shot_blast.take() {
                    state.arm_blast_shot(path);
                }
                if let Some(path) = self.shot_furnace.take() {
                    state.arm_furnace_shot(path);
                }
                if let Some(path) = self.shot_clay.take() {
                    state.arm_clay_shot(path);
                }
                if let Some(path) = self.shot_pot.take() {
                    state.arm_pot_shot(path);
                }
                if let Some(path) = self.shot_compound.take() {
                    state.arm_compound_shot(path);
                }
                if let Some(path) = self.shot_creative.take() {
                    state.arm_creative_shot(path);
                }
                if let Some(path) = self.shot_shrine.take() {
                    state.arm_shrine_shot(path);
                }
                if let Some(path) = self.shot_mine.take() {
                    state.arm_mine_shot(path);
                }
                if let Some(path) = self.shot_keybinds.take() {
                    state.arm_keybinds_shot(path);
                }
                if let Some(path) = self.shot_axolotl.take() {
                    state.arm_axolotl_shot(path);
                }
                if let Some(path) = self.shot_frog.take() {
                    state.arm_frog_shot(path);
                }
                if let Some(path) = self.shot_embers.take() {
                    state.arm_embers_shot(path);
                }
                if let Some(path) = self.shot_lantern.take() {
                    state.arm_lantern_shot(path);
                }
                if let Some(path) = self.shot_sway.take() {
                    state.arm_scarecrow_sway_shot(path);
                }
                if let Some(path) = self.shot_rustle.take() {
                    state.arm_wheat_rustle_shot(path);
                }
                if let Some(path) = self.shot_hiss.take() {
                    state.arm_lantern_hiss_shot(path);
                }
                if let Some(path) = self.shot_creak.take() {
                    state.arm_gate_creak_shot(path);
                }
                if let Some(path) = self.shot_gull.take() {
                    state.arm_dawn_gull_shot(path);
                }
                if let Some(path) = self.shot_thunk.take() {
                    state.arm_lever_thunk_shot(path);
                }
                if let Some(path) = self.shot_door.take() {
                    state.arm_door_creak_shot(path);
                }
                if let Some(path) = self.shot_scrape.take() {
                    state.arm_chest_scrape_shot(path);
                }
                if let Some(path) = self.shot_pop.take() {
                    state.arm_torch_pop_shot(path);
                }
                if let Some(path) = self.shot_patter.take() {
                    state.arm_rain_leaves_shot(path);
                }
                if let Some(path) = self.shot_heat.take() {
                    state.arm_furnace_heat_shot(path);
                }
                if let Some(path) = self.shot_buzz.take() {
                    state.arm_bee_buzz_shot(path);
                }
                if let Some(path) = self.shot_firefly.take() {
                    state.arm_firefly_shot(path);
                }
                if let Some(path) = self.shot_wolf.take() {
                    state.arm_wolf_shot(path);
                }
                if let Some(path) = self.shot_cat.take() {
                    state.arm_cat_shot(path);
                }
                if let Some(path) = self.shot_parrot.take() {
                    state.arm_parrot_shot(path);
                }
                if let Some(path) = self.shot_map_clay.take() {
                    state.arm_map_clay_shot(path);
                }
                if let Some(path) = self.shot_note.take() {
                    state.arm_note_shot(path);
                }
                if let Some(path) = self.shot_hang.take() {
                    state.arm_hang_shot(path);
                }
                if let Some(path) = self.shot_jukebox.take() {
                    state.arm_jukebox_shot(path);
                }
                if let Some(path) = self.shot_bell.take() {
                    state.arm_bell_shot(path);
                }
                if let Some(path) = self.shot_lectern.take() {
                    state.arm_lectern_shot(path);
                }
                if let Some(path) = self.shot_compost.take() {
                    state.arm_compost_shot(path);
                }
                if let Some(path) = self.shot_hive.take() {
                    state.arm_hive_shot(path);
                }
                if let Some(path) = self.shot_cauldron.take() {
                    state.arm_cauldron_shot(path);
                }
                if let Some(path) = self.shot_enchant.take() {
                    state.arm_enchant_shot(path);
                }
                if let Some(path) = self.shot_grind.take() {
                    state.arm_grind_shot(path);
                }
                if let Some(path) = self.shot_brew.take() {
                    state.arm_brew_shot(path);
                }
                if let Some(path) = self.shot_smith.take() {
                    state.arm_smith_shot(path);
                }
                if let Some(path) = self.shot_anvil.take() {
                    state.arm_anvil_shot(path);
                }
                if let Some(path) = self.shot_ladder.take() {
                    state.arm_ladder_shot(path);
                }
                if let Some(path) = self.shot_fish.take() {
                    state.arm_fish_shot(path);
                }
                if let Some(path) = self.shot_grow.take() {
                    state.arm_grow_shot(path);
                }
                if let Some(path) = self.shot_wheat.take() {
                    state.arm_wheat_shot(path);
                }
                if let Some(path) = self.shot_farmland.take() {
                    state.arm_farmland_shot(path);
                }
                if let Some(path) = self.shot_bread.take() {
                    state.arm_bread_shot(path);
                }
                if let Some(path) = self.shot_plot.take() {
                    state.arm_farm_plot_shot(path);
                }
                if let Some(path) = self.shot_scarecrow.take() {
                    state.arm_scarecrow_shot(path);
                }
                if let Some(path) = self.shot_cart.take() {
                    state.arm_minecart_shot(path);
                }
                if let Some(path) = self.shot_boost.take() {
                    state.arm_boost_shot(path);
                }
                if let Some(path) = self.shot_horse.take() {
                    state.arm_horse_shot(path);
                }
                if let Some(path) = self.shot_hitch.take() {
                    state.arm_hitch_shot(path);
                }
                if let Some(path) = self.shot_stable.take() {
                    state.arm_stable_shot(path);
                }
                if let Some(path) = self.shot_pollen.take() {
                    state.arm_pollen_shot(path);
                }
                state.window().request_redraw();
                self.last_frame = Instant::now();
                self.state = Some(state);
            }
            Err(err) => {
                eprintln!("failed to create a renderer: {err}");
                eprintln!("the client needs a display and a graphics adapter.");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
            return;
        };
        let egui_ate = !matches!(
            event,
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) | WindowEvent::CloseRequested
        ) && state.egui_on_event(&event);
        if egui_ate {
            state.window().request_redraw();
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now
                    .saturating_duration_since(self.last_frame)
                    .as_secs_f32()
                    .min(0.05);
                self.last_frame = now;
                state.tick(dt);
                if state.wants_quit() {
                    event_loop.exit();
                    return;
                }
                state.render();
                state.window().request_redraw();
            }
            WindowEvent::Focused(false) => state.set_capture(false),
            WindowEvent::CursorMoved { position, .. } => {
                state.pointer_at(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let down = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                    MouseScrollDelta::PixelDelta(delta) => delta.y < 0.0,
                };
                state.pointer_scroll(down);
            }
            WindowEvent::MouseInput {
                state: element,
                button,
                ..
            } => {
                state.set_mouse(button, element == ElementState::Pressed);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
                {
                    if state.is_dead() {
                        state.set_key(KeyCode::Escape, true);
                        return;
                    }
                    if state.screen_open() {
                        state.close_screen();
                        return;
                    }
                    if state.menu_open() {
                        state.menu_escape();
                        return;
                    }
                    if state.is_captured() {
                        state.set_capture(false);
                        return;
                    }
                    state.open_menu();
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    state.set_key(code, event.state == ElementState::Pressed);
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        match event {
            DeviceEvent::MouseMotion { delta } => {
                if let Some(state) = self.state.as_mut() {
                    state.mouse_look(delta.0, delta.1);
                }
            }
            // Raw right button (1) still fires when a grabbed cursor drops
            // WindowEvent::MouseInput for some Windows trackpads.
            DeviceEvent::Button { button, state: element } => {
                if let Some(mapped) =
                    crate::keybinds::Keybinds::mouse_from_device_button(button)
                {
                    if let Some(state) = self.state.as_mut() {
                        state.set_mouse(mapped, element == ElementState::Pressed);
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_args<I>(iter: I) -> Result<Parsed, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = iter.into_iter().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Parsed::Help);
    }
    let mut parsed = Args {
        port: DEFAULT_PORT,
        shot: None,
        shot_settings: None,
        shot_held_light: None,
        shot_night_spawn: None,
        shot_bed_sleep: None,
        shot_crouch: None,
        shot_sprint: None,
        shot_swim: None,
        shot_boat: None,
        shot_attack: None,
        shot_hunger: None,
        shot_eat: None,
        shot_jump: None,
        shot_place: None,
        shot_swap: None,
        shot_break: None,
        shot_campfire_smoke: None,
        shot_cook: None,
        shot_smoker_cook: None,
        shot_camp_smoker: None,
        shot_blast: None,
        shot_furnace: None,
        shot_clay: None,
        shot_pot: None,
        shot_compound: None,
        shot_creative: None,
        shot_shrine: None,
        shot_mine: None,
        shot_keybinds: None,
        shot_axolotl: None,
        shot_frog: None,
        shot_embers: None,
        shot_lantern: None,
        shot_sway: None,
        shot_rustle: None,
        shot_hiss: None,
        shot_creak: None,
        shot_gull: None,
        shot_thunk: None,
        shot_door: None,
        shot_scrape: None,
        shot_pop: None,
        shot_patter: None,
        shot_heat: None,
        shot_buzz: None,
        shot_firefly: None,
        shot_wolf: None,
        shot_cat: None,
        shot_parrot: None,
        shot_map_clay: None,
        shot_note: None,
        shot_hang: None,
        shot_jukebox: None,
        shot_bell: None,
        shot_lectern: None,
        shot_compost: None,
        shot_hive: None,
        shot_cauldron: None,
        shot_enchant: None,
        shot_grind: None,
        shot_brew: None,
        shot_smith: None,
        shot_anvil: None,
        shot_ladder: None,
        shot_fish: None,
        shot_grow: None,
        shot_wheat: None,
        shot_farmland: None,
        shot_bread: None,
        shot_plot: None,
        shot_scarecrow: None,
        shot_cart: None,
        shot_boost: None,
        shot_horse: None,
        shot_hitch: None,
        shot_stable: None,
        shot_pollen: None,
    };
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --port".to_string())?;
                parsed.port = value
                    .parse()
                    .map_err(|_| format!("bad --port value: {value}"))?;
            }
            "--shot" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot".to_string())?;
                parsed.shot = Some(std::path::PathBuf::from(value));
            }
            "--shot-settings" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-settings".to_string())?;
                parsed.shot_settings = Some(std::path::PathBuf::from(value));
            }
            "--shot-held-light" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-held-light".to_string())?;
                parsed.shot_held_light = Some(std::path::PathBuf::from(value));
            }
            "--shot-night-spawn" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-night-spawn".to_string())?;
                parsed.shot_night_spawn = Some(std::path::PathBuf::from(value));
            }
            "--shot-bed-sleep" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-bed-sleep".to_string())?;
                parsed.shot_bed_sleep = Some(std::path::PathBuf::from(value));
            }
            "--shot-crouch-sneak" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-crouch-sneak".to_string())?;
                parsed.shot_crouch = Some(std::path::PathBuf::from(value));
            }
            "--shot-sprint-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-sprint-fov".to_string())?;
                parsed.shot_sprint = Some(std::path::PathBuf::from(value));
            }
            "--shot-swim-water" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-swim-water".to_string())?;
                parsed.shot_swim = Some(std::path::PathBuf::from(value));
            }
            "--shot-boat-row" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-boat-row".to_string())?;
                parsed.shot_boat = Some(std::path::PathBuf::from(value));
            }
            "--shot-attack-cd" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-attack-cd".to_string())?;
                parsed.shot_attack = Some(std::path::PathBuf::from(value));
            }
            "--shot-hunger-low" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-hunger-low".to_string())?;
                parsed.shot_hunger = Some(std::path::PathBuf::from(value));
            }
            "--shot-eating-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-eating-fov".to_string())?;
                parsed.shot_eat = Some(std::path::PathBuf::from(value));
            }
            "--shot-jump-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-jump-fov".to_string())?;
                parsed.shot_jump = Some(std::path::PathBuf::from(value));
            }
            "--shot-place-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-place-fov".to_string())?;
                parsed.shot_place = Some(std::path::PathBuf::from(value));
            }
            "--shot-swap-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-swap-fov".to_string())?;
                parsed.shot_swap = Some(std::path::PathBuf::from(value));
            }
            "--shot-break-fov" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-break-fov".to_string())?;
                parsed.shot_break = Some(std::path::PathBuf::from(value));
            }
            "--shot-campfire-smoke" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-campfire-smoke".to_string())?;
                parsed.shot_campfire_smoke = Some(std::path::PathBuf::from(value));
            }
            "--shot-cook" | "--shot-campfire-cook" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-cook".to_string())?;
                parsed.shot_cook = Some(std::path::PathBuf::from(value));
            }
            "--shot-smoker-cook" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-smoker-cook".to_string())?;
                parsed.shot_smoker_cook = Some(std::path::PathBuf::from(value));
            }
            "--shot-camp-smoker" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-camp-smoker".to_string())?;
                parsed.shot_camp_smoker = Some(std::path::PathBuf::from(value));
            }
            "--shot-blast" | "--shot-blast-furnace" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-blast".to_string())?;
                parsed.shot_blast = Some(std::path::PathBuf::from(value));
            }
            "--shot-furnace" | "--shot-furnace-bake" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-furnace".to_string())?;
                parsed.shot_furnace = Some(std::path::PathBuf::from(value));
            }
            "--shot-clay" | "--shot-clay-bank" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-clay".to_string())?;
                parsed.shot_clay = Some(std::path::PathBuf::from(value));
            }
            "--shot-pot" | "--shot-decorated-pot" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-pot".to_string())?;
                parsed.shot_pot = Some(std::path::PathBuf::from(value));
            }
            "--shot-compound" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-compound".to_string())?;
                parsed.shot_compound = Some(std::path::PathBuf::from(value));
            }
            "--shot-creative" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-creative".to_string())?;
                parsed.shot_creative = Some(std::path::PathBuf::from(value));
            }
            "--shot-shrine" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-shrine".to_string())?;
                parsed.shot_shrine = Some(std::path::PathBuf::from(value));
            }
            "--shot-mine" | "--shot-mine-chest" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-mine".to_string())?;
                parsed.shot_mine = Some(std::path::PathBuf::from(value));
            }
            "--shot-keybinds" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-keybinds".to_string())?;
                parsed.shot_keybinds = Some(std::path::PathBuf::from(value));
            }
            "--shot-axolotl" | "--shot-leash" | "--shot-axolotl-leash" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-axolotl".to_string())?;
                parsed.shot_axolotl = Some(std::path::PathBuf::from(value));
            }
            "--shot-frog" | "--shot-lily" | "--shot-lily-pad" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-frog".to_string())?;
                parsed.shot_frog = Some(std::path::PathBuf::from(value));
            }
            "--shot-embers" | "--shot-campfire-embers" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-embers".to_string())?;
                parsed.shot_embers = Some(std::path::PathBuf::from(value));
            }
            "--shot-lantern" | "--shot-flicker" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-lantern".to_string())?;
                parsed.shot_lantern = Some(std::path::PathBuf::from(value));
            }
            "--shot-sway" | "--shot-scarecrow-sway" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-sway".to_string())?;
                parsed.shot_sway = Some(std::path::PathBuf::from(value));
            }
            "--shot-rustle" | "--shot-wheat-rustle" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-rustle".to_string())?;
                parsed.shot_rustle = Some(std::path::PathBuf::from(value));
            }
            "--shot-hiss" | "--shot-lantern-hiss" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-hiss".to_string())?;
                parsed.shot_hiss = Some(std::path::PathBuf::from(value));
            }
            "--shot-creak" | "--shot-gate" | "--shot-fence-gate" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-creak".to_string())?;
                parsed.shot_creak = Some(std::path::PathBuf::from(value));
            }
            "--shot-gull" | "--shot-dawn-gull" | "--shot-shore-gull" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-gull".to_string())?;
                parsed.shot_gull = Some(std::path::PathBuf::from(value));
            }
            "--shot-thunk" | "--shot-lever" | "--shot-button" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-thunk".to_string())?;
                parsed.shot_thunk = Some(std::path::PathBuf::from(value));
            }
            "--shot-door" | "--shot-door-creak" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-door".to_string())?;
                parsed.shot_door = Some(std::path::PathBuf::from(value));
            }
            "--shot-scrape" | "--shot-chest" | "--shot-chest-scrape" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-scrape".to_string())?;
                parsed.shot_scrape = Some(std::path::PathBuf::from(value));
            }
            "--shot-pop" | "--shot-torch" | "--shot-torch-pop" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-pop".to_string())?;
                parsed.shot_pop = Some(std::path::PathBuf::from(value));
            }
            "--shot-patter" | "--shot-rain-leaves" | "--shot-leaves" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-patter".to_string())?;
                parsed.shot_patter = Some(std::path::PathBuf::from(value));
            }
            "--shot-heat" | "--shot-furnace-heat" | "--shot-cook-heat" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-heat".to_string())?;
                parsed.shot_heat = Some(std::path::PathBuf::from(value));
            }
            "--shot-buzz" | "--shot-bee" | "--shot-bee-buzz" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-buzz".to_string())?;
                parsed.shot_buzz = Some(std::path::PathBuf::from(value));
            }
            "--shot-firefly" | "--shot-fireflies" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-firefly".to_string())?;
                parsed.shot_firefly = Some(std::path::PathBuf::from(value));
            }
            "--shot-wolf" | "--shot-wolf-sit" | "--shot-heel" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-wolf".to_string())?;
                parsed.shot_wolf = Some(std::path::PathBuf::from(value));
            }
            "--shot-cat" | "--shot-cat-sit" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-cat".to_string())?;
                parsed.shot_cat = Some(std::path::PathBuf::from(value));
            }
            "--shot-parrot" | "--shot-shoulder" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-parrot".to_string())?;
                parsed.shot_parrot = Some(std::path::PathBuf::from(value));
            }
            "--shot-map-clay" | "--shot-clay-map" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-map-clay".to_string())?;
                parsed.shot_map_clay = Some(std::path::PathBuf::from(value));
            }
            "--shot-note" | "--shot-note-block" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-note".to_string())?;
                parsed.shot_note = Some(std::path::PathBuf::from(value));
            }
            "--shot-hang" | "--shot-hanging-sign" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-hang".to_string())?;
                parsed.shot_hang = Some(std::path::PathBuf::from(value));
            }
            "--shot-jukebox" | "--shot-disc" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-jukebox".to_string())?;
                parsed.shot_jukebox = Some(std::path::PathBuf::from(value));
            }
            "--shot-bell" | "--shot-shrine-bell" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-bell".to_string())?;
                parsed.shot_bell = Some(std::path::PathBuf::from(value));
            }
            "--shot-lectern" | "--shot-book" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-lectern".to_string())?;
                parsed.shot_lectern = Some(std::path::PathBuf::from(value));
            }
            "--shot-compost" | "--shot-composter" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-compost".to_string())?;
                parsed.shot_compost = Some(std::path::PathBuf::from(value));
            }
            "--shot-hive" | "--shot-beehive" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-hive".to_string())?;
                parsed.shot_hive = Some(std::path::PathBuf::from(value));
            }
            "--shot-cauldron" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-cauldron".to_string())?;
                parsed.shot_cauldron = Some(std::path::PathBuf::from(value));
            }
            "--shot-enchant" | "--shot-enchanting" | "--shot-chime" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-enchant".to_string())?;
                parsed.shot_enchant = Some(std::path::PathBuf::from(value));
            }
            "--shot-grind" | "--shot-grindstone" | "--shot-grind-scrape" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-grind".to_string())?;
                parsed.shot_grind = Some(std::path::PathBuf::from(value));
            }
            "--shot-brew" | "--shot-brewing" | "--shot-bubble" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-brew".to_string())?;
                parsed.shot_brew = Some(std::path::PathBuf::from(value));
            }
            "--shot-smith" | "--shot-smithing" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-smith".to_string())?;
                parsed.shot_smith = Some(std::path::PathBuf::from(value));
            }
            "--shot-anvil" | "--shot-clang" | "--shot-anvil-ring" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-anvil".to_string())?;
                parsed.shot_anvil = Some(std::path::PathBuf::from(value));
            }
            "--shot-ladder" | "--shot-climb" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-ladder".to_string())?;
                parsed.shot_ladder = Some(std::path::PathBuf::from(value));
            }
            "--shot-fish" | "--shot-fishing" | "--shot-bobber" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-fish".to_string())?;
                parsed.shot_fish = Some(std::path::PathBuf::from(value));
            }
            "--shot-grow" | "--shot-cane" | "--shot-fertilize" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-grow".to_string())?;
                parsed.shot_grow = Some(std::path::PathBuf::from(value));
            }
            "--shot-wheat" | "--shot-crop" | "--shot-harvest" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-wheat".to_string())?;
                parsed.shot_wheat = Some(std::path::PathBuf::from(value));
            }
            "--shot-farmland" | "--shot-till" | "--shot-hoe" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-farmland".to_string())?;
                parsed.shot_farmland = Some(std::path::PathBuf::from(value));
            }
            "--shot-bread" | "--shot-loaf" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-bread".to_string())?;
                parsed.shot_bread = Some(std::path::PathBuf::from(value));
            }
            "--shot-plot" | "--shot-farm-plot" | "--shot-fence-farm" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-plot".to_string())?;
                parsed.shot_plot = Some(std::path::PathBuf::from(value));
            }
            "--shot-scarecrow" | "--shot-crow" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-scarecrow".to_string())?;
                parsed.shot_scarecrow = Some(std::path::PathBuf::from(value));
            }
            "--shot-cart" | "--shot-minecart" | "--shot-rails" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-cart".to_string())?;
                parsed.shot_cart = Some(std::path::PathBuf::from(value));
            }
            "--shot-boost" | "--shot-powered-rail" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-boost".to_string())?;
                parsed.shot_boost = Some(std::path::PathBuf::from(value));
            }
            "--shot-horse" | "--shot-gallop" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-horse".to_string())?;
                parsed.shot_horse = Some(std::path::PathBuf::from(value));
            }
            "--shot-hitch" | "--shot-lead" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-hitch".to_string())?;
                parsed.shot_hitch = Some(std::path::PathBuf::from(value));
            }
            "--shot-stable" | "--shot-meadow-stable" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-stable".to_string())?;
                parsed.shot_stable = Some(std::path::PathBuf::from(value));
            }
            "--shot-pollen" | "--shot-bee-trail" | "--shot-bee-pollen" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --shot-pollen".to_string())?;
                parsed.shot_pollen = Some(std::path::PathBuf::from(value));
            }
            other => return Err(format!("unknown argument: {other}\n{USAGE}")),
        }
    }
    Ok(Parsed::Run(parsed))
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        hwnd: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        typ: u32,
    ) -> i32;
}

/// Keep a pre-window failure on screen when the console would close.
fn tell_user(message: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut text: Vec<u16> = std::ffi::OsStr::new(message).encode_wide().collect();
        text.push(0);
        let mut title: Vec<u16> = std::ffi::OsStr::new("Terralite").encode_wide().collect();
        title.push(0);
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                0x0000_0010,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_defaults_and_rejects_a_region_flag() {
        let Parsed::Run(args) = parse_args(std::iter::empty::<String>()).unwrap() else {
            panic!("expected run args");
        };
        assert_eq!(args.port, DEFAULT_PORT);
        let Parsed::Run(args) = parse_args(["--port".into(), "20000".into()]).unwrap() else {
            panic!("expected run args");
        };
        assert_eq!(args.port, 20000);
        assert!(parse_args(["--region".into(), "r.0.0.mca".into()]).is_err());
        assert!(parse_args(["--port".into()]).is_err());
        assert!(matches!(
            parse_args(["--help".to_string()]).unwrap(),
            Parsed::Help
        ));
    }
}
