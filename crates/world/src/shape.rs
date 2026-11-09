//! Boxes and planes inside one block cell.
//!
//! A shape is a few axis-aligned boxes on a 16×16×16 grid, plus thin quads
//! from a model. A mesh can also add tilted faces. Those do not change the
//! cube: a full box still counts as a full cube. Which block uses which
//! shape is the pack file, not a match in this crate.

/// Oak stairs block id. The boxes for it live in the pack file.
pub const OAK_STAIRS: &str = "oak_stairs";

/// Coal ore block id. The pack file says which stone breaking it leaves.
pub const COAL_ORE: &str = "coal_ore";

/// Iron ore block id. The pack file names a different stone than coal ore.
pub const IRON_ORE: &str = "iron_ore";

/// Stone left behind when coal ore breaks.
pub const STONE: &str = "stone";

/// Stone left behind when iron ore breaks.
pub const DEEPSLATE: &str = "deepslate";

/// Sign block id. Its boxes and plane come from the model the pack points at.
pub const SIGN: &str = "sign";

/// Colored light block id. The color and level live in the pack file.
pub const LANTERN: &str = "lantern";

/// One box inside a cell. `min` is inclusive and `max` is exclusive, each
/// axis in `0..=16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellBox {
    pub min: [u8; 3],
    pub max: [u8; 3],
}

const FULL: CellBox = CellBox {
    min: [0, 0, 0],
    max: [16, 16, 16],
};

/// A quad with no thickness. `axis` is the flat one (0, 1, or 2). `at` is
/// its position in sixteenths. `min` and `max` span the other two axes, in
/// axis order, with `max` exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plane {
    pub axis: u8,
    pub at: u8,
    pub min: [u8; 2],
    pub max: [u8; 2],
}

/// One quad from a Blockbench mesh. Corners are cell-local, in 1/1024 of a
/// block, and may sit just outside the cell. `uv` is 0..=1024 across the
/// face's texture. `tex` indexes the shape's texture list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshFace {
    pub(crate) corner: [[i16; 3]; 4],
    pub(crate) uv: [[u16; 2]; 4],
    pub(crate) tex: u8,
}

impl MeshFace {
    pub fn corners(self) -> [[f32; 3]; 4] {
        self.corner
            .map(|point| point.map(|value| f32::from(value) / 1024.0))
    }

    pub fn uvs(self) -> [[f32; 2]; 4] {
        self.uv
            .map(|point| [f32::from(point[0]) / 1024.0, f32::from(point[1]) / 1024.0])
    }

    pub fn texture_index(self) -> u8 {
        self.tex
    }
}

/// The boxes and planes that fill one block. Empty for air and for ids the
/// pack skips. `faces` are extra quads (grass blades) and are not solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape<'a> {
    boxes: &'a [CellBox],
    planes: &'a [Plane],
    faces: &'a [MeshFace],
    textures: &'a [String],
}

impl<'a> Shape<'a> {
    pub fn empty() -> Shape<'static> {
        Shape {
            boxes: &[],
            planes: &[],
            faces: &[],
            textures: &[],
        }
    }

    pub fn from_boxes(boxes: &'a [CellBox]) -> Self {
        Self {
            boxes,
            planes: &[],
            faces: &[],
            textures: &[],
        }
    }

    pub fn new(boxes: &'a [CellBox], planes: &'a [Plane]) -> Self {
        Self {
            boxes,
            planes,
            faces: &[],
            textures: &[],
        }
    }

    pub fn with_faces(mut self, faces: &'a [MeshFace]) -> Self {
        self.faces = faces;
        self
    }

    pub fn with_textures(mut self, textures: &'a [String]) -> Self {
        self.textures = textures;
        self
    }

    pub fn boxes(self) -> &'a [CellBox] {
        self.boxes
    }

    pub fn planes(self) -> &'a [Plane] {
        self.planes
    }

    pub fn faces(self) -> &'a [MeshFace] {
        self.faces
    }

    pub fn textures(self) -> &'a [String] {
        self.textures
    }

    /// Pack path for this face, when the model named one.
    pub fn texture_of(self, face: MeshFace) -> Option<&'a str> {
        self.textures
            .get(usize::from(face.tex))
            .map(String::as_str)
            .filter(|path| !path.is_empty())
    }

    pub fn is_empty(self) -> bool {
        self.boxes.is_empty() && self.planes.is_empty() && self.faces.is_empty()
    }

    pub fn is_full_cube(self) -> bool {
        self.planes.is_empty() && self.boxes.len() == 1 && self.boxes[0] == FULL
    }

    /// Each solid a body can hit, in sixteenths of the cell.
    ///
    /// Cube elements use their boxes. A plane is the same quad, thickened by
    /// half a sixteenth on the flat axis and clamped to the cell, so a sweep
    /// still stops on it.
    pub fn for_each_solid(self, mut visit: impl FnMut([f32; 3], [f32; 3])) {
        for box_ in self.boxes {
            visit(
                [
                    f32::from(box_.min[0]),
                    f32::from(box_.min[1]),
                    f32::from(box_.min[2]),
                ],
                [
                    f32::from(box_.max[0]),
                    f32::from(box_.max[1]),
                    f32::from(box_.max[2]),
                ],
            );
        }
        for plane in self.planes {
            let mut min = [0.0; 3];
            let mut max = [0.0; 3];
            let mut uv = 0;
            for axis in 0..3 {
                if axis == plane.axis as usize {
                    let at = f32::from(plane.at);
                    min[axis] = (at - 0.5).max(0.0);
                    max[axis] = (at + 0.5).min(16.0);
                } else {
                    min[axis] = f32::from(plane.min[uv]);
                    max[axis] = f32::from(plane.max[uv]);
                    uv += 1;
                }
            }
            visit(min, max);
        }
    }

    /// `x`, `y`, and `z` are in `0..16`.
    pub fn occludes(self, x: u8, y: u8, z: u8) -> bool {
        let point = [x, y, z];
        self.boxes.iter().any(|box_| {
            (0..3).all(|axis| point[axis] >= box_.min[axis] && point[axis] < box_.max[axis])
        })
    }

    /// True when every cell on that side is solid.
    ///
    /// `axis` is 0, 1, or 2 for X, Y, Z. `positive` is the high side.
    pub fn covers_cell_face(self, axis: u8, positive: bool) -> bool {
        if self.is_full_cube() {
            return true;
        }
        if self.is_empty() {
            return false;
        }
        let fixed = if positive { 15 } else { 0 };
        for a in 0..16u8 {
            for b in 0..16u8 {
                let (x, y, z) = match axis {
                    0 => (fixed, a, b),
                    1 => (a, fixed, b),
                    _ => (a, b, fixed),
                };
                if !self.occludes(x, y, z) {
                    return false;
                }
            }
        }
        true
    }
}
