use std::io::{self, ErrorKind};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;

use rebuild_world::{
    CellChange, ClientRequest, Cluster, DEFAULT_PORT, Edit, EntityClock, Hooks, PROTOCOL_VERSION,
    PeerPose, PlayerPlace, TAG_BREAK, TAG_CELL, TAG_PLACE, apply_authoritative, apply_changes,
    bar_from_wire, block_definitions_file, carried_from_wire, chat_line, data_pack_root,
    decode_client, encode_cell, encode_chat, encode_peers, encode_welcome, ensure_chest,
    load_cluster_from_path, load_data_pack, load_dir, localhost, occupied_cells, read_changes,
    read_frame, remove_chest, store_chest_slots, synthetic_cluster, write_changes, write_frame,
    write_hotbar, write_inventory, write_player,
};

const SAVE_PATH: &str = r"C:\CODE\TerraLite\saves\blocks.json";
const MODS_DIR: &str = r"C:\CODE\TerraLite\mods";

const USAGE: &str = "\
Usage: rebuild-server [--region <file.mca>] [--x <chunk x>] [--z <chunk z>] [--port <port>]

Terralite server. With no --region, load the built-in synthetic small world,
31x31 chunks around chunk (0, 0), then lay
C:\\CODE\\TerraLite\\saves\\blocks.json over it when that file exists.
Listen on 127.0.0.1 (default port 17610) for Terralite clients
(rebuild-client). More than one can be joined. Each player's position is
sent to the others. A settings world-size packet rebuilds this square and
keeps the saved cells. The server decides breaks and places and writes
that save. It reads block models from
C:\\CODE\\TerraLite\\resourcepacks\\terralite\\assets\\terralite\\models\\block
and C:\\CODE\\TerraLite\\datapacks\\terralite for hardness and replaces.
This is a private frame protocol, version 1, not
protocol 47.
--x and --z pick the center chunk. A file named r.X.Z.mca also reads
neighboring region files in the same folder. Empty slots are air.
A region file does not read or write the block save.
";

struct Args {
    region: Option<PathBuf>,
    x: i32,
    z: i32,
    port: u16,
}

enum Parsed {
    Help,
    Run(Args),
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(args)) => match serve(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(1)
            }
        },
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

/// Load the world and print the summary. Does not listen and does not read the save.
#[cfg(test)]
fn run(args: &Args) -> Result<String, String> {
    reject_shifted_synthetic(args)?;
    let (cluster, source) = load(args).map_err(|err| err.to_string())?;
    Ok(cluster.summary(source))
}

fn serve(args: &Args) -> Result<(), String> {
    reject_shifted_synthetic(args)?;
    let (world, changes, save, source) = load_to_serve(args)?;
    println!("{}", world.summary(source));
    prepare_data_pack()?;
    let hooks = prepare_hooks()?;
    listen(args.port, world, changes, save, hooks)
}

/// Load hardness and replaces from the data pack. A bad pack stops the server.
fn prepare_data_pack() -> Result<(), String> {
    load_data_pack()?;
    println!("block definitions: {}", block_definitions_file().display());
    println!(
        "data pack: {} (hardness and replaces)",
        data_pack_root().display()
    );
    Ok(())
}

/// Load every Rosegold script in `mods/`. A missing folder leaves the hook list empty.
fn prepare_hooks() -> Result<Hooks, String> {
    let mut hooks = Hooks::new();
    let dir = Path::new(MODS_DIR);
    for (path, found) in load_dir(dir, &mut hooks)? {
        for hook in found {
            println!(
                "rosegold: {} {} \"{}\"",
                path.display(),
                hook.kind(),
                hook.id()
            );
        }
    }
    Ok(hooks)
}

fn reject_shifted_synthetic(args: &Args) -> Result<(), String> {
    if args.region.is_none() && (args.x != 0 || args.z != 0) {
        Err(
            "the synthetic fixture is the small 31x31 around chunk (0, 0); pass --region to load another cluster"
                .into(),
        )
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn load(args: &Args) -> Result<(Cluster, &'static str), rebuild_world::Error> {
    match &args.region {
        Some(path) => Ok((load_cluster_from_path(path, args.x, args.z)?, "region")),
        None => Ok((synthetic_cluster(), "synthetic")),
    }
}

fn load_to_serve(
    args: &Args,
) -> Result<(Cluster, Vec<CellChange>, Option<PathBuf>, &'static str), String> {
    match &args.region {
        Some(path) => {
            let cluster =
                load_cluster_from_path(path, args.x, args.z).map_err(|err| err.to_string())?;
            Ok((cluster, Vec::new(), None, "region"))
        }
        None => {
            let mut cluster = synthetic_cluster();
            let save_path = PathBuf::from(SAVE_PATH);
            let changes = match read_changes(&save_path) {
                Ok(changes) => changes,
                Err(err) => {
                    eprintln!("block save was not loaded: {err}");
                    Vec::new()
                }
            };
            let applied = apply_changes(&mut cluster, &changes);
            if applied > 0 {
                println!("loaded {applied} saved blocks from {}", save_path.display());
            }
            Ok((cluster, changes, Some(save_path), "synthetic"))
        }
    }
}

struct LivePlayer {
    id: u32,
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
}

/// One chat line waiting for other Host/Join peers.
struct PendingChat {
    seq: u64,
    from: u32,
    text: String,
}

struct ServeState {
    world: Cluster,
    changes: Vec<CellChange>,
    peers: Vec<LivePlayer>,
    next_id: u32,
    /// Monotonic chat sequence; each peer tracks how far it has drained.
    next_chat: u64,
    /// Recent chat lines for peer fan-out (sender already echoed locally).
    chat_log: Vec<PendingChat>,
    hooks: Hooks,
}

fn empty_state(world: Cluster, changes: Vec<CellChange>, hooks: Hooks) -> ServeState {
    ServeState {
        world,
        changes,
        peers: Vec::new(),
        next_id: 1,
        next_chat: 1,
        chat_log: Vec::new(),
        hooks,
    }
}

/// Write chat lines this peer has not seen yet (skip lines they authored).
fn deliver_chats(
    stream: &mut impl io::Write,
    state: &ServeState,
    id: u32,
    last_chat: &mut u64,
) -> io::Result<()> {
    for entry in &state.chat_log {
        if entry.seq <= *last_chat {
            continue;
        }
        *last_chat = entry.seq;
        if entry.from == id {
            continue;
        }
        write_frame(stream, &encode_chat(&entry.text))?;
    }
    Ok(())
}

fn note_chat(state: &mut ServeState, from: u32, text: String) {
    if text.is_empty() {
        return;
    }
    let seq = state.next_chat;
    state.next_chat = state.next_chat.saturating_add(1);
    state.chat_log.push(PendingChat { seq, from, text });
    if state.chat_log.len() > 32 {
        let extra = state.chat_log.len() - 32;
        state.chat_log.drain(0..extra);
    }
}

fn listen(
    port: u16,
    world: Cluster,
    changes: Vec<CellChange>,
    save: Option<PathBuf>,
    hooks: Hooks,
) -> Result<(), String> {
    let listener = TcpListener::bind(localhost(port))
        .map_err(|err| format!("could not listen on 127.0.0.1:{port}: {err}"))?;
    println!("listening on 127.0.0.1:{port}");
    let state = Arc::new(Mutex::new(empty_state(world, changes, hooks)));
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(err) = stream.set_nodelay(true) {
                    eprintln!("client session ended: {err}");
                    continue;
                }
                let state = Arc::clone(&state);
                let save = save.clone();
                thread::spawn(move || {
                    if let Err(err) = session(state, save, stream) {
                        if is_disconnect(&err) {
                            println!("client disconnected");
                        } else {
                            eprintln!("client session ended: {err}");
                        }
                    }
                });
            }
            Err(err) => eprintln!("accept failed: {err}"),
        }
    }
    Ok(())
}

struct PeerLeave<'a> {
    state: &'a Mutex<ServeState>,
    id: u32,
}

impl Drop for PeerLeave<'_> {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            guard.peers.retain(|peer| peer.id != self.id);
        }
    }
}

fn note_peer(peers: &mut Vec<LivePlayer>, id: u32, x: f32, y: f32, z: f32, yaw: f32) {
    if let Some(peer) = peers.iter_mut().find(|peer| peer.id == id) {
        peer.x = x;
        peer.y = y;
        peer.z = z;
        peer.yaw = yaw;
        return;
    }
    peers.push(LivePlayer { id, x, y, z, yaw });
}

fn other_peers(peers: &[LivePlayer], id: u32) -> Vec<PeerPose> {
    peers
        .iter()
        .filter(|peer| peer.id != id)
        .map(|peer| PeerPose {
            x: peer.x,
            y: peer.y,
            z: peer.z,
            yaw: peer.yaw,
        })
        .collect()
}

fn session(
    state: Arc<Mutex<ServeState>>,
    save: Option<PathBuf>,
    mut stream: impl io::Read + io::Write,
) -> io::Result<()> {
    let hello = decode_client(&read_frame(&mut stream)?).map_err(invalid)?;
    let ClientRequest::Hello { version } = hello else {
        return Err(invalid("first packet must be hello"));
    };
    if version != PROTOCOL_VERSION {
        return Err(invalid(format!("protocol version {version}")));
    }
    let id = {
        let mut guard = state.lock().map_err(|_| invalid("world lock"))?;
        let id = guard.next_id;
        guard.next_id = guard.next_id.saturating_add(1);
        let (sx, _, sz) = rebuild_world::PLAYER_SPAWN;
        note_peer(
            &mut guard.peers,
            id,
            sx as f32 + 0.5,
            5.12,
            sz as f32 + 0.5,
            -1.57,
        );
        let cells = occupied_cells(&guard.world);
        let (center_x, center_z) = guard.world.center();
        let welcome = encode_welcome(
            center_x,
            center_z,
            guard.world.size().chunk_radius(),
            &cells,
        );
        write_frame(&mut stream, &welcome)?;
        id
    };
    let _leave = PeerLeave { state: &state, id };
    let mut spawned = false;
    let mut last_chat = 0u64;
    let mut entities = EntityClock::new();
    loop {
        let request = decode_client(&read_frame(&mut stream)?).map_err(invalid)?;
        let mut guard = state.lock().map_err(|_| invalid("world lock"))?;
        if let ClientRequest::Chat { text } = &request {
            let line = chat_line(text);
            // Echo so the sender's pump does not stall; queue for other peers.
            note_chat(&mut guard, id, line.clone());
            write_frame(&mut stream, &encode_chat(&line))?;
            continue;
        }
        if let ClientRequest::Pose {
            x,
            y,
            z,
            yaw,
            pitch,
        } = &request
        {
            if let Some(path) = save.as_deref() {
                store_pose(path, *x, *y, *z, *yaw, *pitch);
            }
            note_peer(&mut guard.peers, id, *x, *y, *z, *yaw);
            // Peer chat rides ahead of the pose list; client drains TAG_CHAT then peers.
            deliver_chats(&mut stream, &guard, id, &mut last_chat)?;
            let others = other_peers(&guard.peers, id);
            write_frame(&mut stream, &encode_peers(&others))?;
            continue;
        }
        let ServeState {
            world,
            changes,
            peers: _,
            hooks,
            ..
        } = &mut *guard;
        entities.catch_up(hooks, world);
        let _ = hooks.drain_writes();
        if let ClientRequest::Hotbar { selected, slots } = &request {
            hooks.set_inventory(*selected, slots.clone());
            if let Some(path) = save.as_deref() {
                store_hotbar(path, *selected, slots);
            }
            if !spawned {
                let (x, y, z) = rebuild_world::PLAYER_SPAWN;
                hooks.entity_spawn(world, "player", x, y, z);
                hooks.spawn_demo_mobs(world);
                spawned = true;
                send_writes(world, changes, save.as_deref(), hooks, None, &mut stream)?;
            }
            continue;
        }
        let (changed, kind, x, y, z) = match request {
            ClientRequest::Break { x, y, z } => {
                rebuild_world::ensure_terrain(world, x.div_euclid(16), z.div_euclid(16));
                let changed = apply_authoritative(world, Edit::Break { x, y, z }, hooks);
                (changed, TAG_BREAK, x, y, z)
            }
            ClientRequest::Place {
                x,
                y,
                z,
                block,
                player,
            } => {
                rebuild_world::ensure_terrain(world, x.div_euclid(16), z.div_euclid(16));
                let changed = apply_authoritative(
                    world,
                    Edit::Place {
                        x,
                        y,
                        z,
                        block,
                        player,
                    },
                    hooks,
                );
                (changed, TAG_PLACE, x, y, z)
            }
            ClientRequest::Hello { .. } => {
                return Err(invalid("hello is only the first packet"));
            }
            ClientRequest::Hotbar { .. } => continue,
            ClientRequest::Bag { slots } => {
                if let Some(path) = save.as_deref() {
                    store_bag(path, &slots);
                }
                continue;
            }
            ClientRequest::Pose { .. } => continue,
            ClientRequest::Chat { .. } => continue,
            ClientRequest::Chest { x, y, z, slots } => {
                if let Some(path) = save.as_deref() {
                    let block = world.block_at(x, y, z);
                    if rebuild_world::Pack::builtin().is_block_entity(block.id) {
                        if let Ok(carried) = carried_from_wire(&slots) {
                            if let Err(err) = store_chest_slots(path, x, y, z, block.id, &carried) {
                                eprintln!("could not save chest: {err}");
                            }
                        }
                    }
                }
                continue;
            }
            ClientRequest::World { size } => {
                if save.is_some() {
                    let loaded = changes.clone();
                    let mut next = rebuild_world::synthetic_cluster_sized(size);
                    rebuild_world::seed_terrain_ring(&mut next);
                    apply_changes(&mut next, &loaded);
                    *world = next;
                }
                continue;
            }
            ClientRequest::Save { .. } => continue,
        };
        if changed {
            if let Some(path) = save.as_deref() {
                remember(changes, path, world, x, y, z);
                let block = world.block_at(x, y, z);
                sync_container(path, x, y, z, block);
            }
        }
        let block = world.block_at(x, y, z);
        write_frame(&mut stream, &encode_cell(changed, kind, x, y, z, block))?;
        send_writes(
            world,
            changes,
            save.as_deref(),
            hooks,
            Some((x, y, z)),
            &mut stream,
        )?;
    }
}

fn sync_container(path: &Path, x: i32, y: i32, z: i32, block: rebuild_world::Block) {
    if rebuild_world::Pack::builtin().is_block_entity(block.id) {
        if let Err(err) = ensure_chest(path, x, y, z, block.id) {
            eprintln!("could not save chest: {err}");
        }
    } else if let Err(err) = remove_chest(path, x, y, z) {
        eprintln!("could not save chest: {err}");
    }
}

fn send_writes(
    world: &Cluster,
    changes: &mut Vec<CellChange>,
    save: Option<&Path>,
    hooks: &Hooks,
    skip: Option<(i32, i32, i32)>,
    stream: &mut impl io::Write,
) -> io::Result<()> {
    for change in hooks.drain_writes() {
        if skip == Some((change.x, change.y, change.z)) {
            continue;
        }
        if let Some(path) = save {
            remember(changes, path, world, change.x, change.y, change.z);
        }
        let block = world.block_at(change.x, change.y, change.z);
        write_frame(
            stream,
            &encode_cell(true, TAG_CELL, change.x, change.y, change.z, block),
        )?;
    }
    Ok(())
}

fn store_hotbar(path: &Path, selected: i32, slots: &[(String, i32)]) {
    let saved = match bar_from_wire(selected, slots) {
        Ok(saved) => saved,
        Err(err) => {
            eprintln!("hotbar: {err}");
            return;
        }
    };
    if let Err(err) = write_hotbar(path, &saved) {
        eprintln!("could not save hotbar: {err}");
    }
}

fn store_pose(path: &Path, x: f32, y: f32, z: f32, yaw: f32, pitch: f32) {
    let place = PlayerPlace {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    if let Err(err) = write_player(path, place) {
        eprintln!("could not save player: {err}");
    }
}

fn store_bag(path: &Path, slots: &[(String, i32)]) {
    let carried = match carried_from_wire(slots) {
        Ok(carried) => carried,
        Err(err) => {
            eprintln!("inventory: {err}");
            return;
        }
    };
    if let Err(err) = write_inventory(path, &carried) {
        eprintln!("could not save inventory: {err}");
    }
}

fn remember(changes: &mut Vec<CellChange>, path: &Path, world: &Cluster, x: i32, y: i32, z: i32) {
    let block = world.block_at(x, y, z);
    if let Some(cell) = changes
        .iter_mut()
        .find(|cell| cell.x == x && cell.y == y && cell.z == z)
    {
        cell.block = block;
    } else {
        changes.push(CellChange { x, y, z, block });
    }
    if let Err(err) = write_changes(path, changes) {
        eprintln!("could not save block changes: {err}");
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message.into())
}

fn is_disconnect(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    )
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
        region: None,
        x: 0,
        z: 0,
        port: DEFAULT_PORT,
    };
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--region" => {
                let path = iter
                    .next()
                    .ok_or_else(|| "missing value for --region".to_string())?;
                parsed.region = Some(PathBuf::from(path));
            }
            "--x" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --x".to_string())?;
                parsed.x = value
                    .parse()
                    .map_err(|_| format!("bad --x value: {value}"))?;
            }
            "--z" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --z".to_string())?;
                parsed.z = value
                    .parse()
                    .map_err(|_| format!("bad --z value: {value}"))?;
            }
            "--port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --port".to_string())?;
                parsed.port = value
                    .parse()
                    .map_err(|_| format!("bad --port value: {value}"))?;
            }
            other => return Err(format!("unknown argument: {other}\n{USAGE}")),
        }
    }
    Ok(Parsed::Run(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{
        Block, PlayerBox, TAG_BREAK, TestHook, TestHookEvent, WorldSize, decode_cell, decode_peers,
        decode_client, decode_welcome, encode_break, encode_chat, encode_hello, encode_place,
        encode_pose, encode_world_size, ClientRequest,
    };
    use std::net::TcpStream;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn synthetic_summary_counts_non_air_blocks() {
        let line = run(&Args {
            region: None,
            x: 0,
            z: 0,
            port: DEFAULT_PORT,
        })
        .unwrap();
        assert_eq!(
            line,
            "961 chunks around (0, 0): 2327 non-air blocks [synthetic]"
        );
    }

    #[test]
    fn synthetic_rejects_other_coordinates() {
        let err = run(&Args {
            region: None,
            x: 1,
            z: 0,
            port: 0,
        })
        .unwrap_err();
        assert!(err.contains("synthetic fixture"));
    }

    #[test]
    fn parse_region_coordinates_and_help() {
        let Parsed::Run(args) = parse_args([
            "--region".into(),
            "r.0.0.mca".into(),
            "--x".into(),
            "-1".into(),
            "--z".into(),
            "4".into(),
            "--port".into(),
            "20000".into(),
        ])
        .unwrap() else {
            panic!("expected run args");
        };
        assert_eq!(args.region.unwrap().as_os_str(), "r.0.0.mca");
        assert_eq!((args.x, args.z), (-1, 4));
        assert_eq!(args.port, 20000);
        let Parsed::Run(defaults) = parse_args(std::iter::empty::<String>()).unwrap() else {
            panic!("expected run args");
        };
        assert_eq!(defaults.port, DEFAULT_PORT);
        assert!(matches!(
            parse_args(["--help".to_string()]).unwrap(),
            Parsed::Help
        ));
        assert!(parse_args(["--nope".to_string()]).is_err());
        assert!(parse_args(["--x".to_string()]).is_err());
        assert!(parse_args(["--port".to_string()]).is_err());
    }

    #[test]
    fn one_client_receives_the_world_and_the_server_decides_edits() {
        let listener = TcpListener::bind(localhost(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_ne!(port, DEFAULT_PORT);
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.set_nodelay(true).unwrap();
            let state = Arc::new(Mutex::new(empty_state(
                synthetic_cluster(),
                Vec::new(),
                Hooks::new(),
            )));
            let ended = session(Arc::clone(&state), None, stream);
            (state, ended)
        });

        let mut client = TcpStream::connect(localhost(port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.set_nodelay(true).unwrap();
        write_frame(&mut client, &encode_hello()).unwrap();
        let welcome = decode_welcome(&read_frame(&mut client).unwrap()).unwrap();
        assert_eq!((welcome.center_x, welcome.center_z), (0, 0));
        assert_eq!(welcome.chunk_radius, 15);
        assert_eq!(welcome.cells.len(), 2327);
        assert!(welcome.cells.iter().any(|cell| {
            cell.x == 8 && cell.y == 1 && cell.z == 12 && cell.block.id == "oak_stairs"
        }));

        write_frame(&mut client, &encode_break(8, 1, 12)).unwrap();
        let broken = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(broken.changed);
        assert_eq!(broken.kind, TAG_BREAK);
        assert!(broken.block.is_air());

        write_frame(&mut client, &encode_break(9, 1, 13)).unwrap();
        let ore = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(ore.changed);
        assert_eq!(ore.block, Block::new("stone", 0));

        let overlapping = rebuild_world::PlayerBox {
            min: [8.0, 2.0, 12.0],
            max: [8.6, 3.8, 12.6],
        };
        write_frame(
            &mut client,
            &encode_place(8, 2, 12, Block::new("cobblestone", 0), overlapping),
        )
        .unwrap();
        let refused = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(!refused.changed);
        assert!(refused.block.is_air());

        let far = PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        };
        write_frame(
            &mut client,
            &encode_place(8, 3, 12, Block::new("cobblestone", 0), far),
        )
        .unwrap();
        let placed = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(placed.changed);
        assert_eq!(placed.block, Block::new("cobblestone", 0));

        drop(client);
        let (state, ended) = server.join().unwrap();
        assert!(ended.is_err());
        let world = &state.lock().unwrap().world;
        assert!(world.block_at(8, 1, 12).is_air());
        assert_eq!(world.block_at(9, 1, 13), Block::new("stone", 0));
        assert!(world.block_at(8, 2, 12).is_air());
        assert_eq!(world.block_at(8, 3, 12), Block::new("cobblestone", 0));
    }

    #[test]
    fn the_test_hook_fires_on_the_server_path() {
        let listener = TcpListener::bind(localhost(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut hooks = Hooks::new();
        let test_hook = TestHook::cancels("stone");
        test_hook.register(&mut hooks).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.set_nodelay(true).unwrap();
            let chunk =
                rebuild_world::Chunk::from_blocks(0, 0, [(1, 1, 1, Block::new("stone", 0))])
                    .unwrap();
            let state = Arc::new(Mutex::new(empty_state(
                rebuild_world::Cluster::try_from_chunks(0, 0, [chunk]).unwrap(),
                Vec::new(),
                hooks,
            )));
            let ended = session(Arc::clone(&state), None, stream);
            (state, ended)
        });

        let mut client = TcpStream::connect(localhost(port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.set_nodelay(true).unwrap();
        write_frame(&mut client, &encode_hello()).unwrap();
        let _welcome = read_frame(&mut client).unwrap();

        write_frame(&mut client, &encode_break(1, 1, 1)).unwrap();
        let broken = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(!broken.changed);
        assert_eq!(broken.block, Block::new("stone", 0));

        let far = PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        };
        write_frame(
            &mut client,
            &encode_place(2, 1, 1, Block::new("cobblestone", 0), far),
        )
        .unwrap();
        let placed = decode_cell(&read_frame(&mut client).unwrap()).unwrap();
        assert!(placed.changed);
        assert_eq!(placed.block, Block::new("cobblestone", 0));

        drop(client);
        let (state, ended) = server.join().unwrap();
        assert!(ended.is_err());
        let world = &state.lock().unwrap().world;
        assert_eq!(world.block_at(1, 1, 1), Block::new("stone", 0));
        assert_eq!(world.block_at(2, 1, 1), Block::new("cobblestone", 0));
        assert_eq!(
            test_hook.fired(),
            vec![
                TestHookEvent::BeforeBreak {
                    x: 1,
                    y: 1,
                    z: 1,
                    block: Block::new("stone", 0),
                },
                TestHookEvent::AfterPlace {
                    x: 2,
                    y: 1,
                    z: 1,
                    block: Block::new("cobblestone", 0),
                },
            ]
        );
    }

    #[test]
    fn the_server_reads_the_data_pack() {
        prepare_data_pack().unwrap();
        assert_eq!(
            rebuild_world::Pack::builtin().break_result(rebuild_world::Block::new("coal_ore", 0)),
            rebuild_world::Block::new("stone", 0)
        );
        assert_eq!(
            rebuild_world::Pack::builtin().hardness_of(rebuild_world::Block::new("grass", 0)),
            Some(0.25)
        );
    }

    #[test]
    fn the_mods_folder_script_cancels_stone() {
        let hooks = prepare_hooks().unwrap();
        let chunk = rebuild_world::Chunk::from_blocks(
            0,
            0,
            [
                (1, 1, 1, Block::new("stone", 0)),
                (2, 1, 1, Block::new("grass", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
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
        let far = PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        };
        assert!(apply_authoritative(
            &mut world,
            Edit::Place {
                x: 3,
                y: 1,
                z: 1,
                block: Block::new("cobblestone", 0),
                player: far,
            },
            &hooks
        ));
        assert_eq!(world.block_at(3, 1, 1), Block::new("cobblestone", 0));
        assert_eq!(world.block_at(1, 1, 1), Block::new("stone", 0));
    }

    #[test]
    fn a_second_player_is_visible_and_settings_resize_the_square() {
        let state = Arc::new(Mutex::new(empty_state(
            synthetic_cluster(),
            Vec::new(),
            Hooks::new(),
        )));
        let listener = TcpListener::bind(localhost(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    let _ = session(shared, None, stream);
                });
            }
        });

        let mut first = TcpStream::connect(localhost(port)).unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        first.set_nodelay(true).unwrap();
        write_frame(&mut first, &encode_hello()).unwrap();
        let _ = read_frame(&mut first).unwrap();

        let mut second = TcpStream::connect(localhost(port)).unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        second.set_nodelay(true).unwrap();
        write_frame(&mut second, &encode_hello()).unwrap();
        let _ = read_frame(&mut second).unwrap();

        write_frame(&mut second, &encode_pose(9.0, 5.0, 11.0, 0.4, 0.0)).unwrap();
        let seen = decode_peers(&read_frame(&mut second).unwrap()).unwrap();
        assert_eq!(seen.len(), 1);
        assert!((seen[0].x - 8.5).abs() < 0.1);

        // First player chats (echoed back); second receives it on the next pose poll.
        write_frame(&mut first, &encode_chat("hello there")).unwrap();
        let echo = decode_client(&read_frame(&mut first).unwrap()).unwrap();
        assert!(matches!(echo, ClientRequest::Chat { text } if text == "hello there"));
        write_frame(&mut second, &encode_pose(9.0, 5.0, 11.0, 0.4, 0.0)).unwrap();
        let chat_frame = read_frame(&mut second).unwrap();
        let got = decode_client(&chat_frame).unwrap();
        assert!(matches!(got, ClientRequest::Chat { text } if text == "hello there"));
        let seen_again = decode_peers(&read_frame(&mut second).unwrap()).unwrap();
        assert_eq!(seen_again.len(), 1);

        let save = std::env::temp_dir().join(format!("terralite-size-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&save);
        let sized = Arc::new(Mutex::new(empty_state(
            synthetic_cluster(),
            Vec::new(),
            Hooks::new(),
        )));
        let listener = TcpListener::bind(localhost(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::clone(&sized);
        let save_for_session = save.clone();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let _ = session(shared, Some(save_for_session), stream);
        });
        let mut client = TcpStream::connect(localhost(port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.set_nodelay(true).unwrap();
        write_frame(&mut client, &encode_hello()).unwrap();
        let welcome = decode_welcome(&read_frame(&mut client).unwrap()).unwrap();
        assert_eq!(welcome.chunk_radius, 15);
        write_frame(&mut client, &encode_world_size(WorldSize::Medium)).unwrap();
        drop(client);
        let started = std::time::Instant::now();
        loop {
            if sized.lock().unwrap().world.size() == WorldSize::Medium {
                break;
            }
            if started.elapsed() > Duration::from_secs(2) {
                panic!("the server did not grow the square");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&save);
    }
}
