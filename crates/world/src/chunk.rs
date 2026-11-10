//! One 16×256×16 chunk in the 1.8 section layout.
//!
//! Each present section stores 4096 string block ids and 4096 metadata
//! nibbles. On disk, Anvil still writes a block byte plus an optional Add
//! nibble. Index order inside a section is `local_y << 8 | z << 4 | x`. The
//! low nibble of a packed byte is the even index.

use crate::error::Error;
use crate::ids::{anvil_id, anvil_name, intern};
use crate::nbt::Tag;

/// No compound fill in the cell.
pub const FILL_NONE: u8 = 0;
/// Water occupying a compound host (waterlogging-like).
pub const FILL_WATER: u8 = 1;
/// Ember energy occupying a compound host.
pub const FILL_EMBER: u8 = 2;

/// A block: a string id, 4 bits of metadata, and an optional compound fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub id: &'static str,
    pub metadata: u8,
    /// Secondary occupancy: [`FILL_NONE`], [`FILL_WATER`], or [`FILL_EMBER`].
    pub fill: u8,
}

impl Block {
    pub const AIR: Self = Self {
        id: "air",
        metadata: 0,
        fill: FILL_NONE,
    };

    pub fn new(id: &str, metadata: u8) -> Self {
        Self {
            id: intern(id),
            metadata: metadata & 0x0F,
            fill: FILL_NONE,
        }
    }

    /// Same host with a compound fill. Fill is masked to `0..=3`.
    pub fn with_fill(self, fill: u8) -> Self {
        Self {
            fill: fill & 0x03,
            ..self
        }
    }

    pub fn is_air(self) -> bool {
        self.id == "air"
    }

    /// True when this cell carries a compound fluid or energy fill.
    pub fn has_fill(self) -> bool {
        self.fill != FILL_NONE
    }
}

/// Hosts that can hold water or ember without replacing the solid.
pub fn compound_host(id: &str) -> bool {
    matches!(
        id,
        "copper_grate"
            | "fence"
            | "slab"
            | "oak_stairs"
            | "stone_stairs"
            | "chain"
            | "glass"
    )
}

/// Pack fill byte for a named compound kind.
pub fn fill_kind(kind: &str) -> Option<u8> {
    match kind {
        "water" => Some(FILL_WATER),
        "ember" => Some(FILL_EMBER),
        _ => None,
    }
}

/// Named kind for a fill byte, when it is a known compound.
pub fn fill_name(fill: u8) -> Option<&'static str> {
    match fill {
        FILL_WATER => Some("water"),
        FILL_EMBER => Some("ember"),
        _ => None,
    }
}

#[derive(Clone)]
struct Section {
    y: u8,
    blocks: [&'static str; 4096],
    meta: [u8; 4096],
    fills: [u8; 4096],
}

impl Section {
    fn empty(y: u8) -> Self {
        Self {
            y,
            blocks: ["air"; 4096],
            meta: [0; 4096],
            fills: [FILL_NONE; 4096],
        }
    }
}

/// A single chunk. Sections that were missing on disk stay empty (all air).
///
/// Section storage is boxed. A section is 12KB of block ids and metadata, and
/// sixteen of those on the stack overflow test threads. `Clone` copies those
/// boxes on the heap so a worker can mesh a snapshot while the world changes.
#[derive(Clone)]
pub struct Chunk {
    x: i32,
    z: i32,
    sections: [Option<Box<Section>>; 16],
}

impl Chunk {
    pub fn x(&self) -> i32 {
        self.x
    }

    pub fn z(&self) -> i32 {
        self.z
    }

    pub fn section_count(&self) -> usize {
        self.sections
            .iter()
            .filter(|section| section.is_some())
            .count()
    }

    pub fn non_air_count(&self) -> u32 {
        self.sections
            .iter()
            .flatten()
            .flat_map(|section| section.blocks.iter())
            .filter(|id| **id != "air")
            .count() as u32
    }

    /// True when any stored block matches. Missing sections are skipped.
    pub fn any_block(&self, mut pred: impl FnMut(Block) -> bool) -> bool {
        for section in self.sections.iter().flatten() {
            for index in 0..section.blocks.len() {
                let id = section.blocks[index];
                if id == "air" {
                    continue;
                }
                if pred(Block {
                    id,
                    metadata: section.meta[index],
                    fill: section.fills[index],
                }) {
                    return true;
                }
            }
        }
        false
    }

    /// World coordinates of every stored block. Missing sections are skipped.
    pub fn for_each_block(&self, mut visit: impl FnMut(i32, i32, i32, Block)) {
        let origin_x = self.x * 16;
        let origin_z = self.z * 16;
        for (section_index, section) in self.sections.iter().enumerate() {
            let Some(section) = section else {
                continue;
            };
            let y_base = (section_index as i32) * 16;
            for index in 0..section.blocks.len() {
                let id = section.blocks[index];
                if id == "air" {
                    continue;
                }
                let x = origin_x + (index as i32 & 15);
                let z = origin_z + ((index as i32 >> 4) & 15);
                let y = y_base + (index as i32 >> 8);
                visit(
                    x,
                    y,
                    z,
                    Block {
                        id,
                        metadata: section.meta[index],
                        fill: section.fills[index],
                    },
                );
            }
        }
    }

    /// Chunk-local coordinates. Out-of-range reads are air.
    pub fn block(&self, x: u8, y: u16, z: u8) -> Block {
        if x > 15 || z > 15 || y > 255 {
            return Block::AIR;
        }
        let Some(section) = &self.sections[(y >> 4) as usize] else {
            return Block::AIR;
        };
        let index = block_index(x, (y & 15) as u8, z);
        Block {
            id: section.blocks[index],
            metadata: section.meta[index],
            fill: section.fills[index],
        }
    }

    /// Place blocks by chunk-local `(x, y, z)`. Later entries overwrite earlier ones.
    pub fn from_blocks(
        chunk_x: i32,
        chunk_z: i32,
        blocks: impl IntoIterator<Item = (u8, u16, u8, Block)>,
    ) -> Result<Self, Error> {
        let mut chunk = Self::empty(chunk_x, chunk_z);
        for (x, y, z, block) in blocks {
            if x > 15 || z > 15 || y > 255 {
                return Err(Error::BadBlockCoord);
            }
            let section_y = (y >> 4) as usize;
            let section = chunk.sections[section_y]
                .get_or_insert_with(|| Box::new(Section::empty(section_y as u8)));
            let index = block_index(x, (y & 15) as u8, z);
            section.blocks[index] = block.id;
            section.meta[index] = block.metadata;
            section.fills[index] = block.fill;
        }
        Ok(chunk)
    }

    pub(crate) fn set_block(&mut self, x: u8, y: u16, z: u8, block: Block) {
        let section_y = (y >> 4) as usize;
        if block.is_air() && self.sections[section_y].is_none() {
            return;
        }
        let section = self.sections[section_y]
            .get_or_insert_with(|| Box::new(Section::empty(section_y as u8)));
        let index = block_index(x, (y & 15) as u8, z);
        section.blocks[index] = block.id;
        section.meta[index] = block.metadata;
        section.fills[index] = if block.is_air() {
            FILL_NONE
        } else {
            block.fill
        };
    }

    fn empty(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            sections: std::array::from_fn(|_| None),
        }
    }
}

impl std::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunk")
            .field("x", &self.x)
            .field("z", &self.z)
            .field("sections", &self.section_count())
            .field("non_air", &self.non_air_count())
            .finish()
    }
}

pub(crate) fn block_index(x: u8, local_y: u8, z: u8) -> usize {
    ((local_y as usize) << 8) | ((z as usize) << 4) | (x as usize)
}

fn nibble(data: &[u8], index: usize) -> u8 {
    let byte = data[index >> 1];
    if index & 1 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    }
}

fn set_nibble(data: &mut [u8], index: usize, value: u8) {
    let slot = index >> 1;
    let shift = (index & 1) * 4;
    data[slot] = (data[slot] & !(0x0F << shift)) | ((value & 0x0F) << shift);
}

pub(crate) fn chunk_from_root(root: &Tag, expected: (i32, i32)) -> Result<Chunk, Error> {
    let level = root
        .get("Level")
        .ok_or(Error::BadSection("missing Level"))?;
    let x = level
        .get("xPos")
        .and_then(Tag::as_int)
        .ok_or(Error::BadSection("missing xPos"))?;
    let z = level
        .get("zPos")
        .and_then(Tag::as_int)
        .ok_or(Error::BadSection("missing zPos"))?;
    if (x, z) != expected {
        return Err(Error::ChunkCoordMismatch {
            expected_x: expected.0,
            expected_z: expected.1,
            found_x: x,
            found_z: z,
        });
    }

    let mut chunk = Chunk::empty(x, z);
    let sections = level
        .get("Sections")
        .and_then(Tag::as_list)
        .ok_or(Error::BadSection("missing Sections"))?;
    for section_tag in sections {
        let y = section_tag
            .get("Y")
            .and_then(Tag::as_i8)
            .ok_or(Error::BadSection("section Y"))?;
        if !(0..16).contains(&y) {
            return Err(Error::BadSection("section Y out of range"));
        }
        let y = y as u8;
        if chunk.sections[y as usize].is_some() {
            return Err(Error::BadSection("duplicate section"));
        }
        let blocks = section_tag
            .get("Blocks")
            .and_then(Tag::as_bytes)
            .ok_or(Error::BadSection("section Blocks"))?;
        let data = section_tag
            .get("Data")
            .and_then(Tag::as_bytes)
            .ok_or(Error::BadSection("section Data"))?;
        if blocks.len() != 4096 || data.len() != 2048 {
            return Err(Error::BadSection("section array length"));
        }
        let add = match section_tag.get("Add") {
            None => None,
            Some(tag) => {
                let bytes = tag.as_bytes().ok_or(Error::BadSection("section Add"))?;
                if bytes.len() != 2048 {
                    return Err(Error::BadSection("section Add length"));
                }
                Some(bytes)
            }
        };
        chunk.sections[y as usize] = Some(Box::new(decode_section(y, blocks, data, add)));
    }
    Ok(chunk)
}

fn decode_section(y: u8, blocks: &[u8], data: &[u8], add: Option<&[u8]>) -> Section {
    let mut section = Section::empty(y);
    for index in 0..4096 {
        let mut id = blocks[index] as u16;
        if let Some(add) = add {
            id |= u16::from(nibble(add, index)) << 8;
        }
        section.blocks[index] = anvil_name(id);
        section.meta[index] = nibble(data, index);
    }
    section
}

pub(crate) fn chunk_to_nbt(chunk: &Chunk) -> Tag {
    let sections = chunk
        .sections
        .iter()
        .flatten()
        .map(|section| section_to_tag(section))
        .collect();
    Tag::Compound(vec![(
        "Level".into(),
        Tag::Compound(vec![
            ("xPos".into(), Tag::Int(chunk.x)),
            ("zPos".into(), Tag::Int(chunk.z)),
            ("Sections".into(), Tag::List(sections)),
        ]),
    )])
}

fn section_to_tag(section: &Section) -> Tag {
    let mut blocks = vec![0u8; 4096];
    let mut data = vec![0u8; 2048];
    let mut add = vec![0u8; 2048];
    let mut needs_add = false;
    for index in 0..4096 {
        let id = anvil_id(section.blocks[index]);
        blocks[index] = (id & 0xFF) as u8;
        let high = ((id >> 8) & 0x0F) as u8;
        if high != 0 {
            needs_add = true;
        }
        set_nibble(&mut add, index, high);
        set_nibble(&mut data, index, section.meta[index]);
    }
    let mut entries = vec![
        ("Y".into(), Tag::Byte(section.y as i8)),
        ("Blocks".into(), Tag::ByteArray(blocks)),
        ("Data".into(), Tag::ByteArray(data)),
    ];
    if needs_add {
        entries.push(("Add".into(), Tag::ByteArray(add)));
    }
    Tag::Compound(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_nibbles_pack_low_then_high() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0xA)),
                (1, 0, 0, Block::new("stone", 0xB)),
            ],
        )
        .unwrap();
        let root = chunk_to_nbt(&chunk);
        let data = root
            .get("Level")
            .unwrap()
            .get("Sections")
            .unwrap()
            .as_list()
            .unwrap()[0]
            .get("Data")
            .unwrap()
            .as_bytes()
            .unwrap();
        // Index 0 is the low nibble, index 1 is the high nibble.
        assert_eq!(data[0], 0xBA);
        assert_eq!(chunk.block(0, 0, 0).metadata, 0xA);
        assert_eq!(chunk.block(1, 0, 0).metadata, 0xB);
    }

    #[test]
    fn add_nibble_holds_the_high_id_bits() {
        let chunk = Chunk::from_blocks(0, 0, [(14, 1, 14, Block::new("marker", 7))]).unwrap();
        let root = chunk_to_nbt(&chunk);
        let section = &root
            .get("Level")
            .unwrap()
            .get("Sections")
            .unwrap()
            .as_list()
            .unwrap()[0];
        let blocks = section.get("Blocks").unwrap().as_bytes().unwrap();
        let add = section.get("Add").unwrap().as_bytes().unwrap();
        let data = section.get("Data").unwrap().as_bytes().unwrap();
        let index = block_index(14, 1, 14);
        assert_eq!(index, 494);
        assert_eq!(blocks[index], 0x2C);
        assert_eq!(add[index >> 1] & 0x0F, 0x1);
        assert_eq!(data[index >> 1] & 0x0F, 0x7);
        assert_eq!(chunk.block(14, 1, 14), Block::new("marker", 7));
    }

    #[test]
    fn out_of_range_block_is_rejected() {
        let err = Chunk::from_blocks(0, 0, [(16, 0, 0, Block::new("stone", 0))]).unwrap_err();
        assert!(matches!(err, Error::BadBlockCoord));
    }
}
