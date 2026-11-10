//! Screens read from JSON in the resource pack.
//!
//! The main menu is up at launch: play starts the solo world, settings picks
//! the world size, a southern or northern sun, and mute, worlds loads a named
//! save, quit leaves. Up and down move the row. Enter or Space activates it.
//! Escape during play releases the mouse, then opens pause: resume, settings,
//! or quit to the title. E opens the player inventory (PLAYER | Craft tabs,
//! 4x10 body slots + hotbar x10) with an EMI sidebar on the right. Escape or E
//! closes it. Craft tab hosts a 2x2 input + OUT wired to the recipe book. EMI
//! lists searchable recipe-book items; a selected item shows CRAFTING and USES.
//! F on a placed crafting table opens a textured 3x3. The items in the grid
//! match a book recipe, and Enter takes the result. F on a furnace opens
//! input, fuel, and output. Cobblestone cooks into stone, and stone bricks
//! cook into cracked stone bricks, while oak log or coal ore sits in the fuel slot. Keys 1 through 9 and 0 move one from the hotbar.
//! I opens the creative inventory: category tabs with arrows, a 10-wide grid
//! of placeable blocks, the hotbar x10 under it, and EMI on the right. Arrows
//! move the cursor. Enter on a block puts a full stack in the selected hotbar
//! slot. Keys 1 through 9 and 0 pick the hotbar slot.
//! Screens name their chrome in JSON (`panel`, `button`, `header` block ids).
//! Those ids sample the pack atlas, so a panel is stone and a button is cobblestone.
//! A mod screen, such as the guide on G, opens on the key in its JSON.
//! The hotbar is drawn with these screens, not instead of them. The menu
//! covers the window and hides the hotbar.
//! The pointer hovers a row or slot, a click runs a button or takes that slot,
//! and the wheel scrolls a list. Arrow keys and Enter still do the same work.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::Value;
use winit::keyboard::KeyCode;

use crate::hotbar::{self, Hotbar, Quad};
use crate::palette;
use crate::recipes::{self, Book};

/// Overall HUD / menu / inventory scale. Layout uses a virtual resolution of
/// `size / GUI_SCALE`, so every plate draws 1.5× larger on screen.
pub const GUI_SCALE: f32 = 1.5;

/// Virtual size used by HUD and menu layout.
pub fn gui_space(width: f32, height: f32) -> (f32, f32) {
    ((width / GUI_SCALE).max(1.0), (height / GUI_SCALE).max(1.0))
}

const CELL: f32 = 3.0;
/// Capitals in the title are drawn at this multiple of [`CELL`].
const BRAND_SCALE: f32 = 2.0;
const GLYPH_GAP: f32 = 3.0;
const LINE: f32 = 5.0 * CELL;
const BRAND_H: f32 = 7.0 * CELL * BRAND_SCALE;
const PAD: f32 = 28.0;
const SWATCH: f32 = 16.0;
const ROW_GAP: f32 = 12.0;
const TITLE_GAP: f32 = 18.0;
const ABOVE_HOTBAR: f32 = 108.0;
const BUTTON_H: f32 = 36.0;
const LIST_ROW: f32 = 32.0;

const PANEL: [f32; 3] = [0.05, 0.06, 0.08];
/// A darker well behind a list or a creative grid.
const WELL: [f32; 3] = [0.02, 0.025, 0.04];
/// A light strip along the top of a panel, the same idea as the hotbar lip.
const LIP: [f32; 3] = [0.30, 0.31, 0.36];
/// A flat plate behind a title so the letters stay readable on a textured header.
const TITLE_PLATE: [f32; 3] = [0.07, 0.08, 0.11];
/// The plate under hearts and hunger.
const TRAY: [f32; 3] = [0.04, 0.045, 0.06];
/// A light corner on a full heart.
const PIP: [f32; 3] = [1.0, 0.62, 0.55];
/// The red edge drawn while a hit is still fading.
const HURT: [f32; 3] = [0.62, 0.08, 0.07];
/// A warm strip while food is eaten.
const BITE: [f32; 3] = [0.72, 0.45, 0.22];
/// A cool strip while a potion is drunk.
const SIP: [f32; 3] = [0.42, 0.62, 0.78];
const HEADER: [f32; 3] = [0.10, 0.11, 0.15];
const FRAME: [f32; 3] = [0.93, 0.78, 0.42];
const EDGE: [f32; 3] = [0.0, 0.0, 0.0];
const INK: [f32; 3] = [1.0, 1.0, 1.0];
const SHADOW: [f32; 3] = [0.0, 0.0, 0.0];
const LABEL: [f32; 3] = [0.96, 0.97, 0.98];
const SELECT: [f32; 3] = [0.72, 0.52, 0.12];
const DETAIL: [f32; 3] = [1.0, 0.92, 0.55];
const BUTTON: [f32; 3] = [0.14, 0.15, 0.19];
const ROW: [f32; 3] = [0.09, 0.10, 0.13];
const BACKDROP: [f32; 3] = [0.012, 0.014, 0.02];
const CREATIVE_COLS: usize = 10;
const CREATIVE_ROWS: usize = 4;
const CREATIVE_CELL: f32 = 32.0;
const CREATIVE_GAP: f32 = 4.0;
const TAB_H: f32 = 20.0;
/// Creative groups. `all` is the whole catalog. Tab cycles them.
const TABS: [&str; 6] = ["all", "build", "nature", "ore", "light", "more"];
const CHEST_COLS: usize = 3;

/// Three-wide rows, top to bottom. The high bit is the left column.
const LETTERS: [[u8; 5]; 26] = [
    [0b011, 0b001, 0b111, 0b101, 0b111],
    [0b100, 0b100, 0b111, 0b101, 0b111],
    [0b011, 0b100, 0b100, 0b100, 0b011],
    [0b001, 0b001, 0b111, 0b101, 0b111],
    [0b011, 0b101, 0b110, 0b100, 0b011],
    [0b001, 0b010, 0b111, 0b010, 0b010],
    [0b011, 0b101, 0b011, 0b001, 0b110],
    [0b100, 0b100, 0b111, 0b101, 0b101],
    [0b010, 0b000, 0b010, 0b010, 0b010],
    [0b001, 0b000, 0b001, 0b001, 0b110],
    [0b100, 0b101, 0b110, 0b101, 0b101],
    [0b010, 0b010, 0b010, 0b010, 0b011],
    [0b000, 0b111, 0b101, 0b101, 0b101],
    [0b000, 0b110, 0b101, 0b101, 0b101],
    [0b010, 0b101, 0b101, 0b101, 0b010],
    [0b110, 0b101, 0b110, 0b100, 0b100],
    [0b011, 0b101, 0b011, 0b001, 0b001],
    [0b000, 0b110, 0b101, 0b100, 0b100],
    [0b011, 0b100, 0b010, 0b001, 0b110],
    [0b010, 0b111, 0b010, 0b010, 0b001],
    [0b000, 0b101, 0b101, 0b101, 0b011],
    [0b000, 0b101, 0b101, 0b101, 0b010],
    [0b000, 0b101, 0b101, 0b111, 0b101],
    [0b000, 0b101, 0b010, 0b101, 0b101],
    [0b000, 0b101, 0b011, 0b001, 0b110],
    [0b000, 0b111, 0b010, 0b100, 0b111],
];

/// Five-wide capitals, top to bottom. The high bit of the five is the left column.
const CAPITALS: [[u8; 7]; 26] = [
    [
        0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
    ],
    [
        0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
    ],
    [
        0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
    ],
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
    ],
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    [
        0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
    ],
    [
        0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    [
        0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    [
        0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
    ],
    [
        0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
    ],
    [
        0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
    ],
    [
        0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
    ],
    [
        0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
    ],
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
    ],
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
    ],
    [
        0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
    ],
    [
        0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
    ],
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
    ],
    [
        0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
    ],
    [
        0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
    ],
    [
        0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
    ],
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
    ],
];

/// The items screen file inside the resource pack.
pub fn screen_file() -> PathBuf {
    gui_file("index.json")
}

/// The main menu file inside the resource pack.
pub fn menu_file() -> PathBuf {
    gui_file("menu.json")
}

/// The settings file inside the resource pack.
pub fn settings_file() -> PathBuf {
    gui_file("settings.json")
}

/// Pause during play.
pub fn pause_file() -> PathBuf {
    gui_file("pause.json")
}

/// Named saves.
pub fn worlds_file() -> PathBuf {
    gui_file("worlds.json")
}

fn gui_file(name: &str) -> PathBuf {
    rebuild_world::resource_pack_root()
        .join("assets")
        .join("terralite")
        .join("gui")
        .join(name)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenKind {
    /// Item rows with a color swatch.
    List,
    /// Buttons. Enter activates the selected row.
    Menu,
    /// A grid of placeable blocks. The catalog comes from the pack.
    Creative,
}

#[derive(Clone)]
pub struct Screen {
    #[cfg_attr(not(test), allow(dead_code))]
    id: String,
    title: String,
    open: KeyCode,
    close: KeyCode,
    labels: Vec<String>,
    kind: ScreenKind,
    /// Pack block whose texture tiles the panel fill.
    panel: String,
    /// Pack block whose texture tiles each button.
    button: String,
    /// Pack block whose texture tiles the title band.
    header: String,
}

impl Screen {
    pub fn parse(text: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
        let obj = root
            .as_object()
            .ok_or_else(|| "screen must be an object".to_string())?;
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "id" | "title"
                    | "open"
                    | "close"
                    | "labels"
                    | "kind"
                    | "panel"
                    | "button"
                    | "header"
            ) {
                return Err(format!("screen has unknown field {key}"));
            }
        }
        let id = field_text(obj, "id")?;
        if !valid_id(&id) {
            return Err(format!("bad screen id {id}"));
        }
        let title = field_text(obj, "title")?;
        if !valid_title(&title) {
            return Err("screen title must be letters and spaces".into());
        }
        let open = key_code(&field_text(obj, "open")?, "open")?;
        let close = key_code(&field_text(obj, "close")?, "close")?;
        let kind = match obj.get("kind") {
            None => ScreenKind::List,
            Some(Value::String(text)) if text == "list" => ScreenKind::List,
            Some(Value::String(text)) if text == "menu" => ScreenKind::Menu,
            Some(Value::String(text)) if text == "creative" => ScreenKind::Creative,
            _ => return Err("screen kind must be list, menu, or creative".into()),
        };
        let labels = match obj.get("labels") {
            None if kind == ScreenKind::Creative => Vec::new(),
            Some(Value::Array(items)) => items.clone(),
            _ => return Err("screen labels must be an array".into()),
        };
        let min_labels = if kind == ScreenKind::Creative { 0 } else { 1 };
        if labels.len() < min_labels || labels.len() > 12 {
            return Err("screen needs 1 to 12 labels".into());
        }
        let mut rows = Vec::with_capacity(labels.len());
        for (index, label) in labels.iter().enumerate() {
            let Some(text) = label.as_str() else {
                return Err(format!("label {index} must be a string"));
            };
            if !valid_label(text) {
                return Err(format!("bad label {text}"));
            }
            rows.push(text.to_string());
        }
        let panel = face_field(obj, "panel", "stone")?;
        let button = face_field(obj, "button", "cobblestone")?;
        let header = face_field(obj, "header", "deepslate")?;
        Ok(Self {
            id,
            title,
            open,
            close,
            labels: rows,
            kind,
            panel,
            button,
            header,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Panel, title, rows, and the selected entry's recipe line.
    pub fn quads(
        &self,
        width: f32,
        height: f32,
        rows: &[String],
        selected: usize,
        detail: &str,
    ) -> Vec<Quad> {
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let menu = self.kind == ScreenKind::Menu;
        let row_h = if menu { BUTTON_H } else { LIST_ROW };
        let title = brand(&self.title);
        let shown: Vec<String> = rows
            .iter()
            .map(|row| title_case(&recipes::display_name(row)))
            .collect();
        let detail_shown = sentence(detail);
        let name_pad = if menu { 0.0 } else { SWATCH + 12.0 };
        let mut content_w = measure(&title, BRAND_SCALE);
        for row in &shown {
            content_w = content_w.max(measure(row, 1.0) + name_pad);
        }
        if !detail_shown.is_empty() {
            content_w = content_w.max(measure(&detail_shown, 1.0));
        }
        let panel_w = (content_w + PAD * 2.0).max(if menu { 360.0 } else { 320.0 });
        let detail_h = if detail_shown.is_empty() {
            0.0
        } else {
            LINE + TITLE_GAP
        };
        let panel_h =
            PAD + BRAND_H + TITLE_GAP + rows.len() as f32 * (row_h + ROW_GAP) + detail_h + PAD;
        let left = (width - panel_w) * 0.5;
        let bottom = if menu {
            ((height - panel_h) * 0.5).max(16.0)
        } else {
            ((height - panel_h) * 0.5).max(ABOVE_HOTBAR)
        };
        let mut quads = Vec::new();
        quads.push(pixel_quad(
            width,
            height,
            left - 4.0,
            bottom - 4.0,
            left + panel_w + 4.0,
            bottom + panel_h + 4.0,
            EDGE,
        ));
        quads.push(pixel_quad(
            width,
            height,
            left - 2.0,
            bottom - 2.0,
            left + panel_w + 2.0,
            bottom + panel_h + 2.0,
            FRAME,
        ));
        quads.push(
            pixel_quad(
                width,
                height,
                left,
                bottom,
                left + panel_w,
                bottom + panel_h,
                PANEL,
            )
            .face(&self.panel, panel_w / 16.0, panel_h / 16.0),
        );
        let title_base = bottom + panel_h - PAD - BRAND_H;
        let header_h = PAD + BRAND_H + 8.0;
        quads.push(
            pixel_quad(
                width,
                height,
                left,
                title_base - 8.0,
                left + panel_w,
                bottom + panel_h,
                HEADER,
            )
            .face(&self.header, panel_w / 16.0, header_h / 16.0),
        );
        push_lip(&mut quads, width, height, left, bottom + panel_h, panel_w);
        let title_w = measure(&title, BRAND_SCALE);
        let title_x = left + (panel_w - title_w) * 0.5;
        push_title_plate(&mut quads, width, height, title_x, title_base, title_w);
        draw_text(
            &mut quads,
            width,
            height,
            title_x,
            title_base,
            &title,
            INK,
            BRAND_SCALE,
        );
        quads.push(pixel_quad(
            width,
            height,
            left + PAD,
            title_base - 12.0,
            left + panel_w - PAD,
            title_base - 8.0,
            DETAIL,
        ));
        let mut row_y = title_base - TITLE_GAP - row_h;
        let inner_left = left + PAD;
        let inner_right = left + panel_w - PAD;
        if !shown.is_empty() {
            let rows_top = title_base - TITLE_GAP;
            let rows_h = shown.len() as f32 * (row_h + ROW_GAP);
            quads.push(pixel_quad(
                width,
                height,
                inner_left - 6.0,
                rows_top - rows_h - 4.0,
                inner_right + 6.0,
                rows_top + 4.0,
                WELL,
            ));
        }
        for (index, label) in shown.iter().enumerate() {
            let chosen = index == selected;
            if menu {
                let frame_w = inner_right - inner_left + 4.0;
                quads.push(
                    pixel_quad(
                        width,
                        height,
                        inner_left - 2.0,
                        row_y - 2.0,
                        inner_right + 2.0,
                        row_y + row_h + 2.0,
                        if chosen { FRAME } else { [0.28, 0.30, 0.36] },
                    )
                    .face(&self.button, frame_w / 16.0, (row_h + 4.0) / 16.0),
                );
                let button_w = inner_right - inner_left;
                quads.push(
                    pixel_quad(
                        width,
                        height,
                        inner_left,
                        row_y,
                        inner_right,
                        row_y + row_h,
                        if chosen { SELECT } else { BUTTON },
                    )
                    .face(&self.button, button_w / 16.0, row_h / 16.0)
                    .tag(1 + index as u16),
                );
                if chosen {
                    quads.push(pixel_quad(
                        width,
                        height,
                        inner_left,
                        row_y,
                        inner_left + 4.0,
                        row_y + row_h,
                        FRAME,
                    ));
                }
            } else {
                let row_w = inner_right - inner_left + 12.0;
                quads.push(
                    pixel_quad(
                        width,
                        height,
                        inner_left - 6.0,
                        row_y - 2.0,
                        inner_right + 6.0,
                        row_y + row_h + 2.0,
                        if chosen { SELECT } else { ROW },
                    )
                    .face(&self.panel, row_w / 16.0, (row_h + 4.0) / 16.0)
                    .tag(1 + index as u16),
                );
            }
            if !menu {
                let chip = row_y + (row_h - SWATCH) * 0.5;
                hotbar::push_slot_icon(
                    &mut quads,
                    width,
                    height,
                    inner_left,
                    chip,
                    SWATCH,
                    &rows[index],
                );
            }
            let text_h = block_height(label, 1.0);
            let text_w = measure(label, 1.0);
            let text_x = if menu {
                inner_left + ((inner_right - inner_left) - text_w) * 0.5
            } else {
                inner_left + SWATCH + 12.0
            };
            draw_text(
                &mut quads,
                width,
                height,
                text_x,
                row_y + (row_h - text_h) * 0.5,
                label,
                if menu && chosen { INK } else { LABEL },
                1.0,
            );
            row_y -= row_h + ROW_GAP;
        }
        if !detail_shown.is_empty() {
            draw_text(
                &mut quads,
                width,
                height,
                inner_left,
                bottom + PAD,
                &detail_shown,
                DETAIL,
                1.0,
            );
        }
        quads
    }
}

/// Tab strip on the player inventory screen (E).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerInvTab {
    Player,
    Craft,
}

pub struct Gui {
    screen: Option<Screen>,
    extras: Vec<Screen>,
    /// Screens a script built with GUIAPI.
    built: Vec<rebuild_world::GuiScreen>,
    /// Which built screen is showing.
    built_at: Option<usize>,
    /// Row chosen by Enter or a click, until the client takes it.
    pressed: Option<String>,
    book: Book,
    open: bool,
    /// Which extra screen is showing. `None` is the player inventory.
    extra: Option<usize>,
    selected: usize,
    bag_open: bool,
    bag_selected: usize,
    /// PLAYER vs Craft tab while the player inventory (E) is open.
    inv_tab: PlayerInvTab,
    /// The creative grid has the cursor. False means the carried row does.
    catalog_on: bool,
    catalog_at: usize,
    /// Index into [`TABS`]. `0` is every placeable block.
    catalog_tab: usize,
    /// Letters typed on the creative grid. Empty shows the whole tab.
    /// A space is a word break, and the match uses the readable name.
    catalog_query: String,
    /// EMI sidebar search (player inventory + creative). Empty shows every book entry.
    emi_query: String,
    /// Selected EMI item id. `None` hides CRAFTING / USES panels.
    emi_id: Option<String>,
    command_open: bool,
    command_selected: usize,
    /// `(id, label)` for registered commands. The label is what the screen draws.
    commands: Vec<(String, String)>,
    chat_open: bool,
    chat_draft: String,
    /// Lines still on screen, and how many seconds they have left.
    chat_log: Vec<(String, f32)>,
    chest_open: bool,
    chest_selected: usize,
    chest: hotbar::Bag,
    craft_open: bool,
    /// `0..9` is the grid. `9` is the result.
    craft_selected: usize,
    craft: hotbar::Bag,
    /// Player-inventory Craft tab 2×2 (slots `0..3`). Preview-only until taken.
    inv_craft: hotbar::Bag,
    furnace_open: bool,
    /// `0` input, `1` fuel, `2` output.
    furnace_selected: usize,
    furnace: hotbar::Bag,
    furnace_cook: u32,
    furnace_time: f32,
    /// The furnace screen is smoking food, so stone lines do not cook.
    smoker: bool,
    /// The furnace screen is blasting ore and stone, so food does not cook.
    blast: bool,
    anvil_open: bool,
    /// The line under the anvil title. Enter replaces it with the repair word.
    anvil_note: String,
    enchant_open: bool,
    /// The line under the enchant title. Enter replaces it with the enchant word.
    enchant_note: String,
    brew_open: bool,
    /// The line under the brew title. Enter replaces it with the brew word.
    brew_note: String,
    grind_open: bool,
    /// The line under the grind title. Enter replaces it with the grind word.
    grind_note: String,
    smith_open: bool,
    /// The line under the smith title. Enter replaces it with the upgrade word.
    smith_note: String,
    loom_open: bool,
    /// The line under the loom title. Enter replaces it with the weave word.
    loom_note: String,
    chart_open: bool,
    /// The line under the chart title. Enter replaces it with the zoom word.
    chart_note: String,
    lectern_open: bool,
    /// The line under the lectern title. A held book replaces it with the page.
    lectern_note: String,
    /// The lectern the page belongs to.
    lectern_at: Option<[i32; 3]>,
    compost_open: bool,
    /// The line under the compost title. Enter replaces it with the level.
    compost_note: String,
    /// The composter the panel is filling.
    compost_at: Option<[i32; 3]>,
    hive_open: bool,
    /// The line under the hive title. Enter replaces it with the level.
    hive_note: String,
    /// The beehive the panel is filling.
    hive_at: Option<[i32; 3]>,
    cauldron_open: bool,
    /// The line under the cauldron title. Enter replaces it with the level.
    cauldron_note: String,
    /// The cauldron the panel is filling.
    cauldron_at: Option<[i32; 3]>,
    bell_open: bool,
    bell_note: String,
    bell_at: Option<[i32; 3]>,
    candle_open: bool,
    candle_note: String,
    candle_at: Option<[i32; 3]>,
    lodestone_open: bool,
    lodestone_note: String,
    lodestone_at: Option<[i32; 3]>,
    anchor_open: bool,
    anchor_note: String,
    anchor_at: Option<[i32; 3]>,
    rod_open: bool,
    rod_note: String,
    rod_at: Option<[i32; 3]>,
    target_open: bool,
    target_note: String,
    target_at: Option<[i32; 3]>,
    bulb_open: bool,
    bulb_note: String,
    bulb_at: Option<[i32; 3]>,
    pot_open: bool,
    pot_note: String,
    pot_at: Option<[i32; 3]>,
    pot_id: String,
    pot_count: u16,
    board_open: bool,
    board_note: String,
    board_at: Option<[i32; 3]>,
    board_ids: [String; 3],
    board_counts: [u16; 3],
    shelf_open: bool,
    shelf_note: String,
    shelf_at: Option<[i32; 3]>,
    hang_open: bool,
    hang_note: String,
    hang_at: Option<[i32; 3]>,
    cut_open: bool,
    /// The line under the cut title. Enter replaces it with the cut word.
    cut_note: String,
    fire_open: bool,
    /// The line under the fire title. Enter replaces it with the cook word.
    fire_note: String,
    fletch_open: bool,
    /// The line under the fletch title. Enter replaces it with the arrow word.
    fletch_note: String,
}

/// The one page a lectern reads aloud.
pub const LECTERN_PAGE: &str = "the road west of the stairs";

/// The one line a hanging sign shows.
pub const HANGING_LINE: &str = "the path north of the camp";

impl Gui {
    /// Load the shipped screen. A missing file leaves the key doing nothing.
    pub fn new() -> Self {
        let book = match recipes::load_emi() {
            Ok(book) => book,
            Err(err) => {
                eprintln!("recipes: {err}");
                Book::empty()
            }
        };
        let extras = load_mod_screens();
        let built = load_built();
        match load_screen() {
            Ok(screen) => Self {
                screen: Some(screen),
                extras,
                built: built.clone(),
                built_at: None,
                pressed: None,
                book,
                open: false,
                extra: None,
                selected: 0,
                bag_open: false,
                bag_selected: 0,
                inv_tab: PlayerInvTab::Player,
                catalog_on: false,
                catalog_at: 0,
                catalog_tab: 0,
                catalog_query: String::new(),
                emi_query: String::new(),
                emi_id: None,
                command_open: false,
                command_selected: 0,
                commands: Vec::new(),
                chat_open: false,
                chat_draft: String::new(),
                chat_log: Vec::new(),
                chest_open: false,
                chest_selected: 0,
                chest: hotbar::Bag::new(),
                craft_open: false,
                craft_selected: 0,
                craft: hotbar::Bag::new(),
                inv_craft: hotbar::Bag::new(),
                furnace_open: false,
                furnace_selected: 0,
                furnace: hotbar::Bag::new(),
                furnace_cook: 0,
                furnace_time: 0.0,
                smoker: false,
                blast: false,
                anvil_open: false,
                anvil_note: String::new(),
                enchant_open: false,
                enchant_note: String::new(),
                brew_open: false,
                brew_note: String::new(),
                grind_open: false,
                grind_note: String::new(),
                smith_open: false,
                smith_note: String::new(),
                loom_open: false,
                loom_note: String::new(),
                chart_open: false,
                chart_note: String::new(),
                lectern_open: false,
                lectern_note: String::new(),
                lectern_at: None,
                compost_open: false,
                compost_note: String::new(),
                compost_at: None,
                hive_open: false,
                hive_note: String::new(),
                hive_at: None,
                cauldron_open: false,
                cauldron_note: String::new(),
                cauldron_at: None,
                bell_open: false,
                bell_note: String::new(),
                bell_at: None,
                candle_open: false,
                candle_note: String::new(),
                candle_at: None,
                lodestone_open: false,
                lodestone_note: String::new(),
                lodestone_at: None,
                anchor_open: false,
                anchor_note: String::new(),
                anchor_at: None,
                rod_open: false,
                rod_note: String::new(),
                rod_at: None,
                target_open: false,
                target_note: String::new(),
                target_at: None,
                bulb_open: false,
                bulb_note: String::new(),
                bulb_at: None,
                pot_open: false,
                pot_note: String::new(),
                pot_at: None,
                pot_id: String::new(),
                pot_count: 0,
                board_open: false,
                board_note: String::new(),
                board_at: None,
                board_ids: [String::new(), String::new(), String::new()],
                board_counts: [0, 0, 0],
                shelf_open: false,
                shelf_note: String::new(),
                shelf_at: None,
                hang_open: false,
                hang_note: String::new(),
                hang_at: None,
                cut_open: false,
                cut_note: String::new(),
                fire_open: false,
                fire_note: String::new(),
                fletch_open: false,
                fletch_note: String::new(),
            },
            Err(err) => {
                eprintln!("screen: {err}");
                Self {
                    screen: None,
                    extras,
                    built,
                    built_at: None,
                    pressed: None,
                    book,
                    open: false,
                    extra: None,
                    selected: 0,
                    bag_open: false,
                    bag_selected: 0,
                    inv_tab: PlayerInvTab::Player,
                    catalog_on: false,
                    catalog_at: 0,
                    catalog_tab: 0,
                    catalog_query: String::new(),
                    emi_query: String::new(),
                    emi_id: None,
                    command_open: false,
                    command_selected: 0,
                    commands: Vec::new(),
                    chat_open: false,
                    chat_draft: String::new(),
                    chat_log: Vec::new(),
                    chest_open: false,
                    chest_selected: 0,
                    chest: hotbar::Bag::new(),
                    craft_open: false,
                    craft_selected: 0,
                    craft: hotbar::Bag::new(),
                    inv_craft: hotbar::Bag::new(),
                    furnace_open: false,
                    furnace_selected: 0,
                    furnace: hotbar::Bag::new(),
                    furnace_cook: 0,
                    furnace_time: 0.0,
                    smoker: false,
                    blast: false,
                    anvil_open: false,
                    anvil_note: String::new(),
                    enchant_open: false,
                    enchant_note: String::new(),
                    brew_open: false,
                    brew_note: String::new(),
                    grind_open: false,
                    grind_note: String::new(),
                    smith_open: false,
                    smith_note: String::new(),
                    loom_open: false,
                    loom_note: String::new(),
                    chart_open: false,
                    chart_note: String::new(),
                    lectern_open: false,
                    lectern_note: String::new(),
                    lectern_at: None,
                    compost_open: false,
                    compost_note: String::new(),
                    compost_at: None,
                    hive_open: false,
                    hive_note: String::new(),
                    hive_at: None,
                    cauldron_open: false,
                    cauldron_note: String::new(),
                    cauldron_at: None,
                    bell_open: false,
                    bell_note: String::new(),
                    bell_at: None,
                    candle_open: false,
                    candle_note: String::new(),
                    candle_at: None,
                    lodestone_open: false,
                    lodestone_note: String::new(),
                    lodestone_at: None,
                    anchor_open: false,
                    anchor_note: String::new(),
                    anchor_at: None,
                    rod_open: false,
                    rod_note: String::new(),
                    rod_at: None,
                    target_open: false,
                    target_note: String::new(),
                    target_at: None,
                    bulb_open: false,
                    bulb_note: String::new(),
                    bulb_at: None,
                    pot_open: false,
                    pot_note: String::new(),
                    pot_at: None,
                    pot_id: String::new(),
                    pot_count: 0,
                    board_open: false,
                    board_note: String::new(),
                    board_at: None,
                    board_ids: [String::new(), String::new(), String::new()],
                    board_counts: [0, 0, 0],
                    shelf_open: false,
                    shelf_note: String::new(),
                    shelf_at: None,
                    hang_open: false,
                    hang_note: String::new(),
                    hang_at: None,
                    cut_open: false,
                    cut_note: String::new(),
                    fire_open: false,
                    fire_note: String::new(),
                    fletch_open: false,
                    fletch_note: String::new(),
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_screen(screen: Screen) -> Self {
        let book = recipes::load_book().unwrap_or_else(|_| Book::empty());
        Self::from_parts(screen, book)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_parts(screen: Screen, book: Book) -> Self {
        Self {
            screen: Some(screen),
            extras: Vec::new(),
            built: Vec::new(),
            built_at: None,
            pressed: None,
            book,
            open: false,
            extra: None,
            selected: 0,
            bag_open: false,
            bag_selected: 0,
            inv_tab: PlayerInvTab::Player,
            catalog_on: false,
            catalog_at: 0,
            catalog_tab: 0,
            catalog_query: String::new(),
            emi_query: String::new(),
            emi_id: None,
            command_open: false,
            command_selected: 0,
            commands: Vec::new(),
            chat_open: false,
            chat_draft: String::new(),
            chat_log: Vec::new(),
            chest_open: false,
            chest_selected: 0,
            chest: hotbar::Bag::new(),
            craft_open: false,
            craft_selected: 0,
            craft: hotbar::Bag::new(),
            inv_craft: hotbar::Bag::new(),
            furnace_open: false,
            furnace_selected: 0,
            furnace: hotbar::Bag::new(),
            furnace_cook: 0,
            furnace_time: 0.0,
            smoker: false,
            blast: false,
            anvil_open: false,
            anvil_note: String::new(),
            enchant_open: false,
            enchant_note: String::new(),
            brew_open: false,
            brew_note: String::new(),
            grind_open: false,
            grind_note: String::new(),
            smith_open: false,
            smith_note: String::new(),
            loom_open: false,
            loom_note: String::new(),
            chart_open: false,
            chart_note: String::new(),
            lectern_open: false,
            lectern_note: String::new(),
            lectern_at: None,
            compost_open: false,
            compost_note: String::new(),
            compost_at: None,
            hive_open: false,
            hive_note: String::new(),
            hive_at: None,
            cauldron_open: false,
            cauldron_note: String::new(),
            cauldron_at: None,
            bell_open: false,
            bell_note: String::new(),
            bell_at: None,
            candle_open: false,
            candle_note: String::new(),
            candle_at: None,
            lodestone_open: false,
            lodestone_note: String::new(),
            lodestone_at: None,
            anchor_open: false,
            anchor_note: String::new(),
            anchor_at: None,
            rod_open: false,
            rod_note: String::new(),
            rod_at: None,
            target_open: false,
            target_note: String::new(),
            target_at: None,
            bulb_open: false,
            bulb_note: String::new(),
            bulb_at: None,
            pot_open: false,
            pot_note: String::new(),
            pot_at: None,
            pot_id: String::new(),
            pot_count: 0,
            board_open: false,
            board_note: String::new(),
            board_at: None,
            board_ids: [String::new(), String::new(), String::new()],
            board_counts: [0, 0, 0],
            shelf_open: false,
            shelf_note: String::new(),
            shelf_at: None,
            hang_open: false,
            hang_note: String::new(),
            hang_at: None,
            cut_open: false,
            cut_note: String::new(),
            fire_open: false,
            fire_note: String::new(),
            fletch_open: false,
            fletch_note: String::new(),
        }
    }

    /// A mod screen this gui can open on that screen's key.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn push_screen(&mut self, screen: Screen) {
        self.extras.push(screen);
    }

    pub fn is_open(&self) -> bool {
        self.open
            || self.bag_open
            || self.command_open
            || self.chat_open
            || self.chest_open
            || self.craft_open
            || self.furnace_open
            || self.anvil_open
            || self.enchant_open
            || self.brew_open
            || self.grind_open
            || self.smith_open
            || self.loom_open
            || self.chart_open
            || self.lectern_open
            || self.compost_open
            || self.hive_open
            || self.cauldron_open
            || self.bell_open
            || self.candle_open
            || self.lodestone_open
            || self.anchor_open
            || self.rod_open
            || self.target_open
            || self.bulb_open
            || self.pot_open
            || self.board_open
            || self.shelf_open
            || self.hang_open
            || self.cut_open
            || self.fire_open
            || self.fletch_open
            || self.built_at.is_some()
    }

    pub fn bag_open(&self) -> bool {
        self.bag_open
    }

    /// Alias for [`Self::player_inv_open`] (older call sites / EMI bridge).
    #[allow(dead_code)]
    pub fn items_list_open(&self) -> bool {
        self.player_inv_open()
    }

    /// Player inventory chrome (E): PLAYER | Craft tabs, 4x10 + hotbar.
    pub fn player_inv_open(&self) -> bool {
        self.open && self.extra.is_none() && self.built_at.is_none() && !self.bag_open
    }

    pub fn inv_tab(&self) -> PlayerInvTab {
        self.inv_tab
    }

    pub fn set_inv_tab(&mut self, tab: PlayerInvTab) {
        self.inv_tab = tab;
    }

    pub fn bag_selected(&self) -> usize {
        self.bag_selected
    }

    pub fn catalog_tab(&self) -> usize {
        self.catalog_tab
    }

    pub fn set_catalog_tab(&mut self, tab: usize) {
        self.catalog_tab = tab % TABS.len();
        self.catalog_at = 0;
        self.catalog_on = true;
    }

    /// Step category tabs (`true` = next / `>` , `false` = prev / `<`).
    pub fn cycle_catalog_tab(&mut self, forward: bool) {
        let last = TABS.len() - 1;
        self.catalog_tab = if forward {
            (self.catalog_tab + 1) % TABS.len()
        } else if self.catalog_tab == 0 {
            last
        } else {
            self.catalog_tab - 1
        };
        self.catalog_at = 0;
        self.catalog_on = true;
    }

    /// Number of creative category tabs.
    pub fn catalog_tab_count() -> usize {
        TABS.len()
    }

    pub fn catalog_query(&self) -> &str {
        &self.catalog_query
    }

    pub fn set_catalog_query(&mut self, query: &str) {
        self.catalog_query = query.to_string();
        self.catalog_at = 0;
    }

    pub fn catalog_index(&self) -> usize {
        self.catalog_at
    }

    pub fn set_catalog_index(&mut self, index: usize) {
        self.catalog_at = index;
        self.catalog_on = true;
    }

    pub fn creative_ids(&self) -> Vec<String> {
        creative_view(self.catalog_tab, &self.catalog_query)
    }

    pub fn close_bag(&mut self) {
        self.bag_open = false;
        self.catalog_query.clear();
        self.clear_emi();
    }

    pub fn close_items(&mut self) {
        self.open = false;
        self.extra = None;
        self.inv_tab = PlayerInvTab::Player;
        self.clear_emi();
    }

    fn clear_emi(&mut self) {
        self.emi_query.clear();
        self.emi_id = None;
    }

    pub fn emi_query(&self) -> &str {
        &self.emi_query
    }

    pub fn set_emi_query(&mut self, query: &str) {
        self.emi_query = query.to_string();
    }

    pub fn emi_selected(&self) -> Option<&str> {
        self.emi_id.as_deref()
    }

    pub fn select_emi(&mut self, id: &str) {
        self.emi_id = Some(id.to_string());
        // Keep legacy items-list selection in sync when the id is a book row.
        if let Some(index) = self.rows().iter().position(|row| row == id) {
            self.selected = index;
        }
    }

    /// Recipe-book entries filtered by the EMI search (readable name match).
    pub fn emi_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = self
            .book
            .entries()
            .into_iter()
            .filter(|id| creative_hit(id, &self.emi_query))
            .collect();
        let needle = fold_name(&self.emi_query);
        if !needle.is_empty() {
            rows.sort_by(|left, right| {
                let rank = |id: &str| {
                    let folded = fold_name(id);
                    let shown = fold_name(&recipes::display_name(id));
                    if folded.starts_with(&needle) || shown.starts_with(&needle) {
                        0
                    } else {
                        1
                    }
                };
                rank(left).cmp(&rank(right))
            });
        }
        rows
    }

    /// Ingredients that craft the selected EMI item, when it is a result.
    pub fn emi_crafting(&self) -> Option<Vec<String>> {
        let id = self.emi_id.as_deref()?;
        self.book.inputs(id)
    }

    /// Shaped 3×3 for the selected EMI item, when the recipe is shaped.
    pub fn emi_pattern(&self) -> Option<Vec<String>> {
        let id = self.emi_id.as_deref()?;
        self.book.pattern_of(id).map(|cells| cells.to_vec())
    }

    /// Results that use the selected EMI item as an ingredient.
    pub fn emi_uses(&self) -> Vec<String> {
        let Some(id) = self.emi_id.as_deref() else {
            return Vec::new();
        };
        self.book.uses_of(id)
    }

    /// Recipe-book rows for EMI / Craft.
    #[allow(dead_code)]
    pub fn item_rows(&self) -> Vec<String> {
        self.rows()
    }

    #[allow(dead_code)]
    pub fn selected_row(&self) -> usize {
        self.selected
    }

    #[allow(dead_code)]
    pub fn select_item_row(&mut self, index: usize) {
        self.selected = index;
        if let Some(id) = self.rows().get(index) {
            self.emi_id = Some(id.clone());
        }
    }

    #[allow(dead_code)]
    pub fn item_detail(&self, id: &str) -> String {
        self.book.detail(id)
    }

    /// Craft the selected EMI / items-list recipe into the hotbar / bag.
    pub fn craft_selected_into(
        &self,
        bar: &mut hotbar::Hotbar,
        bag: &mut hotbar::Bag,
    ) -> bool {
        let Some((result, inputs)) = self.craft_job_for_emi() else {
            return false;
        };
        hotbar::craft_carried(bar, bag, &inputs, &result)
    }

    /// Prefer the EMI selection; fall back to the legacy items-list row.
    fn craft_job_for_emi(&self) -> Option<(String, Vec<String>)> {
        if let Some(id) = self.emi_id.clone() {
            let inputs = self.book.inputs(&id)?;
            if !inputs.is_empty() {
                return Some((id, inputs));
            }
        }
        self.craft_job()
    }

    /// Id from the screen file, `inventory` while the bag is open, `commands` for that list, or `""`.
    pub fn screen_id(&self) -> &str {
        if self.chest_open {
            return "chest";
        }
        if self.craft_open {
            return "crafting";
        }
        if self.furnace_open {
            return if self.blast {
                "blast"
            } else if self.smoker {
                "smoker"
            } else {
                "furnace"
            };
        }
        if self.anvil_open {
            return "anvil";
        }
        if self.enchant_open {
            return "enchanting";
        }
        if self.brew_open {
            return "brewing";
        }
        if self.grind_open {
            return "grindstone";
        }
        if self.smith_open {
            return "smithing";
        }
        if self.loom_open {
            return "loom";
        }
        if self.chart_open {
            return "cartography";
        }
        if self.lectern_open {
            return "lectern";
        }
        if self.compost_open {
            return "composter";
        }
        if self.hive_open {
            return "beehive";
        }
        if self.cauldron_open {
            return "cauldron";
        }
        if self.bell_open {
            return "bell";
        }
        if self.candle_open {
            return "candle";
        }
        if self.lodestone_open {
            return "lodestone";
        }
        if self.anchor_open {
            return "anchor";
        }
        if self.rod_open {
            return "rod";
        }
        if self.target_open {
            return "target";
        }
        if self.bulb_open {
            return "bulb";
        }
        if self.pot_open {
            return "pot";
        }
        if self.board_open {
            return "board";
        }
        if self.shelf_open {
            return "shelf";
        }
        if self.hang_open {
            return "hang";
        }
        if self.cut_open {
            return "stonecutter";
        }
        if self.fire_open {
            return "campfire";
        }
        if self.fletch_open {
            return "fletching";
        }
        if self.chat_open {
            return "chat";
        }
        if self.command_open {
            return "commands";
        }
        if self.bag_open {
            return "inventory";
        }
        if let Some(index) = self.built_at {
            if let Some(screen) = self.built.get(index) {
                return screen.name.as_str();
            }
        }
        if let Some(index) = self.extra {
            if let Some(screen) = self.extras.get(index) {
                return screen.id.as_str();
            }
        }
        self.screen
            .as_ref()
            .map(|screen| screen.id.as_str())
            .unwrap_or("")
    }

    pub fn close(&mut self) {
        self.open = false;
        self.extra = None;
        self.built_at = None;
        self.pressed = None;
        self.bag_open = false;
        self.command_open = false;
        self.chat_open = false;
        self.chat_draft.clear();
        self.chest_open = false;
        self.craft_open = false;
        self.furnace_open = false;
        self.smoker = false;
        self.blast = false;
        self.anvil_open = false;
        self.enchant_open = false;
        self.brew_open = false;
        self.grind_open = false;
        self.smith_open = false;
        self.loom_open = false;
        self.chart_open = false;
        self.lectern_open = false;
        self.lectern_at = None;
        self.compost_open = false;
        self.compost_at = None;
        self.hive_open = false;
        self.hive_at = None;
        self.cauldron_open = false;
        self.cauldron_at = None;
        self.bell_open = false;
        self.bell_at = None;
        self.candle_open = false;
        self.candle_at = None;
        self.lodestone_open = false;
        self.lodestone_at = None;
        self.anchor_open = false;
        self.anchor_at = None;
        self.rod_open = false;
        self.rod_at = None;
        self.target_open = false;
        self.target_at = None;
        self.bulb_open = false;
        self.bulb_at = None;
        self.pot_open = false;
        self.pot_at = None;
        self.pot_id.clear();
        self.pot_count = 0;
        self.board_open = false;
        self.board_at = None;
        self.board_ids = [String::new(), String::new(), String::new()];
        self.board_counts = [0, 0, 0];
        self.shelf_open = false;
        self.shelf_at = None;
        self.hang_open = false;
        self.hang_at = None;
        self.cut_open = false;
        self.fire_open = false;
        self.fletch_open = false;
    }

    /// Start a chat line. Another screen keeps the key.
    pub fn open_chest(&mut self, slots: &[rebuild_world::CarriedSlot]) -> bool {
        if self.is_open() {
            return false;
        }
        self.chest = hotbar::Bag::new();
        self.chest.install(slots);
        self.chest_selected = 0;
        self.chest_open = true;
        true
    }

    pub fn chest_open(&self) -> bool {
        self.chest_open
    }

    pub fn chest_wire(&self) -> Vec<(String, i32)> {
        self.chest.wire()
    }

    pub fn take_chest(&mut self) -> Option<&'static str> {
        self.chest.take_one(self.chest_selected)
    }

    pub fn give_chest(&mut self, id: &str) -> bool {
        self.chest.collect(id)
    }

    pub fn close_chest(&mut self) {
        self.chest_open = false;
    }

    /// The crafting grid. Other screens keep it closed.
    pub fn open_craft(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.craft = hotbar::Bag::new();
        self.craft_selected = 0;
        self.craft_open = true;
        true
    }

    pub fn craft_open(&self) -> bool {
        self.craft_open
    }

    /// One from the hotbar into the selected grid slot. The result slot refuses.
    pub fn give_craft(&mut self, id: &str) -> bool {
        if !self.craft_open || self.craft_selected >= 9 {
            return false;
        }
        self.craft.insert_at(self.craft_selected, id)
    }

    /// The book result for whatever is in the grid.
    pub fn craft_preview(&self) -> Option<String> {
        if !self.craft_open {
            return None;
        }
        let slots: Vec<String> = (0..9)
            .map(|index| self.craft.slot(index).unwrap_or("").to_string())
            .collect();
        self.book.match_slots(&slots)
    }

    /// Spend one of each ingredient and return the result id.
    pub fn pull_craft(&mut self) -> Option<String> {
        let result = self.craft_preview()?;
        let inputs = self.book.inputs(&result)?;
        for id in &inputs {
            let index = (0..9).find(|slot| self.craft.slot(*slot) == Some(id.as_str()))?;
            self.craft.take_one(index)?;
        }
        Some(result)
    }

    pub fn drain_craft(&mut self) -> Vec<&'static str> {
        self.craft.drain()
    }

    /// Player-inventory Craft tab input cells (`0..3`).
    pub const INV_CRAFT: usize = 4;

    pub fn inv_craft_slot(&self, index: usize) -> Option<&str> {
        if index >= Self::INV_CRAFT {
            return None;
        }
        self.inv_craft.slot(index)
    }

    pub fn inv_craft_count(&self, index: usize) -> u16 {
        if index >= Self::INV_CRAFT {
            return 0;
        }
        self.inv_craft.count(index)
    }

    /// Place one of `id` into a 2×2 input cell.
    pub fn give_inv_craft(&mut self, index: usize, id: &str) -> bool {
        if index >= Self::INV_CRAFT {
            return false;
        }
        self.inv_craft.insert_at(index, id)
    }

    /// Take one from a 2×2 input cell.
    pub fn take_inv_craft_slot(&mut self, index: usize) -> Option<&'static str> {
        if index >= Self::INV_CRAFT {
            return None;
        }
        self.inv_craft.take_one(index)
    }

    /// Book result for the 2×2 grid (top-left of a virtual 3×3).
    pub fn inv_craft_preview(&self) -> Option<String> {
        self.book.match_slots(&self.inv_craft_as_nine())
    }

    /// Spend one of each ingredient and return the result id.
    pub fn pull_inv_craft(&mut self) -> Option<String> {
        let result = self.inv_craft_preview()?;
        let inputs = self.book.inputs(&result)?;
        for id in &inputs {
            let index = (0..Self::INV_CRAFT)
                .find(|slot| self.inv_craft.slot(*slot) == Some(id.as_str()))?;
            self.inv_craft.take_one(index)?;
        }
        Some(result)
    }

    pub fn drain_inv_craft(&mut self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for index in 0..Self::INV_CRAFT {
            while let Some(id) = self.inv_craft.take_one(index) {
                out.push(id);
            }
        }
        out
    }

    fn inv_craft_as_nine(&self) -> Vec<String> {
        let mut slots = vec![String::new(); 9];
        for index in 0..Self::INV_CRAFT {
            let row = index / 2;
            let col = index % 2;
            if let Some(id) = self.inv_craft.slot(index) {
                slots[row * 3 + col] = id.to_string();
            }
        }
        slots
    }

    /// Input, fuel, and output. Other screens keep it closed.
    pub fn open_furnace(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.furnace = hotbar::Bag::new();
        self.furnace_selected = 0;
        self.furnace_cook = 0;
        self.furnace_time = 0.0;
        self.smoker = false;
        self.blast = false;
        self.furnace_open = true;
        true
    }

    /// The furnace screen, limited to food and a half-second cook.
    pub fn open_smoker(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.furnace = hotbar::Bag::new();
        self.furnace_selected = 0;
        self.furnace_cook = 0;
        self.furnace_time = 0.0;
        self.smoker = true;
        self.blast = false;
        self.furnace_open = true;
        true
    }

    /// The furnace screen, limited to ore and stone and a half-second cook.
    pub fn open_blast(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.furnace = hotbar::Bag::new();
        self.furnace_selected = 0;
        self.furnace_cook = 0;
        self.furnace_time = 0.0;
        self.smoker = false;
        self.blast = true;
        self.furnace_open = true;
        true
    }

    pub fn furnace_open(&self) -> bool {
        self.furnace_open
    }

    /// One from the hotbar into the input or the fuel slot.
    pub fn give_furnace(&mut self, id: &str) -> bool {
        if !self.furnace_open || self.furnace_selected > 1 {
            return false;
        }
        self.furnace.insert_at(self.furnace_selected, id)
    }

    pub fn give_furnace_back(&mut self, id: &str) {
        self.furnace.insert_at(2, id);
    }

    /// The item sitting in the output slot, still in the furnace.
    pub fn furnace_output(&self) -> Option<String> {
        if !self.furnace_open {
            return None;
        }
        self.furnace.slot(2).map(str::to_string)
    }

    pub fn take_furnace_output(&mut self) -> Option<&'static str> {
        if !self.furnace_open {
            return None;
        }
        self.furnace.take_one(2)
    }

    pub fn drain_furnace(&mut self) -> Vec<&'static str> {
        self.furnace_cook = 0;
        self.furnace_time = 0.0;
        self.furnace.drain()
    }

    /// A repair panel. Other screens keep it closed.
    pub fn open_anvil(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.anvil_note = "enter spends one iron ore".to_string();
        self.anvil_open = true;
        true
    }

    pub fn anvil_open(&self) -> bool {
        self.anvil_open
    }

    /// The word the last repair left on the panel.
    pub fn note_anvil(&mut self, note: &str) {
        self.anvil_note = note.to_string();
    }

    /// An enchant panel. Other screens keep it closed.
    pub fn open_enchanting(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.enchant_note = "enter spends one bookshelf".to_string();
        self.enchant_open = true;
        true
    }

    pub fn enchant_open(&self) -> bool {
        self.enchant_open
    }

    /// The word the last enchant left on the panel.
    pub fn note_enchant(&mut self, note: &str) {
        self.enchant_note = note.to_string();
    }

    /// A brew panel. Other screens keep it closed.
    pub fn open_brewing(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.brew_note = "enter spends water and nether wart".to_string();
        self.brew_open = true;
        true
    }

    pub fn brew_open(&self) -> bool {
        self.brew_open
    }

    /// The word the last brew left on the panel.
    pub fn note_brew(&mut self, note: &str) {
        self.brew_note = note.to_string();
    }

    /// A grind panel. Other screens keep it closed.
    pub fn open_grind(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.grind_note = "enter clears efficiency".to_string();
        self.grind_open = true;
        true
    }

    pub fn grind_open(&self) -> bool {
        self.grind_open
    }

    /// The word the last grind left on the panel.
    pub fn note_grind(&mut self, note: &str) {
        self.grind_note = note.to_string();
    }

    /// A smith panel. Other screens keep it closed.
    pub fn open_smith(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.smith_note = "enter spends one iron ore".to_string();
        self.smith_open = true;
        true
    }

    pub fn smith_open(&self) -> bool {
        self.smith_open
    }

    /// The word the last upgrade left on the panel.
    pub fn note_smith(&mut self, note: &str) {
        self.smith_note = note.to_string();
    }

    /// A loom panel. Other screens keep it closed.
    pub fn open_loom(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.loom_note = "enter spends one wool".to_string();
        self.loom_open = true;
        true
    }

    pub fn loom_open(&self) -> bool {
        self.loom_open
    }

    /// The word the last weave left on the panel.
    pub fn note_loom(&mut self, note: &str) {
        self.loom_note = note.to_string();
    }

    /// A chart panel. Other screens keep it closed.
    pub fn open_chart(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.chart_note = "enter widens the held map".to_string();
        self.chart_open = true;
        true
    }

    pub fn chart_open(&self) -> bool {
        self.chart_open
    }

    /// The word the last chart left on the panel.
    pub fn note_chart(&mut self, note: &str) {
        self.chart_note = note.to_string();
    }

    /// A lectern page. `has_book` shows the page. An empty stand asks for a book.
    pub fn open_lectern(&mut self, at: [i32; 3], has_book: bool) -> bool {
        if self.is_open() {
            return false;
        }
        self.lectern_at = Some(at);
        self.lectern_note = if has_book {
            LECTERN_PAGE.to_string()
        } else {
            "enter places a book".to_string()
        };
        self.lectern_open = true;
        true
    }

    pub fn lectern_open(&self) -> bool {
        self.lectern_open
    }

    pub fn lectern_at(&self) -> Option<[i32; 3]> {
        self.lectern_at
    }

    /// The word the last read left on the page.
    pub fn note_lectern(&mut self, note: &str) {
        self.lectern_note = note.to_string();
    }

    /// A composter. `level` of 3 or more asks to take bone meal.
    pub fn open_compost(&mut self, at: [i32; 3], level: u8) -> bool {
        if self.is_open() {
            return false;
        }
        self.compost_at = Some(at);
        self.compost_note = crate::hotbar::compost_prompt(level).to_string();
        self.compost_open = true;
        true
    }

    pub fn compost_open(&self) -> bool {
        self.compost_open
    }

    pub fn compost_at(&self) -> Option<[i32; 3]> {
        self.compost_at
    }

    /// The word the last fill left on the panel.
    pub fn note_compost(&mut self, note: &str) {
        self.compost_note = note.to_string();
    }

    /// A beehive. `level` of 3 or more asks to take honey.
    pub fn open_hive(&mut self, at: [i32; 3], level: u8) -> bool {
        if self.is_open() {
            return false;
        }
        self.hive_at = Some(at);
        self.hive_note = crate::hotbar::hive_prompt(level).to_string();
        self.hive_open = true;
        true
    }

    pub fn hive_open(&self) -> bool {
        self.hive_open
    }

    pub fn hive_at(&self) -> Option<[i32; 3]> {
        self.hive_at
    }

    /// The word the last fill left on the panel.
    pub fn note_hive(&mut self, note: &str) {
        self.hive_note = note.to_string();
    }

    /// A decorated pot. Enter stores one of the held item, or takes one back.
    pub fn open_pot(&mut self, at: [i32; 3], id: &str, count: u16) -> bool {
        if self.is_open() {
            return false;
        }
        self.pot_at = Some(at);
        self.set_pot(id, count);
        self.pot_note = pot_line(id, count);
        self.pot_open = true;
        true
    }

    pub fn pot_open(&self) -> bool {
        self.pot_open
    }

    pub fn pot_at(&self) -> Option<[i32; 3]> {
        self.pot_at
    }

    pub fn pot_stack(&self) -> (String, u16) {
        (self.pot_id.clone(), self.pot_count)
    }

    pub fn set_pot(&mut self, id: &str, count: u16) {
        self.pot_id = id.to_string();
        self.pot_count = count;
    }

    /// A hanging sign. The panel shows one fixed line.
    pub fn open_hang(&mut self, at: [i32; 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.hang_at = Some(at);
        self.hang_note = HANGING_LINE.to_string();
        self.hang_open = true;
        true
    }

    pub fn hang_open(&self) -> bool {
        self.hang_open
    }

    pub fn hang_at(&self) -> Option<[i32; 3]> {
        self.hang_at
    }

    /// The word the last read left on the sign.
    pub fn note_hang(&mut self, note: &str) {
        self.hang_note = note.to_string();
    }

    /// A chiseled bookshelf. Enter stores or takes one book. `count` is the spines now.
    pub fn open_shelf(&mut self, at: [i32; 3], count: u8) -> bool {
        if self.is_open() {
            return false;
        }
        self.shelf_at = Some(at);
        self.shelf_note = shelf_line(count);
        self.shelf_open = true;
        true
    }

    pub fn shelf_open(&self) -> bool {
        self.shelf_open
    }

    pub fn shelf_at(&self) -> Option<[i32; 3]> {
        self.shelf_at
    }

    /// The word the last book left on the panel.
    pub fn note_shelf(&mut self, note: &str) {
        self.shelf_note = note.to_string();
    }

    /// A wall shelf. Enter stores one held item into one of three stacks, or takes one back.
    pub fn open_board(&mut self, at: [i32; 3], slots: &[(String, u16); 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.board_at = Some(at);
        self.board_ids = [slots[0].0.clone(), slots[1].0.clone(), slots[2].0.clone()];
        self.board_counts = [slots[0].1, slots[1].1, slots[2].1];
        self.board_note = board_line(&self.board_ids, &self.board_counts);
        self.board_open = true;
        true
    }

    pub fn board_open(&self) -> bool {
        self.board_open
    }

    pub fn board_at(&self) -> Option<[i32; 3]> {
        self.board_at
    }

    pub fn board_slots(&self) -> [(String, u16); 3] {
        [
            (self.board_ids[0].clone(), self.board_counts[0]),
            (self.board_ids[1].clone(), self.board_counts[1]),
            (self.board_ids[2].clone(), self.board_counts[2]),
        ]
    }

    pub fn set_board_slot(&mut self, index: usize, id: &str, count: u16) {
        if index >= 3 {
            return;
        }
        self.board_ids[index] = id.to_string();
        self.board_counts[index] = count;
        self.board_note = board_line(&self.board_ids, &self.board_counts);
    }

    /// The word the last store left on the shelf.
    pub fn note_board(&mut self, note: &str) {
        self.board_note = note.to_string();
    }

    /// The word the last store left on the panel.
    pub fn note_pot(&mut self, note: &str) {
        self.pot_note = note.to_string();
    }

    /// A copper bulb. Enter lights it or puts it out. `lit` is the core now.
    pub fn open_bulb(&mut self, at: [i32; 3], lit: bool) -> bool {
        if self.is_open() {
            return false;
        }
        self.bulb_at = Some(at);
        self.bulb_note = bulb_prompt(lit).to_string();
        self.bulb_open = true;
        true
    }

    pub fn bulb_open(&self) -> bool {
        self.bulb_open
    }

    pub fn bulb_at(&self) -> Option<[i32; 3]> {
        self.bulb_at
    }

    /// The word the last toggle left on the panel.
    pub fn note_bulb(&mut self, note: &str) {
        self.bulb_note = note.to_string();
    }

    /// A target. Enter scores a hit at `at`.
    pub fn open_target(&mut self, at: [i32; 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.target_at = Some(at);
        self.target_note = "enter scores a hit".to_string();
        self.target_open = true;
        true
    }

    pub fn target_open(&self) -> bool {
        self.target_open
    }

    pub fn target_at(&self) -> Option<[i32; 3]> {
        self.target_at
    }

    /// The word the last hit left on the panel.
    pub fn note_target(&mut self, note: &str) {
        self.target_note = note.to_string();
    }

    /// A lightning rod. Enter calls a strike at `at`.
    pub fn open_rod(&mut self, at: [i32; 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.rod_at = Some(at);
        self.rod_note = "enter calls a strike".to_string();
        self.rod_open = true;
        true
    }

    pub fn rod_open(&self) -> bool {
        self.rod_open
    }

    pub fn rod_at(&self) -> Option<[i32; 3]> {
        self.rod_at
    }

    /// The word the last strike left on the panel.
    pub fn note_rod(&mut self, note: &str) {
        self.rod_note = note.to_string();
    }

    /// A respawn anchor. Enter adds glowstone or sets spawn. `level` is the charge.
    pub fn open_anchor(&mut self, at: [i32; 3], level: u8) -> bool {
        if self.is_open() {
            return false;
        }
        self.anchor_at = Some(at);
        self.anchor_note = crate::hotbar::anchor_prompt(level).to_string();
        self.anchor_open = true;
        true
    }

    pub fn anchor_open(&self) -> bool {
        self.anchor_open
    }

    pub fn anchor_at(&self) -> Option<[i32; 3]> {
        self.anchor_at
    }

    /// The word the last charge or spawn set left on the panel.
    pub fn note_anchor(&mut self, note: &str) {
        self.anchor_note = note.to_string();
    }

    /// A lodestone. Enter links a held compass to the block at `at`.
    pub fn open_lodestone(&mut self, at: [i32; 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.lodestone_at = Some(at);
        self.lodestone_note = "enter links a compass".to_string();
        self.lodestone_open = true;
        true
    }

    pub fn lodestone_open(&self) -> bool {
        self.lodestone_open
    }

    pub fn lodestone_at(&self) -> Option<[i32; 3]> {
        self.lodestone_at
    }

    /// The word the last link left on the panel.
    pub fn note_lodestone(&mut self, note: &str) {
        self.lodestone_note = note.to_string();
    }

    /// A candle. Enter lights it or puts it out. `lit` is the flame now.
    pub fn open_candle(&mut self, at: [i32; 3], lit: bool) -> bool {
        if self.is_open() {
            return false;
        }
        self.candle_at = Some(at);
        self.candle_note = candle_prompt(lit).to_string();
        self.candle_open = true;
        true
    }

    pub fn candle_open(&self) -> bool {
        self.candle_open
    }

    pub fn candle_at(&self) -> Option<[i32; 3]> {
        self.candle_at
    }

    /// The word the last toggle left on the panel.
    pub fn note_candle(&mut self, note: &str) {
        self.candle_note = note.to_string();
    }

    /// A bell. Enter rings the one at `at`.
    pub fn open_bell(&mut self, at: [i32; 3]) -> bool {
        if self.is_open() {
            return false;
        }
        self.bell_at = Some(at);
        self.bell_note = "enter rings the bell".to_string();
        self.bell_open = true;
        true
    }

    pub fn bell_open(&self) -> bool {
        self.bell_open
    }

    pub fn bell_at(&self) -> Option<[i32; 3]> {
        self.bell_at
    }

    /// The word the last ring left on the panel.
    pub fn note_bell(&mut self, note: &str) {
        self.bell_note = note.to_string();
    }

    /// A cauldron. `level` is how much water is in it, up to 3.
    pub fn open_cauldron(&mut self, at: [i32; 3], level: u8) -> bool {
        if self.is_open() {
            return false;
        }
        self.cauldron_at = Some(at);
        self.cauldron_note = crate::hotbar::cauldron_prompt(level).to_string();
        self.cauldron_open = true;
        true
    }

    pub fn cauldron_open(&self) -> bool {
        self.cauldron_open
    }

    pub fn cauldron_at(&self) -> Option<[i32; 3]> {
        self.cauldron_at
    }

    /// The word the last pour or scoop left on the panel.
    pub fn note_cauldron(&mut self, note: &str) {
        self.cauldron_note = note.to_string();
    }

    /// A stonecutter. Enter cuts the held stone.
    pub fn open_cut(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.cut_note = "enter cuts held stone".to_string();
        self.cut_open = true;
        true
    }

    pub fn cut_open(&self) -> bool {
        self.cut_open
    }

    /// The word the last cut left on the panel.
    pub fn note_cut(&mut self, note: &str) {
        self.cut_note = note.to_string();
    }

    /// A campfire. Enter heats raw food, then cooks it.
    pub fn open_fire(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.fire_note = "enter cooks raw food".to_string();
        self.fire_open = true;
        true
    }

    pub fn fire_open(&self) -> bool {
        self.fire_open
    }

    /// The word the last cook left on the panel.
    pub fn note_fire(&mut self, note: &str) {
        self.fire_note = note.to_string();
    }

    /// A fletching table. Enter makes an arrow from stick, wool, and gravel.
    pub fn open_fletch(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.fletch_note = "enter makes an arrow".to_string();
        self.fletch_open = true;
        true
    }

    pub fn fletch_open(&self) -> bool {
        self.fletch_open
    }

    /// The word the last fletch left on the panel.
    pub fn note_fletch(&mut self, note: &str) {
        self.fletch_note = note.to_string();
    }

    /// Advance the furnace while its screen is open. A furnace cook is one second.
    /// A smoker or blast cook is half a second. The smoker finishes raw food.
    /// The blast furnace finishes ore and stone.
    pub fn stoke(&mut self, dt: f32) {
        if !self.furnace_open || dt <= 0.0 {
            return;
        }
        self.furnace_time += dt;
        let steps = (self.furnace_time / 0.05) as u32;
        if steps == 0 {
            return;
        }
        self.furnace_time -= steps as f32 * 0.05;
        let goal = if self.smoker || self.blast { 10 } else { 20 };
        for _ in 0..steps.min(20) {
            let result = self.furnace.slot(0).and_then(|id| {
                if self.smoker {
                    self.book.smoke(id)
                } else if self.blast {
                    self.book.blast(id)
                } else {
                    self.book.smelt(id)
                }
            });
            let fueled = matches!(self.furnace.slot(1), Some("oak_log" | "coal_ore"));
            let room = match (self.furnace.slot(2), result.as_deref()) {
                (None, Some(_)) => true,
                (Some(have), Some(want)) => have == want && self.furnace.count(2) < 64,
                _ => false,
            };
            if result.is_some() && fueled && room {
                self.furnace_cook += 1;
                if self.furnace_cook >= goal {
                    let want = result.unwrap_or_default();
                    self.furnace.take_one(0);
                    self.furnace.take_one(1);
                    self.furnace.insert_at(2, &want);
                    self.furnace_cook = 0;
                }
            } else {
                self.furnace_cook = 0;
            }
        }
    }

    pub fn open_chat(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        self.chat_open = true;
        self.chat_draft.clear();
        true
    }

    pub fn chat_open(&self) -> bool {
        self.chat_open
    }

    /// Close chat and return the letters that were typed.
    pub fn take_chat(&mut self) -> Option<String> {
        if !self.chat_open {
            return None;
        }
        let line = rebuild_world::chat_line(&self.chat_draft);
        self.chat_draft.clear();
        self.chat_open = false;
        if line.is_empty() { None } else { Some(line) }
    }

    pub fn push_chat(&mut self, line: &str) {
        let line = rebuild_world::chat_line(line);
        if line.is_empty() {
            return;
        }
        self.chat_log.push((line, 8.0));
        if self.chat_log.len() > 4 {
            let extra = self.chat_log.len() - 4;
            self.chat_log.drain(0..extra);
        }
    }

    pub fn age_chat(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        for (_, left) in &mut self.chat_log {
            *left -= dt;
        }
        self.chat_log.retain(|(_, left)| *left > 0.0);
    }

    pub fn chat_quads(&self, width: f32, height: f32) -> Vec<Quad> {
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let mut lines: Vec<String> = self.chat_log.iter().map(|(line, _)| line.clone()).collect();
        if self.chat_open {
            if self.chat_draft.is_empty() {
                lines.push("say".to_string());
            } else {
                lines.push(self.chat_draft.clone());
            }
        }
        if lines.is_empty() {
            return Vec::new();
        }
        let mut quads = Vec::new();
        // Sit above the bottom-left status stack (hunger / health / armor).
        let bottom = status_armor_bottom() + HUD_STATUS_ROW;
        let row = 22.0;
        let panel_h = lines.len() as f32 * row + 12.0;
        let top = bottom + panel_h;
        quads.push(
            pixel_quad(width, height, 16.0, bottom, 320.0, top, PANEL).face(
                "stone",
                304.0 / 16.0,
                panel_h / 16.0,
            ),
        );
        for (index, line) in lines.iter().enumerate() {
            let y = top - 18.0 - index as f32 * row;
            draw_text(&mut quads, width, height, 28.0, y, line, INK, 1.0);
        }
        quads
    }

    /// Show registered commands. An empty list leaves the gui closed.
    pub fn show_commands(&mut self, rows: Vec<(String, String)>) -> bool {
        if rows.is_empty() {
            return false;
        }
        self.open = false;
        self.extra = None;
        self.bag_open = false;
        self.chat_open = false;
        self.commands = rows;
        self.command_selected = 0;
        self.command_open = true;
        true
    }

    pub fn command_open(&self) -> bool {
        self.command_open
    }

    pub fn selected_command(&self) -> Option<String> {
        self.commands
            .get(self.command_selected)
            .map(|(id, _)| id.clone())
    }

    /// Open, close, or move the selection. Other keys are ignored.
    pub fn on_press(&mut self, code: KeyCode) -> bool {
        if self.chest_open {
            return self.on_chest(code);
        }
        if self.craft_open {
            return self.on_craft(code);
        }
        if self.furnace_open {
            return self.on_furnace(code);
        }
        if self.anvil_open {
            return self.on_anvil(code);
        }
        if self.enchant_open {
            return self.on_enchant(code);
        }
        if self.brew_open {
            return self.on_brew(code);
        }
        if self.grind_open {
            return self.on_grind(code);
        }
        if self.smith_open {
            return self.on_smith(code);
        }
        if self.loom_open {
            return self.on_loom(code);
        }
        if self.chart_open {
            return self.on_chart(code);
        }
        if self.lectern_open {
            return self.on_lectern(code);
        }
        if self.compost_open {
            return self.on_compost(code);
        }
        if self.hive_open {
            return self.on_hive(code);
        }
        if self.bell_open {
            return self.on_bell(code);
        }
        if self.candle_open {
            return self.on_candle(code);
        }
        if self.lodestone_open {
            return self.on_lodestone(code);
        }
        if self.anchor_open {
            return self.on_anchor(code);
        }
        if self.rod_open {
            return self.on_rod(code);
        }
        if self.target_open {
            return self.on_target(code);
        }
        if self.bulb_open {
            return self.on_bulb(code);
        }
        if self.pot_open {
            return self.on_pot(code);
        }
        if self.board_open {
            return self.on_board(code);
        }
        if self.shelf_open {
            return self.on_shelf(code);
        }
        if self.hang_open {
            return self.on_hang(code);
        }
        if self.cauldron_open {
            return self.on_cauldron(code);
        }
        if self.cut_open {
            return self.on_cut(code);
        }
        if self.fire_open {
            return self.on_fire(code);
        }
        if self.fletch_open {
            return self.on_fletch(code);
        }
        if self.chat_open {
            return self.on_chat(code);
        }
        if self.command_open {
            return self.on_commands(code);
        }
        if self.built_at.is_some() {
            return self.on_built(code);
        }
        if self.bag_open {
            return self.on_bag(code);
        }
        if !self.open && code == KeyCode::KeyI {
            self.bag_open = true;
            self.bag_selected = 0;
            self.catalog_at = 0;
            self.catalog_tab = 0;
            self.catalog_query.clear();
            self.catalog_on = !catalog().is_empty();
            return true;
        }
        if !self.open {
            if self
                .screen
                .as_ref()
                .is_some_and(|screen| screen.open == code)
            {
                self.open = true;
                self.extra = None;
                self.selected = 0;
                self.inv_tab = PlayerInvTab::Player;
                return true;
            }
            if let Some(index) = self.extras.iter().position(|screen| screen.open == code) {
                self.open = true;
                self.extra = Some(index);
                self.selected = 0;
                return true;
            }
            if let Some(index) = self
                .built
                .iter()
                .position(|screen| screen_key(&screen.key) == Some(code))
            {
                self.built_at = Some(index);
                self.selected = self.built.get(index).map(first_built_row).unwrap_or(0);
                return true;
            }
            return false;
        }
        let (open_key, close_key) = self.active_keys();
        if code == KeyCode::ArrowUp || code == KeyCode::ArrowDown {
            let count = self.rows().len();
            if count > 0 && code == KeyCode::ArrowUp {
                self.selected = if self.selected == 0 {
                    count - 1
                } else {
                    self.selected - 1
                };
            }
            if count > 0 && code == KeyCode::ArrowDown {
                self.selected = (self.selected + 1) % count;
            }
            return true;
        }
        if code == open_key || code == close_key {
            self.open = false;
            self.extra = None;
            return true;
        }
        false
    }

    fn on_built(&mut self, code: KeyCode) -> bool {
        let Some(index) = self.built_at else {
            return false;
        };
        let count = self
            .built
            .get(index)
            .map(|screen| screen.elements.len())
            .unwrap_or(0);
        if code == KeyCode::ArrowUp || code == KeyCode::ArrowDown {
            if count > 0 {
                if let Some(screen) = self.built.get(index) {
                    self.selected =
                        step_built_row(screen, self.selected, code == KeyCode::ArrowDown);
                }
            }
            return true;
        }
        if self.field_selected() {
            if code == KeyCode::Backspace {
                self.edit_field(|text| {
                    text.pop();
                });
                return true;
            }
            if code == KeyCode::Space {
                self.edit_field(|text| {
                    if text.len() < 32 && !text.ends_with(' ') {
                        text.push(' ');
                    }
                });
                return true;
            }
            if let Some(ch) = letter_of(code) {
                self.edit_field(|text| {
                    if text.len() < 32 {
                        text.push(ch);
                    }
                });
                return true;
            }
        }
        if code == KeyCode::Enter {
            if let Some(name) = self.built.get(index).and_then(|screen| {
                screen.elements.get(self.selected).and_then(|row| {
                    if built_pressable(&row.font) {
                        Some(row.name.clone())
                    } else {
                        None
                    }
                })
            }) {
                self.pressed = Some(name);
            }
            return true;
        }
        let open_key = self
            .built
            .get(index)
            .and_then(|screen| screen_key(&screen.key));
        if code == KeyCode::Escape || open_key == Some(code) {
            self.built_at = None;
            self.pressed = None;
            return true;
        }
        true
    }

    pub fn built_open(&self) -> bool {
        self.built_at.is_some()
    }

    /// Open a GUIAPI screen by name. False when that screen was never registered.
    pub fn open_built(&mut self, name: &str) -> bool {
        let Some(index) = self.built.iter().position(|screen| screen.name == name) else {
            return false;
        };
        self.built_at = Some(index);
        self.selected = self.built.get(index).map(first_built_row).unwrap_or(0);
        self.pressed = None;
        true
    }

    /// The row Enter or a click just chose.
    pub fn take_press(&mut self) -> Option<String> {
        self.pressed.take()
    }

    /// True when that row is a typed field on the open screen.
    pub fn is_field(&self, name: &str) -> bool {
        let Some(index) = self.built_at else {
            return false;
        };
        self.built
            .get(index)
            .and_then(|screen| screen.elements.iter().find(|row| row.name == name))
            .is_some_and(|row| row.font == "field")
    }

    /// The words in a row, when that row is on the open screen.
    pub fn row_text(&self, name: &str) -> Option<String> {
        let index = self.built_at?;
        self.built
            .get(index)
            .and_then(|screen| screen.elements.iter().find(|row| row.name == name))
            .map(|row| row.text.clone())
    }

    fn field_selected(&self) -> bool {
        let Some(index) = self.built_at else {
            return false;
        };
        self.built
            .get(index)
            .and_then(|screen| screen.elements.get(self.selected))
            .is_some_and(|row| row.font == "field")
    }

    fn edit_field(&mut self, edit: impl FnOnce(&mut String)) {
        let Some(index) = self.built_at else {
            return;
        };
        let selected = self.selected;
        let Some(row) = self
            .built
            .get_mut(index)
            .and_then(|screen| screen.elements.get_mut(selected))
        else {
            return;
        };
        if row.font != "field" {
            return;
        }
        edit(&mut row.text);
        if row.text.len() > 32 {
            row.text.truncate(32);
        }
    }

    /// Copy screens a press just edited. The open panel draws the new rows.
    pub fn sync_built(&mut self, screens: &[rebuild_world::GuiScreen]) {
        for screen in screens {
            if let Some(slot) = self.built.iter_mut().find(|row| row.name == screen.name) {
                *slot = screen.clone();
            } else {
                self.built.push(screen.clone());
            }
        }
        let Some(index) = self.built_at else {
            return;
        };
        let count = self
            .built
            .get(index)
            .map(|screen| screen.elements.len())
            .unwrap_or(0);
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    fn on_bag(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || (code == KeyCode::KeyI && self.catalog_query.is_empty()) {
            self.bag_open = false;
            self.catalog_on = false;
            self.catalog_query.clear();
            self.catalog_tab = 0;
            return true;
        }
        if code == KeyCode::Enter {
            return true;
        }
        if code == KeyCode::Tab || code == KeyCode::BracketLeft {
            self.cycle_catalog_tab(code == KeyCode::Tab);
            return true;
        }
        if code == KeyCode::Backspace {
            self.catalog_query.pop();
            self.catalog_at = 0;
            self.catalog_on = true;
            return true;
        }
        if code == KeyCode::Space {
            if !self.catalog_query.is_empty()
                && !self.catalog_query.ends_with(' ')
                && self.catalog_query.len() < 24
            {
                self.catalog_query.push(' ');
                self.catalog_at = 0;
                self.catalog_on = true;
            }
            return true;
        }
        if let Some(ch) = letter_of(code) {
            if self.catalog_query.len() < 24 {
                self.catalog_query.push(ch);
            }
            self.catalog_at = 0;
            self.catalog_on = true;
            return true;
        }
        let count = creative_view(self.catalog_tab, &self.catalog_query).len();
        if self.catalog_on && count == 0 {
            if code == KeyCode::ArrowDown {
                self.catalog_on = false;
            }
            return matches!(
                code,
                KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown
            );
        }
        if self.catalog_on && count > 0 {
            let cols = CREATIVE_COLS;
            match code {
                KeyCode::ArrowLeft => {
                    self.catalog_at = if self.catalog_at == 0 {
                        count - 1
                    } else {
                        self.catalog_at - 1
                    };
                    true
                }
                KeyCode::ArrowRight => {
                    self.catalog_at = (self.catalog_at + 1) % count;
                    true
                }
                KeyCode::ArrowUp => {
                    if self.catalog_at >= cols {
                        self.catalog_at -= cols;
                    }
                    true
                }
                KeyCode::ArrowDown => {
                    if self.catalog_at + cols < count {
                        self.catalog_at += cols;
                    } else {
                        self.catalog_on = false;
                    }
                    true
                }
                _ => false,
            }
        } else {
            match code {
                KeyCode::ArrowLeft => {
                    self.bag_selected = if self.bag_selected == 0 {
                        hotbar::PACK - 1
                    } else {
                        self.bag_selected - 1
                    };
                    true
                }
                KeyCode::ArrowRight => {
                    self.bag_selected = (self.bag_selected + 1) % hotbar::PACK;
                    true
                }
                KeyCode::ArrowUp => {
                    if count > 0 {
                        self.catalog_on = true;
                    } else if self.bag_selected == 0 {
                        self.bag_selected = hotbar::PACK - 1;
                    } else {
                        self.bag_selected -= 1;
                    }
                    true
                }
                KeyCode::ArrowDown => {
                    self.bag_selected = (self.bag_selected + 1) % hotbar::PACK;
                    true
                }
                _ => false,
            }
        }
    }

    /// The block under the creative cursor. `None` when the carried row is selected.
    pub fn creative_pick(&self) -> Option<String> {
        if !self.creative_focused() {
            return None;
        }
        creative_view(self.catalog_tab, &self.catalog_query)
            .get(self.catalog_at)
            .cloned()
    }

    /// True while the creative grid has the cursor, even if the filter is empty.
    pub fn creative_focused(&self) -> bool {
        self.bag_open && self.catalog_on
    }

    fn on_chest(&mut self, code: KeyCode) -> bool {
        if let Some(next) = nudge_grid(self.chest_selected, CHEST_COLS, hotbar::PACK, code) {
            self.chest_selected = next;
            return true;
        }
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.chest_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_craft(&mut self, code: KeyCode) -> bool {
        if let Some(next) = nudge_craft(self.craft_selected, code) {
            self.craft_selected = next;
            return true;
        }
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.craft_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_anvil(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.anvil_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_enchant(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.enchant_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_brew(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.brew_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_grind(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.grind_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_smith(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.smith_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_loom(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.loom_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_chart(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.chart_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_lectern(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.lectern_open = false;
            self.lectern_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_compost(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.compost_open = false;
            self.compost_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_hive(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.hive_open = false;
            self.hive_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_bell(&mut self, code: KeyCode) -> bool {
        if !self.bell_open {
            return false;
        }
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.bell_open = false;
            self.bell_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_candle(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.candle_open = false;
            self.candle_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_lodestone(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.lodestone_open = false;
            self.lodestone_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_anchor(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.anchor_open = false;
            self.anchor_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_rod(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.rod_open = false;
            self.rod_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_target(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.target_open = false;
            self.target_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_bulb(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.bulb_open = false;
            self.bulb_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_hang(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.hang_open = false;
            self.hang_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_shelf(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.shelf_open = false;
            self.shelf_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_board(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.board_open = false;
            self.board_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_pot(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::Escape || code == KeyCode::KeyF {
            self.pot_open = false;
            self.pot_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_cauldron(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.cauldron_open = false;
            self.cauldron_at = None;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_cut(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.cut_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_fire(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.fire_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_fletch(&mut self, code: KeyCode) -> bool {
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.fletch_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_furnace(&mut self, code: KeyCode) -> bool {
        if let Some(next) = nudge_grid(self.furnace_selected, 3, 3, code) {
            self.furnace_selected = next;
            return true;
        }
        if code == KeyCode::KeyF || code == KeyCode::Escape {
            self.furnace_open = false;
            self.smoker = false;
            self.blast = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn on_chat(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Escape => {
                self.chat_open = false;
                self.chat_draft.clear();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {}
            KeyCode::Backspace => {
                self.chat_draft.pop();
            }
            KeyCode::Space => self.push_draft(' '),
            KeyCode::Slash => self.push_draft('/'),
            other => {
                if let Some(ch) = letter_of(other) {
                    self.push_draft(ch);
                }
            }
        }
        true
    }

    fn push_draft(&mut self, ch: char) {
        if self.chat_draft.len() >= 32 {
            return;
        }
        self.chat_draft.push(ch);
    }

    fn on_commands(&mut self, code: KeyCode) -> bool {
        let count = self.commands.len();
        if count > 0 && (code == KeyCode::ArrowUp || code == KeyCode::ArrowDown) {
            if code == KeyCode::ArrowUp {
                self.command_selected = if self.command_selected == 0 {
                    count - 1
                } else {
                    self.command_selected - 1
                };
            } else {
                self.command_selected = (self.command_selected + 1) % count;
            }
            return true;
        }
        if code == KeyCode::KeyC || code == KeyCode::Escape {
            self.command_open = false;
            return true;
        }
        code == KeyCode::Enter
    }

    fn active_keys(&self) -> (KeyCode, KeyCode) {
        if let Some(screen) = self.active_screen() {
            return (screen.open, screen.close);
        }
        (KeyCode::Escape, KeyCode::Escape)
    }

    fn active_screen(&self) -> Option<&Screen> {
        if let Some(index) = self.extra {
            return self.extras.get(index);
        }
        self.screen.as_ref()
    }

    /// The open items row, when it is a recipe result.
    pub fn craft_job(&self) -> Option<(String, Vec<String>)> {
        if !self.open || self.bag_open || self.command_open || self.extra.is_some() {
            return None;
        }
        let id = self.rows().get(self.selected)?.clone();
        let inputs = self.book.inputs(&id)?;
        if inputs.is_empty() {
            return None;
        }
        Some((id, inputs))
    }

    /// The id the open screen is showing, when it has a row.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn selected_id(&self) -> Option<String> {
        self.rows().get(self.selected).cloned()
    }

    /// Recipe or "used to make" line for the selected entry.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn selected_detail(&self) -> String {
        self.rows()
            .get(self.selected)
            .map(|id| self.book.detail(id))
            .unwrap_or_default()
    }

    fn rows(&self) -> Vec<String> {
        if let Some(index) = self.extra {
            if let Some(screen) = self.extras.get(index) {
                return screen.labels.clone();
            }
        }
        if !self.book.is_empty() {
            return self.book.entries();
        }
        self.screen
            .as_ref()
            .map(|screen| screen.labels.clone())
            .unwrap_or_default()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn quads(&self, width: f32, height: f32) -> Vec<Quad> {
        self.paint(width, height, &hotbar::Bag::new())
    }

    /// Like [`Self::quads`], using the carried inventory when that screen is open.
    pub fn paint(&self, width: f32, height: f32, bag: &hotbar::Bag) -> Vec<Quad> {
        if self.command_open {
            let rows: Vec<String> = self
                .commands
                .iter()
                .map(|(_, label)| label.clone())
                .collect();
            let selected = if rows.is_empty() {
                0
            } else {
                self.command_selected.min(rows.len() - 1)
            };
            let detail = command_detail(rows.get(selected).map(String::as_str).unwrap_or(""));
            return commands_screen().quads(width, height, &rows, selected, &detail);
        }
        if self.chest_open {
            return chest_quads(width, height, &self.chest, self.chest_selected);
        }
        if self.craft_open {
            return craft_quads(
                width,
                height,
                &self.craft,
                self.craft_selected,
                self.craft_preview(),
            );
        }
        if self.furnace_open {
            return furnace_quads(
                width,
                height,
                &self.furnace,
                self.furnace_selected,
                self.furnace_cook,
                self.smoker,
                self.blast,
            );
        }
        if self.anvil_open {
            return anvil_quads(width, height, &self.anvil_note);
        }
        if self.enchant_open {
            return enchant_quads(width, height, &self.enchant_note);
        }
        if self.brew_open {
            return brew_quads(width, height, &self.brew_note);
        }
        if self.grind_open {
            return grind_quads(width, height, &self.grind_note);
        }
        if self.smith_open {
            return smith_quads(width, height, &self.smith_note);
        }
        if self.loom_open {
            return loom_quads(width, height, &self.loom_note);
        }
        if self.chart_open {
            return chart_quads(width, height, &self.chart_note);
        }
        if self.lectern_open {
            return lectern_quads(width, height, &self.lectern_note);
        }
        if self.compost_open {
            return compost_quads(width, height, &self.compost_note);
        }
        if self.hive_open {
            return hive_quads(width, height, &self.hive_note);
        }
        if self.cauldron_open {
            return cauldron_quads(width, height, &self.cauldron_note);
        }
        if self.bell_open {
            return bell_quads(width, height, &self.bell_note);
        }
        if self.candle_open {
            return candle_quads(width, height, &self.candle_note);
        }
        if self.lodestone_open {
            return lodestone_quads(width, height, &self.lodestone_note);
        }
        if self.anchor_open {
            return anchor_quads(width, height, &self.anchor_note);
        }
        if self.rod_open {
            return rod_quads(width, height, &self.rod_note);
        }
        if self.target_open {
            return target_quads(width, height, &self.target_note);
        }
        if self.bulb_open {
            return bulb_quads(width, height, &self.bulb_note);
        }
        if self.pot_open {
            return pot_quads(width, height, &self.pot_note);
        }
        if self.board_open {
            return board_quads(width, height, &self.board_note);
        }
        if self.shelf_open {
            return shelf_quads(width, height, &self.shelf_note);
        }
        if self.hang_open {
            return hang_quads(width, height, &self.hang_note);
        }
        if self.cut_open {
            return cut_quads(width, height, &self.cut_note);
        }
        if self.fire_open {
            return fire_quads(width, height, &self.fire_note);
        }
        if self.fletch_open {
            return fletch_quads(width, height, &self.fletch_note);
        }
        if self.bag_open {
            return creative_quads(
                width,
                height,
                bag,
                self.catalog_on,
                self.catalog_at,
                self.catalog_tab,
                &self.catalog_query,
                self.bag_selected,
            );
        }
        if let Some(index) = self.built_at {
            if let Some(screen) = self.built.get(index) {
                return built_quads(width, height, screen, self.selected);
            }
        }
        if !self.open {
            return Vec::new();
        }
        let Some(screen) = self.active_screen() else {
            return Vec::new();
        };
        let rows = self.rows();
        let selected = if rows.is_empty() {
            0
        } else {
            self.selected.min(rows.len() - 1)
        };
        let detail = if self.extra.is_some() {
            String::new()
        } else {
            rows.get(selected)
                .filter(|_| !self.book.is_empty())
                .map(|id| self.book.detail(id))
                .unwrap_or_default()
        };
        screen.quads(width, height, &rows, selected, &detail)
    }

    /// Move the highlight onto the rectangle under the pointer.
    pub fn pointer_move(&mut self, hit: u16) {
        let _ = self.apply_hit(hit);
    }

    /// Click the rectangle under the pointer. Slots and commands act. A list row only selects.
    pub fn pointer_click(&mut self, hit: u16) -> Click {
        if !self.apply_hit(hit) && !(600..610).contains(&hit) {
            return Click::None;
        }
        if self.bag_open && hit >= 1000 {
            return Click::Give;
        }
        if self.bag_open && (200..209).contains(&hit) {
            return Click::TakeBag;
        }
        if self.chest_open && (300..309).contains(&hit) {
            return Click::TakeChest;
        }
        if self.craft_open && hit == 409 {
            return Click::TakeCraft;
        }
        if self.furnace_open && hit == 502 {
            return Click::TakeFurnace;
        }
        if self.command_open && (1..64).contains(&hit) {
            return Click::RunCommand;
        }
        if self.built_at.is_some() && (800..812).contains(&hit) {
            let row = (hit - 800) as usize;
            if let Some((name, font)) = self.built_at.and_then(|index| {
                self.built.get(index).and_then(|screen| {
                    screen
                        .elements
                        .get(row)
                        .map(|element| (element.name.clone(), element.font.clone()))
                })
            }) {
                if font == "field" {
                    self.selected = row;
                    return Click::None;
                }
                self.pressed = Some(name);
                return Click::Script;
            }
        }
        if (600..610).contains(&hit) {
            return Click::Hotbar((hit - 600) as usize);
        }
        Click::None
    }

    /// Wheel down moves toward later rows. Wheel up moves back.
    pub fn pointer_scroll(&mut self, down: bool) {
        if self.bag_open {
            let count = creative_view(self.catalog_tab, &self.catalog_query).len();
            if count > 0 {
                self.catalog_on = true;
                self.catalog_at = scroll_index(self.catalog_at, count, CREATIVE_COLS, down);
            }
            return;
        }
        if self.chest_open {
            self.chest_selected = scroll_index(self.chest_selected, hotbar::PACK, 1, down);
            return;
        }
        if self.craft_open {
            self.craft_selected = scroll_index(self.craft_selected, 10, 1, down);
            return;
        }
        if self.furnace_open {
            self.furnace_selected = scroll_index(self.furnace_selected, 3, 1, down);
            return;
        }
        if self.command_open {
            let count = self.commands.len();
            if count > 0 {
                self.command_selected = scroll_index(self.command_selected, count, 1, down);
            }
            return;
        }
        if let Some(index) = self.built_at {
            let count = self
                .built
                .get(index)
                .map(|screen| screen.elements.len())
                .unwrap_or(0);
            if count > 0 {
                self.selected = scroll_index(self.selected, count, 1, down);
            }
            return;
        }
        if self.open {
            let count = self.rows().len();
            if count > 0 {
                self.selected = scroll_index(self.selected, count, 1, down);
            }
        }
    }

    fn apply_hit(&mut self, hit: u16) -> bool {
        if hit == 0 {
            return false;
        }
        if self.chest_open && (300..309).contains(&hit) {
            self.chest_selected = (hit - 300) as usize;
            return true;
        }
        if self.craft_open && (400..410).contains(&hit) {
            self.craft_selected = (hit - 400) as usize;
            return true;
        }
        if self.furnace_open && (500..503).contains(&hit) {
            self.furnace_selected = (hit - 500) as usize;
            return true;
        }
        if self.bag_open && (100..110).contains(&hit) {
            let tab = (hit - 100) as usize;
            if tab < TABS.len() {
                self.catalog_tab = tab;
                self.catalog_at = 0;
                self.catalog_on = true;
                return true;
            }
        }
        if self.bag_open && hit >= 1000 {
            let index = (hit - 1000) as usize;
            let count = creative_view(self.catalog_tab, &self.catalog_query).len();
            if index < count {
                self.catalog_at = index;
                self.catalog_on = true;
                return true;
            }
        }
        if self.bag_open && (200..209).contains(&hit) {
            self.bag_selected = (hit - 200) as usize;
            self.catalog_on = false;
            return true;
        }
        if self.command_open && (1..64).contains(&hit) {
            let index = (hit - 1) as usize;
            if index < self.commands.len() {
                self.command_selected = index;
                return true;
            }
        }
        if self.built_at.is_some() && (800..812).contains(&hit) {
            let index = (hit - 800) as usize;
            let count = self
                .built_at
                .and_then(|at| self.built.get(at))
                .map(|screen| screen.elements.len())
                .unwrap_or(0);
            if index < count {
                self.selected = index;
                return true;
            }
        }
        if self.open && (1..64).contains(&hit) {
            let index = (hit - 1) as usize;
            if index < self.rows().len() {
                self.selected = index;
                return true;
            }
        }
        (600..610).contains(&hit)
    }
}

/// Name and one short line for the slot under the pointer.
/// Item rarity for the tooltip chip. Missing pack data defaults to [`Common`](Self::Common).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Uncommon+ reserved until pack rarity lands.
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Epic => "Epic",
            Self::Legendary => "Legendary",
        }
    }

    /// RGB fill behind the rarity chip (GUI plan: colored background).
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Common => [77, 79, 92],
            Self::Uncommon => [55, 120, 70],
            Self::Rare => [55, 95, 160],
            Self::Epic => [120, 60, 150],
            Self::Legendary => [184, 133, 31],
        }
    }
}

/// Rich hover tip for inventory / creative / EMI (GUI plan L9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemTip {
    pub name: String,
    /// Short legacy line (same text as [`Self::description`] for old callers).
    pub line: String,
    pub rarity: Rarity,
    pub description: String,
    /// Tool / armour lines; empty for plain stacks.
    pub stats: Vec<String>,
    /// Enchant name + short description pairs.
    pub enchants: Vec<(String, String)>,
}

impl ItemTip {
    /// Name + one line, Common rarity, no stats or enchants (empty slot stubs).
    pub fn plain(name: impl Into<String>, line: impl Into<String>) -> Self {
        let line = line.into();
        Self {
            name: name.into(),
            rarity: Rarity::Common,
            description: line.clone(),
            line,
            stats: Vec::new(),
            enchants: Vec::new(),
        }
    }
}

/// The item under `hit`, when that rectangle is a hotbar slot, a creative cell,
/// a carried inventory slot, or an items-screen row.
pub fn hover_tip(gui: &Gui, hotbar: &Hotbar, bag: &hotbar::Bag, hit: u16) -> Option<ItemTip> {
    if hit == 0 || gui.command_open {
        return None;
    }
    if (600..610).contains(&hit) {
        let index = (hit - 600) as usize;
        return hotbar.slot(index).map(|id| {
            item_tip(id, Some(hotbar.count(index)), hotbar.is_keen(index))
        });
    }
    if gui.bag_open && (200..209).contains(&hit) {
        let index = (hit - 200) as usize;
        return bag
            .slot(index)
            .map(|id| item_tip(id, Some(bag.count(index)), false));
    }
    if gui.bag_open && hit >= 1000 {
        let index = (hit - 1000) as usize;
        return creative_view(gui.catalog_tab, &gui.catalog_query)
            .get(index)
            .map(|id| item_tip(id, None, false));
    }
    if gui.open && !gui.bag_open && (1..64).contains(&hit) {
        let index = (hit - 1) as usize;
        return gui.rows().get(index).map(|id| item_tip(id, None, false));
    }
    None
}

/// Readable name plus a short use line for egui tooltips and legacy hover tips.
pub fn tip_for(id: &str, count: Option<u16>) -> ItemTip {
    item_tip(id, count, false)
}

/// Same as [`tip_for`], with Efficiency when `keen` (hotbar enchanted pick).
pub fn tip_for_keen(id: &str, count: Option<u16>, keen: bool) -> ItemTip {
    item_tip(id, count, keen)
}

fn item_tip(id: &str, count: Option<u16>, keen: bool) -> ItemTip {
    let name = title_case(&crate::recipes::display_name(id));
    let pack = rebuild_world::Pack::builtin();
    let line = if pack.food_of(id).is_some() {
        "fills hunger".to_string()
    } else if pack.armor_of(id).is_some() {
        "soaks heart damage".to_string()
    } else if pack.tool_uses(id) > 0 {
        "wears down with use".to_string()
    } else if pack.contains_block(id) {
        "place on a face".to_string()
    } else {
        match count {
            Some(1) => "one held".to_string(),
            Some(64) => "full stack".to_string(),
            Some(_) => "a stack".to_string(),
            None => "crafting material".to_string(),
        }
    };
    let description = sentence(&line);
    let mut stats = Vec::new();
    if let Some((slot, soak)) = pack.armor_of(id) {
        let piece = match slot {
            0 => "Head",
            1 => "Chest",
            2 => "Feet",
            _ => "Armor",
        };
        stats.push(format!("{piece} armor"));
        stats.push(format!("Soak {soak}"));
    }
    let max_uses = pack.tool_uses(id);
    if max_uses > 0 {
        let left = count.unwrap_or(max_uses);
        stats.push(format!("Uses {left}/{max_uses}"));
        let factor = pack.mine_factor(id, "stone");
        if factor > 1.0 {
            stats.push(format!("Mine ×{factor}"));
        }
    }
    let mut enchants = Vec::new();
    if keen {
        enchants.push((
            "Efficiency".to_string(),
            "Mines twice as fast".to_string(),
        ));
    }
    ItemTip {
        name,
        line: description.clone(),
        rarity: Rarity::Common,
        description,
        stats,
        enchants,
    }
}

/// A dark label beside the pointer. `x` and `y` use window coordinates, `y` downward.
pub fn tooltip_quads(width: f32, height: f32, x: f32, y: f32, name: &str, line: &str) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 || name.is_empty() {
        return Vec::new();
    }
    let name_w = measure(name, 1.0);
    let line_w = measure(line, 1.0);
    let name_h = block_height(name, 1.0);
    let line_h = block_height(line, 1.0);
    let box_w = name_w.max(line_w) + 16.0;
    let box_h = 8.0 + name_h + 4.0 + line_h + 8.0;
    let y = height - y;
    let mut left = x + 14.0;
    if left + box_w > width - 8.0 {
        left = (x - 14.0 - box_w).max(8.0);
    }
    let mut bottom = y + 12.0;
    if bottom + box_h > height - 8.0 {
        bottom = (y - 12.0 - box_h).max(8.0);
    }
    let mut quads = vec![
        pixel_quad(
            width,
            height,
            left - 2.0,
            bottom - 2.0,
            left + box_w + 2.0,
            bottom + box_h + 2.0,
            FRAME,
        ),
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + box_w,
            bottom + box_h,
            [0.07, 0.08, 0.11],
        ),
    ];
    draw_text(
        &mut quads,
        width,
        height,
        left + 8.0,
        bottom + 8.0 + line_h + 4.0,
        name,
        INK,
        1.0,
    );
    if !line.is_empty() {
        draw_text(
            &mut quads,
            width,
            height,
            left + 8.0,
            bottom + 8.0,
            line,
            DETAIL,
            1.0,
        );
    }
    quads
}

/// What a pointer click asked the screen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    None,
    Give,
    TakeBag,
    TakeChest,
    TakeCraft,
    TakeFurnace,
    RunCommand,
    Hotbar(usize),
    /// Enter or a click on a GUIAPI row. The row name is in [`Gui::take_press`].
    Script,
}

fn scroll_index(index: usize, count: usize, step: usize, down: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if down {
        (index + step).min(count - 1)
    } else {
        index.saturating_sub(step)
    }
}

/// The topmost tagged rectangle under a window point. `y` grows downward.
pub fn quad_hit(quads: &[hotbar::Quad], x: f32, y: f32, width: f32, height: f32) -> u16 {
    if width <= 0.0 || height <= 0.0 {
        return 0;
    }
    let y = height - y;
    let nx = x / width * 2.0 - 1.0;
    let ny = y / height * 2.0 - 1.0;
    for quad in quads.iter().rev() {
        if quad.hit == 0 {
            continue;
        }
        if nx >= quad.min[0] && nx <= quad.max[0] && ny >= quad.min[1] && ny <= quad.max[1] {
            return quad.hit;
        }
    }
    0
}

/// A small cross at the pointer while a screen has the mouse.
pub fn cursor_quads(width: f32, height: f32, x: f32, y: f32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let y = height - y;
    let edge = [0.08, 0.08, 0.10];
    let ink = [0.96, 0.96, 0.94];
    vec![
        pixel_quad(width, height, x - 1.0, y - 9.0, x + 2.0, y + 9.0, edge),
        pixel_quad(width, height, x - 7.0, y - 1.0, x + 9.0, y + 2.0, edge),
        pixel_quad(width, height, x, y - 8.0, x + 1.0, y + 8.0, ink),
        pixel_quad(width, height, x - 6.0, y, x + 8.0, y + 1.0, ink),
    ]
}

/// What the main menu did with the key it just took.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuAction {
    None,
    /// Close the menu and play the chosen world.
    Play,
    /// Close pause and keep playing.
    Resume,
    /// Leave the world for the title menu.
    Title,
    /// Mute flipped. The game plays a blip when sound turns on.
    Volume,
    /// The window should close.
    Quit,
    /// Try localhost. The menu stays up when nothing is listening.
    Join,
    /// The settings world size changed. A joined server should grow or shrink.
    Resize,
    /// A script settings row flipped. The name is on the menu until `take_changed`.
    Setting,
}

/// How an egui settings row draws its control (toggle vs discrete slider).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsControl {
    Toggle {
        on: bool,
        on_label: &'static str,
        off_label: &'static str,
    },
    Slider {
        index: usize,
        choices: Vec<String>,
    },
    /// Opens another screen (keybinds).
    Link {
        caption: &'static str,
    },
}

/// One left-dock category in the Godot-style settings screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsCategory {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuPage {
    Main,
    Settings,
    Keybinds,
    Worlds,
    Pause,
    Closed,
}

/// Play, settings, and quit. It starts open.
pub struct Menu {
    main: Screen,
    settings: Screen,
    pause: Screen,
    worlds_screen: Screen,
    page: MenuPage,
    /// Settings returns here.
    settings_return: MenuPage,
    /// Keybinds returns here (pause or settings).
    keybinds_return: MenuPage,
    selected: usize,
    size: rebuild_world::WorldSize,
    muted: bool,
    /// A dedicated server already chose the square. The row shows it and does not cycle.
    size_locked: bool,
    hemisphere: crate::daylight::Hemisphere,
    bob: crate::camera::Bob,
    /// Soft sun and moon from the ray grid. Off keeps the raster shadow map.
    trace: bool,
    world_name: String,
    worlds: Vec<String>,
    /// Shown on the join row after a refused connection.
    join_note: String,
    /// The script row just flipped, if this press did that.
    changed: Option<String>,
    /// Selected left-dock category on the settings screen.
    settings_category: usize,
    /// Inspector filter text (Godot-style search).
    settings_query: String,
}

impl Menu {
    /// Load the shipped menu. A missing file still leaves a play / settings / quit list.
    pub fn new() -> Self {
        Self {
            main: load_or("menu", &menu_file(), FALLBACK_MENU),
            settings: load_or("settings", &settings_file(), FALLBACK_SETTINGS),
            pause: load_or("pause", &pause_file(), FALLBACK_PAUSE),
            worlds_screen: load_or("worlds", &worlds_file(), FALLBACK_WORLDS),
            page: MenuPage::Main,
            settings_return: MenuPage::Main,
            keybinds_return: MenuPage::Main,
            selected: 0,
            size: rebuild_world::WorldSize::Small,
            muted: false,
            size_locked: false,
            hemisphere: crate::daylight::Hemisphere::South,
            bob: crate::camera::Bob::Full,
            trace: true,
            world_name: crate::net::DEFAULT_WORLD.to_string(),
            worlds: crate::net::list_worlds(),
            join_note: String::new(),
            changed: None,
            settings_category: 0,
            settings_query: String::new(),
        }
    }

    /// No menu. Tests of the items screen use this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn closed() -> Self {
        Self {
            main: Screen::parse(FALLBACK_MENU).expect("fallback menu"),
            settings: Screen::parse(FALLBACK_SETTINGS).expect("fallback settings"),
            pause: Screen::parse(FALLBACK_PAUSE).expect("fallback pause"),
            worlds_screen: Screen::parse(FALLBACK_WORLDS).expect("fallback worlds"),
            page: MenuPage::Closed,
            settings_return: MenuPage::Main,
            keybinds_return: MenuPage::Main,
            selected: 0,
            size: rebuild_world::WorldSize::Small,
            muted: false,
            size_locked: false,
            hemisphere: crate::daylight::Hemisphere::South,
            bob: crate::camera::Bob::Full,
            trace: true,
            world_name: crate::net::DEFAULT_WORLD.to_string(),
            worlds: vec![crate::net::DEFAULT_WORLD.to_string()],
            join_note: String::new(),
            changed: None,
            settings_category: 0,
            settings_query: String::new(),
        }
    }

    /// The script settings row this press flipped.
    pub fn take_changed(&mut self) -> Option<String> {
        self.changed.take()
    }

    /// Leave the title or pause menu.
    pub fn close_menu(&mut self) {
        self.page = MenuPage::Closed;
    }

    pub fn note_join(&mut self, text: &str) {
        self.join_note = text.to_string();
    }

    pub fn world_size(&self) -> rebuild_world::WorldSize {
        self.size
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn size_locked(&self) -> bool {
        self.size_locked
    }

    pub fn settings_category(&self) -> usize {
        self.settings_category
    }

    pub fn set_settings_category(&mut self, index: usize) {
        self.settings_category = index;
    }

    pub fn settings_query(&self) -> &str {
        &self.settings_query
    }

    pub fn set_settings_query(&mut self, query: &str) {
        self.settings_query = query.to_string();
    }

    /// Row index in [`Menu::menu_rows`] for a settings option label.
    pub fn settings_row_index(&self, label: &str) -> Option<usize> {
        self.settings_rows()
            .iter()
            .position(|row| row == label)
    }

    /// Left-dock categories: built-in World / Video / Audio, then mod groups.
    pub fn settings_categories(&self) -> Vec<SettingsCategory> {
        let mut cats = vec![
            SettingsCategory {
                id: "world".into(),
                title: "World".into(),
                detail: "size and sun path".into(),
                options: vec!["world".into(), "hemisphere".into()],
            },
            SettingsCategory {
                id: "video".into(),
                title: "Video".into(),
                detail: "bob and shadows".into(),
                options: vec!["view_bob".into(), "trace".into()],
            },
            SettingsCategory {
                id: "audio".into(),
                title: "Audio".into(),
                detail: "master sound".into(),
                options: vec!["volume".into()],
            },
            SettingsCategory {
                id: "controls".into(),
                title: "Controls".into(),
                detail: "keys and mouse".into(),
                options: vec!["keybinds".into()],
            },
        ];
        let options = rebuild_world::Pack::builtin().settings();
        let mut seen = Vec::new();
        for option in &options {
            if seen.iter().any(|id| id == &option.category) {
                continue;
            }
            seen.push(option.category.clone());
            let rows: Vec<String> = options
                .iter()
                .filter(|row| row.category == option.category)
                .map(|row| row.name.clone())
                .collect();
            cats.push(SettingsCategory {
                id: option.category.clone(),
                title: title_case_label(&option.category),
                detail: option.category_text.clone(),
                options: rows,
            });
        }
        cats
    }

    pub fn hemisphere(&self) -> crate::daylight::Hemisphere {
        self.hemisphere
    }

    pub fn bob(&self) -> crate::camera::Bob {
        self.bob
    }

    pub fn trace(&self) -> bool {
        self.trace
    }

    pub fn world_name(&self) -> &str {
        &self.world_name
    }

    /// Show `size` and stop the world row from cycling. Used when a server is connected.
    pub fn lock_world_size(&mut self, size: rebuild_world::WorldSize) {
        self.size = size;
        self.size_locked = true;
    }

    pub fn is_open(&self) -> bool {
        self.page != MenuPage::Closed
    }

    pub fn is_settings(&self) -> bool {
        self.page == MenuPage::Settings
    }

    /// True when `label` is a mod SettingsAPI category heading, not an option.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_settings_heading(&self, label: &str) -> bool {
        self.category_heading(label)
    }

    /// Short muted line under a settings option (no current value).
    pub fn settings_hint(&self, label: &str) -> String {
        match label {
            "world" if self.size_locked => format!(
                "server lock, {} by {} chunks",
                self.size.side(),
                self.size.side()
            ),
            "world" => format!("{} by {} chunks", self.size.side(), self.size.side()),
            "hemisphere" => "sun path across the sky".to_string(),
            "volume" => "master sound".to_string(),
            "view_bob" => "walk motion".to_string(),
            "trace" => "sun shadows".to_string(),
            "keybinds" => "click a row, then press a key".to_string(),
            "back" => "return to the menu".to_string(),
            _ => rebuild_world::Pack::builtin()
                .settings()
                .into_iter()
                .find(|option| option.name == label)
                .map(|option| option.description)
                .unwrap_or_default(),
        }
    }

    /// Current cycleable value shown on the settings value chip.
    pub fn settings_value(&self, label: &str) -> String {
        match label {
            "world" => self.size.label().to_string(),
            "hemisphere" => self.hemisphere.label().to_string(),
            "volume" if self.muted => "muted".to_string(),
            "volume" => "on".to_string(),
            "view_bob" => self.bob.label().to_string(),
            "trace" if self.trace => "soft".to_string(),
            "trace" => "raster".to_string(),
            _ => rebuild_world::Pack::builtin()
                .settings()
                .into_iter()
                .find(|option| option.name == label)
                .map(|option| option.value)
                .unwrap_or_default(),
        }
    }

    /// How the egui settings row should draw this option.
    pub fn settings_control(&self, label: &str) -> SettingsControl {
        match label {
            "volume" => SettingsControl::Toggle {
                on: !self.muted,
                on_label: "on",
                off_label: "muted",
            },
            "trace" => SettingsControl::Toggle {
                on: self.trace,
                on_label: "soft",
                off_label: "raster",
            },
            "hemisphere" => SettingsControl::Toggle {
                on: matches!(self.hemisphere, crate::daylight::Hemisphere::North),
                on_label: "north",
                off_label: "south",
            },
            "world" => SettingsControl::Slider {
                index: match self.size {
                    rebuild_world::WorldSize::Small => 0,
                    rebuild_world::WorldSize::Medium => 1,
                    rebuild_world::WorldSize::Large => 2,
                    rebuild_world::WorldSize::Mega => 3,
                },
                choices: vec![
                    "small".into(),
                    "medium".into(),
                    "large".into(),
                    "mega".into(),
                ],
            },
            "view_bob" => SettingsControl::Slider {
                index: match self.bob {
                    crate::camera::Bob::Off => 0,
                    crate::camera::Bob::Low => 1,
                    crate::camera::Bob::Full => 2,
                },
                choices: vec!["off".into(), "low".into(), "full".into()],
            },
            "keybinds" => SettingsControl::Link {
                caption: "Open…",
            },
            _ => {
                let Some(option) = rebuild_world::Pack::builtin()
                    .settings()
                    .into_iter()
                    .find(|option| option.name == label)
                else {
                    return SettingsControl::Toggle {
                        on: false,
                        on_label: "on",
                        off_label: "off",
                    };
                };
                if !option.choices.is_empty() {
                    let index = option
                        .choices
                        .iter()
                        .position(|word| word == &option.value)
                        .unwrap_or(0);
                    SettingsControl::Slider {
                        index,
                        choices: option.choices,
                    }
                } else if option.value == "on" || option.value == "off" {
                    SettingsControl::Toggle {
                        on: option.value == "on",
                        on_label: "on",
                        off_label: "off",
                    }
                } else {
                    SettingsControl::Slider {
                        index: 0,
                        choices: vec![option.value],
                    }
                }
            }
        }
    }

    /// Apply a toggle from the egui settings switch.
    pub fn set_settings_toggle(&mut self, index: usize, on: bool) -> MenuAction {
        self.selected = index;
        let label = self.rows().get(self.selected).cloned().unwrap_or_default();
        match label.as_str() {
            "volume" => {
                let want_muted = !on;
                if self.muted == want_muted {
                    return MenuAction::None;
                }
                self.muted = want_muted;
                MenuAction::Volume
            }
            "trace" => {
                if self.trace == on {
                    return MenuAction::None;
                }
                self.trace = on;
                MenuAction::None
            }
            "hemisphere" => {
                let next = if on {
                    crate::daylight::Hemisphere::North
                } else {
                    crate::daylight::Hemisphere::South
                };
                if self.hemisphere == next {
                    return MenuAction::None;
                }
                self.hemisphere = next;
                MenuAction::None
            }
            _ => {
                let value = if on { "on" } else { "off" };
                if rebuild_world::Pack::builtin().set_setting_value(&label, value) {
                    self.changed = Some(label);
                    MenuAction::Setting
                } else {
                    MenuAction::None
                }
            }
        }
    }

    /// Apply a discrete slider / choice index from the egui settings row.
    pub fn set_settings_choice(&mut self, index: usize, choice: usize) -> MenuAction {
        self.selected = index;
        let label = self.rows().get(self.selected).cloned().unwrap_or_default();
        match label.as_str() {
            "world" => {
                let next = match choice {
                    1 => rebuild_world::WorldSize::Medium,
                    2 => rebuild_world::WorldSize::Large,
                    3 => rebuild_world::WorldSize::Mega,
                    _ => rebuild_world::WorldSize::Small,
                };
                if self.size == next {
                    return MenuAction::None;
                }
                self.size = next;
                MenuAction::Resize
            }
            "view_bob" => {
                let next = match choice {
                    1 => crate::camera::Bob::Low,
                    2 => crate::camera::Bob::Full,
                    _ => crate::camera::Bob::Off,
                };
                if self.bob == next {
                    return MenuAction::None;
                }
                self.bob = next;
                MenuAction::None
            }
            _ => {
                let Some(value) = rebuild_world::Pack::builtin()
                    .settings()
                    .into_iter()
                    .find(|option| option.name == label)
                    .and_then(|option| option.choices.get(choice).cloned())
                else {
                    return MenuAction::None;
                };
                if rebuild_world::Pack::builtin().set_setting_value(&label, &value) {
                    self.changed = Some(label);
                    MenuAction::Setting
                } else {
                    MenuAction::None
                }
            }
        }
    }

    /// Step a settings row forward or back (← → and the value chips).
    pub fn step_setting(&mut self, index: usize, forward: bool) -> MenuAction {
        self.selected = index;
        if forward {
            return self.activate();
        }
        let label = self.rows().get(self.selected).cloned().unwrap_or_default();
        match self.page {
            MenuPage::Settings if label == "world" => {
                // Four sizes: one step back is three steps forward.
                self.size = self.size.cycle().cycle().cycle();
                MenuAction::Resize
            }
            MenuPage::Settings if label == "hemisphere" => {
                self.hemisphere = self.hemisphere.cycle();
                MenuAction::None
            }
            MenuPage::Settings if label == "volume" => {
                self.muted = !self.muted;
                MenuAction::Volume
            }
            MenuPage::Settings if label == "view_bob" => {
                // Three bob modes: back is two steps forward.
                self.bob = self.bob.cycle().cycle();
                MenuAction::None
            }
            MenuPage::Settings if label == "trace" => {
                self.trace = !self.trace;
                MenuAction::None
            }
            MenuPage::Settings if self.category_heading(&label) => MenuAction::None,
            MenuPage::Settings => {
                let steps = rebuild_world::Pack::builtin()
                    .settings()
                    .into_iter()
                    .find(|option| option.name == label)
                    .map(|option| {
                        if option.choices.is_empty() {
                            1usize
                        } else {
                            option.choices.len().saturating_sub(1).max(1)
                        }
                    })
                    .unwrap_or(0);
                if steps == 0 {
                    return MenuAction::None;
                }
                let mut ok = false;
                for _ in 0..steps {
                    ok = rebuild_world::Pack::builtin().toggle_setting(&label) || ok;
                }
                if ok {
                    self.changed = Some(label);
                    MenuAction::Setting
                } else {
                    MenuAction::None
                }
            }
            _ => MenuAction::None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_pause(&self) -> bool {
        self.page == MenuPage::Pause
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_worlds(&self) -> bool {
        self.page == MenuPage::Worlds
    }

    pub fn open(&mut self) {
        self.page = MenuPage::Main;
        self.selected = 0;
    }

    pub fn open_pause(&mut self) {
        self.page = MenuPage::Pause;
        self.selected = 0;
    }

    pub fn page_title(&self) -> &'static str {
        match self.page {
            MenuPage::Main => "TERRALITE",
            MenuPage::Settings => "SETTINGS",
            MenuPage::Keybinds => "KEYBINDS",
            MenuPage::Worlds => "WORLDS",
            MenuPage::Pause => "PAUSED",
            MenuPage::Closed => "",
        }
    }

    pub fn menu_rows(&self) -> Vec<String> {
        self.rows()
    }

    pub fn selected_row(&self) -> usize {
        self.selected
    }

    pub fn row_detail(&self, label: &str) -> String {
        match label {
            "play" => format!("solo {}, {}", self.size.label(), self.world_name),
            "join" if self.size_locked => "already on the server".to_string(),
            "join" if !self.join_note.is_empty() => self.join_note.clone(),
            "join" => "localhost".to_string(),
            "settings" => "world size, sun, sound, bob, and trace".to_string(),
            "keybinds" => "move, jump, sneak, inventory, mouse…".to_string(),
            "worlds" => "load or start a named save".to_string(),
            "quit" => "leave the game".to_string(),
            "resume" => "return to the world".to_string(),
            "title" => "quit to the menu".to_string(),
            "world" if self.size_locked => format!(
                "server {}, {} by {} chunks",
                self.size.label(),
                self.size.side(),
                self.size.side()
            ),
            "world" => format!(
                "{}, {} by {} chunks",
                self.size.label(),
                self.size.side(),
                self.size.side()
            ),
            "hemisphere" => format!("{} sun", self.hemisphere.label()),
            "volume" if self.muted => "muted".to_string(),
            "volume" => "sound on".to_string(),
            "view_bob" => format!("bob {}", self.bob.label()),
            "trace" if self.trace => "soft sun".to_string(),
            "trace" => "raster sun".to_string(),
            "back" => "return to the menu".to_string(),
            "new" => "start a blank save".to_string(),
            _ if self.page == MenuPage::Settings && self.category_heading(label) => {
                self.category_text(label)
            }
            _ if self.page == MenuPage::Settings => self.setting_line(label),
            _ if self.page == MenuPage::Worlds && self.size_locked => {
                "the server has the world".to_string()
            }
            _ if self.page == MenuPage::Worlds && label == self.world_name => {
                format!("{label}, ready to play")
            }
            _ if self.page == MenuPage::Worlds => format!("save {label}"),
            _ => String::new(),
        }
    }

    pub fn activate_row(&mut self, index: usize) -> MenuAction {
        self.selected = index;
        self.activate()
    }

    /// Leave settings / keybinds for the page that opened them. Escape on pause resumes.
    pub fn escape(&mut self) -> MenuAction {
        match self.page {
            MenuPage::Settings => {
                self.page = self.settings_return;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Keybinds => {
                self.page = self.keybinds_return;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Worlds => {
                self.page = MenuPage::Main;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Pause => {
                self.page = MenuPage::Closed;
                MenuAction::Resume
            }
            MenuPage::Main | MenuPage::Closed => MenuAction::None,
        }
    }

    /// Leave settings or keybinds. The page that opened them stays up.
    pub fn back(&mut self) {
        if matches!(self.page, MenuPage::Settings | MenuPage::Keybinds) {
            let _ = self.escape();
        }
    }

    /// Arrows move the row. ← → cycle a settings value. Enter or Space runs it.
    pub fn on_press(&mut self, code: KeyCode) -> MenuAction {
        if self.page == MenuPage::Closed {
            return MenuAction::None;
        }
        if code == KeyCode::ArrowUp || code == KeyCode::ArrowDown {
            let count = self.rows().len();
            if count > 0 && code == KeyCode::ArrowUp {
                self.selected = if self.selected == 0 {
                    count - 1
                } else {
                    self.selected - 1
                };
            }
            if count > 0 && code == KeyCode::ArrowDown {
                self.selected = (self.selected + 1) % count;
            }
            return MenuAction::None;
        }
        if self.page == MenuPage::Settings
            && (code == KeyCode::ArrowLeft || code == KeyCode::ArrowRight)
        {
            let label = self.rows().get(self.selected).cloned().unwrap_or_default();
            if label == "back" || self.category_heading(&label) {
                return MenuAction::None;
            }
            return self.step_setting(self.selected, code == KeyCode::ArrowRight);
        }
        if code == KeyCode::Enter || code == KeyCode::NumpadEnter || code == KeyCode::Space {
            return self.activate();
        }
        MenuAction::None
    }

    /// Move the highlight onto the button under the pointer.
    pub fn pointer_move(&mut self, hit: u16) {
        if let Some(index) = self.row_hit(hit) {
            self.selected = index;
        }
    }

    /// Click runs that button. The same path Enter uses.
    pub fn pointer_click(&mut self, hit: u16) -> MenuAction {
        let Some(index) = self.row_hit(hit) else {
            return MenuAction::None;
        };
        self.selected = index;
        self.activate()
    }

    /// Wheel down moves to the next button.
    pub fn pointer_scroll(&mut self, down: bool) {
        let count = self.rows().len();
        if count == 0 {
            return;
        }
        self.selected = scroll_index(self.selected, count, 1, down);
    }

    fn row_hit(&self, hit: u16) -> Option<usize> {
        if !(1..64).contains(&hit) {
            return None;
        }
        let index = (hit - 1) as usize;
        (index < self.rows().len()).then_some(index)
    }

    fn activate(&mut self) -> MenuAction {
        let label = self.rows().get(self.selected).cloned().unwrap_or_default();
        match self.page {
            MenuPage::Settings if label == "world" => {
                self.size = self.size.cycle();
                MenuAction::Resize
            }
            MenuPage::Settings if label == "hemisphere" => {
                self.hemisphere = self.hemisphere.cycle();
                MenuAction::None
            }
            MenuPage::Settings if label == "volume" => {
                self.muted = !self.muted;
                MenuAction::Volume
            }
            MenuPage::Settings if label == "view_bob" => {
                self.bob = self.bob.cycle();
                MenuAction::None
            }
            MenuPage::Settings if label == "trace" => {
                self.trace = !self.trace;
                MenuAction::None
            }
            MenuPage::Settings if self.category_heading(&label) => MenuAction::None,
            MenuPage::Settings if rebuild_world::Pack::builtin().toggle_setting(&label) => {
                self.changed = Some(label);
                MenuAction::Setting
            }
            MenuPage::Settings if label == "keybinds" => {
                self.open_keybinds(MenuPage::Settings);
                MenuAction::None
            }
            MenuPage::Settings => {
                self.back();
                MenuAction::None
            }
            MenuPage::Keybinds if label == "back" => {
                self.page = self.keybinds_return;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Pause if label == "resume" => {
                self.page = MenuPage::Closed;
                MenuAction::Resume
            }
            MenuPage::Pause if label == "settings" => {
                self.open_settings(MenuPage::Pause);
                MenuAction::None
            }
            MenuPage::Pause if label == "keybinds" => {
                self.open_keybinds(MenuPage::Pause);
                MenuAction::None
            }
            MenuPage::Pause if label == "title" => {
                self.page = MenuPage::Main;
                self.selected = 0;
                MenuAction::Title
            }
            MenuPage::Worlds if label == "back" => {
                self.page = MenuPage::Main;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Worlds if label == "new" && !self.size_locked => {
                self.make_world();
                MenuAction::None
            }
            MenuPage::Worlds if !self.size_locked && label != "new" => {
                self.world_name = label;
                MenuAction::None
            }
            MenuPage::Main if label == "play" => {
                self.page = MenuPage::Closed;
                MenuAction::Play
            }
            MenuPage::Main if label == "join" => MenuAction::Join,
            MenuPage::Main if label == "settings" => {
                self.open_settings(MenuPage::Main);
                MenuAction::None
            }
            MenuPage::Main if label == "worlds" => {
                self.worlds = crate::net::list_worlds();
                if self.worlds.len() > 10 {
                    self.worlds.truncate(10);
                }
                self.page = MenuPage::Worlds;
                self.selected = 0;
                MenuAction::None
            }
            MenuPage::Main if label == "quit" => MenuAction::Quit,
            _ => MenuAction::None,
        }
    }

    fn settings_rows(&self) -> Vec<String> {
        let mut rows = self.settings.labels.clone();
        let insert_at = rows
            .iter()
            .position(|label| label == "back")
            .unwrap_or(rows.len());
        let options = rebuild_world::Pack::builtin().settings();
        let mut categories = Vec::new();
        for option in &options {
            if !categories.iter().any(|name| name == &option.category) {
                categories.push(option.category.clone());
            }
        }
        let mut grouped = Vec::new();
        for category in categories {
            let heading_taken = rows.iter().any(|label| label == &category)
                || grouped.iter().any(|label| label == &category)
                || options.iter().any(|option| option.name == category);
            if !heading_taken {
                grouped.push(category.clone());
            }
            for option in options.iter().filter(|option| option.category == category) {
                if rows.iter().any(|label| label == &option.name)
                    || grouped.iter().any(|label| label == &option.name)
                {
                    continue;
                }
                grouped.push(option.name.clone());
            }
        }
        for (offset, name) in grouped.into_iter().enumerate() {
            rows.insert(insert_at + offset, name);
        }
        rows
    }

    fn category_heading(&self, label: &str) -> bool {
        let options = rebuild_world::Pack::builtin().settings();
        options.iter().any(|option| option.category == label)
            && options.iter().all(|option| option.name != label)
    }

    fn category_text(&self, label: &str) -> String {
        rebuild_world::Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.category == label)
            .map(|option| option.category_text)
            .unwrap_or_default()
    }

    fn setting_line(&self, label: &str) -> String {
        // Description + value only — category text lives on the heading row.
        rebuild_world::Pack::builtin()
            .settings()
            .into_iter()
            .find(|option| option.name == label)
            .map(|option| format!("{}, {}", option.description, option.value))
            .unwrap_or_default()
    }

    fn open_settings(&mut self, back: MenuPage) {
        self.settings_return = back;
        self.page = MenuPage::Settings;
        self.selected = 0;
        self.settings_category = 0;
        self.settings_query.clear();
    }

    /// Open settings from the title menu (proof shots / automation).
    pub fn show_settings(&mut self) {
        self.open_settings(MenuPage::Main);
    }

    pub fn is_keybinds(&self) -> bool {
        self.page == MenuPage::Keybinds
    }

    fn open_keybinds(&mut self, back: MenuPage) {
        self.keybinds_return = back;
        self.page = MenuPage::Keybinds;
        self.selected = 0;
    }

    /// Open keybinds from the title menu (proof shots / automation).
    pub fn show_keybinds(&mut self) {
        self.open_keybinds(MenuPage::Main);
    }

    fn make_world(&mut self) {
        let name = crate::net::fresh_world_name(&self.worlds);
        if crate::net::ensure_world(&name).is_err() {
            return;
        }
        self.worlds = crate::net::list_worlds();
        if self.worlds.len() > 10 {
            self.worlds.truncate(10);
        }
        self.world_name = name.clone();
        self.selected = self
            .worlds
            .iter()
            .position(|saved| saved == &name)
            .unwrap_or(0);
    }

    fn rows(&self) -> Vec<String> {
        match self.page {
            MenuPage::Settings => self.settings_rows(),
            MenuPage::Keybinds => vec!["back".to_string()],
            MenuPage::Pause => self.pause.labels.clone(),
            MenuPage::Worlds => {
                let mut rows = self.worlds.clone();
                rows.push("new".to_string());
                rows.push("back".to_string());
                rows
            }
            _ => self.main.labels.clone(),
        }
    }

    fn detail(&self) -> String {
        let rows = self.rows();
        let label = rows.get(self.selected).map(String::as_str).unwrap_or("");
        match label {
            "play" => format!("solo {}, {}", self.size.label(), self.world_name),
            "join" if self.size_locked => "already on the server".to_string(),
            "join" if !self.join_note.is_empty() => self.join_note.clone(),
            "join" => "localhost".to_string(),
            "settings" => "world size, sun, sound, bob, and trace".to_string(),
            "keybinds" => "move, jump, sneak, inventory, mouse…".to_string(),
            "worlds" => "load or start a named save".to_string(),
            "quit" => "leave the game".to_string(),
            "resume" => "return to the world".to_string(),
            "title" => "quit to the menu".to_string(),
            "world" if self.size_locked => format!(
                "server {}, {} by {} chunks",
                self.size.label(),
                self.size.side(),
                self.size.side()
            ),
            "world" => format!(
                "{}, {} by {} chunks",
                self.size.label(),
                self.size.side(),
                self.size.side()
            ),
            "hemisphere" => format!("{} sun", self.hemisphere.label()),
            "volume" if self.muted => "muted".to_string(),
            "volume" => "sound on".to_string(),
            "view_bob" => format!("bob {}", self.bob.label()),
            "trace" if self.trace => "soft sun".to_string(),
            "trace" => "raster sun".to_string(),
            "back" => "return to the menu".to_string(),
            "new" => "start a blank save".to_string(),
            _ if self.page == MenuPage::Settings && self.category_heading(label) => {
                self.category_text(label)
            }
            _ if self.page == MenuPage::Settings => self.setting_line(label),
            _ if self.page == MenuPage::Worlds && self.size_locked => {
                "the server has the world".to_string()
            }
            _ if self.page == MenuPage::Worlds && label == self.world_name => {
                format!("{label}, ready to play")
            }
            _ if self.page == MenuPage::Worlds => format!("save {label}"),
            _ => String::new(),
        }
    }

    fn screen(&self) -> &Screen {
        match self.page {
            MenuPage::Settings => &self.settings,
            MenuPage::Pause => &self.pause,
            MenuPage::Worlds => &self.worlds_screen,
            _ => &self.main,
        }
    }

    pub fn quads(&self, width: f32, height: f32) -> Vec<Quad> {
        if !self.is_open() || width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let mut quads = vec![
            pixel_quad(width, height, 0.0, 0.0, width, height, BACKDROP).face(
                "deepslate",
                width / 16.0,
                height / 16.0,
            ),
        ];
        let rows = self.rows();
        let selected = if rows.is_empty() {
            0
        } else {
            self.selected.min(rows.len() - 1)
        };
        quads.extend(
            self.screen()
                .quads(width, height, &rows, selected, &self.detail()),
        );
        quads
    }
}

fn load_or(what: &str, path: &std::path::Path, fallback: &str) -> Screen {
    match load_at(path) {
        Ok(screen) => screen,
        Err(err) => {
            eprintln!("{what}: {err}");
            Screen::parse(fallback).expect("fallback screen")
        }
    }
}

const FALLBACK_MENU: &str = r#"{
  "id": "menu",
  "title": "Terralite",
  "open": "escape",
  "close": "escape",
  "kind": "menu",
  "labels": ["play", "join", "settings", "worlds", "quit"]
}"#;

const FALLBACK_SETTINGS: &str = r#"{
  "id": "settings",
  "title": "Settings",
  "open": "escape",
  "close": "escape",
  "kind": "menu",
  "labels": ["world", "hemisphere", "volume", "view_bob", "trace", "keybinds", "back"]
}"#;

const FALLBACK_PAUSE: &str = r#"{
  "id": "pause",
  "title": "Paused",
  "open": "escape",
  "close": "escape",
  "kind": "menu",
  "labels": ["resume", "settings", "keybinds", "title"]
}"#;

const FALLBACK_WORLDS: &str = r#"{
  "id": "worlds",
  "title": "Worlds",
  "open": "escape",
  "close": "escape",
  "kind": "menu",
  "labels": ["back"]
}"#;

/// The menu covers the window. Otherwise the hotbar, then the items screen.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ui_quads(
    width: f32,
    height: f32,
    hotbar: &Hotbar,
    bag: &hotbar::Bag,
    gui: &Gui,
    menu: &Menu,
    hearts: i32,
    food: i32,
    armor: [Option<&str>; 3],
) -> Vec<Quad> {
    feel_quads(
        width, height, hotbar, bag, gui, menu, hearts, food, armor, 0.0, 0.0, 0.0, false, false,
    )
}

/// The playing HUD. `punch` lifts the selected slot, `hurt` draws a red edge,
/// `sip` draws a bite or drink strip, and `dead` replaces the HUD with the death screen.
pub fn feel_quads(
    width: f32,
    height: f32,
    hotbar: &Hotbar,
    bag: &hotbar::Bag,
    gui: &Gui,
    menu: &Menu,
    hearts: i32,
    food: i32,
    armor: [Option<&str>; 3],
    punch: f32,
    hurt: f32,
    sip: f32,
    drink: bool,
    dead: bool,
) -> Vec<Quad> {
    let (width, height) = gui_space(width, height);
    if menu.is_open() {
        // egui draws pause / settings / title; keep chat on the quad path.
        return gui.chat_quads(width, height);
    }
    if dead {
        return death_quads(width, height);
    }
    let mut quads = hotbar::bar_lifted(width, height, hotbar, punch.max(0.0) * 10.0);
    // Creative + player inventory are egui; stations and RoseGold GUIAPI stay here.
    if !gui.bag_open() && !gui.player_inv_open() {
        quads.extend(gui.paint(width, height, bag));
    }
    // Chat shares the bottom-left with health/hunger. Hide it under egui menus so
    // a leftover line does not read as a HUD placeholder above the status stack.
    if !gui.bag_open() && !gui.player_inv_open() {
        quads.extend(gui.chat_quads(width, height));
    }
    quads.extend(health_quads(width, height, hearts));
    quads.extend(hunger_quads(width, height, food));
    quads.extend(armor_quads(width, height, armor));
    quads.extend(hurt_quads(width, height, hurt));
    quads.extend(bite_quads(width, height, sip, drink));
    quads
}

/// A dark veil, the words "You died", and a Respawn button. The button's hit is `700`.
pub fn death_quads(width: f32, height: f32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let mut quads = vec![pixel_quad(
        width,
        height,
        0.0,
        0.0,
        width,
        height,
        [0.08, 0.01, 0.01],
    )];
    let panel_w = 300.0;
    let panel_h = 150.0;
    let left = (width - panel_w) * 0.5;
    let bottom = (height - panel_h) * 0.5;
    quads.push(pixel_quad(
        width,
        height,
        left - 4.0,
        bottom - 4.0,
        left + panel_w + 4.0,
        bottom + panel_h + 4.0,
        EDGE,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left - 2.0,
        bottom - 2.0,
        left + panel_w + 2.0,
        bottom + panel_h + 2.0,
        FRAME,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom,
        left + panel_w,
        bottom + panel_h,
        PANEL,
    ));
    let title = "You died";
    let title_w = measure(title, BRAND_SCALE);
    draw_text(
        &mut quads,
        width,
        height,
        left + (panel_w - title_w) * 0.5,
        bottom + panel_h - PAD - BRAND_H,
        title,
        INK,
        BRAND_SCALE,
    );
    let button_w = 140.0;
    let button_h = 32.0;
    let button_x = left + (panel_w - button_w) * 0.5;
    let button_y = bottom + PAD;
    quads.push(
        pixel_quad(
            width,
            height,
            button_x,
            button_y,
            button_x + button_w,
            button_y + button_h,
            SELECT,
        )
        .tag(700),
    );
    let label = "Respawn";
    let label_w = measure(label, 1.0);
    draw_text(
        &mut quads,
        width,
        height,
        button_x + (button_w - label_w) * 0.5,
        button_y + 6.0,
        label,
        INK,
        1.0,
    );
    quads
}

/// One strip along the bottom. Food is warm, a drink is cool. `amount` fades from `1` to `0`.
pub fn bite_quads(width: f32, height: f32, amount: f32, drink: bool) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let band = 6.0 + 10.0 * amount;
    let color = if drink { SIP } else { BITE };
    vec![pixel_quad(width, height, 0.0, 0.0, width, band, color)]
}

/// A small plate above the hotbar while Shift is held. `amount` fades from `0` to `1`.
pub fn crouch_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    status_plate(width, height, amount, "sneak")
}

/// A small plate above the hotbar while Ctrl-sprint is held.
pub fn sprint_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    status_plate(width, height, amount, "sprint")
}

pub fn boat_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    status_plate(width, height, amount, "row")
}

pub fn chew_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    status_plate(width, height, amount, "chew")
}

fn status_plate(width: f32, height: f32, amount: f32, label: &str) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.02 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let text_w = measure(label, 1.0);
    let plate_w = text_w + 16.0;
    let plate_h = 18.0;
    let left = (width - plate_w) * 0.5;
    // Sit just above the hotbar lip; status plates are bottom-left, so this stays clear.
    let bottom = hotbar::bar_chrome_top() + 8.0;
    let mut quads = vec![
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + plate_w,
            bottom + plate_h,
            WELL,
        ),
        pixel_quad(
            width,
            height,
            left,
            bottom + plate_h - 3.0,
            left + plate_w,
            bottom + plate_h,
            LIP,
        ),
    ];
    draw_text(
        &mut quads,
        width,
        height,
        left + 8.0,
        bottom + 3.0,
        label,
        INK,
        1.0,
    );
    quads
}

/// The readable name under the cross, or of the selected item. Air stays quiet.
pub fn look_label(id: &str) -> Option<String> {
    if id.is_empty() || id == "air" {
        return None;
    }
    let name = title_case(&crate::recipes::display_name(id));
    if name.is_empty() { None } else { Some(name) }
}

/// Center crosshair. `ready` is `0` just swung through `1` fully charged.
///
/// Cooling shrinks the arms so the wait reads like a Minecraft attack meter
/// without a separate HUD ring.
pub fn cross_quads(width: f32, height: f32, ready: f32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let ready = ready.clamp(0.0, 1.0);
    let cx = width * 0.5;
    let cy = height * 0.5;
    let arm = 2.0 + 4.0 * ready;
    let edge = 1.0 + arm;
    let ink = if ready >= 0.98 { INK } else { EDGE };
    vec![
        pixel_quad(
            width,
            height,
            cx - 1.0,
            cy - edge,
            cx + 2.0,
            cy + edge,
            EDGE,
        ),
        pixel_quad(
            width,
            height,
            cx - edge,
            cy - 1.0,
            cx + edge,
            cy + 2.0,
            EDGE,
        ),
        pixel_quad(
            width,
            height,
            cx,
            cy - arm,
            cx + 1.0,
            cy + arm,
            ink,
        ),
        pixel_quad(
            width,
            height,
            cx - arm,
            cy,
            cx + arm,
            cy + 1.0,
            ink,
        ),
    ]
}

/// A small cross at the center. `id` is the looked-at block, entity, or fluid.
/// `held` is the selected hotbar item. The look name sits bottom-right; the held
/// name stays above the hotbar. `held_override` replaces the hotbar name when a
/// place shape is armed. `attack_ready` shrinks the cross while a swing cools.
pub fn look_quads(
    width: f32,
    height: f32,
    id: Option<&str>,
    held: Option<&str>,
    held_override: Option<&str>,
    attack_ready: f32,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let cx = width * 0.5;
    let mut quads = cross_quads(width, height, attack_ready);
    if let Some(label) = id.and_then(look_label) {
        let plate_w = measure(&label, 1.0) + 12.0;
        // Bottom-right tip: clear of the centered hotbar and the left status stack.
        let margin = 12.0;
        let center_x = width - margin - plate_w * 0.5;
        push_name_plate(&mut quads, width, height, &label, center_x, margin);
    }
    let held_label = held_override
        .map(str::to_string)
        .or_else(|| held.and_then(look_label));
    if let Some(label) = held_label {
        // Above the hotbar lip; stays centered so it clears the left status plates.
        let held_bottom = hotbar::bar_chrome_top() + 36.0;
        push_name_plate(&mut quads, width, height, &label, cx, held_bottom);
    }
    quads
}

/// Alt place-shape menu. `labels` are the option names; `selected` is highlighted.
pub fn shape_picker_quads(
    width: f32,
    height: f32,
    labels: &[&str],
    selected: usize,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 || labels.is_empty() {
        return Vec::new();
    }
    let row_h = 22.0;
    let pad = 10.0;
    let title = "shape";
    let mut content_w = measure(title, 1.0);
    for (index, label) in labels.iter().enumerate() {
        let line = format!("{}  {}", index + 1, label);
        content_w = content_w.max(measure(&line, 1.0));
    }
    let panel_w = (content_w + pad * 2.0).max(120.0);
    let panel_h = pad + 18.0 + row_h * labels.len() as f32 + pad;
    let left = width * 0.5 - panel_w * 0.5;
    let bottom = height * 0.5 - panel_h * 0.5;
    let mut quads = Vec::new();
    quads.push(pixel_quad(
        width,
        height,
        left - 2.0,
        bottom - 2.0,
        left + panel_w + 2.0,
        bottom + panel_h + 2.0,
        FRAME,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom,
        left + panel_w,
        bottom + panel_h,
        PANEL,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom + panel_h - 2.0,
        left + panel_w,
        bottom + panel_h,
        LIP,
    ));
    let title_w = measure(title, 1.0);
    draw_text(
        &mut quads,
        width,
        height,
        left + (panel_w - title_w) * 0.5,
        bottom + panel_h - pad - 14.0,
        title,
        DETAIL,
        1.0,
    );
    for (index, label) in labels.iter().enumerate() {
        let row_bottom = bottom + pad + row_h * (labels.len() - 1 - index) as f32;
        let chosen = index == selected;
        if chosen {
            quads.push(pixel_quad(
                width,
                height,
                left + 4.0,
                row_bottom,
                left + panel_w - 4.0,
                row_bottom + row_h - 2.0,
                SELECT,
            ));
        }
        let line = format!("{}  {}", index + 1, label);
        draw_text(
            &mut quads,
            width,
            height,
            left + pad,
            row_bottom + 3.0,
            &line,
            if chosen { INK } else { LABEL },
            1.0,
        );
    }
    quads
}

fn push_name_plate(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    label: &str,
    center_x: f32,
    bottom: f32,
) {
    let text_w = measure(label, 1.0);
    let plate_w = text_w + 12.0;
    let plate_h = 16.0;
    let left = center_x - plate_w * 0.5;
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom,
        left + plate_w,
        bottom + plate_h,
        WELL,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom + plate_h - 2.0,
        left + plate_w,
        bottom + plate_h,
        LIP,
    ));
    draw_text(
        quads,
        width,
        height,
        left + 6.0,
        bottom + 2.0,
        label,
        INK,
        1.0,
    );
}

/// Four red strips at the screen edge. `amount` is `0` clear through `1` just hit.
pub fn hurt_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let band = 8.0 + 20.0 * amount;
    vec![
        pixel_quad(width, height, 0.0, height - band, width, height, HURT),
        pixel_quad(width, height, 0.0, 0.0, width, band, HURT),
        pixel_quad(width, height, 0.0, 0.0, band, height, HURT),
        pixel_quad(width, height, width - band, 0.0, width, height, HURT),
    ]
}

/// Cyan edge veil while the eye is underwater. `amount` is `0` dry through `1` submerged.
pub fn water_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.02 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let band = 10.0 + 28.0 * amount;
    let shade = [0.08, 0.28, 0.36];
    vec![
        pixel_quad(width, height, 0.0, height - band, width, height, shade),
        pixel_quad(width, height, 0.0, 0.0, width, band, shade),
        pixel_quad(width, height, 0.0, 0.0, band, height, shade),
        pixel_quad(width, height, width - band, 0.0, width, height, shade),
    ]
}

/// Brown edge veil when hunger is critically low. `amount` is `0` fed through `1` empty.
pub fn starve_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.02 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let band = 8.0 + 22.0 * amount;
    let shade = [0.28, 0.14, 0.06];
    vec![
        pixel_quad(width, height, 0.0, height - band, width, height, shade),
        pixel_quad(width, height, 0.0, 0.0, width, band, shade),
        pixel_quad(width, height, 0.0, 0.0, band, height, shade),
        pixel_quad(width, height, width - band, 0.0, width, height, shade),
    ]
}

/// Dark edge veil after sleeping through the night. `amount` is `1` just woke through `0`.
///
/// Opaque HUD quads have no alpha, so the wake cue closes in from the edges
/// (full cover at `1`, clear at `0`) instead of tinting the whole frame.
pub fn sleep_quads(width: f32, height: f32, amount: f32) -> Vec<Quad> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let band_y = (height * 0.5 * amount).max(1.0);
    let band_x = (width * 0.5 * amount).max(1.0);
    let shade = [0.03, 0.04, 0.08];
    vec![
        pixel_quad(width, height, 0.0, height - band_y, width, height, shade),
        pixel_quad(width, height, 0.0, 0.0, width, band_y, shade),
        pixel_quad(width, height, 0.0, 0.0, band_x, height, shade),
        pixel_quad(width, height, width - band_x, 0.0, width, height, shade),
    ]
}

/// Left inset for the HUD status cluster (hearts / hunger / armor).
const HUD_STATUS_LEFT: f32 = 16.0;
/// Gap between the hotbar chrome and the stacked status plates.
const HUD_STATUS_GAP: f32 = 10.0;
/// Vertical pitch from one status row to the next.
const HUD_STATUS_ROW: f32 = 24.0;
const HUD_STATUS_PIP: f32 = 10.0;
const HUD_STATUS_STEP: f32 = 14.0;

/// Bottom of the lower status row (hunger), clear above the hotbar.
fn status_hunger_bottom() -> f32 {
    hotbar::bar_chrome_top() + HUD_STATUS_GAP
}

fn status_health_bottom() -> f32 {
    status_hunger_bottom() + HUD_STATUS_ROW
}

fn status_armor_bottom() -> f32 {
    status_health_bottom() + HUD_STATUS_ROW
}

fn armor_quads(width: f32, height: f32, worn: [Option<&str>; 3]) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    // Skip the empty plate so a blank armor row does not look like HUD chrome text.
    if worn.iter().all(|piece| piece.is_none()) {
        return Vec::new();
    }
    let empty = [0.16, 0.12, 0.10];
    let bottom = status_armor_bottom();
    let span = 3.0 * HUD_STATUS_STEP;
    let left = HUD_STATUS_LEFT;
    let mut quads = vec![pixel_quad(
        width,
        height,
        left,
        bottom - 6.0,
        left + 4.0 + span,
        bottom + 16.0,
        TRAY,
    )];
    for (index, piece) in worn.iter().enumerate() {
        let x = left + 8.0 + index as f32 * HUD_STATUS_STEP;
        let color = piece.map(|id| palette::color(id, 0)).unwrap_or(empty);
        quads.push(pixel_quad(
            width,
            height,
            x,
            bottom,
            x + HUD_STATUS_PIP,
            bottom + HUD_STATUS_PIP,
            color,
        ));
    }
    quads
}

/// A dial and a mark. `needle` is pixels right and forward from the dial's center.
pub fn compass_quads(width: f32, height: f32, needle: [f32; 2]) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let cx = width * 0.5;
    let cy = 158.0;
    let dial = [0.12, 0.14, 0.16];
    let mark = [0.86, 0.18, 0.14];
    let mut quads = vec![pixel_quad(
        width,
        height,
        cx - 16.0,
        cy - 16.0,
        cx + 16.0,
        cy + 16.0,
        dial,
    )];
    let nx = cx + needle[0];
    let ny = cy + needle[1];
    quads.push(pixel_quad(
        width,
        height,
        nx - 4.0,
        ny - 4.0,
        nx + 4.0,
        ny + 4.0,
        mark,
    ));
    quads
}

/// A dial and a mark. `needle` is pixels right and up from the dial's center.
pub fn clock_quads(width: f32, height: f32, needle: [f32; 2]) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let cx = width * 0.5;
    let cy = 158.0;
    let dial = [0.16, 0.13, 0.08];
    let mark = [0.90, 0.78, 0.22];
    let mut quads = vec![pixel_quad(
        width,
        height,
        cx - 16.0,
        cy - 16.0,
        cx + 16.0,
        cy + 16.0,
        dial,
    )];
    let nx = cx + needle[0];
    let ny = cy + needle[1];
    quads.push(pixel_quad(
        width,
        height,
        nx - 4.0,
        ny - 4.0,
        nx + 4.0,
        ny + 4.0,
        mark,
    ));
    quads
}

fn commands_screen() -> Screen {
    Screen {
        id: "commands".to_string(),
        title: "Commands".to_string(),
        open: KeyCode::KeyC,
        close: KeyCode::Escape,
        labels: vec!["day".to_string()],
        kind: ScreenKind::List,
        panel: "stone".to_string(),
        button: "cobblestone".to_string(),
        header: "deepslate".to_string(),
    }
}

fn command_detail(label: &str) -> String {
    match label {
        "day" => "set the sun to noon".to_string(),
        "night" => "set the sun to midnight".to_string(),
        "home" => "return to the stairs".to_string(),
        _ => "run this command".to_string(),
    }
}

fn health_quads(width: f32, height: f32, hearts: i32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let mut quads = Vec::new();
    let full = [0.75, 0.16, 0.14];
    let empty = [0.22, 0.08, 0.08];
    // Bottom-left stack: health sits above hunger, both clear of the hotbar.
    let bottom = status_health_bottom();
    let span = crate::body::HEARTS as f32 * HUD_STATUS_STEP;
    let left = HUD_STATUS_LEFT;
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom - 6.0,
        left + 4.0 + span,
        bottom + 16.0,
        TRAY,
    ));
    for index in 0..crate::body::HEARTS {
        let x = left + 8.0 + index as f32 * HUD_STATUS_STEP;
        let filled = index < hearts;
        let color = if filled { full } else { empty };
        quads.push(pixel_quad(
            width,
            height,
            x,
            bottom,
            x + HUD_STATUS_PIP,
            bottom + HUD_STATUS_PIP,
            color,
        ));
        if filled {
            quads.push(pixel_quad(
                width,
                height,
                x + 6.0,
                bottom + 6.0,
                x + 9.0,
                bottom + 9.0,
                PIP,
            ));
        }
    }
    quads
}

fn hunger_quads(width: f32, height: f32, food: i32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let mut quads = Vec::new();
    let full = [0.86, 0.62, 0.16];
    let empty = [0.28, 0.18, 0.08];
    // Bottom-left with health (GUI plan L10); was previously bottom-right.
    let bottom = status_hunger_bottom();
    let span = crate::body::HUNGER as f32 * HUD_STATUS_STEP;
    let left = HUD_STATUS_LEFT;
    quads.push(pixel_quad(
        width,
        height,
        left,
        bottom - 6.0,
        left + 4.0 + span,
        bottom + 16.0,
        TRAY,
    ));
    for index in 0..crate::body::HUNGER {
        let x = left + 8.0 + index as f32 * HUD_STATUS_STEP;
        let color = if index < food { full } else { empty };
        quads.push(pixel_quad(
            width,
            height,
            x,
            bottom,
            x + HUD_STATUS_PIP,
            bottom + HUD_STATUS_PIP,
            color,
        ));
    }
    quads
}

fn inventory_screen() -> Screen {
    Screen {
        id: "inventory".to_string(),
        title: "Inventory".to_string(),
        open: KeyCode::KeyI,
        close: KeyCode::Escape,
        labels: vec!["empty".to_string()],
        kind: ScreenKind::List,
        panel: "stone".to_string(),
        button: "cobblestone".to_string(),
        header: "deepslate".to_string(),
    }
}

fn face_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    default: &str,
) -> Result<String, String> {
    match obj.get(key) {
        None => Ok(default.to_string()),
        Some(Value::String(text)) if valid_label(text) => Ok(text.clone()),
        _ => Err(format!("screen {key} must name a block")),
    }
}

fn creative_screen() -> Screen {
    static LOADED: OnceLock<Screen> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let path = gui_file("creative.json");
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(screen) = Screen::parse(&text) {
                    if screen.kind == ScreenKind::Creative {
                        return screen;
                    }
                }
            }
            Screen {
                id: "inventory".to_string(),
                title: "Creative".to_string(),
                open: KeyCode::KeyI,
                close: KeyCode::Escape,
                labels: Vec::new(),
                kind: ScreenKind::Creative,
                panel: "stone".to_string(),
                button: "cobblestone".to_string(),
                header: "deepslate".to_string(),
            }
        })
        .clone()
}

/// Placeable pack blocks, then script blocks and items that place a block.
fn catalog() -> Vec<String> {
    let pack = rebuild_world::Pack::builtin();
    let mut ids = Vec::new();
    for (id, _) in pack.blocks() {
        if id == "air" {
            continue;
        }
        ids.push(id.to_string());
    }
    for id in pack.script_block_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.placeable_item_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.tool_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.food_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.armor_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.shield_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.boat_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.omen_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.key_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.core_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.mace_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.horn_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.brush_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.spyglass_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.compass_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.bucket_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.saddle_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.clock_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.map_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.book_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.model_item_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.fertilizer_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.arrow_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.rod_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.wind_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.pearl_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    for id in pack.eye_ids() {
        if !ids.iter().any(|have| have == &id) {
            ids.push(id);
        }
    }
    ids
}

/// The catalog after the tab and the name search.
///
/// A name match ignores spaces and underscores, so `oak log` and `oaklog` both
/// find `oak_log`. Words can sit in either order. Names that start with the
/// query come first.
fn creative_view(tab: usize, query: &str) -> Vec<String> {
    let name = TABS.get(tab).copied().unwrap_or("all");
    let needle = fold_name(query);
    let mut hits: Vec<String> = catalog()
        .into_iter()
        .filter(|id| (name == "all" || tab_of(id) == name) && creative_hit(id, query))
        .collect();
    if !needle.is_empty() {
        hits.sort_by(|left, right| {
            let rank = |id: &str| {
                let folded = fold_name(id);
                let shown = fold_name(&crate::recipes::display_name(id));
                if folded.starts_with(&needle) || shown.starts_with(&needle) {
                    0
                } else {
                    1
                }
            };
            rank(left).cmp(&rank(right))
        });
    }
    hits
}

/// True when `query` fits the id or its readable name.
fn creative_hit(id: &str, query: &str) -> bool {
    let needle = fold_name(query);
    if needle.is_empty() {
        return true;
    }
    let id_fold = fold_name(id);
    let shown = fold_name(&crate::recipes::display_name(id));
    if id_fold.contains(&needle) || shown.contains(&needle) {
        return true;
    }
    let words: Vec<String> = query
        .split_whitespace()
        .map(fold_name)
        .filter(|word| !word.is_empty())
        .collect();
    words.len() > 1
        && words
            .iter()
            .all(|word| id_fold.contains(word) || shown.contains(word))
}

fn fold_name(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

/// The line in an empty creative grid.
fn creative_empty_line(query: &str) -> &'static str {
    if fold_name(query).is_empty() {
        "no blocks"
    } else {
        "nothing matches"
    }
}

fn tab_of(id: &str) -> &'static str {
    if id.contains("ore") {
        "ore"
    } else if id == "lantern"
        || id == "torch"
        || id == "end_rod"
        || id == "end_crystal"
        || id == "glowstone"
        || id == "sea_lantern"
        || id.contains("light")
    {
        "light"
    } else if matches!(
        id,
        "grass"
            | "dirt"
            | "water"
            | "oak_log"
            | "oak_stairs"
            | "stone_stairs"
            | "stonecutter"
            | "boat"
            | "flower"
            | "flower_pot"
            | "sand"
            | "soul_sand"
            | "magma"
            | "gravel"
            | "cactus"
            | "pumpkin"
            | "melon"
            | "melon_slice"
            | "hay_bale"
            | "campfire"
            | "raw"
            | "cooked"
            | "nether_wart_block"
            | "vine"
            | "lily_pad"
            | "snow"
            | "sugar_cane"
            | "chorus_flower"
            | "chorus_fruit"
            | "healing_potion"
            | "ender_pearl"
            | "eye_of_ender"
            | "chorus_plant"
            | "ice"
            | "clay"
    ) {
        "nature"
    } else if matches!(
        id,
        "stone"
            | "smooth_stone"
            | "quartz"
            | "stone_bricks"
            | "cracked_stone_bricks"
            | "sandstone"
            | "end_stone"
            | "end_stone_bricks"
            | "purpur"
            | "purpur_pillar"
            | "end_portal_frame"
            | "dragon_egg"
            | "anvil"
            | "enchanting_table"
            | "chest"
            | "copper_chest"
            | "cauldron"
            | "ender_chest"
            | "shulker_box"
            | "barrel"
            | "prismarine"
            | "prismarine_bricks"
            | "dark_prismarine"
            | "red_sandstone"
            | "deepslate"
            | "deepslate_bricks"
            | "deepslate_tiles"
            | "cracked_deepslate_tiles"
            | "cobbled_deepslate"
            | "polished_deepslate"
            | "cobblestone"
            | "blackstone"
            | "polished_blackstone"
            | "blackstone_bricks"
            | "cracked_blackstone_bricks"
            | "chiseled_blackstone"
            | "gilded_blackstone"
            | "basalt"
            | "polished_basalt"
            | "smooth_basalt"
            | "nether_bricks"
            | "cracked_nether_bricks"
            | "red_nether_bricks"
            | "chiseled_nether_bricks"
            | "netherrack"
            | "mossy_cobblestone"
            | "mossy_stone_bricks"
            | "chiseled_stone_bricks"
            | "wool"
            | "crafting_table"
            | "furnace"
            | "smoker"
            | "blast_furnace"
            | "bed"
            | "beehive"
            | "honey"
            | "slimeball"
            | "bell"
            | "bookshelf"
            | "chiseled_bookshelf"
            | "shelf"
            | "brewing_stand"
            | "grindstone"
            | "smithing_table"
            | "loom"
            | "banner"
            | "cartography_table"
            | "lectern"
            | "book"
            | "composter"
            | "bone_meal"
            | "bricks"
            | "button"
            | "cake"
            | "candle"
            | "chain"
            | "hanging_sign"
            | "lodestone"
            | "respawn_anchor"
            | "lightning_rod"
            | "target"
            | "trial_spawner"
            | "copper_bulb"
            | "copper_grate"
            | "copper_door"
            | "copper_trapdoor"
            | "decorated_pot"
            | "carpet"
            | "door"
            | "fence"
            | "fence_gate"
            | "fletching_table"
            | "arrow"
            | "glass"
            | "ladder"
            | "lever"
            | "note_block"
            | "painting"
            | "item_frame"
            | "plate"
            | "slab"
            | "trapdoor"
            | "tripwire"
            | "cobweb"
    ) {
        "build"
    } else {
        "more"
    }
}

/// The 3x3 plus the result slot at index 9. `None` when the key is not an arrow.
fn nudge_craft(selected: usize, code: KeyCode) -> Option<usize> {
    let next = match (selected, code) {
        (9, KeyCode::ArrowLeft | KeyCode::ArrowUp) => 2,
        (9, KeyCode::ArrowDown) => 8,
        (9, KeyCode::ArrowRight) => 9,
        (index, KeyCode::ArrowRight) if index < 9 && index % 3 == 2 => 9,
        (index, KeyCode::ArrowLeft) if index < 9 => {
            let col = index % 3;
            let row = index / 3;
            row * 3 + if col == 0 { 2 } else { col - 1 }
        }
        (index, KeyCode::ArrowRight) if index < 9 => {
            let col = index % 3;
            let row = index / 3;
            row * 3 + col + 1
        }
        (index, KeyCode::ArrowUp) if index < 9 => {
            let col = index % 3;
            let row = index / 3;
            if row == 0 {
                6 + col
            } else {
                (row - 1) * 3 + col
            }
        }
        (index, KeyCode::ArrowDown) if index < 9 => {
            let col = index % 3;
            let row = index / 3;
            if row == 2 { col } else { (row + 1) * 3 + col }
        }
        _ => return None,
    };
    Some(next)
}

/// Move a cursor on a row-major grid. `None` when the key is not an arrow.
fn nudge_grid(selected: usize, cols: usize, count: usize, code: KeyCode) -> Option<usize> {
    if cols == 0 || count == 0 {
        return None;
    }
    let rows = count.div_ceil(cols);
    let col = selected % cols;
    let row = (selected / cols).min(rows - 1);
    let (col, row) = match code {
        KeyCode::ArrowLeft => (if col == 0 { cols - 1 } else { col - 1 }, row),
        KeyCode::ArrowRight => ((col + 1) % cols, row),
        KeyCode::ArrowUp => (col, if row == 0 { rows - 1 } else { row - 1 }),
        KeyCode::ArrowDown => (col, (row + 1) % rows),
        _ => return None,
    };
    Some((row * cols + col).min(count - 1))
}

/// Creative grid of pack blocks (10-wide), then the bag row (legacy path).
fn creative_quads(
    width: f32,
    height: f32,
    bag: &hotbar::Bag,
    catalog_on: bool,
    catalog_at: usize,
    catalog_tab: usize,
    query: &str,
    bag_selected: usize,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let screen = creative_screen();
    let ids = creative_view(catalog_tab, query);
    let index = if ids.is_empty() {
        0
    } else {
        catalog_at.min(ids.len() - 1)
    };
    let page = CREATIVE_COLS * CREATIVE_ROWS;
    let start = if ids.is_empty() {
        0
    } else {
        index / page * page
    };
    let grid_w = CREATIVE_COLS as f32 * CREATIVE_CELL + (CREATIVE_COLS - 1) as f32 * CREATIVE_GAP;
    let grid_h = CREATIVE_ROWS as f32 * CREATIVE_CELL + (CREATIVE_ROWS - 1) as f32 * CREATIVE_GAP;
    let title = brand(&screen.title);
    let detail = if ids.is_empty() {
        if fold_name(query).is_empty() {
            "no blocks".to_string()
        } else {
            format!("nothing named {}", query.replace('_', " "))
        }
    } else if catalog_on {
        let shown = title_case(&crate::recipes::display_name(&ids[index]));
        if fold_name(query).is_empty() {
            format!("{shown} full stack")
        } else {
            format!("{shown}  {}", query.replace('_', " "))
        }
    } else {
        sentence(&bag_detail(bag, bag_selected.min(hotbar::PACK - 1)))
    };
    let detail_shown = sentence(&detail);
    let tabs_w = TABS
        .iter()
        .map(|name| measure(name, 1.0) + 16.0)
        .sum::<f32>()
        + (TABS.len() - 1) as f32 * CREATIVE_GAP;
    let panel_w = (grid_w + PAD * 2.0)
        .max(measure(&title, BRAND_SCALE) + PAD * 2.0)
        .max(tabs_w + PAD * 2.0);
    let panel_h =
        PAD + BRAND_H + TITLE_GAP + TAB_H + 8.0 + grid_h + 16.0 + CREATIVE_CELL + LINE + PAD;
    let left = (width - panel_w) * 0.5;
    let bottom = ((height - panel_h) * 0.5).max(ABOVE_HOTBAR);
    let mut quads = Vec::new();
    quads.push(pixel_quad(
        width,
        height,
        left - 4.0,
        bottom - 4.0,
        left + panel_w + 4.0,
        bottom + panel_h + 4.0,
        EDGE,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left - 2.0,
        bottom - 2.0,
        left + panel_w + 2.0,
        bottom + panel_h + 2.0,
        FRAME,
    ));
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + panel_w,
            bottom + panel_h,
            PANEL,
        )
        .face(&screen.panel, panel_w / 16.0, panel_h / 16.0),
    );
    let title_base = bottom + panel_h - PAD - BRAND_H;
    let header_h = PAD + BRAND_H + 8.0;
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            title_base - 8.0,
            left + panel_w,
            bottom + panel_h,
            HEADER,
        )
        .face(&screen.header, panel_w / 16.0, header_h / 16.0),
    );
    push_lip(&mut quads, width, height, left, bottom + panel_h, panel_w);
    let title_w = measure(&title, BRAND_SCALE);
    let title_x = left + (panel_w - title_w) * 0.5;
    push_title_plate(&mut quads, width, height, title_x, title_base, title_w);
    draw_text(
        &mut quads,
        width,
        height,
        title_x,
        title_base,
        &title,
        INK,
        BRAND_SCALE,
    );
    let inner_left = left + (panel_w - grid_w) * 0.5;
    let tab_top = title_base - TITLE_GAP;
    let tab_span = panel_w - PAD * 2.0;
    let tab_w = (tab_span - (TABS.len() - 1) as f32 * CREATIVE_GAP) / TABS.len() as f32;
    let tab_left = left + PAD;
    for (index, name) in TABS.iter().enumerate() {
        let x = tab_left + index as f32 * (tab_w + CREATIVE_GAP);
        let chosen = index == catalog_tab;
        quads.push(
            pixel_quad(
                width,
                height,
                x,
                tab_top - TAB_H,
                x + tab_w,
                tab_top,
                if chosen { SELECT } else { BUTTON },
            )
            .face(
                if chosen {
                    &screen.button
                } else {
                    &screen.panel
                },
                1.0,
                1.0,
            )
            .tag(100 + index as u16),
        );
        let label_w = measure(name, 1.0);
        draw_text(
            &mut quads,
            width,
            height,
            x + (tab_w - label_w) * 0.5,
            tab_top - TAB_H + 2.0,
            name,
            INK,
            1.0,
        );
    }
    let grid_top = tab_top - TAB_H - 8.0;
    quads.push(pixel_quad(
        width,
        height,
        inner_left - 6.0,
        grid_top - grid_h - 6.0,
        inner_left + grid_w + 6.0,
        grid_top + 6.0,
        WELL,
    ));
    for slot in 0..page {
        let id_index = start + slot;
        let col = slot % CREATIVE_COLS;
        let row = slot / CREATIVE_COLS;
        let x = inner_left + col as f32 * (CREATIVE_CELL + CREATIVE_GAP);
        let y = grid_top - (row as f32 + 1.0) * CREATIVE_CELL - row as f32 * CREATIVE_GAP;
        let chosen = catalog_on && id_index == index && id_index < ids.len();
        quads.push(
            pixel_quad(
                width,
                height,
                x - 2.0,
                y - 2.0,
                x + CREATIVE_CELL + 2.0,
                y + CREATIVE_CELL + 2.0,
                if chosen { SELECT } else { ROW },
            )
            .tag(1000 + id_index as u16),
        );
        let Some(id) = ids.get(id_index) else {
            continue;
        };
        let inset = 2.0;
        hotbar::push_slot_icon(
            &mut quads,
            width,
            height,
            x + inset,
            y + inset,
            CREATIVE_CELL - inset * 2.0,
            id,
        );
    }
    if ids.is_empty() {
        let note = creative_empty_line(query);
        let note_w = measure(note, 1.0);
        let note_h = block_height(note, 1.0);
        draw_text(
            &mut quads,
            width,
            height,
            inner_left + (grid_w - note_w) * 0.5,
            grid_top - grid_h * 0.5 - note_h * 0.5,
            note,
            INK,
            1.0,
        );
    }
    let bag_y = bottom + PAD + LINE;
    let bag_w = hotbar::PACK as f32 * CREATIVE_CELL + (hotbar::PACK - 1) as f32 * CREATIVE_GAP;
    let bag_left = left + (panel_w - bag_w) * 0.5;
    quads.push(pixel_quad(
        width,
        height,
        bag_left - 6.0,
        bag_y - 6.0,
        bag_left + bag_w + 6.0,
        bag_y + CREATIVE_CELL + 6.0,
        WELL,
    ));
    for slot in 0..hotbar::PACK {
        let x = bag_left + slot as f32 * (CREATIVE_CELL + CREATIVE_GAP);
        let chosen = !catalog_on && slot == bag_selected.min(hotbar::PACK - 1);
        quads.push(
            pixel_quad(
                width,
                height,
                x - 2.0,
                bag_y - 2.0,
                x + CREATIVE_CELL + 2.0,
                bag_y + CREATIVE_CELL + 2.0,
                if chosen { FRAME } else { [0.28, 0.30, 0.36] },
            )
            .face(&screen.button, 1.0, 1.0)
            .tag(200 + slot as u16),
        );
        if let Some(id) = bag.slot(slot) {
            let inset = 2.0;
            hotbar::push_slot_icon(
                &mut quads,
                width,
                height,
                x + inset,
                bag_y + inset,
                CREATIVE_CELL - inset * 2.0,
                id,
            );
        } else {
            quads.push(pixel_quad(
                width,
                height,
                x,
                bag_y,
                x + CREATIVE_CELL,
                bag_y + CREATIVE_CELL,
                ROW,
            ));
        }
    }
    if !detail_shown.is_empty() {
        draw_text(
            &mut quads,
            width,
            height,
            left + PAD,
            bottom + PAD,
            &detail_shown,
            DETAIL,
            1.0,
        );
    }
    quads
}

/// Nine chest slots in a stone panel. Each occupied slot samples the pack atlas.
fn chest_quads(width: f32, height: f32, chest: &hotbar::Bag, selected: usize) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let screen = inventory_screen();
    let cols = CHEST_COLS;
    let rows = hotbar::PACK.div_ceil(cols);
    let cell = CREATIVE_CELL + 4.0;
    let grid_w = cols as f32 * cell + (cols - 1) as f32 * CREATIVE_GAP;
    let grid_h = rows as f32 * cell + (rows - 1) as f32 * CREATIVE_GAP;
    let title = brand("Chest");
    let selected = selected.min(hotbar::PACK - 1);
    let detail = sentence(&bag_detail(chest, selected));
    let panel_w = (grid_w + PAD * 2.0).max(measure(&title, BRAND_SCALE) + PAD * 2.0);
    let panel_h = PAD + BRAND_H + TITLE_GAP + grid_h + LINE + PAD;
    let left = (width - panel_w) * 0.5;
    let bottom = ((height - panel_h) * 0.5).max(ABOVE_HOTBAR);
    let mut quads = Vec::new();
    quads.push(pixel_quad(
        width,
        height,
        left - 4.0,
        bottom - 4.0,
        left + panel_w + 4.0,
        bottom + panel_h + 4.0,
        EDGE,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left - 2.0,
        bottom - 2.0,
        left + panel_w + 2.0,
        bottom + panel_h + 2.0,
        FRAME,
    ));
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + panel_w,
            bottom + panel_h,
            PANEL,
        )
        .face(&screen.panel, panel_w / 16.0, panel_h / 16.0),
    );
    let title_base = bottom + panel_h - PAD - BRAND_H;
    let header_h = PAD + BRAND_H + 8.0;
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            title_base - 8.0,
            left + panel_w,
            bottom + panel_h,
            HEADER,
        )
        .face(&screen.header, panel_w / 16.0, header_h / 16.0),
    );
    push_lip(&mut quads, width, height, left, bottom + panel_h, panel_w);
    let title_w = measure(&title, BRAND_SCALE);
    let title_x = left + (panel_w - title_w) * 0.5;
    push_title_plate(&mut quads, width, height, title_x, title_base, title_w);
    draw_text(
        &mut quads,
        width,
        height,
        title_x,
        title_base,
        &title,
        INK,
        BRAND_SCALE,
    );
    let origin_x = left + (panel_w - grid_w) * 0.5;
    let grid_top = title_base - TITLE_GAP;
    quads.push(pixel_quad(
        width,
        height,
        origin_x - 6.0,
        grid_top - grid_h - 6.0,
        origin_x + grid_w + 6.0,
        grid_top + 6.0,
        WELL,
    ));
    for slot in 0..hotbar::PACK {
        let col = slot % cols;
        let row = slot / cols;
        let x = origin_x + col as f32 * (cell + CREATIVE_GAP);
        let y = grid_top - (row as f32 + 1.0) * cell - row as f32 * CREATIVE_GAP;
        let chosen = slot == selected;
        quads.push(
            pixel_quad(
                width,
                height,
                x - 2.0,
                y - 2.0,
                x + cell + 2.0,
                y + cell + 2.0,
                if chosen { SELECT } else { ROW },
            )
            .face(&screen.button, 1.0, 1.0)
            .tag(300 + slot as u16),
        );
        let Some(id) = chest.slot(slot) else {
            continue;
        };
        let inset = 4.0;
        hotbar::push_slot_icon(
            &mut quads,
            width,
            height,
            x + inset,
            y + inset,
            cell - inset * 2.0,
            id,
        );
    }
    if !detail.is_empty() {
        draw_text(
            &mut quads,
            width,
            height,
            left + PAD,
            bottom + PAD,
            &detail,
            DETAIL,
            1.0,
        );
    }
    quads
}

fn craft_quads(
    width: f32,
    height: f32,
    grid: &hotbar::Bag,
    selected: usize,
    result: Option<String>,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let screen = inventory_screen();
    let cell = CREATIVE_CELL + 4.0;
    let grid_w = 3.0 * cell + 2.0 * CREATIVE_GAP;
    let grid_h = grid_w;
    let title = brand("Crafting");
    let detail = match &result {
        Some(id) => sentence(&format!("{} ready", recipes::display_name(id))),
        None => "put items in the grid".to_string(),
    };
    let panel_w = grid_w + cell + 48.0 + PAD * 2.0;
    let panel_h = PAD + BRAND_H + TITLE_GAP + grid_h + LINE + PAD;
    let (mut quads, left, bottom, title_base) =
        station_chrome(width, height, &title, panel_w, panel_h, &screen);
    let origin_x = left + PAD;
    let grid_top = title_base - TITLE_GAP;
    for slot in 0..9 {
        let col = slot % 3;
        let row = slot / 3;
        let x = origin_x + col as f32 * (cell + CREATIVE_GAP);
        let y = grid_top - (row as f32 + 1.0) * cell - row as f32 * CREATIVE_GAP;
        paint_slot(
            &mut quads,
            width,
            height,
            x,
            y,
            cell,
            slot == selected,
            grid.slot(slot),
            &screen.button,
            400 + slot as u16,
        );
    }
    let result_x = origin_x + grid_w + 48.0;
    let result_y = grid_top - 2.0 * cell - CREATIVE_GAP;
    paint_slot(
        &mut quads,
        width,
        height,
        result_x,
        result_y,
        cell,
        selected == 9,
        result.as_deref(),
        &screen.button,
        409,
    );
    draw_text(
        &mut quads,
        width,
        height,
        left + PAD,
        bottom + PAD,
        &detail,
        DETAIL,
        1.0,
    );
    quads
}

fn brew_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Brew", note)
}

fn grind_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Grind", note)
}

fn smith_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Smith", note)
}

fn loom_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Loom", note)
}

fn chart_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Chart", note)
}

fn lectern_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Lectern", note)
}

fn compost_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Compost", note)
}

fn hive_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Hive", note)
}

fn cauldron_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Cauldron", note)
}

fn bell_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Bell", note)
}

fn candle_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Candle", note)
}

fn lodestone_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Lodestone", note)
}

fn hang_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Sign", note)
}

fn shelf_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Shelf", note)
}

fn shelf_line(count: u8) -> String {
    match count.min(6) {
        0 => "enter stores a book".to_string(),
        1 => "1 book".to_string(),
        n => format!("{n} books"),
    }
}

fn board_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Wall shelf", note)
}

fn board_line(ids: &[String; 3], counts: &[u16; 3]) -> String {
    let parts: Vec<String> = ids
        .iter()
        .zip(counts)
        .filter(|(_, count)| **count > 0)
        .map(|(id, count)| format!("{count} {}", id.replace('_', " ")))
        .collect();
    if parts.is_empty() {
        "enter stores one".to_string()
    } else {
        parts.join(", ")
    }
}

fn pot_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Pot", note)
}

fn pot_line(id: &str, count: u16) -> String {
    if count == 0 || id.is_empty() {
        "enter stores one".to_string()
    } else {
        format!("{count} {}", id.replace('_', " "))
    }
}

fn bulb_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Bulb", note)
}

fn bulb_prompt(lit: bool) -> &'static str {
    if lit {
        "enter puts the bulb out"
    } else {
        "enter lights the bulb"
    }
}

fn target_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Target", note)
}

fn rod_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Rod", note)
}

fn anchor_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Anchor", note)
}

fn candle_prompt(lit: bool) -> &'static str {
    if lit {
        "enter puts the candle out"
    } else {
        "enter lights the candle"
    }
}

fn cut_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Cut", note)
}

fn fire_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Fire", note)
}

fn fletch_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Fletch", note)
}

fn enchant_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Enchant", note)
}

fn anvil_quads(width: f32, height: f32, note: &str) -> Vec<Quad> {
    station_note(width, height, "Anvil", note)
}

fn station_note(width: f32, height: f32, title: &str, note: &str) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let screen = inventory_screen();
    let title = brand(title);
    let panel_w = measure(&title, BRAND_SCALE) + PAD * 2.0;
    let panel_w = panel_w.max(measure(note, 1.0) + PAD * 2.0);
    let panel_h = PAD + BRAND_H + TITLE_GAP + LINE + PAD;
    let (mut quads, left, bottom, _) =
        station_chrome(width, height, &title, panel_w, panel_h, &screen);
    draw_text(
        &mut quads,
        width,
        height,
        left + PAD,
        bottom + PAD,
        note,
        INK,
        1.0,
    );
    quads
}

fn blasts_ore_or_stone(id: &str) -> bool {
    matches!(
        id,
        "cobblestone"
            | "stone"
            | "stone_bricks"
            | "cobbled_deepslate"
            | "deepslate_tiles"
            | "blackstone_bricks"
            | "basalt"
            | "polished_basalt"
            | "nether_bricks"
            | "netherrack"
    )
}

fn furnace_quads(
    width: f32,
    height: f32,
    furnace: &hotbar::Bag,
    selected: usize,
    cook: u32,
    smoker: bool,
    blast: bool,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let screen = inventory_screen();
    let cell = CREATIVE_CELL + 4.0;
    let grid_w = 3.0 * cell + 2.0 * CREATIVE_GAP;
    let title = brand(if blast {
        "Blast"
    } else if smoker {
        "Smoker"
    } else {
        "Furnace"
    });
    let goal = if smoker || blast { 10.0 } else { 20.0 };
    let detail = if blast {
        if cook > 0 {
            match furnace.slot(0) {
                Some("stone") => "blasting smooth stone".to_string(),
                Some("stone_bricks") => "blasting cracked stone bricks".to_string(),
                Some("cobbled_deepslate") => "blasting polished deepslate".to_string(),
                Some("deepslate_tiles") => "blasting cracked deepslate tiles".to_string(),
                Some("blackstone_bricks") => "blasting cracked blackstone bricks".to_string(),
                Some("basalt") | Some("polished_basalt") => "blasting smooth basalt".to_string(),
                Some("nether_bricks") => "blasting cracked nether bricks".to_string(),
                Some("netherrack") => "blasting nether bricks".to_string(),
                _ => "blasting stone".to_string(),
            }
        } else if furnace.slot(0).is_some() && furnace.slot(1).is_none() {
            "needs oak log or coal ore".to_string()
        } else if furnace.slot(0).is_some_and(|id| !blasts_ore_or_stone(id)) {
            "blasts ore and stone only".to_string()
        } else {
            "ore and stone blast faster than a furnace".to_string()
        }
    } else if smoker {
        if cook > 0 {
            "smoking cooked food".to_string()
        } else if furnace.slot(0).is_some() && furnace.slot(1).is_none() {
            "needs oak log or coal ore".to_string()
        } else if furnace.slot(0).is_some() && furnace.slot(0) != Some("raw") {
            "smokes raw food only".to_string()
        } else {
            "raw food smokes into cooked".to_string()
        }
    } else if cook > 0 {
        match furnace.slot(0) {
            Some("clay") => "cooking bricks".to_string(),
            Some("stone") => "cooking smooth stone".to_string(),
            Some("stone_bricks") => "cooking cracked stone bricks".to_string(),
            Some("cobbled_deepslate") => "cooking polished deepslate".to_string(),
            Some("deepslate_tiles") => "cooking cracked deepslate tiles".to_string(),
            Some("blackstone_bricks") => "cooking cracked blackstone bricks".to_string(),
            Some("basalt") | Some("polished_basalt") => "cooking smooth basalt".to_string(),
            Some("nether_bricks") => "cooking cracked nether bricks".to_string(),
            Some("netherrack") => "cooking nether bricks".to_string(),
            Some("raw") => "cooking cooked food".to_string(),
            _ => "cooking stone".to_string(),
        }
    } else if furnace.slot(0).is_some() && furnace.slot(1).is_none() {
        "needs oak log or coal ore".to_string()
    } else {
        "cobblestone cooks into stone, clay into bricks, stone into smooth stone, stone bricks into cracked stone bricks, cobbled deepslate into polished deepslate, deepslate tiles into cracked deepslate tiles, blackstone bricks into cracked blackstone bricks, basalt or polished basalt into smooth basalt, nether bricks into cracked nether bricks, and netherrack into nether bricks, and raw food into cooked".to_string()
    };
    let panel_w = (grid_w + PAD * 2.0).max(measure(&title, BRAND_SCALE) + PAD * 2.0);
    let panel_h = PAD + BRAND_H + TITLE_GAP + cell + 28.0 + LINE + PAD;
    let (mut quads, left, bottom, title_base) =
        station_chrome(width, height, &title, panel_w, panel_h, &screen);
    let origin_x = left + (panel_w - grid_w) * 0.5;
    let y = title_base - TITLE_GAP - cell;
    for slot in 0..3 {
        let x = origin_x + slot as f32 * (cell + CREATIVE_GAP);
        paint_slot(
            &mut quads,
            width,
            height,
            x,
            y,
            cell,
            slot == selected,
            furnace.slot(slot),
            &screen.button,
            500 + slot as u16,
        );
    }
    let bar_w = (grid_w * (cook as f32 / goal)).clamp(0.0, grid_w);
    if bar_w > 0.0 {
        quads.push(
            pixel_quad(
                width,
                height,
                origin_x,
                y - 16.0,
                origin_x + bar_w,
                y - 8.0,
                SELECT,
            )
            .face(&screen.button, 1.0, 1.0),
        );
    }
    draw_text(
        &mut quads,
        width,
        height,
        left + PAD,
        bottom + PAD,
        &detail,
        DETAIL,
        1.0,
    );
    quads
}

fn station_chrome(
    width: f32,
    height: f32,
    title: &str,
    panel_w: f32,
    panel_h: f32,
    screen: &Screen,
) -> (Vec<Quad>, f32, f32, f32) {
    let left = (width - panel_w) * 0.5;
    let bottom = ((height - panel_h) * 0.5).max(ABOVE_HOTBAR);
    let mut quads = Vec::new();
    quads.push(pixel_quad(
        width,
        height,
        left - 4.0,
        bottom - 4.0,
        left + panel_w + 4.0,
        bottom + panel_h + 4.0,
        EDGE,
    ));
    quads.push(pixel_quad(
        width,
        height,
        left - 2.0,
        bottom - 2.0,
        left + panel_w + 2.0,
        bottom + panel_h + 2.0,
        FRAME,
    ));
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + panel_w,
            bottom + panel_h,
            PANEL,
        )
        .face(&screen.panel, panel_w / 16.0, panel_h / 16.0),
    );
    let title_base = bottom + panel_h - PAD - BRAND_H;
    let header_h = PAD + BRAND_H + 8.0;
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            title_base - 8.0,
            left + panel_w,
            bottom + panel_h,
            HEADER,
        )
        .face(&screen.header, panel_w / 16.0, header_h / 16.0),
    );
    push_lip(&mut quads, width, height, left, bottom + panel_h, panel_w);
    let title_w = measure(title, BRAND_SCALE);
    let title_x = left + (panel_w - title_w) * 0.5;
    push_title_plate(&mut quads, width, height, title_x, title_base, title_w);
    draw_text(
        &mut quads,
        width,
        height,
        title_x,
        title_base,
        title,
        INK,
        BRAND_SCALE,
    );
    (quads, left, bottom, title_base)
}

fn push_lip(quads: &mut Vec<Quad>, width: f32, height: f32, left: f32, top: f32, panel_w: f32) {
    quads.push(pixel_quad(
        width,
        height,
        left,
        top - 4.0,
        left + panel_w,
        top,
        LIP,
    ));
}

fn push_title_plate(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    title_x: f32,
    title_base: f32,
    title_w: f32,
) {
    quads.push(pixel_quad(
        width,
        height,
        title_x - 8.0,
        title_base - 4.0,
        title_x + title_w + 8.0,
        title_base + BRAND_H,
        TITLE_PLATE,
    ));
}

fn paint_slot(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    cell: f32,
    chosen: bool,
    id: Option<&str>,
    button: &str,
    hit: u16,
) {
    quads.push(
        pixel_quad(
            width,
            height,
            x - 2.0,
            y - 2.0,
            x + cell + 2.0,
            y + cell + 2.0,
            if chosen { SELECT } else { ROW },
        )
        .face(button, 1.0, 1.0)
        .tag(hit),
    );
    let Some(id) = id else {
        return;
    };
    let inset = 4.0;
    hotbar::push_slot_icon(
        quads,
        width,
        height,
        x + inset,
        y + inset,
        cell - inset * 2.0,
        id,
    );
}

fn bag_detail(bag: &hotbar::Bag, index: usize) -> String {
    match bag.count(index) {
        0 => "empty slot".to_string(),
        1 => "one held".to_string(),
        64 => "full stack".to_string(),
        _ => "held stack".to_string(),
    }
}

fn load_built() -> Vec<rebuild_world::GuiScreen> {
    rebuild_world::Pack::builtin().gui_screens()
}

fn built_pressable(font: &str) -> bool {
    font != "heading" && font != "icon"
}

fn first_built_row(screen: &rebuild_world::GuiScreen) -> usize {
    screen
        .elements
        .iter()
        .position(|row| built_pressable(&row.font))
        .unwrap_or(0)
}

fn step_built_row(screen: &rebuild_world::GuiScreen, selected: usize, down: bool) -> usize {
    let count = screen.elements.len();
    if count == 0 {
        return 0;
    }
    let mut index = selected;
    for _ in 0..count {
        index = if down {
            (index + 1) % count
        } else if index == 0 {
            count - 1
        } else {
            index - 1
        };
        if built_pressable(&screen.elements[index].font) {
            return index;
        }
    }
    selected
}

fn screen_key(key: &str) -> Option<KeyCode> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    letter_code(ch)
}

fn rgb_of(packed: i32) -> [f32; 3] {
    let n = packed.max(0) as u32;
    [
        ((n / 65536) & 255) as f32 / 255.0,
        ((n / 256) & 255) as f32 / 255.0,
        (n & 255) as f32 / 255.0,
    ]
}

/// The stored plate. A chosen row keeps that color and pulls a little toward the gold highlight.
fn plate_of(packed: i32, selected: bool) -> [f32; 3] {
    let rgb = rgb_of(packed);
    if !selected {
        return rgb;
    }
    [
        rgb[0] * 0.62 + FRAME[0] * 0.38,
        rgb[1] * 0.62 + FRAME[1] * 0.38,
        rgb[2] * 0.62 + FRAME[2] * 0.38,
    ]
}

fn built_quads(
    width: f32,
    height: f32,
    screen: &rebuild_world::GuiScreen,
    selected: usize,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let mut quads = Vec::new();
    let left = screen.x;
    let bottom = height - screen.y - screen.h;
    quads.push(
        pixel_quad(
            width,
            height,
            left,
            bottom,
            left + screen.w,
            bottom + screen.h,
            PANEL,
        )
        .face("stone", 1.0, 1.0),
    );
    draw_text(
        &mut quads,
        width,
        height,
        left + 16.0,
        bottom + screen.h - 28.0,
        &title_case(&screen.name),
        INK,
        1.0,
    );
    for (index, row) in screen.elements.iter().enumerate() {
        let x = left + row.x;
        let y = height - (screen.y + row.y) - row.h;
        if row.font == "heading" {
            let ink = rgb_of(row.font_color);
            let scale = if row.font_size <= 0 {
                1.0
            } else {
                (row.font_size as f32 / 16.0).clamp(0.5, 2.0)
            };
            draw_text(&mut quads, width, height, x, y + 4.0, &row.text, ink, scale);
            quads.push(pixel_quad(width, height, x, y, x + row.w, y + 2.0, ink));
            continue;
        }
        if row.font == "field" {
            let fill = rgb_of(row.color);
            let hit = 800 + index as u16;
            quads.push(pixel_quad(width, height, x, y, x + row.w, y + row.h, fill).tag(hit));
            let lip = [
                fill[0] + (1.0 - fill[0]) * 0.35,
                fill[1] + (1.0 - fill[1]) * 0.35,
                fill[2] + (1.0 - fill[2]) * 0.35,
            ];
            quads.push(pixel_quad(
                width,
                height,
                x,
                y + row.h - 2.0,
                x + row.w,
                y + row.h,
                lip,
            ));
            let scale = if row.font_size <= 0 {
                1.0
            } else {
                (row.font_size as f32 / 16.0).clamp(0.5, 2.0)
            };
            draw_text(
                &mut quads,
                width,
                height,
                x + 8.0,
                y + 6.0,
                &row.text,
                rgb_of(row.font_color),
                scale,
            );
            if index == selected {
                quads.push(pixel_quad(
                    width,
                    height,
                    x + row.w - 6.0,
                    y + 6.0,
                    x + row.w - 3.0,
                    y + row.h - 6.0,
                    [0.93, 0.78, 0.36],
                ));
            }
            continue;
        }
        if row.font == "icon" {
            let well = rgb_of(row.color);
            quads.push(pixel_quad(width, height, x, y, x + row.w, y + row.h, well));
            let inset = 4.0;
            quads.push(
                pixel_quad(
                    width,
                    height,
                    x + inset,
                    y + inset,
                    x + row.w - inset,
                    y + row.h - inset,
                    [1.0, 1.0, 1.0],
                )
                .face(&row.text, 1.0, 1.0),
            );
            continue;
        }
        let fill = plate_of(row.color, index == selected);
        let hit = 800 + index as u16;
        if row.font == "button" {
            let lip = [fill[0] * 0.55, fill[1] * 0.55, fill[2] * 0.55];
            quads.push(pixel_quad(width, height, x, y - 4.0, x + row.w, y, lip));
        }
        quads.push(pixel_quad(width, height, x, y, x + row.w, y + row.h, fill).tag(hit));
        if row.font == "button" {
            let top = [
                fill[0] + (1.0 - fill[0]) * 0.28,
                fill[1] + (1.0 - fill[1]) * 0.28,
                fill[2] + (1.0 - fill[2]) * 0.28,
            ];
            quads.push(pixel_quad(
                width,
                height,
                x,
                y + row.h - 3.0,
                x + row.w,
                y + row.h,
                top,
            ));
        }
        let scale = if row.font_size <= 0 {
            1.0
        } else {
            (row.font_size as f32 / 16.0).clamp(0.5, 2.0)
        };
        let words = if row.font == "capital" {
            row.text.to_ascii_uppercase()
        } else {
            row.text.clone()
        };
        draw_text(
            &mut quads,
            width,
            height,
            x + 8.0,
            y + 6.0,
            &words,
            rgb_of(row.font_color),
            scale,
        );
    }
    quads
}

fn load_mod_screens() -> Vec<Screen> {
    let mut screens = Vec::new();
    for id in rebuild_world::Pack::builtin().script_screens() {
        let Some(path) = screen_asset(&id) else {
            eprintln!("screen: bad id {id}");
            continue;
        };
        let Some(text) = rebuild_world::asset_text(&path) else {
            eprintln!("screen: missing {path}");
            continue;
        };
        match Screen::parse(&text) {
            Ok(screen) => screens.push(screen),
            Err(err) => eprintln!("screen {id}: {err}"),
        }
    }
    screens
}

/// `guide:book` is `assets/guide/gui/book.json`.
fn screen_asset(id: &str) -> Option<String> {
    let (namespace, path) = id.split_once(':')?;
    if namespace.is_empty() || path.is_empty() || path.contains(':') {
        return None;
    }
    Some(format!("assets/{namespace}/gui/{path}.json"))
}

pub fn load_screen() -> Result<Screen, String> {
    load_at(&screen_file())
}

fn load_at(path: &std::path::Path) -> Result<Screen, String> {
    let root = rebuild_world::resource_pack_root();
    if !root.join("pack.json").is_file() {
        return Err(format!(
            "resource pack missing pack.json ({})",
            root.display()
        ));
    }
    let text =
        fs::read_to_string(path).map_err(|_| format!("missing screen {}", path.display()))?;
    Screen::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
}

fn field_text(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    match obj.get(key) {
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        Some(Value::String(_)) => Err(format!("screen {key} is empty")),
        Some(_) => Err(format!("screen {key} must be a string")),
        None => Err(format!("screen needs a {key}")),
    }
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn valid_title(title: &str) -> bool {
    let mut chars = title.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if title.len() > 32 || !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphabetic() || ch == ' ')
        && !title.contains("  ")
        && !title.ends_with(' ')
}

fn valid_label(label: &str) -> bool {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    label.len() <= 24 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn key_code(name: &str, what: &str) -> Result<KeyCode, String> {
    if name == "escape" {
        return Ok(KeyCode::Escape);
    }
    let mut chars = name.chars();
    let Some(ch) = chars.next() else {
        return Err(format!("screen {what} key is empty"));
    };
    if chars.next().is_none() {
        if let Some(code) = letter_code(ch) {
            return Ok(code);
        }
    }
    Err(format!(
        "screen {what} key {name} must be a letter or escape"
    ))
}

fn letter_of(code: KeyCode) -> Option<char> {
    Some(match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        _ => return None,
    })
}

fn letter_code(ch: char) -> Option<KeyCode> {
    Some(match ch {
        'a' => KeyCode::KeyA,
        'b' => KeyCode::KeyB,
        'c' => KeyCode::KeyC,
        'd' => KeyCode::KeyD,
        'e' => KeyCode::KeyE,
        'f' => KeyCode::KeyF,
        'g' => KeyCode::KeyG,
        'h' => KeyCode::KeyH,
        'i' => KeyCode::KeyI,
        'j' => KeyCode::KeyJ,
        'k' => KeyCode::KeyK,
        'l' => KeyCode::KeyL,
        'm' => KeyCode::KeyM,
        'n' => KeyCode::KeyN,
        'o' => KeyCode::KeyO,
        'p' => KeyCode::KeyP,
        'q' => KeyCode::KeyQ,
        'r' => KeyCode::KeyR,
        's' => KeyCode::KeyS,
        't' => KeyCode::KeyT,
        'u' => KeyCode::KeyU,
        'v' => KeyCode::KeyV,
        'w' => KeyCode::KeyW,
        'x' => KeyCode::KeyX,
        'y' => KeyCode::KeyY,
        'z' => KeyCode::KeyZ,
        _ => return None,
    })
}

fn brand(text: &str) -> String {
    text.to_ascii_uppercase()
}

pub fn title_case_label(text: &str) -> String {
    title_case(text)
}

fn title_case(text: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for ch in text.chars() {
        if ch == ' ' {
            cap = true;
            out.push(ch);
            continue;
        }
        if cap {
            out.push(ch.to_ascii_uppercase());
            cap = false;
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn block_height(text: &str, scale: f32) -> f32 {
    let rows = if text.chars().any(|ch| ch.is_ascii_uppercase()) {
        7.0
    } else {
        5.0
    };
    rows * CELL * scale
}

fn measure(text: &str, scale: f32) -> f32 {
    let mut width = 0.0;
    let mut spaced = false;
    for ch in text.chars() {
        let adv = glyph_advance(ch, scale);
        if adv == 0.0 {
            continue;
        }
        if spaced {
            width += GLYPH_GAP * scale;
        }
        width += adv;
        spaced = true;
    }
    width
}

fn glyph_advance(ch: char, scale: f32) -> f32 {
    if ch == ' ' {
        return 3.0 * CELL * scale;
    }
    if ch.is_ascii_uppercase() {
        return 5.0 * CELL * scale;
    }
    if ch.is_ascii_lowercase() {
        return 3.0 * CELL * scale;
    }
    0.0
}

pub(crate) fn draw_text(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    text: &str,
    color: [f32; 3],
    scale: f32,
) {
    let shadow = CELL * scale * 0.5;
    draw_glyphs(
        quads,
        width,
        height,
        x + shadow,
        y - shadow,
        text,
        SHADOW,
        scale,
    );
    draw_glyphs(quads, width, height, x, y, text, color, scale);
}

fn draw_glyphs(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    mut x: f32,
    y: f32,
    text: &str,
    color: [f32; 3],
    scale: f32,
) {
    let mut spaced = false;
    for ch in text.chars() {
        let adv = glyph_advance(ch, scale);
        if adv == 0.0 {
            continue;
        }
        if spaced {
            x += GLYPH_GAP * scale;
        }
        spaced = true;
        if ch == ' ' {
            x += adv;
            continue;
        }
        let cell = CELL * scale;
        if ch.is_ascii_uppercase() {
            if let Some(rows) = capital_rows(ch) {
                blit(quads, width, height, x, y, &rows, 5, cell, color);
            }
        } else if let Some(rows) = letter_rows(ch) {
            blit(quads, width, height, x, y, &rows, 3, cell, color);
        }
        x += adv;
    }
}

fn blit(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    bits: &[u8],
    cols: u32,
    cell: f32,
    color: [f32; 3],
) {
    let rows = bits.len();
    for (row, bits) in bits.iter().enumerate() {
        let py = y + (rows - 1 - row) as f32 * cell;
        for col in 0..cols {
            if bits & (1 << (cols - 1 - col)) == 0 {
                continue;
            }
            let px = x + col as f32 * cell;
            quads.push(pixel_quad(
                width,
                height,
                px,
                py,
                px + cell,
                py + cell,
                color,
            ));
        }
    }
}

fn letter_rows(ch: char) -> Option<[u8; 5]> {
    if ch.is_ascii_lowercase() {
        Some(LETTERS[(ch as u8 - b'a') as usize])
    } else {
        None
    }
}

fn capital_rows(ch: char) -> Option<[u8; 7]> {
    if ch.is_ascii_uppercase() {
        Some(CAPITALS[(ch as u8 - b'A') as usize])
    } else {
        None
    }
}

fn pixel_quad(
    width: f32,
    height: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 3],
) -> Quad {
    Quad {
        min: [x0 / width * 2.0 - 1.0, y0 / height * 2.0 - 1.0],
        max: [x1 / width * 2.0 - 1.0, y1 / height * 2.0 - 1.0],
        color,
        tex: 0.0,
        span: [0.0, 0.0],
        hit: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Screen {
        load_screen().unwrap()
    }

    #[test]
    fn the_shipped_screen_is_a_titled_list() {
        let path = screen_file();
        assert!(path.ends_with(std::path::Path::new("assets/terralite/gui/index.json")));
        assert!(
            rebuild_world::resource_pack_root()
                .join("pack.json")
                .is_file()
        );
        let screen = shipped();
        assert_eq!(screen.id(), "index");
        assert_eq!(screen.title(), "Items");
        assert_eq!(screen.open, KeyCode::KeyE);
        assert_eq!(screen.close, KeyCode::Escape);
        assert_eq!(
            screen.labels(),
            &[
                "stone".to_string(),
                "grass".to_string(),
                "dirt".to_string(),
                "cobblestone".to_string(),
                "oak_log".to_string(),
                "wool".to_string(),
            ]
        );
        assert!(
            Screen::parse(r#"{"id":"index","title":"Items","open":"e","close":"escape"}"#).is_err()
        );
        assert!(Screen::parse(r#"{"id":"index","title":"Items","open":"e","close":"escape","labels":[],"extra":1}"#).is_err());
        assert!(Screen::parse(r#"{"id":"index","title":"Items","open":"shift","close":"escape","labels":["stone"]}"#).is_err());
        assert!(
            Screen::parse(
                r#"{"id":"index","title":"","open":"e","close":"escape","labels":["stone"]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn e_opens_the_screen_and_escape_or_e_closes_it() {
        let mut gui = Gui::from_screen(shipped());
        assert!(!gui.is_open());
        assert!(gui.quads(1280.0, 720.0).is_empty());
        assert!(!gui.on_press(KeyCode::KeyW));
        assert!(!gui.is_open());
        assert!(gui.on_press(KeyCode::KeyE));
        assert!(gui.is_open());
        assert!(gui.player_inv_open());
        assert_eq!(gui.inv_tab(), PlayerInvTab::Player);
        gui.set_inv_tab(PlayerInvTab::Craft);
        assert_eq!(gui.inv_tab(), PlayerInvTab::Craft);
        assert!(!gui.on_press(KeyCode::Digit1));
        assert!(gui.is_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.is_open());
        assert!(!gui.player_inv_open());
        assert!(gui.on_press(KeyCode::KeyE));
        assert_eq!(gui.inv_tab(), PlayerInvTab::Player);
        assert!(gui.on_press(KeyCode::KeyE));
        assert!(!gui.is_open());
        let mut missing = Gui {
            screen: None,
            extras: Vec::new(),
            built: Vec::new(),
            built_at: None,
            pressed: None,
            book: Book::empty(),
            open: false,
            extra: None,
            selected: 0,
            bag_open: false,
            bag_selected: 0,
            inv_tab: PlayerInvTab::Player,
            catalog_on: false,
            catalog_at: 0,
            catalog_tab: 0,
            catalog_query: String::new(),
            emi_query: String::new(),
            emi_id: None,
            command_open: false,
            command_selected: 0,
            commands: Vec::new(),
            chat_open: false,
            chat_draft: String::new(),
            chat_log: Vec::new(),
            chest_open: false,
            chest_selected: 0,
            chest: hotbar::Bag::new(),
            craft_open: false,
            craft_selected: 0,
            craft: hotbar::Bag::new(),
            inv_craft: hotbar::Bag::new(),
            furnace_open: false,
            furnace_selected: 0,
            furnace: hotbar::Bag::new(),
            furnace_cook: 0,
            furnace_time: 0.0,
            smoker: false,
            blast: false,
            anvil_open: false,
            anvil_note: String::new(),
            enchant_open: false,
            enchant_note: String::new(),
            brew_open: false,
            brew_note: String::new(),
            grind_open: false,
            grind_note: String::new(),
            smith_open: false,
            smith_note: String::new(),
            loom_open: false,
            loom_note: String::new(),
            chart_open: false,
            chart_note: String::new(),
            lectern_open: false,
            lectern_note: String::new(),
            lectern_at: None,
            compost_open: false,
            compost_note: String::new(),
            compost_at: None,
            hive_open: false,
            hive_note: String::new(),
            hive_at: None,
            cauldron_open: false,
            cauldron_note: String::new(),
            cauldron_at: None,
            bell_open: false,
            bell_note: String::new(),
            bell_at: None,
            candle_open: false,
            candle_note: String::new(),
            candle_at: None,
            lodestone_open: false,
            lodestone_note: String::new(),
            lodestone_at: None,
            anchor_open: false,
            anchor_note: String::new(),
            anchor_at: None,
            rod_open: false,
            rod_note: String::new(),
            rod_at: None,
            target_open: false,
            target_note: String::new(),
            target_at: None,
            bulb_open: false,
            bulb_note: String::new(),
            bulb_at: None,
            pot_open: false,
            pot_note: String::new(),
            pot_at: None,
            pot_id: String::new(),
            pot_count: 0,
            board_open: false,
            board_note: String::new(),
            board_at: None,
            board_ids: [String::new(), String::new(), String::new()],
            board_counts: [0, 0, 0],
            shelf_open: false,
            shelf_note: String::new(),
            shelf_at: None,
            hang_open: false,
            hang_note: String::new(),
            hang_at: None,
            cut_open: false,
            cut_note: String::new(),
            fire_open: false,
            fire_note: String::new(),
            fletch_open: false,
            fletch_note: String::new(),
        };
        assert!(!missing.on_press(KeyCode::KeyE));
    }

    #[test]
    fn an_open_screen_draws_a_panel_title_and_rows_above_the_hotbar() {
        let mut gui = Gui::from_screen(shipped());
        assert!(gui.on_press(KeyCode::KeyE));
        let bag = hotbar::Bag::new();
        let closed = ui_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &bag,
            &Gui::from_screen(shipped()),
            &Menu::closed(),
            10,
            10,
            [None; 3],
        );
        // Items list is egui at runtime; legacy paint still draws the panel for tests / GUIAPI bridge.
        let open = gui.paint(1280.0 / GUI_SCALE, 720.0 / GUI_SCALE, &bag);
        let cobble = crate::texture::block_face("cobblestone");
        let stone = palette::color("stone", 0);
        assert!(cobble > 0.0);
        assert!(closed.iter().any(|quad| quad.tex == cobble));
        assert!(open.iter().any(|quad| quad.tex == cobble));
        assert!(
            closed
                .iter()
                .all(|quad| quad.color != stone && quad.color != PANEL)
        );
        let panel = open.iter().find(|quad| quad.color == PANEL).expect("panel");
        let center_x = (panel.min[0] + panel.max[0]) * 0.5;
        let center_y = (panel.min[1] + panel.max[1]) * 0.5;
        assert!(center_x.abs() < 0.05, "the panel is centered");
        assert!(center_y > -0.2, "the panel sits above the hotbar");
        let title = open.iter().filter(|quad| quad.color == INK).count();
        let labels = open.iter().filter(|quad| quad.color == LABEL).count();
        assert!(title > 10, "the title is drawn");
        assert!(open.iter().any(|quad| quad.color == WELL));
        assert!(open.iter().any(|quad| quad.color == LIP));
        assert!(open.iter().any(|quad| quad.color == TITLE_PLATE));
        assert!(labels > title, "each row has a label");
        assert!(
            open.iter().any(|quad| quad.color == DETAIL),
            "the selected recipe line is drawn"
        );
        assert!(open.len() < 32768, "the ui buffer can hold the screen");
        let stone_tex = crate::texture::block_face("stone");
        assert!(stone_tex > 0.0);
        let swatch = open
            .iter()
            .find(|quad| quad.tex == stone_tex && quad.max[0] - quad.min[0] > 0.005)
            .expect("stone swatch");
        assert!(swatch.min[1] < panel.max[1] && swatch.max[1] > panel.min[1]);
        let _ = stone;
    }

    #[test]
    fn a_hit_draws_a_red_edge_and_death_offers_respawn() {
        assert!(hurt_quads(200.0, 100.0, 0.0).is_empty());
        assert_eq!(hurt_quads(200.0, 100.0, 1.0).len(), 4);
        assert!(sleep_quads(200.0, 100.0, 0.0).is_empty());
        assert_eq!(sleep_quads(200.0, 100.0, 1.0).len(), 4);
        assert!(bite_quads(200.0, 100.0, 0.0, false).is_empty());
        assert!(crouch_quads(200.0, 100.0, 0.0).is_empty());
        let low = crouch_quads(200.0, 100.0, 1.0);
        assert!(low.iter().any(|quad| quad.color == WELL));
        assert!(low.iter().any(|quad| quad.color == INK));
        assert!(sprint_quads(200.0, 100.0, 0.0).is_empty());
        assert!(sprint_quads(200.0, 100.0, 1.0)
            .iter()
            .any(|quad| quad.color == WELL));
        assert!(water_quads(200.0, 100.0, 0.0).is_empty());
        assert_eq!(water_quads(200.0, 100.0, 1.0).len(), 4);
        assert!(starve_quads(200.0, 100.0, 0.0).is_empty());
        assert_eq!(starve_quads(200.0, 100.0, 1.0).len(), 4);
        assert!(boat_quads(200.0, 100.0, 0.0).is_empty());
        assert!(boat_quads(200.0, 100.0, 1.0)
            .iter()
            .any(|quad| quad.color == WELL));
        assert!(chew_quads(200.0, 100.0, 0.0).is_empty());
        assert!(chew_quads(200.0, 100.0, 1.0)
            .iter()
            .any(|quad| quad.color == WELL));
        assert_eq!(look_label("oak_log").as_deref(), Some("Oak Log"));
        assert_eq!(look_label("coal_ore").as_deref(), Some("Coal Ore"));
        assert_eq!(look_label("water").as_deref(), Some("Water"));
        assert_eq!(look_label("zombie").as_deref(), Some("Zombie"));
        assert_eq!(look_label("iron_pickaxe").as_deref(), Some("Iron Pickaxe"));
        assert!(look_label("air").is_none());
        assert!(look_label("").is_none());
        let bare = look_quads(200.0, 100.0, None, None, None, 1.0);
        assert_eq!(bare.len(), 4);
        assert!(bare.iter().all(|quad| quad.color != WELL));
        assert!(
            look_quads(200.0, 100.0, Some("air"), None, None, 1.0)
                .iter()
                .all(|quad| quad.color != WELL)
        );
        let named = look_quads(200.0, 100.0, Some("oak_log"), None, None, 1.0);
        let plate = named
            .iter()
            .find(|quad| quad.color == WELL)
            .expect("name plate");
        assert!(plate.min[1] < -0.5, "the look tip sits near the bottom");
        let mid_x = (plate.min[0] + plate.max[0]) * 0.5;
        assert!(
            mid_x > 0.25,
            "the look tip sits on the right, got mid={mid_x}"
        );
        assert!(named.iter().any(|quad| quad.color == LIP));
        assert!(named.iter().any(|quad| quad.color == INK));
        let held = look_quads(200.0, 400.0, None, Some("cobblestone"), None, 1.0);
        let held_plate = held
            .iter()
            .find(|quad| quad.color == WELL)
            .expect("held plate");
        assert!(
            held_plate.min[1] < 0.0,
            "the held name sits above the hotbar"
        );
        assert!((held_plate.min[0] + held_plate.max[0]).abs() < 0.05);
        assert!(held.iter().any(|quad| quad.color == LIP));
        assert!(held.iter().any(|quad| quad.color == INK));
        let both = look_quads(200.0, 400.0, Some("pig"), Some("dirt"), None, 1.0);
        assert_eq!(both.iter().filter(|quad| quad.color == WELL).count(), 2);
        assert!(look_quads(0.0, 100.0, Some("stone"), Some("dirt"), None, 1.0).is_empty());
        let shaped = look_quads(
            200.0,
            400.0,
            None,
            Some("stone"),
            Some("Stone Stairs · stairs"),
            1.0,
        );
        assert!(cross_quads(200.0, 100.0, 0.0).len() == 4);
        assert!(cross_quads(200.0, 100.0, 1.0)
            .iter()
            .any(|quad| quad.color == INK));
        assert!(shaped.iter().any(|quad| quad.color == WELL));
        let picker = shape_picker_quads(400.0, 300.0, &["block", "slab", "stairs"], 1);
        assert!(picker.iter().any(|quad| quad.color == SELECT));
        assert!(picker.iter().any(|quad| quad.color == PANEL));
        assert!(shape_picker_quads(0.0, 100.0, &["block"], 0).is_empty());
        let bite = bite_quads(200.0, 100.0, 1.0, false);
        let sip = bite_quads(200.0, 100.0, 1.0, true);
        assert_eq!(bite.len(), 1);
        assert_eq!(bite[0].color, BITE);
        assert_eq!(sip[0].color, SIP);
        let dead = death_quads(1280.0, 720.0);
        assert!(dead.iter().any(|quad| quad.hit == 700));
        assert!(dead.iter().filter(|quad| quad.color == INK).count() > 8);
    }

    #[test]
    fn a_slot_hover_names_the_item_and_a_short_line() {
        let bar = Hotbar::new();
        let bag = hotbar::Bag::new();
        let gui = Gui::from_screen(shipped());
        let tip = hover_tip(&gui, &bar, &bag, 600).expect("slot one");
        assert_eq!(tip.name, "Cobblestone");
        assert_eq!(tip.line, "Place on a face");
        assert!(hover_tip(&gui, &bar, &bag, 601).is_none());

        let mut creative = Gui::from_screen(shipped());
        creative.bag_open = true;
        let first = creative_view(0, "").first().cloned().expect("catalog");
        let shown = hover_tip(&creative, &bar, &bag, 1000).expect("creative cell");
        assert_eq!(
            shown.name,
            title_case(&crate::recipes::display_name(&first))
        );
        assert!(!shown.line.is_empty());

        let apple = item_tip("apple", None, false);
        assert_eq!(apple.name, "Apple");
        assert_eq!(apple.line, "Fills hunger");
        assert_eq!(apple.rarity, Rarity::Common);
        assert_eq!(apple.description, apple.line);

        let near = tooltip_quads(400.0, 300.0, 20.0, 40.0, "Cobblestone", "Place on a face");
        assert!(near.iter().any(|quad| quad.color == INK));
        assert!(
            near.iter()
                .any(|quad| quad.min[0] > 20.0 / 400.0 * 2.0 - 1.0)
        );
        let far = tooltip_quads(400.0, 300.0, 380.0, 40.0, "Cobblestone", "Place on a face");
        let far_right = far.iter().map(|quad| quad.max[0]).fold(f32::MIN, f32::max);
        assert!(far_right < 380.0 / 400.0 * 2.0 - 1.0);
    }

    #[test]
    fn chat_stays_off_the_status_stack_and_under_inventory() {
        let mut gui = Gui::from_screen(shipped());
        gui.push_chat("line");
        let chat = gui.chat_quads(1280.0, 720.0);
        assert!(!chat.is_empty());
        let chat_bottom = chat
            .iter()
            .map(|quad| quad.min[1])
            .fold(f32::MAX, f32::min);
        let armor_top = {
            let tray = armor_quads(1280.0, 720.0, [Some("leather_cap"), None, None]);
            tray.iter()
                .map(|quad| quad.max[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(
            chat_bottom >= armor_top - 0.02,
            "chat sits above the armor/status stack"
        );
        assert!(gui.on_press(KeyCode::KeyE));
        assert!(gui.player_inv_open());
        let bag = hotbar::Bag::new();
        let hud = feel_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &bag,
            &gui,
            &Menu::closed(),
            10,
            10,
            [None; 3],
            0.0,
            0.0,
            0.0,
            false,
            false,
        );
        // Inventory open: no chat plate (would read as a "Line" HUD placeholder).
        assert!(
            !hud.iter().any(|quad| quad.color == PANEL && {
                let mid = (quad.min[0] + quad.max[0]) * 0.5;
                mid < -0.4
            }),
            "chat panel stays hidden while the inventory covers the HUD"
        );
    }

    #[test]
    fn hunger_and_health_sit_bottom_left_and_empties() {
        let heart = [0.75, 0.16, 0.14];
        let full = [0.86, 0.62, 0.16];
        let empty = [0.28, 0.18, 0.08];
        let bag = hotbar::Bag::new();
        let fed = ui_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &bag,
            &Gui::from_screen(shipped()),
            &Menu::closed(),
            10,
            10,
            [None; 3],
        );
        let starved = ui_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &bag,
            &Gui::from_screen(shipped()),
            &Menu::closed(),
            10,
            0,
            [None; 3],
        );
        assert_eq!(fed.iter().filter(|quad| quad.color == full).count(), 10);
        assert_eq!(fed.iter().filter(|quad| quad.color == heart).count(), 10);
        assert_eq!(
            starved.iter().filter(|quad| quad.color == empty).count(),
            10
        );
        assert!(
            fed.iter()
                .filter(|quad| quad.color == full || quad.color == heart)
                .all(|quad| {
                    let mid = (quad.min[0] + quad.max[0]) * 0.5;
                    mid < -0.2
                }),
            "hunger and health sit on the bottom-left"
        );
        assert!(
            fed.iter().filter(|quad| quad.color == TRAY).count() >= 2,
            "hearts and hunger each sit on a plate"
        );
        let trays: Vec<_> = fed.iter().filter(|quad| quad.color == TRAY).collect();
        assert!(
            trays.iter().all(|quad| {
                let mid = (quad.min[0] + quad.max[0]) * 0.5;
                mid < -0.2
            }),
            "both status trays stay left"
        );
        let cap = palette::color("leather_cap", 0);
        let dressed = ui_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &bag,
            &Gui::from_screen(shipped()),
            &Menu::closed(),
            10,
            10,
            [Some("leather_cap"), None, None],
        );
        assert!(dressed.iter().any(|quad| quad.color == cap));
    }

    #[test]
    fn hud_layout_keeps_crosshair_hotbar_and_plates_apart() {
        let (gw, gh) = gui_space(1280.0, 720.0);
        let bar = Hotbar::new();
        let bag = hotbar::Bag::new();
        let hud = ui_quads(
            1280.0,
            720.0,
            &bar,
            &bag,
            &Gui::from_screen(shipped()),
            &Menu::closed(),
            10,
            10,
            [None; 3],
        );
        let look = look_quads(gw, gh, Some("oak_log"), Some("cobblestone"), None, 1.0);
        // First four quads are the center cross; later INK/EDGE are tip glyphs.
        assert!(look.len() >= 4);
        let cross = &look[..4];
        assert!(cross.iter().all(|quad| quad.color == INK || quad.color == EDGE));
        let cross_mid_x: f32 = cross
            .iter()
            .map(|quad| (quad.min[0] + quad.max[0]) * 0.5)
            .sum::<f32>()
            / cross.len() as f32;
        let cross_mid_y: f32 = cross
            .iter()
            .map(|quad| (quad.min[1] + quad.max[1]) * 0.5)
            .sum::<f32>()
            / cross.len() as f32;
        assert!(cross_mid_x.abs() < 0.05, "crosshair is centered on x");
        assert!(cross_mid_y.abs() < 0.05, "crosshair is centered on y");

        let bar_quads = hotbar::bar_quads(gw, gh, &bar);
        let bar_mid = {
            let first = bar_quads.first().expect("hotbar chrome");
            (first.min[0] + first.max[0]) * 0.5
        };
        assert!(bar_mid.abs() < 0.05, "hotbar is bottom-center");
        let bar_span = bar_quads[0].max[0] - bar_quads[0].min[0];
        assert!(
            bar_span > 0.8 && bar_span < 1.4,
            "×10 hotbar spans the bottom center"
        );
        let bar_left = bar_quads[0].min[0];
        let bar_right = bar_quads[0].max[0];

        let trays: Vec<_> = hud.iter().filter(|quad| quad.color == TRAY).collect();
        assert!(trays.len() >= 2);
        for tray in &trays {
            let mid = (tray.min[0] + tray.max[0]) * 0.5;
            assert!(mid < -0.2, "status tray stays left, mid={mid}");
            // Above the hotbar chrome in NDC (bar top ≈ bar_chrome_top / height).
            let bar_top_ndc = hotbar::bar_chrome_top() / gh * 2.0 - 1.0;
            assert!(
                tray.min[1] > bar_top_ndc - 0.02,
                "status tray clears the hotbar"
            );
            assert!(
                tray.max[0] < bar_left + 0.02,
                "status tray does not overlap the hotbar horizontally"
            );
        }

        let tip = look
            .iter()
            .find(|quad| quad.color == WELL)
            .expect("look tip");
        let tip_mid = (tip.min[0] + tip.max[0]) * 0.5;
        assert!(tip_mid > 0.25, "look tip stays bottom-right");
        assert!(tip.min[1] < -0.5, "look tip sits near the bottom");
        assert!(
            tip.min[0] > bar_right - 0.02,
            "look tip clears the hotbar on the right"
        );
        for tray in &trays {
            assert!(
                tip.min[0] > tray.max[0] + 0.05,
                "look tip does not overlap status trays"
            );
        }
    }

    #[test]
    fn the_pointer_clicks_buttons_slots_and_scrolls_lists() {
        let mut menu = Menu::new();
        let quads = menu.quads(1280.0, 720.0);
        let play = quads
            .iter()
            .find(|quad| quad.hit == 1)
            .expect("play button");
        let x = (play.min[0] + play.max[0]) * 0.5;
        let y = (play.min[1] + play.max[1]) * 0.5;
        let px = (x + 1.0) * 0.5 * 1280.0;
        let py = 720.0 - (y + 1.0) * 0.5 * 720.0;
        assert_eq!(quad_hit(&quads, px, py, 1280.0, 720.0), 1);
        assert_eq!(menu.pointer_click(1), MenuAction::Play);
        assert!(!menu.is_open());

        let book = recipes::load_book().unwrap();
        let mut gui = Gui::from_parts(shipped(), book);
        assert!(gui.on_press(KeyCode::KeyE));
        let first = gui.selected_id();
        gui.pointer_scroll(true);
        assert_ne!(gui.selected_id(), first);
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.on_press(KeyCode::KeyI));
        assert_eq!(gui.pointer_click(1000), Click::Give);
        assert!(gui.creative_focused());
        gui.pointer_scroll(true);
        assert_eq!(gui.pointer_click(200), Click::TakeBag);
        assert!(!gui.creative_focused());
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_craft());
        assert_eq!(gui.pointer_click(402), Click::None);
        assert_eq!(gui.pointer_click(409), Click::TakeCraft);
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_furnace());
        assert_eq!(gui.pointer_click(502), Click::TakeFurnace);
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_chest(&[]));
        assert_eq!(gui.pointer_click(304), Click::TakeChest);
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_anvil());
        assert!(gui.is_open());
        assert_eq!(gui.screen_id(), "anvil");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.anvil_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.anvil_open());

        assert!(gui.open_enchanting());
        assert_eq!(gui.screen_id(), "enchanting");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.enchant_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.enchant_open());

        assert!(gui.open_brewing());
        assert_eq!(gui.screen_id(), "brewing");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.brew_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.brew_open());

        assert!(gui.open_grind());
        assert_eq!(gui.screen_id(), "grindstone");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.grind_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.grind_open());

        assert!(gui.open_smith());
        assert_eq!(gui.screen_id(), "smithing");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.smith_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.smith_open());

        assert!(gui.open_loom());
        assert_eq!(gui.screen_id(), "loom");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.loom_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.loom_open());

        assert!(gui.open_chart());
        assert_eq!(gui.screen_id(), "cartography");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.chart_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.chart_open());

        assert!(gui.open_lectern([2, 1, 2], false));
        assert_eq!(gui.screen_id(), "lectern");
        assert_eq!(gui.lectern_at(), Some([2, 1, 2]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.lectern_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.lectern_open());
        assert!(gui.open_lectern([1, 1, 1], true));
        gui.note_lectern(LECTERN_PAGE);
        assert_eq!(gui.screen_id(), "lectern");
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_compost([3, 1, 3], 0));
        assert_eq!(gui.screen_id(), "composter");
        assert_eq!(gui.compost_at(), Some([3, 1, 3]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.compost_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.compost_open());
        assert!(gui.open_compost([3, 1, 3], 3));
        gui.note_compost("level 3");
        assert_eq!(gui.screen_id(), "composter");
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_hive([4, 1, 4], 0));
        assert_eq!(gui.screen_id(), "beehive");
        assert_eq!(gui.hive_at(), Some([4, 1, 4]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.hive_open());
        gui.note_hive("level 1");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.hive_open());
        assert!(gui.open_hive([4, 1, 4], 3));
        assert_eq!(gui.screen_id(), "beehive");
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_cauldron([5, 1, 5], 0));
        assert_eq!(gui.screen_id(), "cauldron");
        assert_eq!(gui.cauldron_at(), Some([5, 1, 5]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.cauldron_open());
        gui.note_cauldron("level 1");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.cauldron_open());
        assert!(gui.open_bell([6, 1, 6]));
        assert_eq!(gui.screen_id(), "bell");
        assert_eq!(gui.bell_at(), Some([6, 1, 6]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.bell_open());
        gui.note_bell("ring");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.bell_open());
        assert!(gui.open_candle([7, 1, 7], false));
        assert_eq!(gui.screen_id(), "candle");
        assert_eq!(gui.candle_at(), Some([7, 1, 7]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.candle_open());
        gui.note_candle("lit");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.candle_open());
        assert!(gui.open_lodestone([8, 1, 8]));
        assert_eq!(gui.screen_id(), "lodestone");
        assert_eq!(gui.lodestone_at(), Some([8, 1, 8]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.lodestone_open());
        gui.note_lodestone("linked");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.lodestone_open());
        assert!(gui.open_anchor([9, 1, 9], 0));
        assert_eq!(gui.screen_id(), "anchor");
        assert_eq!(gui.anchor_at(), Some([9, 1, 9]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.anchor_open());
        gui.note_anchor("charge 1");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.anchor_open());
        assert!(gui.open_rod([3, 2, 3]));
        assert_eq!(gui.screen_id(), "rod");
        assert_eq!(gui.rod_at(), Some([3, 2, 3]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.rod_open());
        gui.note_rod("strike");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.rod_open());
        assert!(gui.open_target([4, 2, 4]));
        assert_eq!(gui.screen_id(), "target");
        assert_eq!(gui.target_at(), Some([4, 2, 4]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.target_open());
        gui.note_target("hit");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.target_open());
        assert!(gui.open_bulb([5, 2, 5], false));
        assert_eq!(gui.screen_id(), "bulb");
        assert_eq!(gui.bulb_at(), Some([5, 2, 5]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.bulb_open());
        gui.note_bulb("lit");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.bulb_open());
        assert!(gui.open_shelf([7, 2, 7], 2));
        assert_eq!(gui.screen_id(), "shelf");
        assert_eq!(gui.shelf_at(), Some([7, 2, 7]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.shelf_open());
        gui.note_shelf("3 books");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.shelf_open());
        assert!(gui.open_hang([8, 2, 8]));
        assert_eq!(gui.screen_id(), "hang");
        assert_eq!(gui.hang_at(), Some([8, 2, 8]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.hang_open());
        gui.note_hang(HANGING_LINE);
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.hang_open());
        assert!(gui.open_pot([6, 2, 6], "", 0));
        assert_eq!(gui.screen_id(), "pot");
        assert_eq!(gui.pot_at(), Some([6, 2, 6]));
        assert_eq!(gui.pot_stack(), (String::new(), 0));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.pot_open());
        gui.set_pot("clay", 1);
        gui.note_pot("stored");
        assert_eq!(gui.pot_stack(), ("clay".to_string(), 1));
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.pot_open());
        let empty = [(String::new(), 0), (String::new(), 0), (String::new(), 0)];
        assert!(gui.open_board([4, 2, 4], &empty));
        assert_eq!(gui.screen_id(), "board");
        assert_eq!(gui.board_at(), Some([4, 2, 4]));
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        gui.set_board_slot(0, "apple", 2);
        assert_eq!(gui.board_slots()[0], ("apple".to_string(), 2));
        gui.note_board("stored");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.board_open());

        assert!(gui.open_cut());
        assert_eq!(gui.screen_id(), "stonecutter");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.cut_open());
        gui.note_cut("slab");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.cut_open());

        assert!(gui.open_fire());
        assert_eq!(gui.screen_id(), "campfire");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.fire_open());
        gui.note_fire("cooking");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.fire_open());

        assert!(gui.open_fletch());
        assert_eq!(gui.screen_id(), "fletching");
        assert!(!gui.quads(1280.0, 720.0).is_empty());
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.fletch_open());
        gui.note_fletch("fletched");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.fletch_open());
    }

    #[test]
    fn arrows_select_a_recipe_or_a_use() {
        let book = recipes::load_book().unwrap();
        let mut gui = Gui::from_parts(shipped(), book);
        assert!(gui.on_press(KeyCode::KeyE));
        assert_eq!(gui.selected_id().as_deref(), Some("anvil"));
        assert_eq!(gui.selected_detail(), "iron ore and iron ore make anvil");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("apple"));
        assert_eq!(gui.selected_detail(), "grass makes apple");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("arrow"));
        assert_eq!(
            gui.selected_detail(),
            "stick and wool and gravel make arrow"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("barrel"));
        assert_eq!(gui.selected_detail(), "chest and slab make barrel");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("basalt"));
        assert_eq!(
            gui.selected_detail(),
            "blackstone and deepslate make basalt"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("bed"));
        assert_eq!(gui.selected_detail(), "oak log and wool make bed");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("beehive"));
        assert_eq!(gui.selected_detail(), "flower and hay bale make beehive");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("bell"));
        assert_eq!(gui.selected_detail(), "cauldron and stick make bell");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("blackstone"));
        assert_eq!(gui.selected_detail(), "coal ore and stone make blackstone");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("blackstone_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "polished blackstone makes blackstone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("blast_furnace"));
        assert_eq!(gui.selected_detail(), "smoker and stone make blast furnace");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("boat"));
        assert_eq!(gui.selected_detail(), "grass and oak log make boat");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("book"));
        assert_eq!(gui.selected_detail(), "bookshelf and wool make book");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("bookshelf"));
        assert_eq!(
            gui.selected_detail(),
            "oak log and wool and stick make bookshelf"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("breeze_rod"));
        assert_eq!(
            gui.selected_detail(),
            "stick and chorus fruit make breeze rod"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("brewing_stand"));
        assert_eq!(
            gui.selected_detail(),
            "nether wart block and iron ore make brewing stand"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("bricks"));
        assert_eq!(gui.selected_detail(), "clay makes bricks");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("brush"));
        assert_eq!(gui.selected_detail(), "wool and gravel make brush");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("bucket"));
        assert_eq!(gui.selected_detail(), "iron ore makes bucket");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("button"));
        assert_eq!(gui.selected_detail(), "stone and fence make button");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cactus"));
        assert_eq!(gui.selected_detail(), "sand and stick make cactus");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cake"));
        assert_eq!(gui.selected_detail(), "apple and wool and bucket make cake");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("campfire"));
        assert_eq!(gui.selected_detail(), "hay bale and coal ore make campfire");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("candle"));
        assert_eq!(gui.selected_detail(), "campfire and stick make candle");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("carpet"));
        assert_eq!(gui.selected_detail(), "wool and bucket make carpet");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cartography_table"));
        assert_eq!(
            gui.selected_detail(),
            "map and stick make cartography table"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cauldron"));
        assert_eq!(gui.selected_detail(), "bucket and stone make cauldron");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chain"));
        assert_eq!(gui.selected_detail(), "bell and candle make chain");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chest"));
        assert_eq!(
            gui.selected_detail(),
            "used to make barrel and copper chest and ender chest and shulker box"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chiseled_blackstone"));
        assert_eq!(
            gui.selected_detail(),
            "blackstone bricks and blackstone bricks make chiseled blackstone"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chiseled_bookshelf"));
        assert_eq!(
            gui.selected_detail(),
            "bookshelf and book make chiseled bookshelf"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chiseled_nether_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "nether bricks and nether bricks make chiseled nether bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chiseled_stone_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "stone bricks and stone bricks make chiseled stone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chorus_flower"));
        assert_eq!(
            gui.selected_detail(),
            "chorus plant and flower make chorus flower"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chorus_fruit"));
        assert_eq!(
            gui.selected_detail(),
            "chorus plant and chorus flower make chorus fruit"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("chorus_plant"));
        assert_eq!(
            gui.selected_detail(),
            "end stone and purpur make chorus plant"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("clay"));
        assert_eq!(gui.selected_detail(), "dirt and sand make clay");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("clock"));
        assert_eq!(gui.selected_detail(), "grass and iron ore make clock");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("coal_ore"));
        assert_eq!(
            gui.selected_detail(),
            "used to make blackstone and campfire and dark prismarine and torch"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cobbled_deepslate"));
        assert_eq!(
            gui.selected_detail(),
            "deepslate and deepslate make cobbled deepslate"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cobblestone"));
        assert_eq!(
            gui.selected_detail(),
            "used to make compass and mossy cobblestone and saddle and shield and smooth stone and stone and stone pickaxe"
        );
        let first = gui.quads(1280.0, 720.0);
        let first_y = first
            .iter()
            .find(|quad| quad.color == SELECT)
            .expect("selected row")
            .min[1];
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cobweb"));
        assert_eq!(gui.selected_detail(), "wool and tripwire make cobweb");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("compass"));
        assert_eq!(
            gui.selected_detail(),
            "iron ore and cobblestone make compass"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("composter"));
        assert_eq!(gui.selected_detail(), "slab and fence make composter");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_bulb"));
        assert_eq!(
            gui.selected_detail(),
            "lightning rod and glowstone make copper bulb"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_chest"));
        assert_eq!(
            gui.selected_detail(),
            "chest and copper grate make copper chest"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_door"));
        assert_eq!(
            gui.selected_detail(),
            "copper grate and door make copper door"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_grate"));
        assert_eq!(
            gui.selected_detail(),
            "copper bulb and chain make copper grate"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_trapdoor"));
        assert_eq!(
            gui.selected_detail(),
            "copper door and trapdoor make copper trapdoor"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(
            gui.selected_id().as_deref(),
            Some("cracked_blackstone_bricks")
        );
        assert_eq!(
            gui.selected_detail(),
            "blackstone bricks makes cracked blackstone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(
            gui.selected_id().as_deref(),
            Some("cracked_deepslate_tiles")
        );
        assert_eq!(
            gui.selected_detail(),
            "deepslate tiles makes cracked deepslate tiles"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cracked_nether_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "nether bricks makes cracked nether bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("cracked_stone_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "stone bricks makes cracked stone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("dark_prismarine"));
        assert_eq!(
            gui.selected_detail(),
            "prismarine and coal ore make dark prismarine"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("decorated_pot"));
        assert_eq!(gui.selected_detail(), "bricks and clay make decorated pot");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate"));
        assert_eq!(
            gui.selected_detail(),
            "used to make basalt and cobbled deepslate and deepslate bricks and heavy core and mace"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate_bricks"));
        assert_eq!(gui.selected_detail(), "deepslate makes deepslate bricks");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate_tiles"));
        assert_eq!(
            gui.selected_detail(),
            "polished deepslate makes deepslate tiles"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("dirt"));
        assert_eq!(
            gui.selected_detail(),
            "used to make clay and flower pot and hay bale and sand"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("door"));
        assert_eq!(gui.selected_detail(), "oak log and stone make door");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("dragon_egg"));
        assert_eq!(
            gui.selected_detail(),
            "end portal frame and eye of ender make dragon egg"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("enchanting_table"));
        assert_eq!(
            gui.selected_detail(),
            "bookshelf and sea lantern make enchanting table"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("end_crystal"));
        assert_eq!(
            gui.selected_detail(),
            "dragon egg and glowstone make end crystal"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("end_portal_frame"));
        assert_eq!(
            gui.selected_detail(),
            "end stone bricks and purpur make end portal frame"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("end_rod"));
        assert_eq!(gui.selected_detail(), "purpur and glowstone make end rod");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("end_stone"));
        assert_eq!(gui.selected_detail(), "quartz and sandstone make end stone");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("end_stone_bricks"));
        assert_eq!(gui.selected_detail(), "end stone makes end stone bricks");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("ender_chest"));
        assert_eq!(
            gui.selected_detail(),
            "chest and eye of ender make ender chest"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("ender_pearl"));
        assert_eq!(
            gui.selected_detail(),
            "end stone and chorus fruit make ender pearl"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("eye_of_ender"));
        assert_eq!(
            gui.selected_detail(),
            "ender pearl and glowstone make eye of ender"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("fence"));
        assert_eq!(gui.selected_detail(), "stick makes fence");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("fence_gate"));
        assert_eq!(gui.selected_detail(), "stick and fence make fence gate");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("fletching_table"));
        assert_eq!(
            gui.selected_detail(),
            "gravel and hay bale make fletching table"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("flower"));
        assert_eq!(gui.selected_detail(), "grass and wool make flower");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("flower_pot"));
        assert_eq!(gui.selected_detail(), "stone and dirt make flower pot");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("gilded_blackstone"));
        assert_eq!(
            gui.selected_detail(),
            "blackstone and iron ore make gilded blackstone"
        );
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("glass"));
        assert_eq!(gui.selected_detail(), "iron ore and stone make glass");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("glowstone"));
        assert_eq!(gui.selected_detail(), "lantern and sand make glowstone");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("goat_horn"));
        assert_eq!(gui.selected_detail(), "note block and stick make goat horn");
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_id().as_deref(), Some("grass"));
        assert_eq!(
            gui.selected_detail(),
            "used to make apple and boat and clock and flower and hay bale"
        );
        let second = gui.quads(1280.0, 720.0);
        let second_y = second
            .iter()
            .find(|quad| quad.color == SELECT)
            .expect("next row")
            .min[1];
        assert!(
            second_y < first_y,
            "down moves the highlight to the next row"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("goat_horn"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("glowstone"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("glass"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("gilded_blackstone"));
        assert_eq!(
            gui.selected_detail(),
            "blackstone and iron ore make gilded blackstone"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("flower_pot"));
        assert_eq!(gui.selected_detail(), "stone and dirt make flower pot");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("flower"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("fletching_table"));
        assert_eq!(
            gui.selected_detail(),
            "gravel and hay bale make fletching table"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("fence_gate"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("fence"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("eye_of_ender"));
        assert_eq!(
            gui.selected_detail(),
            "ender pearl and glowstone make eye of ender"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("ender_pearl"));
        assert_eq!(
            gui.selected_detail(),
            "end stone and chorus fruit make ender pearl"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("ender_chest"));
        assert_eq!(
            gui.selected_detail(),
            "chest and eye of ender make ender chest"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("end_stone_bricks"));
        assert_eq!(gui.selected_detail(), "end stone makes end stone bricks");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("end_stone"));
        assert_eq!(gui.selected_detail(), "quartz and sandstone make end stone");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("end_rod"));
        assert_eq!(gui.selected_detail(), "purpur and glowstone make end rod");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("end_portal_frame"));
        assert_eq!(
            gui.selected_detail(),
            "end stone bricks and purpur make end portal frame"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("end_crystal"));
        assert_eq!(
            gui.selected_detail(),
            "dragon egg and glowstone make end crystal"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("enchanting_table"));
        assert_eq!(
            gui.selected_detail(),
            "bookshelf and sea lantern make enchanting table"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("dragon_egg"));
        assert_eq!(
            gui.selected_detail(),
            "end portal frame and eye of ender make dragon egg"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("door"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("dirt"));
        assert_eq!(
            gui.selected_detail(),
            "used to make clay and flower pot and hay bale and sand"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate_tiles"));
        assert_eq!(
            gui.selected_detail(),
            "polished deepslate makes deepslate tiles"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate_bricks"));
        assert_eq!(gui.selected_detail(), "deepslate makes deepslate bricks");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("deepslate"));
        assert_eq!(
            gui.selected_detail(),
            "used to make basalt and cobbled deepslate and deepslate bricks and heavy core and mace"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("decorated_pot"));
        assert_eq!(gui.selected_detail(), "bricks and clay make decorated pot");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("dark_prismarine"));
        assert_eq!(
            gui.selected_detail(),
            "prismarine and coal ore make dark prismarine"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("cracked_stone_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "stone bricks makes cracked stone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("cracked_nether_bricks"));
        assert_eq!(
            gui.selected_detail(),
            "nether bricks makes cracked nether bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(
            gui.selected_id().as_deref(),
            Some("cracked_deepslate_tiles")
        );
        assert_eq!(
            gui.selected_detail(),
            "deepslate tiles makes cracked deepslate tiles"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(
            gui.selected_id().as_deref(),
            Some("cracked_blackstone_bricks")
        );
        assert_eq!(
            gui.selected_detail(),
            "blackstone bricks makes cracked blackstone bricks"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_trapdoor"));
        assert_eq!(
            gui.selected_detail(),
            "copper door and trapdoor make copper trapdoor"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_grate"));
        assert_eq!(
            gui.selected_detail(),
            "copper bulb and chain make copper grate"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_door"));
        assert_eq!(
            gui.selected_detail(),
            "copper grate and door make copper door"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_chest"));
        assert_eq!(
            gui.selected_detail(),
            "chest and copper grate make copper chest"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("copper_bulb"));
        assert_eq!(
            gui.selected_detail(),
            "lightning rod and glowstone make copper bulb"
        );
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("composter"));
        assert_eq!(gui.selected_detail(), "slab and fence make composter");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("compass"));
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("cobweb"));
        assert_eq!(gui.selected_detail(), "wool and tripwire make cobweb");
        assert!(gui.on_press(KeyCode::ArrowUp));
        assert_eq!(gui.selected_id().as_deref(), Some("cobblestone"));
        while gui.selected_id().as_deref() != Some("oak_stairs") {
            assert!(gui.on_press(KeyCode::ArrowDown));
        }
        assert_eq!(gui.selected_detail(), "oak log makes oak stairs");
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.is_open());
    }

    #[test]
    fn the_guide_screen_opens_on_g_and_the_bag_lists_a_stack() {
        let text =
            fs::read_to_string(r"C:\CODE\TerraLite\mods\k's-guide\assets\guide\gui\book.json")
                .unwrap();
        let book = Screen::parse(&text).unwrap();
        assert_eq!(book.title(), "Guide");
        assert_eq!(book.open, KeyCode::KeyG);
        assert_eq!(
            book.labels(),
            &[
                "marker".to_string(),
                "page".to_string(),
                "post".to_string(),
                "slip".to_string(),
                "night".to_string(),
                "chest".to_string(),
                "key".to_string()
            ]
        );
        let mut gui = Gui::from_screen(shipped());
        gui.push_screen(book);
        assert!(gui.on_press(KeyCode::KeyG));
        assert!(gui.is_open());
        assert_eq!(gui.screen_id(), "book");
        assert_eq!(gui.selected_id().as_deref(), Some("marker"));
        assert!(gui.craft_job().is_none());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.is_open());

        let mut bag = hotbar::Bag::new();
        assert!(bag.collect("dirt"));
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(gui.bag_open());
        assert_eq!(gui.screen_id(), "inventory");
        let drawn = gui.paint(1280.0, 720.0, &bag);
        let dirt = crate::texture::block_face("dirt");
        assert!(dirt > 0.0 && drawn.iter().any(|quad| quad.tex == dirt));
        assert!(drawn.iter().any(|quad| quad.color == INK));
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.bag_open());
        assert_eq!(gui.bag_selected(), 0);
        let pick = gui.creative_pick().expect("creative cursor");
        let pick_tex = crate::texture::block_face(&pick);
        assert!(
            pick_tex > 0.0 && drawn.iter().any(|quad| quad.tex == pick_tex),
            "creative grid shows a textured pick"
        );
        assert!(
            drawn
                .iter()
                .any(|quad| quad.color == PANEL && quad.tex > 0.0)
        );
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert_ne!(gui.creative_pick().as_deref(), Some(pick.as_str()));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert_eq!(gui.creative_pick().as_deref(), Some(pick.as_str()));
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(!gui.bag_open());

        assert!(gui.on_press(KeyCode::KeyI));
        assert!(gui.on_press(KeyCode::KeyD));
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(gui.on_press(KeyCode::KeyR));
        assert!(gui.on_press(KeyCode::KeyT));
        assert_eq!(gui.creative_pick().as_deref(), Some("dirt"));
        assert!(gui.on_press(KeyCode::Backspace));
        assert!(gui.on_press(KeyCode::Backspace));
        assert!(gui.on_press(KeyCode::Backspace));
        assert!(gui.on_press(KeyCode::Backspace));
        let all_first = creative_view(0, "").first().cloned().expect("catalog");
        assert_eq!(gui.creative_pick().as_deref(), Some(all_first.as_str()));
        assert!(gui.on_press(KeyCode::Tab));
        let build_first = creative_view(1, "").first().cloned().expect("build tab");
        assert_eq!(gui.creative_pick().as_deref(), Some(build_first.as_str()));
        assert!(gui.on_press(KeyCode::Tab));
        let nature_first = creative_view(2, "").first().cloned().expect("nature tab");
        assert_eq!(gui.creative_pick().as_deref(), Some(nature_first.as_str()));
        assert!(gui.on_press(KeyCode::BracketLeft));
        assert!(gui.on_press(KeyCode::BracketLeft));
        assert_eq!(gui.creative_pick().as_deref(), Some(all_first.as_str()));
        let sought = gui.paint(1280.0, 720.0, &bag);
        assert!(
            sought
                .iter()
                .any(|quad| quad.color == SELECT && quad.tex > 0.0)
        );
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(!gui.bag_open());

        assert!(creative_view(0, "oak log").iter().any(|id| id == "oak_log"));
        assert!(creative_view(0, "oaklog").iter().any(|id| id == "oak_log"));
        assert!(creative_view(0, "log oak").iter().any(|id| id == "oak_log"));
        assert!(creative_view(0, "zzzz").is_empty());
        assert_eq!(creative_empty_line(""), "no blocks");
        assert_eq!(creative_empty_line("zzzz"), "nothing matches");
        assert_eq!(CREATIVE_COLS, 10);
        assert_eq!(Gui::catalog_tab_count(), TABS.len());
        let mut cycle = Gui::from_screen(shipped());
        cycle.cycle_catalog_tab(false);
        assert_eq!(cycle.catalog_tab(), TABS.len() - 1);
        cycle.cycle_catalog_tab(true);
        assert_eq!(cycle.catalog_tab(), 0);
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(gui.on_press(KeyCode::KeyO));
        assert!(gui.on_press(KeyCode::KeyA));
        assert!(gui.on_press(KeyCode::KeyK));
        assert!(gui.on_press(KeyCode::Space));
        assert!(gui.on_press(KeyCode::KeyL));
        assert!(gui.on_press(KeyCode::KeyO));
        assert!(gui.on_press(KeyCode::KeyG));
        assert_eq!(gui.creative_pick().as_deref(), Some("oak_log"));
        for _ in 0..7 {
            assert!(gui.on_press(KeyCode::Backspace));
        }
        for _ in 0..4 {
            assert!(gui.on_press(KeyCode::KeyZ));
        }
        assert!(gui.creative_pick().is_none());
        let blank = gui.paint(1280.0, 720.0, &bag);
        assert!(blank.iter().any(|quad| quad.color == INK));
        assert!(blank.iter().any(|quad| quad.color == WELL));
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.bag_open());

        let recipes = recipes::load_book().unwrap();
        let mut items = Gui::from_parts(shipped(), recipes);
        assert!(items.on_press(KeyCode::KeyE));
        while items.selected_id().as_deref() != Some("stone") {
            assert!(items.on_press(KeyCode::ArrowDown));
        }
        assert_eq!(
            items.craft_job(),
            Some(("stone".to_string(), vec!["cobblestone".to_string()]))
        );
    }

    #[test]
    fn a_chest_draws_atlas_slots() {
        let mut gui = Gui::from_screen(shipped());
        assert!(gui.open_chest(&[rebuild_world::CarriedSlot {
            slot: 4,
            id: "dirt".into(),
            count: 4,
        }]));
        assert!(gui.chest_open());
        let drawn = gui.paint(1280.0, 720.0, &hotbar::Bag::new());
        let dirt = crate::texture::block_face("dirt");
        assert!(dirt > 0.0 && drawn.iter().any(|quad| quad.tex == dirt));
        assert!(
            drawn
                .iter()
                .any(|quad| quad.color == PANEL && quad.tex > 0.0)
        );
        assert!(
            drawn
                .iter()
                .any(|quad| quad.color == HEADER && quad.tex > 0.0)
        );
        assert!(drawn.iter().any(|quad| quad.color == WELL));
        assert!(drawn.iter().any(|quad| quad.color == LIP));
        assert!(drawn.iter().any(|quad| quad.color == TITLE_PLATE));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert!(gui.on_press(KeyCode::Enter));
        assert!(gui.chest_open());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.chest_open());
    }

    #[test]
    fn inv_craft_two_by_two_matches_the_book() {
        let mut gui = Gui::from_screen(shipped());
        // A lone oak_log matches oak_stairs first in the book; torch is unique.
        assert!(gui.give_inv_craft(0, "stick"));
        assert!(gui.give_inv_craft(1, "coal_ore"));
        assert_eq!(gui.inv_craft_preview().as_deref(), Some("torch"));
        assert_eq!(gui.pull_inv_craft().as_deref(), Some("torch"));
        assert!(gui.inv_craft_slot(0).is_none());
        assert!(gui.inv_craft_slot(1).is_none());
        assert!(gui.give_inv_craft(0, "stick"));
        assert!(gui.give_inv_craft(2, "coal_ore"));
        assert_eq!(gui.inv_craft_preview().as_deref(), Some("torch"));
        assert_eq!(gui.drain_inv_craft(), vec!["stick", "coal_ore"]);
        assert!(gui.inv_craft_preview().is_none());
    }

    #[test]
    fn a_table_crafts_from_the_grid_and_a_furnace_cooks_stone() {
        assert!(rebuild_world::Pack::builtin().contains_block("crafting_table"));
        assert!(rebuild_world::Pack::builtin().contains_block("furnace"));
        let mut gui = Gui::from_screen(shipped());
        assert!(gui.open_craft());
        assert!(gui.give_craft("iron_ore"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_craft("oak_log"));
        assert_eq!(gui.craft_preview().as_deref(), Some("lantern"));
        assert_eq!(gui.pull_craft().as_deref(), Some("lantern"));
        assert!(gui.craft_preview().is_none());
        assert!(gui.give_craft("stone_bricks"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_craft("stone_bricks"));
        assert_eq!(
            gui.craft_preview().as_deref(),
            Some("chiseled_stone_bricks")
        );
        assert_eq!(gui.pull_craft().as_deref(), Some("chiseled_stone_bricks"));
        let drawn = gui.paint(1280.0, 720.0, &hotbar::Bag::new());
        assert!(
            drawn
                .iter()
                .any(|quad| quad.color == PANEL && quad.tex > 0.0)
        );
        assert!(
            drawn
                .iter()
                .any(|quad| quad.color == HEADER && quad.tex > 0.0)
        );
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.craft_open());

        assert!(gui.open_furnace());
        assert!(gui.give_furnace("cobblestone"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("stone"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("clay"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("bricks"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("stone"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("smooth_stone"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("stone_bricks"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("cracked_stone_bricks"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("cobbled_deepslate"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("polished_deepslate"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("deepslate_tiles"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("cracked_deepslate_tiles"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("blackstone_bricks"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("cracked_blackstone_bricks"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("basalt"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("smooth_basalt"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("polished_basalt"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("smooth_basalt"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("nether_bricks"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("cracked_nether_bricks"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("netherrack"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert_eq!(gui.take_furnace_output(), Some("nether_bricks"));
        let cooked = gui.paint(1280.0, 720.0, &hotbar::Bag::new());
        assert!(
            cooked
                .iter()
                .any(|quad| quad.color == palette::color("cobblestone", 0) || quad.tex > 0.0)
        );
        assert!(gui.on_press(KeyCode::KeyF));
        assert!(!gui.furnace_open());

        assert!(gui.open_smoker());
        assert_eq!(gui.screen_id(), "smoker");
        assert!(gui.give_furnace("raw"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(0.2);
        assert!(gui.take_furnace_output().is_none());
        gui.stoke(0.5);
        assert_eq!(gui.take_furnace_output(), Some("cooked"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("cobblestone"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("coal_ore"));
        gui.stoke(1.0);
        assert!(gui.take_furnace_output().is_none());
        assert!(!gui.paint(1280.0, 720.0, &hotbar::Bag::new()).is_empty());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.furnace_open());

        assert!(gui.open_furnace());
        assert_eq!(gui.screen_id(), "furnace");
        assert!(gui.give_furnace("raw"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(0.5);
        assert!(gui.take_furnace_output().is_none());
        gui.stoke(0.5);
        assert_eq!(gui.take_furnace_output(), Some("cooked"));
        assert!(gui.on_press(KeyCode::Escape));

        assert!(gui.open_blast());
        assert_eq!(gui.screen_id(), "blast");
        assert!(gui.give_furnace("cobblestone"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(0.2);
        assert!(gui.take_furnace_output().is_none());
        gui.stoke(0.5);
        assert_eq!(gui.take_furnace_output(), Some("stone"));
        assert!(gui.on_press(KeyCode::ArrowLeft));
        assert!(gui.give_furnace("raw"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("coal_ore"));
        gui.stoke(1.0);
        assert!(gui.take_furnace_output().is_none());
        assert!(gui.on_press(KeyCode::ArrowLeft));
        gui.drain_furnace();
        assert!(gui.give_furnace("clay"));
        assert!(gui.on_press(KeyCode::ArrowRight));
        assert!(gui.give_furnace("oak_log"));
        gui.stoke(1.0);
        assert!(gui.take_furnace_output().is_none());
        assert!(!gui.paint(1280.0, 720.0, &hotbar::Bag::new()).is_empty());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.furnace_open());
    }

    #[test]
    fn the_main_menu_plays_settings_and_quits() {
        let menu_path = menu_file();
        assert!(menu_path.ends_with(std::path::Path::new("assets/terralite/gui/menu.json")));
        assert!(
            settings_file().ends_with(std::path::Path::new("assets/terralite/gui/settings.json"))
        );
        let mut menu = Menu::new();
        assert!(menu.is_open());
        assert!(!menu.is_settings());
        // feel_quads leaves the title menu to egui; legacy menu.quads still paints for tests.
        let hud = ui_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &hotbar::Bag::new(),
            &Gui::from_screen(shipped()),
            &menu,
            10,
            10,
            [None; 3],
        );
        assert!(
            hud.iter()
                .all(|quad| quad.color != palette::color("cobblestone", 0)),
            "the menu hides the hotbar"
        );
        let quads = menu.quads(1280.0 / GUI_SCALE, 720.0 / GUI_SCALE);
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == BACKDROP && quad.tex > 0.0)
        );
        let panel = quads
            .iter()
            .find(|quad| quad.color == PANEL)
            .expect("panel");
        let center_x = (panel.min[0] + panel.max[0]) * 0.5;
        assert!(center_x.abs() < 0.05, "the menu is centered");
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == BUTTON && quad.tex > 0.0)
        );
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == PANEL && quad.tex > 0.0)
        );
        assert!(quads.iter().any(|quad| quad.color == SELECT));
        let brand_cell = quads
            .iter()
            .filter(|quad| quad.color == INK)
            .map(|quad| quad.max[1] - quad.min[1])
            .fold(0.0, f32::max);
        assert!(
            brand_cell > 0.012,
            "the brand title uses a larger cell than the buttons"
        );
        assert!(
            quads
                .iter()
                .any(|quad| { quad.color == DETAIL && (quad.max[0] - quad.min[0]) > 0.05 }),
            "a rule sits under the title"
        );
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Play);
        assert!(!menu.is_open());

        menu.open();
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Join);
        assert!(
            menu.is_open(),
            "join leaves the menu up until a server answers"
        );
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert!(menu.is_settings());
        assert_eq!(menu.world_size(), rebuild_world::WorldSize::Small);
        assert!(!menu.muted());
        let settings = menu.quads(1280.0, 720.0);
        assert!(
            settings.iter().any(|quad| quad.color == DETAIL),
            "settings draws the world size line"
        );
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Resize);
        assert!(menu.is_settings());
        assert_eq!(menu.world_size(), rebuild_world::WorldSize::Medium);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert_eq!(menu.hemisphere(), crate::daylight::Hemisphere::North);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::Volume);
        assert!(menu.muted());
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.bob(), crate::camera::Bob::Full);
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::None);
        assert_eq!(menu.bob(), crate::camera::Bob::Low);
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::None);
        assert_eq!(menu.bob(), crate::camera::Bob::Off);
        let bobbed = menu.quads(1280.0, 720.0);
        assert!(
            bobbed.iter().any(|quad| quad.color == DETAIL),
            "view bob names off, low, or full under the row"
        );
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert!(menu.trace());
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::None);
        assert!(!menu.trace());
        let traced = menu.quads(1280.0, 720.0);
        assert!(
            traced.iter().any(|quad| quad.color == DETAIL),
            "trace names soft sun or the raster shadow map"
        );
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::None);
        assert!(menu.trace());
        while menu.rows().get(menu.selected).map(String::as_str) != Some("back") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        assert_eq!(menu.on_press(KeyCode::Space), MenuAction::None);
        assert!(!menu.is_settings());
        menu.back();
        assert!(menu.is_open());

        assert_eq!(menu.on_press(KeyCode::ArrowUp), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Quit);
        assert!(
            menu.is_open(),
            "quit leaves the menu up until the window closes"
        );
        assert!(
            Screen::parse(
                r#"{"id":"menu","title":"Terralite","open":"escape","close":"escape","kind":"other","labels":["play"]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_settings_option_shows_above_back_and_flips() {
        let source = r#"
import SettingsAPI;

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    SettingsAPI.createOption(game, "lamp", "soft glow", "on");
    return 0;
}
"#;
        let mut hooks = rebuild_world::Hooks::new();
        rebuild_world::load_source(source, &mut hooks).unwrap();
        let mut menu = Menu::new();
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert!(menu.is_settings());
        let rows = menu.rows();
        let lamp = rows.iter().position(|row| row == "lamp").expect("lamp row");
        let back = rows.iter().position(|row| row == "back").expect("back");
        assert!(lamp < back);
        while menu.rows().get(menu.selected).map(String::as_str) != Some("lamp") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        assert_eq!(menu.detail(), "soft glow, on");
        assert_eq!(menu.settings_value("lamp"), "on");
        assert_eq!(menu.settings_hint("lamp"), "soft glow");
        assert!(
            menu.quads(1280.0, 720.0)
                .iter()
                .any(|quad| quad.color == DETAIL)
        );
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.take_changed().as_deref(), Some("lamp"));
        assert!(menu.is_settings());
        assert_eq!(menu.detail(), "soft glow, off");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.detail(), "soft glow, on");
    }

    #[test]
    fn a_settings_flip_says_the_value_on_the_menu() {
        let source = r#"
import SettingsAPI;
import GUIAPI;

fn flipped(name: String) {
    world.write(1, 1, 1, "dirt");
    GUIAPI.say("glow " + SettingsAPI.value(name));
}

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var ember = SettingsAPI.createOption(game, "ember", "soft glow", "on");
    SettingsAPI.on_change(ember, flipped);
    return 0;
}
"#;
        let mut hooks = rebuild_world::Hooks::new();
        rebuild_world::load_source(source, &mut hooks).unwrap();
        let mut menu = Menu::new();
        let mut gui = Gui::new();
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        while menu.rows().get(menu.selected).map(String::as_str) != Some("ember") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        let before = feel_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &hotbar::Bag::new(),
            &gui,
            &menu,
            10,
            10,
            [None; 3],
            0.0,
            0.0,
            0.0,
            false,
            false,
        )
        .len();
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.take_changed().as_deref(), Some("ember"));
        let mut world = rebuild_world::synthetic_cluster();
        let said = hooks.setting_changed(&mut world, "ember", &rebuild_world::ScriptView::bare());
        assert_eq!(said, vec!["glow off".to_string()]);
        for line in &said {
            gui.push_chat(line);
        }
        let after = feel_quads(
            1280.0,
            720.0,
            &Hotbar::new(),
            &hotbar::Bag::new(),
            &gui,
            &menu,
            10,
            10,
            [None; 3],
            0.0,
            0.0,
            0.0,
            false,
            false,
        )
        .len();
        assert!(after > before, "the menu draws the chat line");
        assert!(menu.is_settings());
    }

    #[test]
    fn a_settings_choice_list_steps_on_the_page() {
        let source = r#"
import SettingsAPI;
import GUIAPI;

fn flipped(name: String) {
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
        let mut hooks = rebuild_world::Hooks::new();
        rebuild_world::load_source(source, &mut hooks).unwrap();
        let mut menu = Menu::new();
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        let rows = menu.rows();
        let game = rows
            .iter()
            .position(|row| row == "game")
            .expect("game heading");
        let pace_at = rows.iter().position(|row| row == "pace").expect("pace");
        assert!(game < pace_at);
        while menu.rows().get(menu.selected).map(String::as_str) != Some("game") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        assert_eq!(menu.detail(), "how it feels");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert!(menu.is_settings());
        while menu.rows().get(menu.selected).map(String::as_str) != Some("pace") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        assert_eq!(menu.detail(), "walk speed, mid");
        assert_eq!(menu.settings_value("pace"), "mid");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.detail(), "walk speed, high");
        let mut world = rebuild_world::synthetic_cluster();
        let said = hooks.setting_changed(&mut world, "pace", &rebuild_world::ScriptView::bare());
        assert_eq!(said, vec!["pace high".to_string()]);
        assert_eq!(menu.on_press(KeyCode::ArrowLeft), MenuAction::Setting);
        assert_eq!(menu.detail(), "walk speed, mid");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.detail(), "walk speed, high");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.detail(), "walk speed, low");
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Setting);
        assert_eq!(menu.detail(), "walk speed, mid");
    }

    #[test]
    fn pause_resumes_and_a_named_world_can_be_chosen() {
        let mut menu = Menu::new();
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Play);
        menu.open_pause();
        assert!(menu.is_pause());
        let paused = menu.quads(1280.0, 720.0);
        assert!(paused.iter().any(|quad| quad.color == DETAIL));
        assert!(
            paused
                .iter()
                .any(|quad| quad.color == PANEL && quad.tex > 0.0)
        );
        assert!(
            paused
                .iter()
                .any(|quad| quad.color == BUTTON && quad.tex > 0.0)
        );
        assert!(
            paused
                .iter()
                .any(|quad| quad.color == BACKDROP && quad.tex > 0.0)
        );
        assert!(paused.iter().any(|quad| quad.color == WELL));
        assert!(paused.iter().any(|quad| quad.color == LIP));
        assert!(paused.iter().any(|quad| quad.color == TITLE_PLATE));
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Resume);
        assert!(!menu.is_open());

        menu.open_pause();
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert!(menu.is_settings());
        assert_eq!(menu.escape(), MenuAction::None);
        assert!(menu.is_pause());
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::Title);
        assert!(menu.is_open());
        assert!(!menu.is_pause());

        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert!(menu.is_worlds());
        assert_eq!(menu.world_name(), "world");
        let before = menu.worlds.clone();
        while menu.rows().get(menu.selected).map(String::as_str) != Some("new") {
            assert_eq!(menu.on_press(KeyCode::ArrowDown), MenuAction::None);
        }
        assert_eq!(menu.on_press(KeyCode::Enter), MenuAction::None);
        assert_ne!(menu.world_name(), "world");
        let created = menu.world_name().to_string();
        assert!(menu.worlds.iter().any(|name| name == &created));
        if !before.iter().any(|name| name == &created) {
            let _ = std::fs::remove_file(crate::net::world_file(&created));
        }
        assert_eq!(menu.escape(), MenuAction::None);
        assert!(!menu.is_worlds());
        assert!(menu.is_open());
    }

    #[test]
    fn the_command_list_selects_home() {
        let mut gui = Gui::new();
        assert!(!gui.show_commands(Vec::new()));
        assert!(gui.show_commands(vec![
            ("guide:day".into(), "day".into()),
            ("guide:home".into(), "home".into()),
        ]));
        assert!(gui.is_open());
        assert_eq!(gui.screen_id(), "commands");
        assert_eq!(gui.selected_command().as_deref(), Some("guide:day"));
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert_eq!(gui.selected_command().as_deref(), Some("guide:home"));
        assert!(!gui.paint(1280.0, 720.0, &hotbar::Bag::new()).is_empty());
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.command_open());
        assert!(!gui.is_open());
    }

    #[test]
    fn chat_keeps_letters_and_a_line_draws() {
        let mut gui = Gui::new();
        assert!(gui.open_chat());
        assert!(gui.is_open());
        assert_eq!(gui.screen_id(), "chat");
        assert!(gui.on_press(KeyCode::KeyH));
        assert!(gui.on_press(KeyCode::KeyI));
        assert!(gui.on_press(KeyCode::Space));
        assert!(gui.on_press(KeyCode::Digit2));
        assert_eq!(gui.take_chat().as_deref(), Some("hi"));
        assert!(gui.open_chat());
        assert!(gui.on_press(KeyCode::Slash));
        assert!(gui.on_press(KeyCode::KeyS));
        assert!(gui.on_press(KeyCode::KeyP));
        assert!(gui.on_press(KeyCode::KeyA));
        assert!(gui.on_press(KeyCode::KeyR));
        assert!(gui.on_press(KeyCode::KeyK));
        assert!(gui.on_press(KeyCode::Space));
        assert!(gui.on_press(KeyCode::KeyH));
        assert!(gui.on_press(KeyCode::KeyI));
        assert_eq!(gui.take_chat().as_deref(), Some("/spark hi"));
        assert!(!gui.chat_open());
        gui.push_chat("hello");
        assert!(!gui.chat_quads(1280.0, 720.0).is_empty());
        gui.age_chat(9.0);
        assert!(gui.chat_quads(1280.0, 720.0).is_empty());
    }

    #[test]
    fn a_guiapi_screen_opens_and_a_click_names_the_row() {
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
        rebuild_world::load_source(source, &mut rebuild_world::Hooks::new()).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyN));
        assert!(gui.is_open());
        assert_eq!(gui.screen_id(), "notes");
        let drawn = gui.quads(1280.0, 720.0);
        assert!(drawn.len() > 4);
        assert!(drawn.iter().any(|quad| quad.hit == 800));
        assert_eq!(gui.pointer_click(800), Click::Script);
        assert_eq!(gui.take_press().as_deref(), Some("marker"));
        assert!(gui.on_press(KeyCode::Enter));
        assert_eq!(gui.take_press().as_deref(), Some("marker"));
        assert!(gui.on_press(KeyCode::Escape));
        assert!(!gui.is_open());
        assert!(gui.on_press(KeyCode::KeyE));
        assert!(gui.is_open());
        assert_ne!(gui.screen_id(), "notes");
        assert!(!gui.on_press(KeyCode::Enter));
        assert!(gui.take_press().is_none());
    }

    #[test]
    fn a_button_draws_a_lip_and_reserved_keys_are_refused() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("marks", GUIAPI.vec(320.0, 160.0), GUIAPI.vec(40.0, 48.0));
    var mark = GUIAPI.button("mark", "mark oak");
    GUIAPI.addElement(screen, GUIAPI.place(mark, GUIAPI.vec(16.0, 48.0)));
    GUIAPI.on_press(screen, "m", fn(row: String) {
        GUIAPI.say(row);
    });
    return 0;
}
"#;
        rebuild_world::load_source(source, &mut rebuild_world::Hooks::new()).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyM));
        assert_eq!(gui.screen_id(), "marks");
        let drawn = gui.quads(1280.0, 720.0);
        let plate = drawn
            .iter()
            .find(|quad| quad.hit == 800)
            .expect("button plate");
        let lip = [
            plate.color[0] * 0.55,
            plate.color[1] * 0.55,
            plate.color[2] * 0.55,
        ];
        assert!(drawn.iter().any(|quad| {
            quad.hit == 0
                && (quad.color[0] - lip[0]).abs() < 0.02
                && (quad.color[1] - lip[1]).abs() < 0.02
                && (quad.color[2] - lip[2]).abs() < 0.02
        }));
        let taken = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("items", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(8.0, 8.0));
    GUIAPI.addElement(screen, GUIAPI.text("row", "row"));
    GUIAPI.on_press(screen, "e", fn(row: String) {
        GUIAPI.say(row);
    });
    return 0;
}
"#;
        let err = rebuild_world::load_source(taken, &mut rebuild_world::Hooks::new())
            .expect_err("e is the items list");
        assert!(err.contains("e opens the items list"), "{err}");
        let shared = r#"
import GUIAPI;

fn main(): Int {
    var first = GUIAPI.createScreen("one", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(8.0, 8.0));
    GUIAPI.addElement(first, GUIAPI.text("row", "row"));
    GUIAPI.on_press(first, "q", fn(row: String) {
        GUIAPI.say(row);
    });
    var second = GUIAPI.createScreen("two", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(8.0, 8.0));
    GUIAPI.addElement(second, GUIAPI.text("row", "row"));
    GUIAPI.on_press(second, "q", fn(row: String) {
        GUIAPI.say(row);
    });
    return 0;
}
"#;
        let err = rebuild_world::load_source(shared, &mut rebuild_world::Hooks::new())
            .expect_err("one letter opens one screen");
        assert!(err.contains("q already opens a screen"), "{err}");
    }

    #[test]
    fn a_heading_is_skipped_and_an_icon_draws_a_face() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("signs", GUIAPI.vec(320.0, 200.0), GUIAPI.vec(40.0, 48.0));
    var places = GUIAPI.heading("places", "places");
    GUIAPI.addElement(screen, GUIAPI.place(places, GUIAPI.vec(16.0, 40.0)));
    var face = GUIAPI.icon("oakface", "oak_log");
    GUIAPI.addElement(screen, GUIAPI.place(face, GUIAPI.vec(16.0, 68.0)));
    var oak = GUIAPI.text("oak", "oak");
    GUIAPI.addElement(screen, GUIAPI.place(oak, GUIAPI.vec(56.0, 68.0)));
    var marsh = GUIAPI.text("marsh", "marsh");
    GUIAPI.addElement(screen, GUIAPI.place(marsh, GUIAPI.vec(56.0, 108.0)));
    GUIAPI.on_press(screen, "h", fn(row: String) {
        GUIAPI.say(row);
    });
    return 0;
}
"#;
        rebuild_world::load_source(source, &mut rebuild_world::Hooks::new()).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyH));
        assert_eq!(gui.screen_id(), "signs");
        let drawn = gui.quads(1280.0, 720.0);
        assert!(
            drawn
                .iter()
                .any(|quad| { quad.hit == 0 && quad.tex > 0.0 && quad.color == [1.0, 1.0, 1.0] })
        );
        assert!(gui.on_press(KeyCode::Enter));
        assert_eq!(gui.take_press().as_deref(), Some("oak"));
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert!(gui.on_press(KeyCode::Enter));
        assert_eq!(gui.take_press().as_deref(), Some("marsh"));
        assert!(gui.on_press(KeyCode::ArrowDown));
        assert!(gui.on_press(KeyCode::Enter));
        assert_eq!(gui.take_press().as_deref(), Some("oak"));
    }

    #[test]
    fn a_field_takes_letters_and_enter_commits_the_line() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("pad", GUIAPI.vec(320.0, 160.0), GUIAPI.vec(40.0, 48.0));
    var line = GUIAPI.field("line", "");
    GUIAPI.addElement(screen, GUIAPI.place(line, GUIAPI.vec(16.0, 64.0)));
    GUIAPI.on_press(screen, "j", fn(row: String) {
        GUIAPI.say(row);
    });
    return 0;
}
"#;
        rebuild_world::load_source(source, &mut rebuild_world::Hooks::new()).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyJ));
        assert!(gui.is_field("line"));
        assert_eq!(gui.row_text("line").as_deref(), Some(""));
        assert!(gui.on_press(KeyCode::KeyW));
        assert!(gui.on_press(KeyCode::KeyE));
        assert!(gui.on_press(KeyCode::Space));
        assert!(gui.on_press(KeyCode::KeyN));
        assert!(gui.built_open());
        assert_eq!(gui.row_text("line").as_deref(), Some("we n"));
        assert!(gui.on_press(KeyCode::Backspace));
        assert_eq!(gui.row_text("line").as_deref(), Some("we "));
        assert!(gui.on_press(KeyCode::Enter));
        assert_eq!(gui.take_press().as_deref(), Some("line"));
        assert_eq!(gui.row_text("line").as_deref(), Some("we "));
    }

    #[test]
    fn a_guiapi_press_adds_and_removes_a_row_on_the_open_panel() {
        let source = r#"
import GUIAPI;

fn main(): Int {
    var screen = GUIAPI.createScreen("board", GUIAPI.vec(320.0, 200.0), GUIAPI.vec(40.0, 48.0));
    var oak = GUIAPI.text("oak", "oak");
    oak.position = GUIAPI.vec(16.0, 48.0);
    oak.size = GUIAPI.vec(200.0, 32.0);
    GUIAPI.addElement(screen, oak);
    GUIAPI.on_press(screen, "b", fn(row: String) {
        if row == "oak" {
            var kept = GUIAPI.text("kept", "oak noted");
            kept.position = GUIAPI.vec(16.0, 88.0);
            kept.size = GUIAPI.vec(200.0, 32.0);
            GUIAPI.addElement(screen, kept);
        }
        if row == "kept" {
            var gone = GUIAPI.text("kept", "oak noted");
            GUIAPI.removeElement(screen, gone);
        }
    });
    return 0;
}
"#;
        let mut hooks = rebuild_world::Hooks::new();
        rebuild_world::load_source(source, &mut hooks).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyB));
        assert_eq!(gui.screen_id(), "board");
        assert!(gui.quads(1280.0, 720.0).iter().any(|quad| quad.hit == 800));
        assert!(gui.quads(1280.0, 720.0).iter().all(|quad| quad.hit != 801));
        let mut world = rebuild_world::synthetic_cluster();
        hooks.screen_row(
            &mut world,
            "board",
            "oak",
            &rebuild_world::ScriptView::bare(),
        );
        gui.sync_built(&rebuild_world::Pack::builtin().gui_screens());
        assert!(gui.quads(1280.0, 720.0).iter().any(|quad| quad.hit == 801));
        hooks.screen_row(
            &mut world,
            "board",
            "kept",
            &rebuild_world::ScriptView::bare(),
        );
        gui.sync_built(&rebuild_world::Pack::builtin().gui_screens());
        assert!(gui.quads(1280.0, 720.0).iter().all(|quad| quad.hit != 801));
    }

    #[test]
    fn a_guiapi_press_line_draws_in_chat() {
        let source = r#"
import GUIAPI;

fn picked(row: String) {
    GUIAPI.say("oak noted");
}

fn main(): Int {
    var screen = GUIAPI.createScreen("talk", GUIAPI.vec(200.0, 120.0), GUIAPI.vec(16.0, 16.0));
    GUIAPI.addElement(screen, GUIAPI.text("oak", "oak"));
    GUIAPI.on_press(screen, "k", picked);
    return 0;
}
"#;
        let mut hooks = rebuild_world::Hooks::new();
        rebuild_world::load_source(source, &mut hooks).unwrap();
        let mut gui = Gui::new();
        let mut world = rebuild_world::synthetic_cluster();
        let said = hooks.screen_row(
            &mut world,
            "talk",
            "oak",
            &rebuild_world::ScriptView::bare(),
        );
        assert_eq!(said, vec!["oak noted".to_string()]);
        for line in &said {
            gui.push_chat(line);
        }
        assert!(!gui.chat_quads(1280.0, 720.0).is_empty());
    }

    #[test]
    fn a_guiapi_row_draws_its_plate_and_black_ink() {
        let source = r#"
import GUIAPI;
from ui import Color;

fn picked(row: String) {
    print(row);
}

fn main(): Int {
    var screen = GUIAPI.createScreen("ink", GUIAPI.vec(320.0, 160.0), GUIAPI.vec(40.0, 40.0));
    var oak = GUIAPI.text("oak", "oak");
    oak.position = GUIAPI.vec(16.0, 48.0);
    oak.size = GUIAPI.vec(200.0, 32.0);
    oak.color = Color.Rgb(40, 90, 140);
    GUIAPI.addElement(screen, oak);
    var hen = GUIAPI.text("hen", "hen");
    hen.position = GUIAPI.vec(16.0, 88.0);
    hen.size = GUIAPI.vec(200.0, 32.0);
    hen.color = Color.Rgb(220, 190, 120);
    hen.fontColor = Color.Black;
    hen.font = "capital";
    GUIAPI.addElement(screen, hen);
    GUIAPI.on_press(screen, "u", picked);
    return 0;
}
"#;
        rebuild_world::load_source(source, &mut rebuild_world::Hooks::new()).unwrap();
        let mut gui = Gui::new();
        assert!(gui.on_press(KeyCode::KeyU));
        let drawn = gui.quads(1280.0, 720.0);
        let plate = [220.0 / 255.0, 190.0 / 255.0, 120.0 / 255.0];
        assert!(
            drawn
                .iter()
                .any(|quad| quad.hit == 801 && quad.color == plate)
        );
        assert!(
            drawn
                .iter()
                .any(|quad| quad.hit == 0 && quad.color == [0.0, 0.0, 0.0])
        );
        assert!(drawn.iter().all(|quad| quad.color != LABEL));
    }
}
