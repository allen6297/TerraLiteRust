//! One background thread owns the socket, or the local world when no server is up.
//!
//! The window thread sends a break or a place, then polls the cell reply.
//! Connecting happens before the window opens. After that, a read never runs
//! on the thread that draws. A refused connection loads the same synthetic
//! world and save the server would, and answers those edits locally.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use rebuild_world::{
    CellChange, CellUpdate, Chunk, ClientRequest, Cluster, Edit, Hooks, PeerPose, PlayerPlace,
    TAG_BREAK, TAG_CELL, TAG_CHAT, TAG_PLACE, TAG_POSE, apply_authoritative, apply_changes,
    bar_from_wire, block_definitions_file, carried_from_wire, decode_cell, decode_client,
    decode_peers, decode_welcome, encode_hello, ensure_chest, load_data_pack, load_dir, localhost,
    read_changes, read_frame, remove_chest, store_chest_slots, synthetic_cluster_sized,
    world_size_from_radius, write_changes, write_frame, write_hotbar, write_inventory,
    write_player,
};

const SAVE_PATH: &str = r"C:\CODE\TerraLite\saves\blocks.json";
const SAVES_DIR: &str = r"C:\CODE\TerraLite\saves";
const MODS_DIR: &str = r"C:\CODE\TerraLite\mods";
/// The name of [`SAVE_PATH`].
pub const DEFAULT_WORLD: &str = "world";

const FRESH_NAMES: [&str; 8] = [
    "grove", "meadow", "ridge", "brook", "heath", "marsh", "field", "cove",
];

/// `world` stays `saves/blocks.json`. Every other name is `saves/<name>.json`.
pub fn world_file(name: &str) -> PathBuf {
    let root = PathBuf::from(SAVES_DIR);
    if name == DEFAULT_WORLD {
        root.join("blocks.json")
    } else {
        root.join(format!("{name}.json"))
    }
}

pub fn list_worlds() -> Vec<String> {
    let mut extra = Vec::new();
    if let Ok(read) = std::fs::read_dir(SAVES_DIR) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if stem == "blocks" || !rebuild_world::valid_save_name(stem) {
                continue;
            }
            extra.push(stem.to_string());
        }
    }
    extra.sort();
    extra.dedup();
    let mut names = vec![DEFAULT_WORLD.to_string()];
    names.extend(extra);
    names
}

pub fn fresh_world_name(existing: &[String]) -> String {
    for name in FRESH_NAMES {
        if !existing.iter().any(|have| have == name) {
            return name.to_string();
        }
    }
    let mut index = 2u32;
    loop {
        let name = format!("save{index}");
        if !existing.iter().any(|have| have == &name) {
            return name;
        }
        index += 1;
    }
}

pub fn ensure_world(name: &str) -> Result<(), String> {
    let path = world_file(name);
    if path.is_file() {
        return Ok(());
    }
    write_changes(&path, &[])
}

/// Owned `rebuild-server` child started by Host. Killed when dropped.
pub struct HostProcess {
    child: Child,
    pub port: u16,
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// True when something already answers hello-capable TCP on localhost:`port`.
pub fn port_listening(port: u16) -> bool {
    let address = SocketAddr::from(localhost(port));
    TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
}

fn server_binary() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| format!("current exe: {err}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current exe has no parent folder".to_string())?;
    let names = if cfg!(windows) {
        ["rebuild-server.exe", "rebuild-server-soft.exe"]
    } else {
        ["rebuild-server", "rebuild-server-soft"]
    };
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "missing rebuild-server next to {}",
        exe.display()
    ))
}

/// Spawn a private localhost listen (`rebuild-server`) on `port`.
///
/// Returns once the port accepts connections, or errors if bind/start fails.
pub fn start_host(port: u16) -> Result<HostProcess, String> {
    if port_listening(port) {
        return Err(format!("already listening on 127.0.0.1:{port}"));
    }
    let server = server_binary()?;
    let child = Command::new(&server)
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("could not start {}: {err}", server.display()))?;
    let mut host = HostProcess { child, port };
    for _ in 0..80 {
        if port_listening(port) {
            return Ok(host);
        }
        if let Ok(Some(status)) = host.child.try_wait() {
            return Err(format!(
                "host exited before listen ({status})"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "host did not open 127.0.0.1:{port} in time"
    ))
}

pub struct Link {
    outgoing: Sender<Vec<u8>>,
    incoming: Receiver<CellUpdate>,
    chat: Receiver<String>,
    peers: Receiver<Vec<PeerPose>>,
    disconnected: bool,
}

impl Link {
    pub fn send(&self, body: Vec<u8>) -> Result<(), ()> {
        self.outgoing.send(body).map_err(|_| ())
    }

    pub fn try_recv(&mut self) -> Result<Option<CellUpdate>, ()> {
        if self.disconnected {
            return Err(());
        }
        match self.incoming.try_recv() {
            Ok(cell) => Ok(Some(cell)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.disconnected = true;
                Err(())
            }
        }
    }

    /// A chat line the server echoed. Empty when none is waiting.
    pub fn try_chat(&mut self) -> Option<String> {
        match self.chat.try_recv() {
            Ok(line) => Some(line),
            Err(_) => None,
        }
    }

    /// Other players from the last pose reply. Empty when none is waiting.
    pub fn try_peers(&mut self) -> Option<Vec<PeerPose>> {
        match self.peers.try_recv() {
            Ok(peers) => Some(peers),
            Err(_) => None,
        }
    }
}

/// Connect, read the welcome, and build the cluster the server described.
pub fn connect(port: u16) -> Result<(Cluster, Link), String> {
    let address = SocketAddr::from(localhost(port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|err| format!("could not connect to 127.0.0.1:{port} ({err})"))?;
    stream
        .set_nodelay(true)
        .map_err(|err| format!("socket: {err}"))?;
    write_frame(&mut stream, &encode_hello()).map_err(|err| format!("sending hello: {err}"))?;
    let welcome =
        decode_welcome(&read_frame(&mut stream).map_err(|err| format!("reading welcome: {err}"))?)?;
    let size = world_size_from_radius(welcome.chunk_radius)
        .ok_or_else(|| format!("server sent chunk radius {}", welcome.chunk_radius))?;
    let mut cluster = Cluster::try_from_chunks_sized(
        size,
        welcome.center_x,
        welcome.center_z,
        std::iter::empty::<Chunk>(),
    )
    .map_err(|err| err.to_string())?;
    let changes: Vec<CellChange> = welcome
        .cells
        .iter()
        .map(|cell| CellChange {
            x: cell.x,
            y: cell.y,
            z: cell.z,
            block: cell.block,
        })
        .collect();
    apply_changes(&mut cluster, &changes);

    let (outgoing, jobs) = mpsc::channel::<Vec<u8>>();
    let (replies, incoming) = mpsc::channel::<CellUpdate>();
    let (chat_tx, chat) = mpsc::channel::<String>();
    let (peer_tx, peers) = mpsc::channel::<Vec<PeerPose>>();
    thread::spawn(move || pump(stream, jobs, replies, chat_tx, peer_tx));
    Ok((
        cluster,
        Link {
            outgoing,
            incoming,
            chat,
            peers,
            disconnected: false,
        },
    ))
}

/// Use the server when it answers. Otherwise open the local synthetic world.
pub fn connect_or_local(port: u16) -> Result<(Cluster, Link, &'static str), String> {
    match connect(port) {
        Ok((cluster, link)) => Ok((cluster, link, "server")),
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("opening the local world instead.");
            let (cluster, link) = open_local(Path::new(SAVE_PATH))?;
            Ok((cluster, link, "local"))
        }
    }
}

/// Load the synthetic world, the block save, and the mods, then answer edits here.
pub fn open_local(save: &Path) -> Result<(Cluster, Link), String> {
    load_data_pack()?;
    println!("block definitions: {}", block_definitions_file().display());
    let hooks = load_mods()?;
    let (display, changes) = load_play_world(save)?;
    if !changes.is_empty() {
        println!(
            "loaded {} saved blocks from {}",
            changes.len(),
            save.display()
        );
    }
    let (authority, authority_changes) = load_play_world(save)?;
    let (outgoing, jobs) = mpsc::channel::<Vec<u8>>();
    let (replies, incoming) = mpsc::channel::<CellUpdate>();
    let (_chat_tx, chat) = mpsc::channel::<String>();
    let (_peer_tx, peers) = mpsc::channel::<Vec<PeerPose>>();
    let save_path = save.to_path_buf();
    thread::spawn(move || {
        local_pump(
            authority,
            authority_changes,
            save_path,
            hooks,
            jobs,
            replies,
        )
    });
    Ok((
        display,
        Link {
            outgoing,
            incoming,
            chat,
            peers,
            disconnected: false,
        },
    ))
}

fn load_play_world(save: &Path) -> Result<(Cluster, Vec<CellChange>), String> {
    load_play_world_sized(save, rebuild_world::WorldSize::Small)
}

/// The demo, the seed ring, and one named save.
pub fn load_named(size: rebuild_world::WorldSize, name: &str) -> Result<Cluster, String> {
    load_play_world_sized(&world_file(name), size).map(|(cluster, _)| cluster)
}

/// The demo, the seed ring, and the save, inside a square of `size`.
pub fn load_play_world_sized(
    save: &Path,
    size: rebuild_world::WorldSize,
) -> Result<(Cluster, Vec<CellChange>), String> {
    let mut cluster = synthetic_cluster_sized(size);
    rebuild_world::seed_terrain_ring(&mut cluster);
    let changes = read_changes(save)?;
    apply_changes(&mut cluster, &changes);
    Ok((cluster, changes))
}

fn load_mods() -> Result<Hooks, String> {
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

fn local_pump(
    mut world: Cluster,
    mut changes: Vec<CellChange>,
    mut save: PathBuf,
    hooks: Hooks,
    jobs: Receiver<Vec<u8>>,
    replies: Sender<CellUpdate>,
) {
    let mut spawned = false;
    let mut entities = rebuild_world::EntityClock::new();
    while let Ok(body) = jobs.recv() {
        entities.catch_up(&hooks, &mut world);
        let Ok(request) = decode_client(&body) else {
            break;
        };
        if let ClientRequest::Hotbar { selected, slots } = &request {
            hooks.set_inventory(*selected, slots.clone());
            store_hotbar(&save, *selected, slots);
            if !spawned {
                let (x, y, z) = rebuild_world::PLAYER_SPAWN;
                hooks.entity_spawn(&mut world, "player", x, y, z);
                hooks.spawn_demo_mobs(&mut world);
                spawned = true;
                if !send_local_writes(&mut world, &mut changes, &save, &hooks, None, &replies) {
                    break;
                }
            }
            continue;
        }
        let (changed, kind, x, y, z) = match request {
            ClientRequest::Break { x, y, z } => {
                rebuild_world::ensure_terrain(&mut world, x.div_euclid(16), z.div_euclid(16));
                let changed = apply_authoritative(&mut world, Edit::Break { x, y, z }, &hooks);
                (changed, TAG_BREAK, x, y, z)
            }
            ClientRequest::Place {
                x,
                y,
                z,
                block,
                player,
            } => {
                rebuild_world::ensure_terrain(&mut world, x.div_euclid(16), z.div_euclid(16));
                let changed = apply_authoritative(
                    &mut world,
                    Edit::Place {
                        x,
                        y,
                        z,
                        block,
                        player,
                    },
                    &hooks,
                );
                (changed, TAG_PLACE, x, y, z)
            }
            ClientRequest::Hello { .. } | ClientRequest::Hotbar { .. } => continue,
            ClientRequest::Bag { slots } => {
                store_bag(&save, &slots);
                continue;
            }
            ClientRequest::Pose {
                x,
                y,
                z,
                yaw,
                pitch,
                name: _,
                held: _,
            } => {
                store_pose(&save, x, y, z, yaw, pitch);
                continue;
            }
            ClientRequest::Chat { .. } => continue,
            ClientRequest::Chest { x, y, z, slots } => {
                let block = world.block_at(x, y, z);
                if rebuild_world::Pack::builtin().is_container(block.id) {
                    if let Ok(carried) = carried_from_wire(&slots) {
                        if let Err(err) = store_chest_slots(&save, x, y, z, block.id, &carried) {
                            eprintln!("could not save chest: {err}");
                        }
                    }
                }
                continue;
            }
            ClientRequest::Save { name } => {
                let path = world_file(&name);
                if let Ok(loaded) = read_changes(&path) {
                    let mut next = synthetic_cluster_sized(world.size());
                    rebuild_world::seed_terrain_ring(&mut next);
                    apply_changes(&mut next, &loaded);
                    world = next;
                    changes = loaded;
                    save = path;
                }
                continue;
            }
            ClientRequest::World { size } => {
                if let Ok(loaded) = read_changes(&save) {
                    let mut next = synthetic_cluster_sized(size);
                    rebuild_world::seed_terrain_ring(&mut next);
                    apply_changes(&mut next, &loaded);
                    world = next;
                    changes = loaded;
                }
                continue;
            }
        };
        if changed {
            remember(&mut changes, &save, &world, x, y, z);
            let block = world.block_at(x, y, z);
            sync_container(&save, x, y, z, block);
        }
        let block = world.block_at(x, y, z);
        if replies
            .send(CellUpdate {
                changed,
                kind,
                x,
                y,
                z,
                block,
            })
            .is_err()
        {
            break;
        }
        if !send_local_writes(
            &mut world,
            &mut changes,
            &save,
            &hooks,
            Some((x, y, z)),
            &replies,
        ) {
            break;
        }
    }
}

fn send_local_writes(
    world: &mut Cluster,
    changes: &mut Vec<CellChange>,
    save: &Path,
    hooks: &Hooks,
    skip: Option<(i32, i32, i32)>,
    replies: &Sender<CellUpdate>,
) -> bool {
    for change in hooks.drain_writes() {
        if skip == Some((change.x, change.y, change.z)) {
            continue;
        }
        remember(changes, save, world, change.x, change.y, change.z);
        let block = world.block_at(change.x, change.y, change.z);
        if replies
            .send(CellUpdate {
                changed: true,
                kind: TAG_CELL,
                x: change.x,
                y: change.y,
                z: change.z,
                block,
            })
            .is_err()
        {
            return false;
        }
    }
    true
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
    if let Err(err) = write_player(
        path,
        PlayerPlace {
            x,
            y,
            z,
            yaw,
            pitch,
        },
    ) {
        eprintln!("could not save player: {err}");
    }
}

fn sync_container(path: &Path, x: i32, y: i32, z: i32, block: rebuild_world::Block) {
    let pack = rebuild_world::Pack::builtin();
    if pack.is_container(block.id) {
        if let Err(err) = ensure_chest(path, x, y, z, block.id) {
            eprintln!("could not save chest: {err}");
        }
    } else {
        if !pack.is_script_block_entity(block.id) {
            pack.clear_tile_at(x, y, z);
        }
        if let Err(err) = remove_chest(path, x, y, z) {
            eprintln!("could not save chest: {err}");
        }
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

fn pump(
    mut stream: TcpStream,
    jobs: Receiver<Vec<u8>>,
    replies: Sender<CellUpdate>,
    chat: Sender<String>,
    peers: Sender<Vec<PeerPose>>,
) {
    while let Ok(body) = jobs.recv() {
        let tag = body.first().copied();
        if write_frame(&mut stream, &body).is_err() {
            break;
        }
        match tag {
            Some(TAG_POSE) => {
                // Server may write pending peer chat frames, then the pose list.
                let mut got_peers = false;
                for _ in 0..8 {
                    let Ok(frame) = read_frame(&mut stream) else {
                        break;
                    };
                    if let Ok(list) = decode_peers(&frame) {
                        if peers.send(list).is_err() {
                            return;
                        }
                        got_peers = true;
                        break;
                    }
                    if let Ok(ClientRequest::Chat { text }) = decode_client(&frame) {
                        if chat.send(text).is_err() {
                            return;
                        }
                        continue;
                    }
                    break;
                }
                if !got_peers {
                    break;
                }
            }
            Some(TAG_BREAK) | Some(TAG_PLACE) => {
                let Ok(frame) = read_frame(&mut stream) else {
                    break;
                };
                let Ok(cell) = decode_cell(&frame) else {
                    break;
                };
                if replies.send(cell).is_err() {
                    break;
                }
            }
            Some(TAG_CHAT) => {
                let Ok(frame) = read_frame(&mut stream) else {
                    break;
                };
                // Sender already painted the line; drain the server echo.
                if decode_client(&frame).is_err() {
                    break;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use rebuild_world::{CellChange, apply_changes, encode_break};

    use super::*;

    #[test]
    fn a_closed_port_opens_the_local_world() {
        let (cluster, _link, source) = connect_or_local(49151).expect("local world");
        assert_eq!(source, "local");
        assert_eq!(cluster.size().chunk_radius(), 15);
    }

    #[test]
    fn a_local_break_changes_grass_without_the_server_save() {
        let path = std::env::temp_dir().join("terralite-offline-break.json");
        let _ = std::fs::remove_file(&path);
        let (mut cluster, mut link) = open_local(&path).expect("local world");
        assert_eq!(cluster.block_at(1, 0, 1).id, "grass");
        link.send(encode_break(1, 0, 1)).expect("send");
        let started = Instant::now();
        let cell = loop {
            if let Some(cell) = link.try_recv().expect("link") {
                break cell;
            }
            if started.elapsed() > Duration::from_secs(2) {
                panic!("the local world did not answer the break");
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(cell.changed);
        assert!(cell.block.is_air());
        apply_changes(
            &mut cluster,
            &[CellChange {
                x: cell.x,
                y: cell.y,
                z: cell.z,
                block: cell.block,
            }],
        );
        assert!(cluster.block_at(1, 0, 1).is_air());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn port_listening_tracks_a_bound_localhost_socket() {
        use std::net::TcpListener;
        let listener = TcpListener::bind(localhost(0)).expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        assert!(port_listening(port), "open listener should answer");
        drop(listener);
        thread::sleep(Duration::from_millis(20));
        assert!(
            !port_listening(port),
            "closed port should not answer"
        );
    }

    #[test]
    fn a_named_world_is_listed_without_replacing_blocks_json() {
        assert_eq!(
            world_file(DEFAULT_WORLD),
            std::path::PathBuf::from(SAVE_PATH)
        );
        assert_eq!(fresh_world_name(&["grove".into()]), "meadow");
        let name = "zztest";
        let path = world_file(name);
        let existed = path.is_file();
        if !existed {
            ensure_world(name).expect("create");
            assert!(list_worlds().iter().any(|saved| saved == name));
            let loaded = load_named(rebuild_world::WorldSize::Small, name).expect("load");
            assert_eq!(loaded.block_at(1, 0, 1).id, "grass");
            let _ = std::fs::remove_file(&path);
        }
        assert!(list_worlds().iter().any(|saved| saved == DEFAULT_WORLD));
    }
}
