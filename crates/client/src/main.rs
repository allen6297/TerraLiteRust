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
mod look;
mod loot;
mod map;
mod mesh;
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
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
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
WASD walks, mouse looks, Space jumps, Ctrl is faster, Shift crouches. F5 toggles a third-person look so you can see your Blockbench body walk and dig. A small cross marks the center, and the block under it is named just below. Stepping into water splashes, and swimming is a stroke. An axolotl pauses, then swims the marsh just south of the stairs, at (8, 32). Right-click it and it follows through the water. A chicken pecks the grass west of the oak, at (3, 8). It stands and pecks, walks one side of the yard, and steps back when you stand close. Right-click the egg and it hands you raw food. A Blockbench zombie idles by the ore line at (11, 13), walks in to bite, and flashes red when the mace hits. Esc releases the mouse,
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
            } => match button {
                MouseButton::Left if state.is_dead() && element == ElementState::Pressed => {
                    state.pointer_click();
                }
                MouseButton::Left
                    if (state.screen_open() || state.menu_open())
                        && element == ElementState::Pressed =>
                {
                    state.pointer_click();
                }
                MouseButton::Left if state.screen_open() || state.menu_open() => {}
                MouseButton::Left if element == ElementState::Released => state.set_breaking(false),
                MouseButton::Left if state.is_captured() && element == ElementState::Pressed => {
                    state.set_breaking(true);
                }
                MouseButton::Left if element == ElementState::Pressed => state.set_capture(true),
                MouseButton::Right if element == ElementState::Released => state.release_spyglass(),
                MouseButton::Right if state.screen_open() || state.menu_open() => {}
                MouseButton::Right if state.is_captured() && element == ElementState::Pressed => {
                    state.place_targeted();
                }
                _ => {}
            },
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
        if let DeviceEvent::MouseMotion { delta } = event {
            if let Some(state) = self.state.as_mut() {
                state.mouse_look(delta.0, delta.1);
            }
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
