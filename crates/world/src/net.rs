//! Private localhost frames shared by the server and the client.
//!
//! A frame is a big-endian `u32` length, then a body whose first byte is the
//! tag. Version 1. This is not protocol 47.
//!
//! Tags:
//! - 1 Hello, client to server: protocol version.
//! - 2 Welcome, server to client: center chunk, radius, and each non-air cell.
//! - 3 Break, client to server: the cell to break.
//! - 4 Place, client to server: the cell, the block, and the player box.
//! - 5 Cell, server to client: whether the cell changed, and the block now there.
//! - 6 Hotbar, client to server: selected slot (`1..=10`) and ten id/count pairs.
//! - 7 World, client to server: one byte, the finite size to open. A dedicated
//!   server keeps its own square. The local world rebuilds from the save.
//! - 8 Save, client to server: a named local save. A dedicated server ignores it.
//! - 9 Bag, client to server: nine inventory id/count pairs. Empty is an empty id.
//! - 10 Pose, client to server: eye x y z and yaw pitch. No reply.
//! - 11 Chat, client to server: a short line of letters. The server echoes it.
//! - 12 Peers, server to client: other players' eyes and yaw. The reply to pose.
//! - 13 Chest, client to server: a cell and nine id/count pairs. No reply.

use std::io::{self, Read, Write};
use std::net::Ipv4Addr;

use crate::chunk::Block;
use crate::cluster::Cluster;
use crate::edit::PlayerBox;
use crate::size::WorldSize;

/// Default TCP port on [`Ipv4Addr::LOCALHOST`]. Not the Minecraft port.
pub const DEFAULT_PORT: u16 = 17610;

/// Body version carried by Hello. Not protocol 47.
pub const PROTOCOL_VERSION: u8 = 1;

/// Largest accepted frame body, in bytes.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

pub const TAG_HELLO: u8 = 1;
pub const TAG_WELCOME: u8 = 2;
pub const TAG_BREAK: u8 = 3;
pub const TAG_PLACE: u8 = 4;
pub const TAG_CELL: u8 = 5;
pub const TAG_HOTBAR: u8 = 6;
pub const TAG_WORLD: u8 = 7;
/// Switch the local block save. A dedicated server ignores this.
pub const TAG_SAVE: u8 = 8;
/// Nine inventory slots past the hotbar. Version stays 1.
pub const TAG_BAG: u8 = 9;
/// Eye position and look. Version stays 1.
pub const TAG_POSE: u8 = 10;
/// A chat line. The server sends the same tag back.
pub const TAG_CHAT: u8 = 11;
/// Other joined players. The server sends this as the reply to pose.
pub const TAG_PEERS: u8 = 12;
/// One chest's nine slots. Version stays 1.
pub const TAG_CHEST: u8 = 13;

/// How many hotbar slots a [`ClientRequest::Hotbar`] carries.
pub const HOTBAR_SLOTS: usize = 10;

/// How many inventory slots a [`ClientRequest::Bag`] carries.
pub const BAG_SLOTS: usize = 9;

/// One non-air cell on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetCell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

/// What the server sends once, after Hello.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub center_x: i32,
    pub center_z: i32,
    pub chunk_radius: i32,
    pub cells: Vec<NetCell>,
}

/// The server's answer to one break or place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellUpdate {
    pub changed: bool,
    pub kind: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

/// A body the client sends.
#[derive(Clone, Debug)]
pub enum ClientRequest {
    Hello {
        version: u8,
    },
    Break {
        x: i32,
        y: i32,
        z: i32,
    },
    Place {
        x: i32,
        y: i32,
        z: i32,
        block: Block,
        player: PlayerBox,
    },
    /// `selected` is 1-based. An empty slot has an empty id and a count of 0.
    Hotbar {
        selected: i32,
        slots: Vec<(String, i32)>,
    },
    /// Replace the local square. A dedicated server ignores this.
    World {
        size: WorldSize,
    },
    /// Load a named local save. A dedicated server ignores this.
    /// `world` is `saves/blocks.json`. Any other name is `saves/<name>.json`.
    Save {
        name: String,
    },
    /// Nine inventory slots. An empty slot has an empty id and a count of 0.
    Bag {
        slots: Vec<(String, i32)>,
    },
    /// Eye position and look, in radians for yaw and pitch.
    Pose {
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    /// A line of letters and spaces, at most 32 characters.
    Chat {
        text: String,
    },
    /// Nine slots in the chest at `x, y, z`.
    Chest {
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<(String, i32)>,
    },
}

/// One other player's eye and look, from a peers frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeerPose {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
}

/// Map a welcome radius back to a finite world size.
pub fn world_size_from_radius(radius: i32) -> Option<WorldSize> {
    match radius {
        15 => Some(WorldSize::Small),
        31 => Some(WorldSize::Medium),
        63 => Some(WorldSize::Large),
        127 => Some(WorldSize::Mega),
        _ => None,
    }
}

/// Every non-air cell. Empty chunks are skipped.
pub fn occupied_cells(cluster: &Cluster) -> Vec<NetCell> {
    let mut cells = Vec::new();
    for chunk in cluster.chunks() {
        if chunk.non_air_count() == 0 {
            continue;
        }
        let origin_x = chunk.x() * 16;
        let origin_z = chunk.z() * 16;
        for y in 0..256u16 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let block = chunk.block(x, y, z);
                    if block.is_air() {
                        continue;
                    }
                    cells.push(NetCell {
                        x: origin_x + i32::from(x),
                        y: i32::from(y),
                        z: origin_z + i32::from(z),
                        block,
                    });
                }
            }
        }
    }
    cells
}

pub fn encode_hello() -> Vec<u8> {
    vec![TAG_HELLO, PROTOCOL_VERSION]
}

pub fn encode_world_size(size: WorldSize) -> Vec<u8> {
    vec![TAG_WORLD, size.code()]
}

/// A lowercase save name, 1 to 24 characters.
pub fn valid_save_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    name.len() <= 24 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

pub fn encode_save(name: &str) -> Vec<u8> {
    let bytes = if valid_save_name(name) {
        name.as_bytes()
    } else {
        b"world".as_slice()
    };
    let mut out = Vec::with_capacity(2 + bytes.len());
    out.push(TAG_SAVE);
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
    out
}

pub fn encode_welcome(
    center_x: i32,
    center_z: i32,
    chunk_radius: i32,
    cells: &[NetCell],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + cells.len() * 24);
    out.push(TAG_WELCOME);
    push_i32(&mut out, center_x);
    push_i32(&mut out, center_z);
    push_i32(&mut out, chunk_radius);
    push_u32(&mut out, cells.len() as u32);
    for cell in cells {
        push_cell(&mut out, cell);
    }
    out
}

pub fn encode_break(x: i32, y: i32, z: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(13);
    out.push(TAG_BREAK);
    push_i32(&mut out, x);
    push_i32(&mut out, y);
    push_i32(&mut out, z);
    out
}

pub fn encode_place(x: i32, y: i32, z: i32, block: Block, player: PlayerBox) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.push(TAG_PLACE);
    push_i32(&mut out, x);
    push_i32(&mut out, y);
    push_i32(&mut out, z);
    push_block(&mut out, block);
    for value in player.min.into_iter().chain(player.max) {
        push_f32(&mut out, value);
    }
    out
}

pub fn encode_hotbar(selected: i32, slots: &[(String, i32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.push(TAG_HOTBAR);
    out.push(selected.clamp(0, 255) as u8);
    out.push(HOTBAR_SLOTS as u8);
    for index in 0..HOTBAR_SLOTS {
        let (id, count) = slots.get(index).cloned().unwrap_or_default();
        let bytes = id.as_bytes();
        let len = bytes.len().min(64);
        out.push(len as u8);
        out.extend_from_slice(&bytes[..len]);
        push_i32(&mut out, count.max(0));
    }
    out
}

pub fn encode_bag(slots: &[(String, i32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(96);
    out.push(TAG_BAG);
    out.push(BAG_SLOTS as u8);
    for index in 0..BAG_SLOTS {
        let (id, count) = slots.get(index).cloned().unwrap_or_default();
        let bytes = id.as_bytes();
        let len = bytes.len().min(64);
        out.push(len as u8);
        out.extend_from_slice(&bytes[..len]);
        push_i32(&mut out, count.max(0));
    }
    out
}

pub fn encode_pose(x: f32, y: f32, z: f32, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(21);
    out.push(TAG_POSE);
    for value in [x, y, z, yaw, pitch] {
        push_f32(&mut out, value);
    }
    out
}

/// Letters, a leading slash, and single spaces, lower case, at most 32 characters.
pub fn chat_line(text: &str) -> String {
    let mut out = String::new();
    let mut spaced = false;
    for ch in text.chars() {
        if ch == '/' && out.is_empty() {
            out.push('/');
            spaced = false;
        } else if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_lowercase());
            spaced = false;
        } else if ch == ' ' && !out.is_empty() && !spaced {
            out.push(' ');
            spaced = true;
        }
        if out.len() >= 32 {
            break;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

pub fn encode_chat(text: &str) -> Vec<u8> {
    let line = chat_line(text);
    let bytes = line.as_bytes();
    let len = bytes.len().min(32);
    let mut out = Vec::with_capacity(2 + len);
    out.push(TAG_CHAT);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
    out
}

pub fn encode_peers(peers: &[PeerPose]) -> Vec<u8> {
    let count = peers.len().min(8);
    let mut out = Vec::with_capacity(2 + count * 16);
    out.push(TAG_PEERS);
    out.push(count as u8);
    for peer in peers.iter().take(count) {
        push_f32(&mut out, peer.x);
        push_f32(&mut out, peer.y);
        push_f32(&mut out, peer.z);
        push_f32(&mut out, peer.yaw);
    }
    out
}

pub fn decode_peers(body: &[u8]) -> Result<Vec<PeerPose>, String> {
    let mut reader = Reader::new(body);
    let tag = reader.u8()?;
    if tag != TAG_PEERS {
        return Err(format!("expected peers tag {TAG_PEERS}, got {tag}"));
    }
    let count = reader.u8()? as usize;
    if count > 8 {
        return Err(format!("peers has {count} players"));
    }
    let mut peers = Vec::with_capacity(count);
    for _ in 0..count {
        let pose = [reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?];
        if pose.iter().any(|value| !value.is_finite()) {
            return Err("peer is not finite".into());
        }
        peers.push(PeerPose {
            x: pose[0],
            y: pose[1],
            z: pose[2],
            yaw: pose[3],
        });
    }
    reader.finish()?;
    Ok(peers)
}

pub fn encode_chest(x: i32, y: i32, z: i32, slots: &[(String, i32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(112);
    out.push(TAG_CHEST);
    push_i32(&mut out, x);
    push_i32(&mut out, y);
    push_i32(&mut out, z);
    out.push(BAG_SLOTS as u8);
    for index in 0..BAG_SLOTS {
        let (id, count) = slots.get(index).cloned().unwrap_or_default();
        let bytes = id.as_bytes();
        let len = bytes.len().min(64);
        out.push(len as u8);
        out.extend_from_slice(&bytes[..len]);
        push_i32(&mut out, count.max(0));
    }
    out
}

pub fn encode_cell(changed: bool, kind: u8, x: i32, y: i32, z: i32, block: Block) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.push(TAG_CELL);
    out.push(u8::from(changed));
    out.push(kind);
    push_i32(&mut out, x);
    push_i32(&mut out, y);
    push_i32(&mut out, z);
    push_block(&mut out, block);
    out
}

pub fn decode_client(body: &[u8]) -> Result<ClientRequest, String> {
    let mut reader = Reader::new(body);
    let request = match reader.u8()? {
        TAG_HELLO => ClientRequest::Hello {
            version: reader.u8()?,
        },
        TAG_BREAK => ClientRequest::Break {
            x: reader.i32()?,
            y: reader.i32()?,
            z: reader.i32()?,
        },
        TAG_PLACE => ClientRequest::Place {
            x: reader.i32()?,
            y: reader.i32()?,
            z: reader.i32()?,
            block: reader.block()?,
            player: PlayerBox {
                min: [reader.f32()?, reader.f32()?, reader.f32()?],
                max: [reader.f32()?, reader.f32()?, reader.f32()?],
            },
        },
        TAG_HOTBAR => {
            let selected = i32::from(reader.u8()?);
            let count = reader.u8()? as usize;
            if count != HOTBAR_SLOTS {
                return Err(format!("hotbar has {count} slots"));
            }
            let mut slots = Vec::with_capacity(count);
            for _ in 0..count {
                let len = reader.u8()? as usize;
                if len > 64 {
                    return Err("hotbar id is too long".into());
                }
                let bytes = reader.take(len)?;
                let id = std::str::from_utf8(bytes)
                    .map_err(|_| "hotbar id is not utf-8".to_string())?
                    .to_string();
                slots.push((id, reader.i32()?.max(0)));
            }
            ClientRequest::Hotbar { selected, slots }
        }
        TAG_WORLD => {
            let size = WorldSize::from_code(reader.u8()?)
                .ok_or_else(|| "world size byte is not a finite square".to_string())?;
            ClientRequest::World { size }
        }
        TAG_SAVE => {
            let len = reader.u8()? as usize;
            if !(1..=24).contains(&len) {
                return Err("save name length".into());
            }
            let bytes = reader.take(len)?;
            let name = std::str::from_utf8(bytes)
                .map_err(|_| "save name is not utf-8".to_string())?
                .to_string();
            if !valid_save_name(&name) {
                return Err(format!("bad save name {name}"));
            }
            ClientRequest::Save { name }
        }
        TAG_BAG => {
            let count = reader.u8()? as usize;
            if count != BAG_SLOTS {
                return Err(format!("inventory has {count} slots"));
            }
            let mut slots = Vec::with_capacity(count);
            for _ in 0..count {
                let len = reader.u8()? as usize;
                if len > 64 {
                    return Err("inventory id is too long".into());
                }
                let bytes = reader.take(len)?;
                let id = std::str::from_utf8(bytes)
                    .map_err(|_| "inventory id is not utf-8".to_string())?
                    .to_string();
                slots.push((id, reader.i32()?.max(0)));
            }
            ClientRequest::Bag { slots }
        }
        TAG_POSE => {
            let pose = [
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
                reader.f32()?,
            ];
            if pose.iter().any(|value| !value.is_finite()) {
                return Err("pose is not finite".into());
            }
            ClientRequest::Pose {
                x: pose[0],
                y: pose[1],
                z: pose[2],
                yaw: pose[3],
                pitch: pose[4],
            }
        }
        TAG_CHAT => {
            let len = reader.u8()? as usize;
            if len > 32 {
                return Err("chat line is too long".into());
            }
            let bytes = reader.take(len)?;
            let text =
                std::str::from_utf8(bytes).map_err(|_| "chat line is not utf-8".to_string())?;
            let text = chat_line(text);
            ClientRequest::Chat { text }
        }
        TAG_CHEST => {
            let x = reader.i32()?;
            let y = reader.i32()?;
            let z = reader.i32()?;
            let count = reader.u8()? as usize;
            if count != BAG_SLOTS {
                return Err(format!("chest has {count} slots"));
            }
            let mut slots = Vec::with_capacity(count);
            for _ in 0..count {
                let len = reader.u8()? as usize;
                if len > 64 {
                    return Err("chest id is too long".into());
                }
                let bytes = reader.take(len)?;
                let id = std::str::from_utf8(bytes)
                    .map_err(|_| "chest id is not utf-8".to_string())?
                    .to_string();
                slots.push((id, reader.i32()?.max(0)));
            }
            ClientRequest::Chest { x, y, z, slots }
        }
        tag => return Err(format!("unknown client tag {tag}")),
    };
    reader.finish()?;
    Ok(request)
}

pub fn decode_welcome(body: &[u8]) -> Result<Welcome, String> {
    let mut reader = Reader::new(body);
    let tag = reader.u8()?;
    if tag != TAG_WELCOME {
        return Err(format!("expected welcome tag {TAG_WELCOME}, got {tag}"));
    }
    let center_x = reader.i32()?;
    let center_z = reader.i32()?;
    let chunk_radius = reader.i32()?;
    let count = reader.u32()? as usize;
    if count > reader.remaining() / 15 {
        return Err("welcome cell count does not fit in the frame".into());
    }
    let mut cells = Vec::with_capacity(count);
    for _ in 0..count {
        cells.push(reader.cell()?);
    }
    reader.finish()?;
    Ok(Welcome {
        center_x,
        center_z,
        chunk_radius,
        cells,
    })
}

pub fn decode_cell(body: &[u8]) -> Result<CellUpdate, String> {
    let mut reader = Reader::new(body);
    let tag = reader.u8()?;
    if tag != TAG_CELL {
        return Err(format!("expected cell tag {TAG_CELL}, got {tag}"));
    }
    let changed = match reader.u8()? {
        0 => false,
        1 => true,
        other => return Err(format!("cell changed flag {other} is not 0 or 1")),
    };
    let update = CellUpdate {
        changed,
        kind: reader.u8()?,
        x: reader.i32()?,
        y: reader.i32()?,
        z: reader.i32()?,
        block: reader.block()?,
    };
    reader.finish()?;
    Ok(update)
}

/// Write one frame. `body` includes the tag. The length is rejected above [`MAX_FRAME`].
pub fn write_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length is empty or above the maximum",
        ));
    }
    writer.write_all(&(body.len() as u32).to_be_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

/// Read one frame body. A length of 0 or above [`MAX_FRAME`] is an error and is not allocated.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes);
    if len == 0 || len as usize > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length is empty or above the maximum",
        ));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// The address both processes use. Port [`DEFAULT_PORT`] unless `--port` says otherwise.
pub fn localhost(port: u16) -> (Ipv4Addr, u16) {
    (Ipv4Addr::LOCALHOST, port)
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_id(out: &mut Vec<u8>, id: &str) {
    let bytes = id.as_bytes();
    assert!(
        !bytes.is_empty() && bytes.len() <= 255,
        "block id must fit in one length byte"
    );
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
}

fn push_block(out: &mut Vec<u8>, block: Block) {
    push_id(out, block.id);
    out.push(block.metadata);
    out.push(block.fill);
}

fn push_cell(out: &mut Vec<u8>, cell: &NetCell) {
    push_i32(out, cell.x);
    push_i32(out, cell.y);
    push_i32(out, cell.z);
    push_block(out, cell.block);
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.remaining() < n {
            return Err("short frame".into());
        }
        let start = self.at;
        self.at += n;
        Ok(&self.data[start..self.at])
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_be_bytes(self.u32()?.to_be_bytes()))
    }

    fn id(&mut self) -> Result<String, String> {
        let len = self.u8()? as usize;
        if len == 0 {
            return Err("empty block id".into());
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(|text| text.to_string())
            .map_err(|_| "block id is not utf-8".to_string())
    }

    fn block(&mut self) -> Result<Block, String> {
        let id = self.id()?;
        let metadata = self.u8()?;
        let fill = self.u8()?;
        Ok(Block::new(&id, metadata).with_fill(fill))
    }

    fn cell(&mut self) -> Result<NetCell, String> {
        Ok(NetCell {
            x: self.i32()?,
            y: self.i32()?,
            z: self.i32()?,
            block: self.block()?,
        })
    }

    fn finish(self) -> Result<(), String> {
        if self.at != self.data.len() {
            Err("trailing bytes".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk};
    use std::io;

    #[test]
    fn hello_welcome_break_place_and_cell_roundtrip() {
        let hello = encode_hello();
        match decode_client(&hello).unwrap() {
            ClientRequest::Hello { version } => assert_eq!(version, PROTOCOL_VERSION),
            other => panic!("hello, got {other:?}"),
        }

        let cells = [
            NetCell {
                x: 8,
                y: 1,
                z: 12,
                block: Block::new("oak_stairs", 0),
            },
            NetCell {
                x: 2,
                y: 1,
                z: 4,
                block: Block::new("wool", 14),
            },
        ];
        let welcome = decode_welcome(&encode_welcome(0, -1, 15, &cells)).unwrap();
        assert_eq!(welcome.center_x, 0);
        assert_eq!(welcome.center_z, -1);
        assert_eq!(welcome.chunk_radius, 15);
        assert_eq!(welcome.cells, cells);

        match decode_client(&encode_break(9, 1, 13)).unwrap() {
            ClientRequest::Break { x, y, z } => assert_eq!((x, y, z), (9, 1, 13)),
            other => panic!("break, got {other:?}"),
        }

        let player = PlayerBox {
            min: [1.5, -2.25, 3.5],
            max: [4.5, 5.5, 6.5],
        };
        match decode_client(&encode_place(1, 2, 3, Block::new("wool", 5), player)).unwrap() {
            ClientRequest::Place {
                x,
                y,
                z,
                block,
                player: decoded,
            } => {
                assert_eq!((x, y, z), (1, 2, 3));
                assert_eq!(block, Block::new("wool", 5));
                for (got, want) in decoded.min.into_iter().zip(player.min) {
                    assert_eq!(got.to_bits(), want.to_bits());
                }
                for (got, want) in decoded.max.into_iter().zip(player.max) {
                    assert_eq!(got.to_bits(), want.to_bits());
                }
            }
            other => panic!("place, got {other:?}"),
        }

        let cell = decode_cell(&encode_cell(
            true,
            TAG_BREAK,
            9,
            1,
            13,
            Block::new("stone", 0),
        ))
        .unwrap();
        assert!(cell.changed);
        assert_eq!(cell.kind, TAG_BREAK);
        assert_eq!((cell.x, cell.y, cell.z), (9, 1, 13));
        assert_eq!(cell.block, Block::new("stone", 0));

        let rejected = decode_cell(&encode_cell(false, TAG_PLACE, 8, 2, 12, Block::AIR)).unwrap();
        assert!(!rejected.changed);
        assert!(rejected.block.is_air());

        let slots = vec![
            (String::new(), 0),
            ("cobblestone".into(), 64),
            ("rosegold:rose".into(), 1),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
        ];
        match decode_client(&encode_hotbar(2, &slots)).unwrap() {
            ClientRequest::Hotbar {
                selected,
                slots: got,
            } => {
                assert_eq!(selected, 2);
                assert_eq!(got, slots);
            }
            other => panic!("hotbar, got {other:?}"),
        }

        let mut bag = vec![(String::new(), 0); BAG_SLOTS];
        bag[3] = ("dirt".into(), 4);
        match decode_client(&encode_bag(&bag)).unwrap() {
            ClientRequest::Bag { slots } => assert_eq!(slots, bag),
            other => panic!("bag, got {other:?}"),
        }

        match decode_client(&encode_pose(8.5, 5.0, 14.4, -1.2, 0.1)).unwrap() {
            ClientRequest::Pose {
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                assert_eq!(x.to_bits(), 8.5f32.to_bits());
                assert_eq!(y.to_bits(), 5.0f32.to_bits());
                assert_eq!(z.to_bits(), 14.4f32.to_bits());
                assert_eq!(yaw.to_bits(), (-1.2f32).to_bits());
                assert_eq!(pitch.to_bits(), 0.1f32.to_bits());
            }
            other => panic!("pose, got {other:?}"),
        }
        let peers = decode_peers(&encode_peers(&[PeerPose {
            x: 8.5,
            y: 5.0,
            z: 14.0,
            yaw: 0.5,
        }]))
        .unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].x.to_bits(), 8.5f32.to_bits());
        match decode_client(&encode_chest(1, 2, 3, &[("dirt".into(), 4)])).unwrap() {
            ClientRequest::Chest { x, y, z, slots } => {
                assert_eq!((x, y, z), (1, 2, 3));
                assert_eq!(slots[0], ("dirt".into(), 4));
                assert_eq!(slots.len(), BAG_SLOTS);
            }
            other => panic!("chest, got {other:?}"),
        }
        assert_eq!(chat_line("Hello 2 there"), "hello there");
        assert_eq!(chat_line("/Spark Hello"), "/spark hello");
        match decode_client(&encode_chat("Hello")).unwrap() {
            ClientRequest::Chat { text } => assert_eq!(text, "hello"),
            other => panic!("chat, got {other:?}"),
        }
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn frames_roundtrip_and_reject_a_huge_or_empty_length() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &encode_hello()).unwrap();
        let body = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(body, encode_hello());

        let huge = u32::MAX.to_be_bytes();
        assert_eq!(
            read_frame(&mut huge.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(write_frame(&mut Vec::new(), &[]).is_err());

        let mut short = 10u32.to_be_bytes().to_vec();
        short.extend_from_slice(&[1, 2, 3]);
        assert!(read_frame(&mut short.as_slice()).is_err());
    }

    #[test]
    fn rejects_empty_ids_bad_utf8_and_trailing_bytes() {
        let mut body = vec![TAG_CELL, 1, TAG_BREAK];
        body.extend_from_slice(&1i32.to_be_bytes());
        body.extend_from_slice(&2i32.to_be_bytes());
        body.extend_from_slice(&3i32.to_be_bytes());
        body.push(0);
        assert!(decode_cell(&body).unwrap_err().contains("empty"));

        body.pop();
        body.push(1);
        body.push(0xFF);
        body.push(0);
        assert!(decode_cell(&body).unwrap_err().contains("utf-8"));

        let mut hello = encode_hello();
        hello.push(9);
        assert!(decode_client(&hello).unwrap_err().contains("trailing"));
        assert!(decode_client(&[TAG_HELLO]).unwrap_err().contains("short"));
    }

    #[test]
    fn radii_map_to_the_four_sizes_and_occupied_cells_skip_air() {
        assert_eq!(world_size_from_radius(15), Some(WorldSize::Small));
        assert_eq!(world_size_from_radius(31), Some(WorldSize::Medium));
        assert_eq!(world_size_from_radius(63), Some(WorldSize::Large));
        assert_eq!(world_size_from_radius(127), Some(WorldSize::Mega));
        assert_eq!(world_size_from_radius(16), None);
        let body = encode_world_size(WorldSize::Medium);
        match decode_client(&body).unwrap() {
            ClientRequest::World { size } => assert_eq!(size, WorldSize::Medium),
            other => panic!("world, got {other:?}"),
        }
        assert!(
            decode_client(&[TAG_WORLD, 9])
                .unwrap_err()
                .contains("finite")
        );
        let saved = encode_save("grove");
        match decode_client(&saved).unwrap() {
            ClientRequest::Save { name } => assert_eq!(name, "grove"),
            other => panic!("save, got {other:?}"),
        }
        assert_eq!(encode_save("Nope"), encode_save("world"));
        assert!(
            decode_client(&[TAG_SAVE, 0])
                .unwrap_err()
                .contains("length")
        );
        assert!(!valid_save_name(""));
        assert!(!valid_save_name("Grove"));

        let chunk = Chunk::from_blocks(
            0,
            0,
            [(1, 2, 3, Block::new("stone", 4)), (1, 2, 4, Block::AIR)],
        )
        .unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cells = occupied_cells(&cluster);
        assert_eq!(
            cells,
            vec![NetCell {
                x: 1,
                y: 2,
                z: 3,
                block: Block::new("stone", 4),
            }]
        );
    }
}
