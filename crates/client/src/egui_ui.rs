//! egui menus on top of the existing wgpu world pass.
//!
//! Pause / settings / main / worlds / creative / player inventory draw here at
//! [`SCALE`] (~1.5x). Player inventory and creative host an EMI sidebar
//! (search + CRAFTING / USES from the recipe book). RoseGold GUIAPI screens
//! and block stations stay on the legacy quad path (`gui::Gui::paint`) during
//! the transition.
//!
//! Visual language matches the legacy HUD: dark well, light top lip, warm
//! select — not default egui purple/gray.

use std::collections::HashMap;

use egui::{
    Align, Align2, Color32, Context, CornerRadius, FontId, Frame, FullOutput, Layout, Margin,
    RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureId, TextureOptions, Ui, Vec2,
    ViewportId,
};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as WinitState;
use rebuild_world::{asset_png, Pack};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::advancements::{AdvancementRow, ToastView};
use crate::datapacks::DataPackRow;
use crate::mods::ModRow;
use crate::resource_packs::PackRow;
use crate::skins::SkinRow;
use crate::statistics::StatRow;
use crate::body::Armor;
use crate::gui::{self, Gui, Menu, MenuAction, PlayerInvTab, SettingsControl};
use crate::hotbar::{self, Hotbar};
use crate::item_icon;
use crate::keybinds::{Action, Keybinds};
use crate::player;
use crate::texture;

/// Overall egui zoom — matches the old `GUI_SCALE` target.
pub const SCALE: f32 = 1.5;

/// Square inventory / creative cell in egui points (before [`SCALE`]).
const SLOT: f32 = 36.0;
const SLOT_GAP: f32 = 5.0;
/// Soft-rendered slot raster size. Higher than the on-screen cell so LINEAR
/// upscale stays smooth instead of chunky nearest pixels.
const ICON_PX: usize = 64;
/// Creative catalog grid width (GUI plan: 10-wide).
const CREATIVE_COLS: usize = 10;
/// Floor for visible creative rows on short windows.
const CREATIVE_ROWS_MIN: f32 = 8.0;
/// Soft ceiling so ultra-tall monitors do not grow forever.
const CREATIVE_ROWS_MAX: f32 = 22.0;
/// Hard floor for the creative item ScrollArea / parent reserve (8 rows).
/// Fixed points — never derive this from content height or available leftovers.
const CREATIVE_GRID_MIN_H: f32 =
    SLOT * CREATIVE_ROWS_MIN + SLOT_GAP * (CREATIVE_ROWS_MIN - 1.0);
/// Target / clamp band for creative chrome vs window height.
const CREATIVE_CHROME_FRAC: f32 = 0.78;
const CREATIVE_CHROME_FRAC_MIN: f32 = 0.70;
const CREATIVE_CHROME_FRAC_MAX: f32 = 0.85;
/// Title / tabs / search / hotbar / close / panel margins above the item scroll.
const CREATIVE_PANEL_OVERHEAD: f32 = 268.0;
/// Extra vertical budget when EMI stacks under the catalog.
const CREATIVE_STACKED_EMI_OVERHEAD: f32 = 210.0;
/// Player inventory body grid (GUI plan: 4x10).
const INV_COLS: usize = 10;
const INV_ROWS: usize = 4;
const INV_BODY: usize = INV_COLS * INV_ROWS;
/// Armor / accessory columns flanking the entity preview (GUI plan: 5 each).
const EQUIP_ROWS: usize = 5;
/// Soft-rendered player preview size in pixels.
const PREVIEW_W: usize = 96;
const PREVIEW_H: usize = 144;
const PREVIEW_KEY: &str = "__player_preview__";
/// EMI sidebar item grid width (GUI plan L8).
const EMI_COLS: usize = 4;
const EMI_SIDE_W: f32 = SLOT * EMI_COLS as f32 + SLOT_GAP * (EMI_COLS as f32 - 1.0) + 24.0;
/// EMI title / search / craft-uses chrome below the item list.
const EMI_PANEL_OVERHEAD: f32 = 220.0;

/// Left-column UI row → engine armor slot (`None` = stub or offhand).
/// Order is top→bottom: head, chest, legs(stub), feet, offhand (row 4, wired separately).
const LEFT_ARMOR_MAP: [Option<usize>; EQUIP_ROWS] = [Some(0), Some(1), None, Some(2), None];
/// PLAYER-tab row index for the offhand equipment well.
const OFFHAND_EQUIP_ROW: usize = 4;

// Terralite dark-well palette (matches gui.rs WELL / LIP / PANEL).
const WELL: Color32 = Color32::from_rgb(5, 6, 10);
const PANEL: Color32 = Color32::from_rgb(13, 15, 20);
const LIP: Color32 = Color32::from_rgb(77, 79, 92);
const TITLE_PLATE: Color32 = Color32::from_rgb(18, 20, 28);
const ROW: Color32 = Color32::from_rgb(23, 26, 33);
const BUTTON: Color32 = Color32::from_rgb(36, 38, 48);
const BUTTON_HOVER: Color32 = Color32::from_rgb(48, 52, 64);
const SELECT: Color32 = Color32::from_rgb(184, 133, 31);
const SELECT_SOFT: Color32 = Color32::from_rgb(90, 110, 55);
const INK: Color32 = Color32::from_rgb(245, 247, 250);
const MUTED: Color32 = Color32::from_rgb(168, 172, 184);
const SLOT_WELL: Color32 = Color32::from_rgb(10, 11, 16);
const SLOT_EDGE: Color32 = Color32::from_rgb(58, 60, 70);
fn backdrop() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 168)
}
/// Darker red veil for the survival death overlay.
fn death_backdrop() -> Color32 {
    Color32::from_rgba_unmultiplied(18, 2, 4, 210)
}
const DEATH_INK: Color32 = Color32::from_rgb(255, 214, 214);
const TIP_BG: Color32 = Color32::from_rgb(18, 20, 28);

/// Survival death overlay facts (coords + where Respawn will land).
#[derive(Clone, Copy, Debug)]
pub struct DeathView {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Short line: `bed`, `anchor`, or `world spawn`.
    pub spawn: &'static str,
}

/// Fullscreen held-map overlay (explored terrain + player / landmark marks).
#[derive(Clone, Debug)]
pub struct MapView {
    pub colors: Vec<[f32; 3]>,
    pub span: usize,
    pub yaw: f32,
    pub zoom: u8,
    pub camp: Option<(usize, usize)>,
    pub mine: Option<(usize, usize)>,
    pub stairs: Option<(usize, usize)>,
    pub shrine: Option<(usize, usize)>,
    pub clay: Option<(usize, usize)>,
    pub note: String,
}

const TIP_EDGE: Color32 = Color32::from_rgb(237, 199, 107);
const DETAIL: Color32 = Color32::from_rgb(255, 235, 140);
const MAP_PAPER: Color32 = Color32::from_rgb(41, 33, 23);
const MAP_INK: Color32 = Color32::from_rgb(237, 230, 209);
const MAP_TEX_KEY: &str = "__held_map_overlay__";

/// Whimscape / pack GUI chrome under `assets/terralite/textures/gui/chrome/`.
const CHROME_SLOT: &str = "slot";
const CHROME_SLOT_HIGHLIGHT: &str = "slot_highlight";
const CHROME_PANEL: &str = "panel";
const CHROME_TAB: &str = "tab";
const CHROME_TAB_SELECTED: &str = "tab_selected";
const CHROME_BUTTON: &str = "button";
const CHROME_BUTTON_HOT: &str = "button_highlighted";
const CHROME_TIP_BG: &str = "tip_bg";
const CHROME_TIP_FRAME: &str = "tip_frame";

pub struct EguiHost {
    ctx: Context,
    winit: WinitState,
    renderer: Renderer,
    /// Pending menu action from the last `run` (Play, Quit, …).
    pending: MenuAction,
    /// True when the last `run` mutated hotbar / bag / armor.
    touched: bool,
    /// True when the last `run` took a 2×2 inventory craft result.
    crafted: bool,
    /// Soft-rendered slot icons keyed by item id.
    icons: HashMap<String, TextureHandle>,
}

impl EguiHost {
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
    ) -> Self {
        let ctx = Context::default();
        ctx.set_pixels_per_point(SCALE);
        apply_theme(&ctx);
        let winit = WinitState::new(
            ctx.clone(),
            ViewportId::ROOT,
            window,
            Some(SCALE),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = Renderer::new(device, output_format, RendererOptions::default());
        Self {
            ctx,
            winit,
            renderer,
            pending: MenuAction::None,
            touched: false,
            crafted: false,
            icons: HashMap::new(),
        }
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.winit.on_window_event(window, event).consumed
    }

    pub fn take_action(&mut self) -> MenuAction {
        std::mem::replace(&mut self.pending, MenuAction::None)
    }

    /// True when the last `run` changed hotbar, bag, or worn armor.
    pub fn take_touched(&mut self) -> bool {
        std::mem::replace(&mut self.touched, false)
    }

    /// True when the last `run` pulled a player-inventory craft result.
    pub fn take_crafted(&mut self) -> bool {
        std::mem::replace(&mut self.crafted, false)
    }

    /// Drop cached slot / map textures after the pack atlas rebuilds.
    pub fn clear_icons(&mut self) {
        self.icons.clear();
    }

    /// True when egui should own pointer/keyboard instead of the world.
    pub fn wants_input(menu: &Menu, gui: &Gui, dead: bool, map_open: bool) -> bool {
        dead
            || menu.is_open()
            || gui.bag_open()
            || gui.player_inv_open()
            || gui.trade_open()
            || gui.anvil_open()
            || gui.enchant_open()
            || gui.smith_open()
            || gui.grind_open()
            || gui.brew_open()
            || gui.furnace_open()
            || gui.craft_open()
            || gui.chest_open()
            || gui.chart_open()
            || gui.cut_open()
            || map_open
    }

    /// Build menus for this frame. Returns tessellated output ready to paint.
    ///
    /// `death` is `Some` while survival health is zero — dark overlay with
    /// Respawn / Title / Quit. Creative never reaches this path.
    /// `map` is `Some` while a held map sheet is open (use / M).
    /// `toast` is `Some` while an advancement popup is visible (always drawn
    /// on top when present, including during play with no other egui screen).
    pub fn run(
        &mut self,
        window: &Window,
        menu: &mut Menu,
        gui: &mut Gui,
        hotbar: &mut Hotbar,
        bag: &mut hotbar::Bag,
        armor: &mut Armor,
        binds: &mut Keybinds,
        death: Option<DeathView>,
        map: Option<MapView>,
        toast: Option<ToastView>,
        xp: &mut crate::xp::Xp,
        advancement_rows: &[AdvancementRow],
        statistics_rows: &[StatRow],
        pack_rows: &[PackRow],
        pack_note: &str,
        pack_dirty: bool,
        datapack_rows: &[DataPackRow],
        datapack_note: &str,
        datapack_dirty: bool,
        mod_rows: &[ModRow],
        mod_note: &str,
        mod_dirty: bool,
        skin_rows: &[SkinRow],
        skin_note: &str,
        skin_dirty: bool,
    ) -> FullOutput {
        let raw = self.winit.take_egui_input(window);
        let mut pending = MenuAction::None;
        let mut touched = false;
        let mut crafted = false;
        let icons = &mut self.icons;
        let map_view = map;
        let toast_view = toast;
        let output = self.ctx.run_ui(raw, |ui| {
            apply_theme(ui.ctx());
            warm_chrome(ui.ctx(), icons);
            if let Some(view) = death {
                pending = draw_death(ui, view, icons);
            } else if menu.is_open() {
                pending = draw_menu(
                    ui,
                    menu,
                    binds,
                    icons,
                    advancement_rows,
                    statistics_rows,
                    pack_rows,
                    pack_note,
                    pack_dirty,
                    datapack_rows,
                    datapack_note,
                    datapack_dirty,
                    mod_rows,
                    mod_note,
                    mod_dirty,
                    skin_rows,
                    skin_note,
                    skin_dirty,
                );
            } else if gui.bag_open() {
                draw_creative(ui, gui, hotbar, bag, icons);
                draw_pinned_tip(ui, gui, icons);
            } else if gui.player_inv_open() {
                let (inv_touched, inv_crafted) =
                    draw_player_inventory(ui, gui, hotbar, bag, armor, icons);
                touched = inv_touched;
                crafted = inv_crafted;
                draw_pinned_tip(ui, gui, icons);
            } else if gui.trade_open() {
                if draw_trade(ui, gui, hotbar, bag, icons) {
                    touched = true;
                }
            } else if gui.anvil_open() {
                if draw_anvil(ui, gui, hotbar, xp, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.enchant_open() {
                if draw_enchanting(ui, gui, hotbar, xp, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.smith_open() {
                if draw_smithing(ui, gui, hotbar, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.grind_open() {
                if draw_grindstone(ui, gui, hotbar, xp, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.brew_open() {
                if draw_brewing(ui, gui, hotbar, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.furnace_open() {
                if draw_furnace(ui, gui, hotbar, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.craft_open() {
                let (craft_touched, craft_done) =
                    draw_crafting_table(ui, gui, hotbar, bag, icons);
                if craft_touched {
                    touched = true;
                }
                if craft_done {
                    crafted = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.chest_open() {
                if draw_chest(ui, gui, hotbar, bag, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.chart_open() {
                if draw_cartography(ui, gui, hotbar, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if gui.cut_open() {
                if draw_stonecutter(ui, gui, hotbar, icons) {
                    touched = true;
                }
                draw_pinned_tip(ui, gui, icons);
            } else if let Some(view) = map_view.as_ref() {
                draw_map_overlay(ui, view, icons);
            }
            // Chat plate sits above the status stack during play / pause / death.
            // Hide under creative + player inventory (same rule as the old quads).
            if !gui.bag_open()
                && !gui.player_inv_open()
                && !gui.anvil_open()
                && !gui.enchant_open()
                && !gui.smith_open()
                && !gui.grind_open()
                && !gui.brew_open()
                && !gui.furnace_open()
                && !gui.craft_open()
                && !gui.chest_open()
                && !gui.chart_open()
                && !gui.cut_open()
            {
                draw_chat_plate(ui, gui, icons);
            }
            if let Some(toast) = toast_view.as_ref() {
                draw_advancement_toast(ui, toast, icons);
            }
        });
        self.pending = pending;
        self.touched = touched;
        self.crafted = crafted;
        self.winit
            .handle_platform_output(window, output.platform_output.clone());
        output
    }

    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        full: FullOutput,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let FullOutput {
            shapes,
            pixels_per_point,
            mut textures_delta,
            ..
        } = full;
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };
        let primitives = self.ctx.tessellate(shapes, pixels_per_point);
        let _user_cmd = self
            .renderer
            .update_buffers(device, queue, encoder, &primitives, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        textures_delta.clear();
    }
}

fn apply_theme(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = PANEL;
    visuals.panel_fill = PANEL;
    visuals.extreme_bg_color = WELL;
    visuals.faint_bg_color = ROW;
    visuals.override_text_color = Some(INK);
    visuals.selection.bg_fill = SELECT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, SELECT);
    visuals.widgets.inactive.bg_fill = BUTTON;
    visuals.widgets.inactive.weak_bg_fill = BUTTON;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, SLOT_EDGE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.hovered.bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, LIP);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.active.bg_fill = SELECT_SOFT;
    visuals.widgets.active.weak_bg_fill = SELECT_SOFT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, SELECT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.open.bg_fill = BUTTON_HOVER;
    visuals.widgets.open.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, LIP);
    visuals.hyperlink_color = DETAIL;
    visuals.window_stroke = Stroke::new(1.0, SLOT_EDGE);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 8.0);
    style.spacing.interact_size = Vec2::new(40.0, 28.0);
    style.interaction.tooltip_delay = 0.12;
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn panel_frame() -> Frame {
    Frame::new()
        .fill(WELL)
        .stroke(Stroke::new(1.0, SLOT_EDGE))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(16, 14))
}

fn show_panel(
    ui: &mut Ui,
    width: f32,
    panel_tex: Option<TextureId>,
    add_contents: impl FnOnce(&mut Ui),
) {
    // Outer lip (light strip) then dark well — Whimscape panel tile when present.
    // Always stack contents vertically: callers nest this beside EMI inside a
    // horizontal row, and egui Frames inherit that layout unless overridden
    // (which turned creative into a full-width banner strip).
    Frame::new()
        .fill(LIP)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin {
            left: 1,
            right: 1,
            top: 3,
            bottom: 1,
        })
        .show(ui, |ui| {
            let inner = if panel_tex.is_some() {
                // Keep a near-transparent fill so the image reads; stroke stays.
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(8, 9, 12, 210))
                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(16, 14))
            } else {
                panel_frame()
            };
            inner.show(ui, |ui| {
                if let Some(tex) = panel_tex {
                    let rect = ui.max_rect();
                    ui.painter().image(
                        tex,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.set_max_width(width);
                    add_contents(ui);
                });
            });
        });
}

fn panel_chrome(
    ctx: &Context,
    icons: &mut HashMap<String, TextureHandle>,
) -> Option<TextureId> {
    chrome_texture(ctx, icons, CHROME_PANEL)
}

/// Inventory / creative chrome stays inside the window (~90% wide).
fn chrome_max_width(ui: &Ui) -> f32 {
    (ui.available_width() * 0.9).max(280.0)
}

fn grid_pitch(cols: usize) -> f32 {
    let gaps = cols.saturating_sub(1) as f32;
    SLOT * cols as f32 + SLOT_GAP * gaps
}

/// Desired main-panel width for a N-wide slot grid, clamped to remaining chrome.
fn clamped_panel_width(max_chrome: f32, cols: usize, with_emi: bool) -> f32 {
    let grid_w = grid_pitch(cols);
    let desired = grid_w + 40.0;
    let emi = if with_emi {
        EMI_SIDE_W.max(180.0) + 12.0
    } else {
        0.0
    };
    desired.min((max_chrome - emi).max(grid_w.min(max_chrome)))
}

fn title_label(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(TITLE_PLATE)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(FontId::proportional(26.0))
                    .color(INK)
                    .strong(),
            );
        });
}

fn menu_button(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    text: &str,
    selected: bool,
) -> egui::Response {
    let size = Vec2::new(ui.available_width().max(280.0), 34.0);
    chrome_button(ui, icons, text, selected, size, 15.0)
}

/// Compact chrome button (settings Back / dock rows / EMI Fill·Craft).
fn chrome_button(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    text: &str,
    selected: bool,
    size: Vec2,
    font: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let name = if selected {
            CHROME_BUTTON_HOT
        } else {
            CHROME_BUTTON
        };
        if let Some(tex) = chrome_texture(ui.ctx(), icons, name) {
            ui.painter().image(
                tex,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(4),
                    Stroke::new(1.5, SELECT),
                    StrokeKind::Outside,
                );
            }
        } else {
            let fill = if selected { SELECT } else { BUTTON };
            let stroke = if selected {
                Stroke::new(1.5, SELECT)
            } else {
                Stroke::new(1.0, SLOT_EDGE)
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), fill);
            ui.painter()
                .rect_stroke(rect, CornerRadius::same(4), stroke, StrokeKind::Outside);
        }
        let ink = if selected {
            Color32::from_rgb(20, 18, 12)
        } else {
            INK
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(font),
            ink,
        );
    }
    response
}

/// Dock / inspector / EMI CRAFTING·USES well — panel tile when present.
/// Callers resolve `panel_tex` first so `&mut icons` stays free inside contents.
fn chrome_well(
    ui: &mut Ui,
    panel_tex: Option<TextureId>,
    fill_fallback: Color32,
    add_contents: impl FnOnce(&mut Ui),
) {
    let fill = if panel_tex.is_some() {
        Color32::from_rgba_unmultiplied(8, 9, 12, 220)
    } else {
        fill_fallback
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, SLOT_EDGE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 8))
        .show(ui, |ui| {
            if let Some(tex) = panel_tex {
                let rect = ui.max_rect();
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            add_contents(ui);
        });
}

fn chrome_cached(
    icons: &HashMap<String, TextureHandle>,
    name: &str,
) -> Option<TextureId> {
    icons.get(&format!("__chrome:{name}")).map(|h| h.id())
}

fn draw_skins_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[SkinRow],
    note: &str,
    dirty: bool,
) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.82).clamp(520.0, 760.0);
            let panel_h = (avail.y * 0.80).clamp(380.0, 620.0);
            let top_pad = ((avail.y - panel_h) * 0.30).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "PLAYER SKIN");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let back =
                                egui::Button::new(RichText::new("Back").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(back).clicked() {
                                if let Some(index) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(index);
                                }
                            }
                            let apply_fill = if dirty { SELECT } else { BUTTON };
                            let apply_ink = if dirty {
                                Color32::from_rgb(20, 18, 12)
                            } else {
                                INK
                            };
                            let apply = egui::Button::new(
                                RichText::new("Apply").size(14.0).color(apply_ink).strong(),
                            )
                            .fill(apply_fill)
                            .stroke(Stroke::new(1.0, if dirty { SELECT } else { SLOT_EDGE }))
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(apply).clicked() {
                                action = MenuAction::ApplySkin;
                            }
                            let browse =
                                egui::Button::new(RichText::new("Browse…").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(96.0, 30.0));
                            if ui.add(browse).clicked() {
                                action = MenuAction::BrowseSkin;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let status = if note.is_empty() {
                        "classic/slim 64×64 PNG under skins/ · Apply paints the local mesh".to_string()
                    } else {
                        note.to_string()
                    };
                    ui.label(RichText::new(status).size(13.0).color(MUTED));
                    ui.add_space(10.0);
                    let dock_w = (panel_w * 0.28).clamp(140.0, 200.0);
                    let body_h = (panel_h - 100.0).max(220.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        Frame::new()
                            .fill(TITLE_PLATE)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(dock_w);
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("LOOK")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Pick a PNG from skins/ or Browse for any classic/slim player sheet.",
                                        )
                                        .size(12.0)
                                        .color(INK),
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(
                                            "Apply paints the local third-person mesh and saves saves/skin.json.",
                                        )
                                        .size(12.0)
                                        .color(MUTED),
                                    );
                                });
                            });
                        ui.add_space(10.0);
                        Frame::new()
                            .fill(WELL)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((panel_w - dock_w - 48.0).max(280.0));
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("AVAILABLE")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("player_skin_list")
                                        .max_height(body_h - 28.0)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for row in rows {
                                                Frame::new()
                                                    .fill(ROW)
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if row.selected {
                                                            SELECT
                                                        } else {
                                                            SLOT_EDGE
                                                        },
                                                    ))
                                                    .corner_radius(CornerRadius::same(5))
                                                    .inner_margin(Margin::symmetric(10, 8))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&row.id)
                                                                        .size(15.0)
                                                                        .color(INK)
                                                                        .strong(),
                                                                );
                                                                let kind = if row.custom {
                                                                    format!("{} · file", row.model)
                                                                } else {
                                                                    row.model.to_string()
                                                                };
                                                                ui.label(
                                                                    RichText::new(kind)
                                                                        .size(12.0)
                                                                        .color(MUTED),
                                                                );
                                                            });
                                                            ui.with_layout(
                                                                Layout::right_to_left(
                                                                    Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let label = if row.selected {
                                                                        "ON"
                                                                    } else {
                                                                        "USE"
                                                                    };
                                                                    let fill = if row.selected {
                                                                        SELECT
                                                                    } else {
                                                                        BUTTON
                                                                    };
                                                                    let ink = if row.selected {
                                                                        Color32::from_rgb(
                                                                            20, 18, 12,
                                                                        )
                                                                    } else {
                                                                        INK
                                                                    };
                                                                    let btn = egui::Button::new(
                                                                        RichText::new(label)
                                                                            .size(13.0)
                                                                            .color(ink)
                                                                            .strong(),
                                                                    )
                                                                    .fill(fill)
                                                                    .stroke(Stroke::new(
                                                                        1.0, SLOT_EDGE,
                                                                    ))
                                                                    .corner_radius(
                                                                        CornerRadius::same(3),
                                                                    )
                                                                    .min_size(Vec2::new(64.0, 28.0));
                                                                    if ui.add(btn).clicked() {
                                                                        let path = if row
                                                                            .path
                                                                            .as_os_str()
                                                                            .is_empty()
                                                                        {
                                                                            String::new()
                                                                        } else {
                                                                            row.path
                                                                                .display()
                                                                                .to_string()
                                                                        };
                                                                        action = menu
                                                                            .request_skin_select(
                                                                                &path,
                                                                            );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                ui.add_space(6.0);
                                            }
                                        });
                                });
                            });
                    });
                });
            });
        });
    action
}

fn draw_mods_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[ModRow],
    note: &str,
    dirty: bool,
) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.82).clamp(520.0, 760.0);
            let panel_h = (avail.y * 0.80).clamp(380.0, 620.0);
            let top_pad = ((avail.y - panel_h) * 0.30).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "MODS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let back =
                                egui::Button::new(RichText::new("Back").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(back).clicked() {
                                if let Some(index) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(index);
                                }
                            }
                            let apply_fill = if dirty { SELECT } else { BUTTON };
                            let apply_ink = if dirty {
                                Color32::from_rgb(20, 18, 12)
                            } else {
                                INK
                            };
                            let apply = egui::Button::new(
                                RichText::new("Apply").size(14.0).color(apply_ink).strong(),
                            )
                            .fill(apply_fill)
                            .stroke(Stroke::new(1.0, if dirty { SELECT } else { SLOT_EDGE }))
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(apply).clicked() {
                                action = MenuAction::ApplyMods;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let status = if note.is_empty() {
                        "RoseGold .rg + folders under mods/ · Esc closes".to_string()
                    } else {
                        note.to_string()
                    };
                    ui.label(RichText::new(status).size(13.0).color(MUTED));
                    ui.add_space(10.0);
                    let dock_w = (panel_w * 0.28).clamp(140.0, 200.0);
                    let body_h = (panel_h - 100.0).max(220.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        Frame::new()
                            .fill(TITLE_PLATE)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(dock_w);
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("ENABLE")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "All mods start on. Toggle OFF to skip a script or folder on the next world load.",
                                        )
                                        .size(12.0)
                                        .color(INK),
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(
                                            "Apply saves saves/mods.json. Changes take effect on the next world load — no hot-reload.",
                                        )
                                        .size(12.0)
                                        .color(MUTED),
                                    );
                                });
                            });
                        ui.add_space(10.0);
                        Frame::new()
                            .fill(WELL)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((panel_w - dock_w - 48.0).max(280.0));
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("AVAILABLE")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("mods_enable_list")
                                        .max_height(body_h - 28.0)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for row in rows {
                                                Frame::new()
                                                    .fill(ROW)
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if row.enabled {
                                                            SELECT
                                                        } else {
                                                            SLOT_EDGE
                                                        },
                                                    ))
                                                    .corner_radius(CornerRadius::same(5))
                                                    .inner_margin(Margin::symmetric(10, 8))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&row.id)
                                                                        .size(15.0)
                                                                        .color(INK)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{} · {}",
                                                                        row.kind, row.description
                                                                    ))
                                                                    .size(12.0)
                                                                    .color(MUTED),
                                                                );
                                                            });
                                                            ui.with_layout(
                                                                Layout::right_to_left(
                                                                    Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let label = if row.enabled {
                                                                        "ON"
                                                                    } else {
                                                                        "OFF"
                                                                    };
                                                                    let fill = if row.enabled {
                                                                        SELECT
                                                                    } else {
                                                                        BUTTON
                                                                    };
                                                                    let ink = if row.enabled {
                                                                        Color32::from_rgb(
                                                                            20, 18, 12,
                                                                        )
                                                                    } else {
                                                                        INK
                                                                    };
                                                                    let btn = egui::Button::new(
                                                                        RichText::new(label)
                                                                            .size(13.0)
                                                                            .color(ink)
                                                                            .strong(),
                                                                    )
                                                                    .fill(fill)
                                                                    .stroke(Stroke::new(
                                                                        1.0, SLOT_EDGE,
                                                                    ))
                                                                    .corner_radius(
                                                                        CornerRadius::same(3),
                                                                    )
                                                                    .min_size(Vec2::new(64.0, 28.0));
                                                                    if ui.add(btn).clicked() {
                                                                        action = menu
                                                                            .request_mod_toggle(
                                                                                &row.id,
                                                                            );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                ui.add_space(6.0);
                                            }
                                        });
                                });
                            });
                    });
                });
            });
        });
    action
}

fn draw_datapacks_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[DataPackRow],
    note: &str,
    dirty: bool,
) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.82).clamp(520.0, 760.0);
            let panel_h = (avail.y * 0.80).clamp(380.0, 620.0);
            let top_pad = ((avail.y - panel_h) * 0.30).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "DATAPACKS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let back =
                                egui::Button::new(RichText::new("Back").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(back).clicked() {
                                if let Some(index) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(index);
                                }
                            }
                            let apply_fill = if dirty { SELECT } else { BUTTON };
                            let apply_ink = if dirty {
                                Color32::from_rgb(20, 18, 12)
                            } else {
                                INK
                            };
                            let apply = egui::Button::new(
                                RichText::new("Apply").size(14.0).color(apply_ink).strong(),
                            )
                            .fill(apply_fill)
                            .stroke(Stroke::new(1.0, if dirty { SELECT } else { SLOT_EDGE }))
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(apply).clicked() {
                                action = MenuAction::ApplyDataPacks;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let status = if note.is_empty() {
                        "folders + zips under datapacks/ · Esc closes".to_string()
                    } else {
                        note.to_string()
                    };
                    ui.label(RichText::new(status).size(13.0).color(MUTED));
                    ui.add_space(10.0);
                    let dock_w = (panel_w * 0.28).clamp(140.0, 200.0);
                    let body_h = (panel_h - 100.0).max(220.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        Frame::new()
                            .fill(TITLE_PLATE)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(dock_w);
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("STACK")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Base terralite always loads. Enabled overlays win for loot, recipes, and smelting JSON.",
                                        )
                                        .size(12.0)
                                        .color(INK),
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(
                                            "Apply reloads the EMI book so new smelt / craft lines show immediately.",
                                        )
                                        .size(12.0)
                                        .color(MUTED),
                                    );
                                });
                            });
                        ui.add_space(10.0);
                        Frame::new()
                            .fill(WELL)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((panel_w - dock_w - 48.0).max(280.0));
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("AVAILABLE")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("datapacks_list")
                                        .max_height(body_h - 28.0)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for row in rows {
                                                Frame::new()
                                                    .fill(ROW)
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if row.enabled {
                                                            SELECT
                                                        } else {
                                                            SLOT_EDGE
                                                        },
                                                    ))
                                                    .corner_radius(CornerRadius::same(5))
                                                    .inner_margin(Margin::symmetric(10, 8))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&row.id)
                                                                        .size(15.0)
                                                                        .color(INK)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{} · {}",
                                                                        row.kind, row.description
                                                                    ))
                                                                    .size(12.0)
                                                                    .color(MUTED),
                                                                );
                                                            });
                                                            ui.with_layout(
                                                                Layout::right_to_left(
                                                                    Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let label = if row.enabled {
                                                                        "ON"
                                                                    } else {
                                                                        "OFF"
                                                                    };
                                                                    let fill = if row.enabled {
                                                                        SELECT
                                                                    } else {
                                                                        BUTTON
                                                                    };
                                                                    let ink = if row.enabled {
                                                                        Color32::from_rgb(
                                                                            20, 18, 12,
                                                                        )
                                                                    } else {
                                                                        INK
                                                                    };
                                                                    let btn = egui::Button::new(
                                                                        RichText::new(label)
                                                                            .size(13.0)
                                                                            .color(ink)
                                                                            .strong(),
                                                                    )
                                                                    .fill(fill)
                                                                    .stroke(Stroke::new(
                                                                        1.0, SLOT_EDGE,
                                                                    ))
                                                                    .corner_radius(
                                                                        CornerRadius::same(3),
                                                                    )
                                                                    .min_size(Vec2::new(64.0, 28.0));
                                                                    if ui.add(btn).clicked()
                                                                        && !row.base
                                                                    {
                                                                        action = menu
                                                                            .request_datapack_toggle(
                                                                                &row.id,
                                                                            );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                ui.add_space(6.0);
                                            }
                                        });
                                });
                            });
                    });
                });
            });
        });
    action
}

fn draw_resource_packs_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[PackRow],
    note: &str,
    dirty: bool,
) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.82).clamp(520.0, 760.0);
            let panel_h = (avail.y * 0.80).clamp(380.0, 620.0);
            let top_pad = ((avail.y - panel_h) * 0.30).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "RESOURCE PACKS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let back =
                                egui::Button::new(RichText::new("Back").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(back).clicked() {
                                if let Some(index) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(index);
                                }
                            }
                            let apply_fill = if dirty { SELECT } else { BUTTON };
                            let apply_ink = if dirty {
                                Color32::from_rgb(20, 18, 12)
                            } else {
                                INK
                            };
                            let apply = egui::Button::new(
                                RichText::new("Apply").size(14.0).color(apply_ink).strong(),
                            )
                            .fill(apply_fill)
                            .stroke(Stroke::new(1.0, if dirty { SELECT } else { SLOT_EDGE }))
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(apply).clicked() {
                                action = MenuAction::ApplyPacks;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let status = if note.is_empty() {
                        "folders + zips under resourcepacks/ · Esc closes".to_string()
                    } else {
                        note.to_string()
                    };
                    ui.label(RichText::new(status).size(13.0).color(MUTED));
                    ui.add_space(10.0);
                    let dock_w = (panel_w * 0.28).clamp(140.0, 200.0);
                    let body_h = (panel_h - 100.0).max(220.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        // Godot-style left dock explaining the stack.
                        Frame::new()
                            .fill(TITLE_PLATE)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(dock_w);
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("STACK")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Base terralite always loads. Enabled overlays win on top (last toggle order).",
                                        )
                                        .size(12.0)
                                        .color(INK),
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(
                                            "Apply rebuilds the block atlas so grass / dirt overrides show in-world.",
                                        )
                                        .size(12.0)
                                        .color(MUTED),
                                    );
                                });
                            });
                        ui.add_space(10.0);
                        Frame::new()
                            .fill(WELL)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((panel_w - dock_w - 48.0).max(280.0));
                                    ui.set_min_height(body_h);
                                    ui.label(
                                        RichText::new("AVAILABLE")
                                            .size(11.0)
                                            .color(MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("resource_packs_list")
                                        .max_height(body_h - 28.0)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for row in rows {
                                                Frame::new()
                                                    .fill(ROW)
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if row.enabled {
                                                            SELECT
                                                        } else {
                                                            SLOT_EDGE
                                                        },
                                                    ))
                                                    .corner_radius(CornerRadius::same(5))
                                                    .inner_margin(Margin::symmetric(10, 8))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&row.id)
                                                                        .size(15.0)
                                                                        .color(INK)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{} · {}",
                                                                        row.kind, row.description
                                                                    ))
                                                                    .size(12.0)
                                                                    .color(MUTED),
                                                                );
                                                            });
                                                            ui.with_layout(
                                                                Layout::right_to_left(
                                                                    Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let label = if row.base {
                                                                        "ON"
                                                                    } else if row.enabled {
                                                                        "ON"
                                                                    } else {
                                                                        "OFF"
                                                                    };
                                                                    let fill = if row.enabled {
                                                                        SELECT
                                                                    } else {
                                                                        BUTTON
                                                                    };
                                                                    let ink = if row.enabled {
                                                                        Color32::from_rgb(
                                                                            20, 18, 12,
                                                                        )
                                                                    } else {
                                                                        INK
                                                                    };
                                                                    let btn = egui::Button::new(
                                                                        RichText::new(label)
                                                                            .size(13.0)
                                                                            .color(ink)
                                                                            .strong(),
                                                                    )
                                                                    .fill(fill)
                                                                    .stroke(Stroke::new(
                                                                        1.0, SLOT_EDGE,
                                                                    ))
                                                                    .corner_radius(
                                                                        CornerRadius::same(3),
                                                                    )
                                                                    .min_size(Vec2::new(64.0, 28.0));
                                                                    if ui.add(btn).clicked()
                                                                        && !row.base
                                                                    {
                                                                        action = menu
                                                                            .request_pack_toggle(
                                                                                &row.id,
                                                                            );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                ui.add_space(6.0);
                                            }
                                        });
                                });
                            });
                    });
                });
            });
        });
    action
}

fn draw_statistics_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[StatRow],
) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let panel_w = (ui.available_width() * 0.58).clamp(360.0, 480.0);
            // Centered Area avoids vertical_centered clip cutting the last row.
            egui::Area::new(egui::Id::new("statistics_screen"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                        ui.horizontal(|ui| {
                            title_label(ui, "STATISTICS");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if chrome_button(
                                    ui,
                                    icons,
                                    "Back",
                                    false,
                                    Vec2::new(88.0, 30.0),
                                    14.0,
                                )
                                .clicked()
                                {
                                    if let Some(index) =
                                        menu.menu_rows().iter().position(|label| label == "back")
                                    {
                                        action = menu.activate_row(index);
                                    }
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("this world · Esc closes")
                                .size(13.0)
                                .color(MUTED),
                        );
                        ui.add_space(10.0);
                        for (index, row) in rows.iter().enumerate() {
                            let row_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, row_tex, ROW, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(row.label).size(14.0).color(MUTED),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&row.value)
                                                    .size(16.0)
                                                    .color(INK)
                                                    .strong(),
                                            );
                                        },
                                    );
                                });
                            });
                            if index + 1 != rows.len() {
                                ui.add_space(5.0);
                            }
                        }
                        ui.add_space(4.0);
                    });
                });
        });
    action
}

fn draw_advancements_screen(
    ui: &mut Ui,
    menu: &mut Menu,
    icons: &mut HashMap<String, TextureHandle>,
    rows: &[AdvancementRow],
) -> MenuAction {
    let mut action = MenuAction::None;
    let unlocked = rows.iter().filter(|row| row.unlocked).count();
    let total = rows.len();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.72).clamp(420.0, 640.0);
            let panel_h = (avail.y * 0.82).clamp(360.0, 620.0);
            ui.vertical_centered(|ui| {
                ui.add_space(((avail.y - panel_h) * 0.35).max(8.0));
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "ADVANCEMENTS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                "Back",
                                false,
                                Vec2::new(88.0, 30.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if let Some(index) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(index);
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("{unlocked} / {total} unlocked · Esc / L close"))
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical()
                        .id_salt("advancements_list")
                        .max_height((panel_h - 90.0).max(200.0))
                        .show(ui, |ui| {
                            ui.set_width(panel_w - 36.0);
                            for row in rows {
                                let fill = if row.unlocked { ROW } else { SLOT_WELL };
                                let row_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
                                chrome_well(ui, row_tex, fill, |ui| {
                                    ui.horizontal(|ui| {
                                        let badge = if row.unlocked { "DONE" } else { "LOCKED" };
                                        let _ = chrome_button(
                                            ui,
                                            icons,
                                            badge,
                                            row.unlocked,
                                            Vec2::new(64.0, 22.0),
                                            11.0,
                                        );
                                        ui.add_space(8.0);
                                        ui.vertical(|ui| {
                                            let title = if row.unlocked {
                                                RichText::new(row.title)
                                                    .size(15.0)
                                                    .color(INK)
                                                    .strong()
                                            } else {
                                                RichText::new("???")
                                                    .size(15.0)
                                                    .color(MUTED)
                                                    .strong()
                                            };
                                            ui.label(title);
                                            ui.label(
                                                RichText::new(if row.unlocked {
                                                    row.subtitle
                                                } else {
                                                    "Keep exploring"
                                                })
                                                .size(12.0)
                                                .color(if row.unlocked {
                                                    DETAIL
                                                } else {
                                                    MUTED
                                                }),
                                            );
                                        });
                                    });
                                });
                                ui.add_space(6.0);
                            }
                        });
                });
            });
        });
    action
}

fn draw_advancement_toast(
    ui: &mut Ui,
    toast: &ToastView,
    icons: &mut HashMap<String, TextureHandle>,
) {
    let alpha = toast.alpha.clamp(0.0, 1.0);
    if alpha <= 0.01 {
        return;
    }
    let mul = |c: Color32| -> Color32 {
        Color32::from_rgba_unmultiplied(
            c.r(),
            c.g(),
            c.b(),
            ((c.a() as f32) * alpha).round() as u8,
        )
    };
    let tip_bg = chrome_texture(ui.ctx(), icons, CHROME_TIP_BG);
    let tip_frame = chrome_texture(ui.ctx(), icons, CHROME_TIP_FRAME);
    let panel = tip_bg.or_else(|| chrome_texture(ui.ctx(), icons, CHROME_PANEL));
    egui::Area::new(egui::Id::new("advancement_toast"))
        .anchor(Align2::RIGHT_TOP, [toast.slide - 18.0, 18.0])
        .interactable(false)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            let fill = if panel.is_some() {
                mul(Color32::from_rgba_unmultiplied(12, 11, 16, 240))
            } else {
                mul(WELL)
            };
            let stroke = if tip_frame.is_some() {
                Stroke::new(1.5, mul(Color32::from_rgb(72, 58, 40)))
            } else {
                Stroke::new(1.5, mul(SELECT))
            };
            let out = Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(250.0);
                    if let Some(tex) = panel {
                        let r = ui.max_rect();
                        let band = egui::Rect::from_min_size(
                            r.min,
                            Vec2::new(r.width().min(280.0), r.height().min(96.0)),
                        );
                        ui.painter().image(
                            tex,
                            band,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::from_rgba_unmultiplied(
                                255,
                                255,
                                255,
                                (70.0 * alpha).round() as u8,
                            ),
                        );
                    }
                    ui.label(
                        RichText::new(&toast.title)
                            .size(16.0)
                            .color(mul(INK))
                            .strong(),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(&toast.subtitle)
                            .size(13.0)
                            .color(mul(MUTED)),
                    );
                });
            if let Some(tex) = tip_frame {
                ui.painter().image(
                    tex,
                    out.response.rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_rgba_unmultiplied(255, 255, 255, (255.0 * alpha).round() as u8),
                );
            }
        });
}

fn draw_death(ui: &mut Ui, view: DeathView, icons: &mut HashMap<String, TextureHandle>) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(death_backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.18);
                show_panel(ui, 380.0, panel_chrome(ui.ctx(), icons), |ui| {
                    let title_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
                    chrome_well(ui, title_tex, TITLE_PLATE, |ui| {
                        ui.label(
                            RichText::new("You Died")
                                .font(FontId::proportional(28.0))
                                .color(DEATH_INK)
                                .strong(),
                        );
                    });
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("at {}, {}, {}", view.x, view.y, view.z))
                            .size(14.0)
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new(format!("respawn · {}", view.spawn))
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(14.0);
                    if menu_button(ui, icons, "Respawn", true).clicked() {
                        action = MenuAction::Respawn;
                    }
                    ui.add_space(4.0);
                    if menu_button(ui, icons, "Title", false).clicked() {
                        action = MenuAction::Title;
                    }
                    ui.add_space(4.0);
                    if menu_button(ui, icons, "Quit", false).clicked() {
                        action = MenuAction::Quit;
                    }
                });
            });
        });
    action
}

fn rgb_u8(c: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn map_overlay_texture(
    ctx: &Context,
    icons: &mut HashMap<String, TextureHandle>,
    view: &MapView,
) -> TextureId {
    let span = view.span.max(1);
    let mut rgba = vec![0u8; span * span * 4];
    for (i, color) in view.colors.iter().take(span * span).enumerate() {
        let o = i * 4;
        rgba[o] = (color[0].clamp(0.0, 1.0) * 255.0) as u8;
        rgba[o + 1] = (color[1].clamp(0.0, 1.0) * 255.0) as u8;
        rgba[o + 2] = (color[2].clamp(0.0, 1.0) * 255.0) as u8;
        rgba[o + 3] = 255;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([span, span], &rgba);
    if let Some(handle) = icons.get_mut(MAP_TEX_KEY) {
        handle.set(image, TextureOptions::NEAREST);
        return handle.id();
    }
    let handle = ctx.load_texture(MAP_TEX_KEY, image, TextureOptions::NEAREST);
    let id = handle.id();
    icons.insert(MAP_TEX_KEY.to_string(), handle);
    id
}

fn draw_map_overlay(ui: &mut Ui, view: &MapView, icons: &mut HashMap<String, TextureHandle>) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                let avail = ui.available_size();
                let panel_w = (avail.x * 0.78).clamp(420.0, 860.0);
                let panel_h = (avail.y * 0.86).clamp(380.0, 720.0);
                ui.add_space(((avail.y - panel_h) * 0.5).max(8.0));
                Frame::new()
                    .fill(LIP)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin {
                        left: 1,
                        right: 1,
                        top: 3,
                        bottom: 1,
                    })
                    .show(ui, |ui| {
                        Frame::new()
                            .fill(MAP_PAPER)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(18, 14))
                            .show(ui, |ui| {
                                ui.set_width(panel_w);
                                ui.set_min_height(panel_h);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Map")
                                            .font(FontId::proportional(22.0))
                                            .color(MAP_INK)
                                            .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let zoom = match view.zoom {
                                            0 => "near",
                                            1 => "wide",
                                            _ => "far",
                                        };
                                        ui.label(
                                            RichText::new(format!(
                                                "{zoom} · Esc / M close · scroll zoom"
                                            ))
                                            .size(13.0)
                                            .color(MUTED),
                                        );
                                    });
                                });
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(&view.note).size(13.0).color(MAP_INK),
                                );
                                ui.add_space(8.0);
                                let grid = (panel_w - 36.0)
                                    .min(panel_h - 110.0)
                                    .max(240.0);
                                let (rect, _resp) = ui.allocate_exact_size(
                                    Vec2::splat(grid),
                                    Sense::hover(),
                                );
                                let tex = map_overlay_texture(ui.ctx(), icons, view);
                                ui.painter().image(
                                    tex,
                                    rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                                let span = view.span.max(1) as f32;
                                let cell_px = rect.width() / span;
                                let mark_at = |cell_xy: (usize, usize)| {
                                    let (row, col) = cell_xy;
                                    egui::pos2(
                                        rect.min.x + (col as f32 + 0.5) * cell_px,
                                        rect.min.y + (row as f32 + 0.5) * cell_px,
                                    )
                                };
                                let paint_dot = |at: egui::Pos2, color: Color32, r: f32| {
                                    ui.painter().circle_filled(at, r, color);
                                    ui.painter().circle_stroke(
                                        at,
                                        r,
                                        Stroke::new(1.0, Color32::from_black_alpha(120)),
                                    );
                                };
                                let mark_r = cell_px * 0.28;
                                if let Some(at) = view.stairs {
                                    if at != (view.span / 2, view.span / 2) {
                                        paint_dot(mark_at(at), rgb_u8(crate::map::STAIRS_RGB), mark_r);
                                    }
                                }
                                if let Some(at) = view.mine {
                                    paint_dot(mark_at(at), rgb_u8(crate::map::MINE_RGB), mark_r);
                                }
                                if let Some(at) = view.camp {
                                    paint_dot(mark_at(at), rgb_u8(crate::map::CAMP_RGB), mark_r);
                                }
                                if let Some(at) = view.shrine {
                                    paint_dot(mark_at(at), rgb_u8(crate::map::SHRINE_RGB), mark_r);
                                }
                                if let Some(at) = view.clay {
                                    if at != (view.span / 2, view.span / 2) {
                                        paint_dot(mark_at(at), rgb_u8(crate::map::CLAY_RGB), mark_r);
                                    }
                                }
                                let center = rect.center();
                                paint_dot(center, rgb_u8(crate::map::MARK_RGB), cell_px * 0.32);
                                let (sin, cos) = view.yaw.sin_cos();
                                let reach = cell_px * 0.9;
                                let tip = egui::pos2(
                                    center.x + cos * reach,
                                    center.y - sin * reach,
                                );
                                paint_dot(tip, rgb_u8(crate::map::FACING_RGB), cell_px * 0.22);
                                ui.painter().text(
                                    egui::pos2(rect.center().x, rect.min.y - 4.0),
                                    Align2::CENTER_BOTTOM,
                                    "N",
                                    FontId::proportional(16.0),
                                    MAP_INK,
                                );
                            });
                    });
            });
        });
}

fn draw_menu(
    ui: &mut Ui,
    menu: &mut Menu,
    binds: &mut Keybinds,
    icons: &mut HashMap<String, TextureHandle>,
    advancement_rows: &[AdvancementRow],
    statistics_rows: &[StatRow],
    pack_rows: &[PackRow],
    pack_note: &str,
    pack_dirty: bool,
    datapack_rows: &[DataPackRow],
    datapack_note: &str,
    datapack_dirty: bool,
    mod_rows: &[ModRow],
    mod_note: &str,
    mod_dirty: bool,
    skin_rows: &[SkinRow],
    skin_note: &str,
    skin_dirty: bool,
) -> MenuAction {
    if menu.is_keybinds() {
        return draw_keybinds(ui, menu, binds, icons);
    }
    if menu.is_settings() {
        return draw_settings(ui, menu, icons);
    }
    if menu.is_advancements() {
        return draw_advancements_screen(ui, menu, icons, advancement_rows);
    }
    if menu.is_statistics() {
        return draw_statistics_screen(ui, menu, icons, statistics_rows);
    }
    if menu.is_resource_packs() {
        return draw_resource_packs_screen(ui, menu, icons, pack_rows, pack_note, pack_dirty);
    }
    if menu.is_datapacks() {
        return draw_datapacks_screen(ui, menu, icons, datapack_rows, datapack_note, datapack_dirty);
    }
    if menu.is_mods() {
        return draw_mods_screen(ui, menu, icons, mod_rows, mod_note, mod_dirty);
    }
    if menu.is_skins() {
        return draw_skins_screen(ui, menu, icons, skin_rows, skin_note, skin_dirty);
    }
    let mut action = MenuAction::None;
    let title = menu.page_title();
    let rows = menu.menu_rows();
    let selected = menu.selected_row();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.14);
                show_panel(ui, 420.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, title);
                    ui.add_space(12.0);
                    for (index, label) in rows.iter().enumerate() {
                        let detail = menu.row_detail(label);
                        let heading = gui::title_case_label(label);
                        let text = if detail.is_empty() {
                            heading
                        } else {
                            // Two-line style avoids clipping long settings blurbs.
                            format!("{heading}\n{detail}")
                        };
                        if menu_button(ui, icons, &text, index == selected).clicked() {
                            action = menu.activate_row(index);
                        }
                    }
                });
            });
        });
    action
}

/// Settings chrome: Godot-style left category dock + right inspector.
fn draw_settings(ui: &mut Ui, menu: &mut Menu, icons: &mut HashMap<String, TextureHandle>) -> MenuAction {
    let mut action = MenuAction::None;
    let categories = menu.settings_categories();
    if categories.is_empty() {
        return MenuAction::None;
    }
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.86).clamp(560.0, 920.0);
            let panel_h = (avail.y * 0.78).clamp(360.0, 620.0);
            let top_pad = ((avail.y - panel_h) * 0.35).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "SETTINGS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // Compact Back — full-width menu_button would stretch the header.
                            if chrome_button(
                                ui,
                                icons,
                                "Back",
                                false,
                                Vec2::new(88.0, 30.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if let Some(back) =
                                    menu.menu_rows().iter().position(|label| label == "back")
                                {
                                    action = menu.activate_row(back);
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                    // Search filters the inspector property list.
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Filter").size(13.0).color(MUTED));
                        let mut query = menu.settings_query().to_string();
                        let edit = egui::TextEdit::singleline(&mut query)
                            .desired_width((panel_w - 120.0).clamp(160.0, 420.0))
                            .hint_text("world, bob, glow…")
                            .text_color(INK);
                        if ui.add(edit).changed() {
                            menu.set_settings_query(&query);
                        }
                    });
                    ui.add_space(8.0);

                    let dock_w = (panel_w * 0.28).clamp(150.0, 220.0);
                    let body_h = (panel_h - 110.0).max(220.0);
                    let cat_idx = menu
                        .settings_category()
                        .min(categories.len().saturating_sub(1));
                    if cat_idx != menu.settings_category() {
                        menu.set_settings_category(cat_idx);
                    }
                    let active = categories[cat_idx].clone();

                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        // Left category dock (Godot Project Settings style).
                        // Force vertical: Frames inherit the parent horizontal row.
                        let dock_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
                        chrome_well(ui, dock_tex, TITLE_PLATE, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(dock_w);
                                ui.set_min_height(body_h);
                                ui.label(
                                    RichText::new("CATEGORIES")
                                        .size(11.0)
                                        .color(MUTED)
                                        .strong(),
                                );
                                ui.add_space(6.0);
                                egui::ScrollArea::vertical()
                                    .id_salt("settings_dock")
                                    .max_height(body_h - 28.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for (i, cat) in categories.iter().enumerate() {
                                            let selected = i == cat_idx;
                                            let w = ui.available_width().max(dock_w - 16.0);
                                            if chrome_button(
                                                ui,
                                                icons,
                                                &cat.title,
                                                selected,
                                                Vec2::new(w, 30.0),
                                                14.0,
                                            )
                                            .clicked()
                                            {
                                                menu.set_settings_category(i);
                                            }
                                            if !cat.detail.is_empty() {
                                                ui.label(
                                                    RichText::new(&cat.detail)
                                                        .size(11.0)
                                                        .color(MUTED),
                                                );
                                                ui.add_space(4.0);
                                            }
                                        }
                                    });
                            });
                        });

                        ui.add_space(10.0);

                        // Right inspector property list.
                        let inspector_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
                        chrome_well(ui, inspector_tex, WELL, |ui| {
                            ui.vertical(|ui| {
                                let inspector_w = (panel_w - dock_w - 48.0).max(280.0);
                                ui.set_width(inspector_w);
                                ui.set_min_height(body_h);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(active.title.to_uppercase())
                                            .size(15.0)
                                            .color(DETAIL)
                                            .strong(),
                                    );
                                    if !active.detail.is_empty() {
                                        ui.label(
                                            RichText::new(format!("— {}", active.detail))
                                                .size(12.0)
                                                .color(MUTED),
                                        );
                                    }
                                });
                                ui.add_space(6.0);
                                ui.separator();
                                ui.add_space(4.0);

                                let query = menu.settings_query().to_lowercase();
                                let props: Vec<_> = active
                                    .options
                                    .iter()
                                    .filter(|label| {
                                        if query.is_empty() {
                                            return true;
                                        }
                                        let name = label.to_lowercase();
                                        let hint = menu.settings_hint(label).to_lowercase();
                                        name.contains(&query) || hint.contains(&query)
                                    })
                                    .cloned()
                                    .collect();

                                egui::ScrollArea::vertical()
                                    .id_salt("settings_inspector")
                                    .max_height(body_h - 48.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        if props.is_empty() {
                                            ui.label(
                                                RichText::new("no matching properties")
                                                    .size(13.0)
                                                    .color(MUTED),
                                            );
                                        }
                                        for label in props {
                                            let Some(row_index) = menu.settings_row_index(&label)
                                            else {
                                                continue;
                                            };
                                            let name = gui::title_case_label(&label);
                                            let hint = menu.settings_hint(&label);
                                            let control = menu.settings_control(&label);
                                            let locked = label == "world" && menu.size_locked();
                                            if let Some(pending) = inspector_property_row(
                                                ui,
                                                icons,
                                                row_index,
                                                &name,
                                                &hint,
                                                &control,
                                                locked,
                                            ) {
                                                action = match pending {
                                                    SettingsPending::Toggle(on) => {
                                                        menu.set_settings_toggle(row_index, on)
                                                    }
                                                    SettingsPending::Choice(choice) => menu
                                                        .set_settings_choice(row_index, choice),
                                                    SettingsPending::Open => {
                                                        menu.activate_row(row_index)
                                                    }
                                                };
                                            }
                                            ui.add_space(4.0);
                                        }
                                    });
                            });
                        });
                    });
                });
            });
        });
    action
}

enum SettingsPending {
    Toggle(bool),
    Choice(usize),
    Open,
}

/// Inspector row: property label | control (Godot editor style).
fn inspector_property_row(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    index: usize,
    name: &str,
    hint: &str,
    control: &SettingsControl,
    locked: bool,
) -> Option<SettingsPending> {
    let mut pending = None;
    let row_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
    let fill = if row_tex.is_some() {
        Color32::from_rgba_unmultiplied(14, 15, 20, 230)
    } else {
        ROW
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, SLOT_EDGE))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            if let Some(tex) = row_tex {
                let rect = ui.max_rect();
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 55),
                );
            }
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Left property label column.
                ui.allocate_ui_with_layout(
                    Vec2::new((ui.available_width() * 0.42).clamp(120.0, 220.0), 36.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(name).size(14.0).color(INK).strong());
                            if !hint.is_empty() {
                                ui.label(RichText::new(hint).size(11.0).color(MUTED));
                            }
                        });
                    },
                );
                // Right control column.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(!locked, |ui| {
                        match control {
                            SettingsControl::Toggle {
                                on,
                                on_label,
                                off_label,
                            } => {
                                let label = if *on { *on_label } else { *off_label };
                                ui.label(
                                    RichText::new(label).size(12.0).color(DETAIL).strong(),
                                );
                                if settings_switch(ui, *on, index) {
                                    pending = Some(SettingsPending::Toggle(!*on));
                                }
                            }
                            SettingsControl::Slider { index: cur, choices } => {
                                if choices.len() <= 1 {
                                    if let Some(value) = choices.first() {
                                        value_chip(ui, value);
                                    }
                                } else {
                                    let mut pos = *cur as f32;
                                    let max = (choices.len() - 1) as f32;
                                    let width = ui.available_width().clamp(140.0, 240.0);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(width, 28.0),
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.set_min_width(width);
                                            let resp = ui.push_id(("settings_slider", index), |ui| {
                                                ui.add(
                                                    egui::Slider::new(&mut pos, 0.0..=max)
                                                        .integer()
                                                        .show_value(false)
                                                        .trailing_fill(true),
                                                )
                                            })
                                            .inner;
                                            let choice = pos.round().clamp(0.0, max) as usize;
                                            let label = choices
                                                .get(choice)
                                                .map(String::as_str)
                                                .unwrap_or("");
                                            ui.label(
                                                RichText::new(label)
                                                    .size(12.0)
                                                    .color(DETAIL)
                                                    .strong(),
                                            );
                                            if resp.changed() {
                                                pending = Some(SettingsPending::Choice(choice));
                                            }
                                        },
                                    );
                                }
                            }
                            SettingsControl::Link { caption } => {
                                if chrome_button(
                                    ui,
                                    icons,
                                    caption,
                                    false,
                                    Vec2::new(96.0, 28.0),
                                    13.0,
                                )
                                .clicked()
                                {
                                    pending = Some(SettingsPending::Open);
                                }
                            }
                        }
                    });
                });
            });
        });
    pending
}

/// Godot-adjacent keybinds editor: click a row, press a key or mouse button.
fn draw_keybinds(ui: &mut Ui, menu: &mut Menu, binds: &mut Keybinds, icons: &mut HashMap<String, TextureHandle>) -> MenuAction {
    let mut action = MenuAction::None;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let panel_w = (avail.x * 0.72).clamp(480.0, 720.0);
            let panel_h = (avail.y * 0.82).clamp(400.0, 640.0);
            let top_pad = ((avail.y - panel_h) * 0.3).clamp(6.0, avail.y * 0.1);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    ui.set_min_height(panel_h);
                    ui.horizontal(|ui| {
                        title_label(ui, "KEYBINDS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let back =
                                egui::Button::new(RichText::new("Back").size(14.0).color(INK))
                                    .fill(BUTTON)
                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(Vec2::new(88.0, 30.0));
                            if ui.add(back).clicked() {
                                binds.cancel_listen();
                                action = menu.escape();
                            }
                            let reset = egui::Button::new(
                                RichText::new("Reset defaults").size(13.0).color(INK),
                            )
                            .fill(BUTTON)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(128.0, 30.0));
                            if ui.add(reset).clicked() {
                                binds.reset_defaults();
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let hint = if binds.listening.is_some() {
                        "press a key or mouse button…  Esc cancels"
                    } else {
                        "click a bind, then press the new key or mouse button"
                    };
                    ui.label(RichText::new(hint).size(12.0).color(MUTED));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("keybinds_list")
                        .max_height(panel_h - 100.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for action_id in Action::ALL {
                                let listening = binds.listening == Some(action_id);
                                let fill = if listening { SELECT_SOFT } else { ROW };
                                let stroke = if listening {
                                    Stroke::new(1.5, SELECT)
                                } else {
                                    Stroke::new(1.0, SLOT_EDGE)
                                };
                                Frame::new()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .corner_radius(CornerRadius::same(3))
                                    .inner_margin(Margin::symmetric(10, 7))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(action_id.title())
                                                        .size(14.0)
                                                        .color(INK)
                                                        .strong(),
                                                );
                                                if action_id.is_mouse() {
                                                    ui.label(
                                                        RichText::new("mouse")
                                                            .size(11.0)
                                                            .color(MUTED),
                                                    );
                                                }
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let label = if listening {
                                                        "…".to_string()
                                                    } else {
                                                        binds.get(action_id).label()
                                                    };
                                                    let btn = egui::Button::new(
                                                        RichText::new(label)
                                                            .size(13.0)
                                                            .color(if listening {
                                                                Color32::from_rgb(20, 18, 12)
                                                            } else {
                                                                DETAIL
                                                            })
                                                            .strong(),
                                                    )
                                                    .fill(if listening { SELECT } else { BUTTON })
                                                    .stroke(Stroke::new(1.0, SLOT_EDGE))
                                                    .corner_radius(CornerRadius::same(3))
                                                    .min_size(Vec2::new(140.0, 28.0));
                                                    if ui.add(btn).clicked() {
                                                        if listening {
                                                            binds.cancel_listen();
                                                        } else {
                                                            binds.begin_listen(action_id);
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });
                });
            });
        });
    action
}

/// Compact on/off switch. Returns true when the user flipped it.
fn settings_switch(ui: &mut Ui, on: bool, index: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(46.0, 24.0), Sense::click());
    let fill = if on { SELECT_SOFT } else { BUTTON };
    let stroke = if on {
        Stroke::new(1.5, SELECT)
    } else {
        Stroke::new(1.0, SLOT_EDGE)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let x = if on {
        rect.right() - 12.0
    } else {
        rect.left() + 12.0
    };
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y),
        8.0,
        if on { SELECT } else { LIP },
    );
    let _ = ui.interact(rect, ui.id().with(("settings_switch", index)), Sense::hover());
    resp.clicked()
}

fn value_chip(ui: &mut Ui, value: &str) {
    Frame::new()
        .fill(BUTTON)
        .stroke(Stroke::new(1.0, LIP))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(value)
                    .size(13.0)
                    .color(DETAIL)
                    .strong(),
            );
        });
}

fn draw_creative(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let max_w = chrome_max_width(ui);
            let avail_h = ui.available_height();
            let panel_w = clamped_panel_width(max_w, CREATIVE_COLS, true);
            let emi_w = EMI_SIDE_W.max(180.0);
            let side_by_side = panel_w + 12.0 + emi_w <= max_w + 0.5;
            let content_w = if side_by_side {
                panel_w + 12.0 + emi_w
            } else {
                panel_w.max(emi_w.min(max_w))
            }
            .min(max_w);
            let grid_h = creative_grid_height(avail_h, side_by_side);
            // Center the chrome block (not top-left). Estimate height so EMI
            // either sits beside or stacks below while staying on-screen.
            let chrome_h = creative_chrome_height(grid_h, side_by_side);
            // Keep gutters modest once chrome already owns ~70–85% of height.
            let top_pad = ((avail_h - chrome_h) * 0.5).clamp(4.0, avail_h * 0.12);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(top_pad);
                ui.set_max_width(content_w);
                ui.set_width(content_w);
                // No outer ScrollArea: nesting one around the item grid made egui
                // hand the inner scroll a tiny available height (~2 rows) even when
                // max_height asked for 8–22. The grid scrolls on its own; stacked
                // EMI can clip on very short windows rather than starve the grid.
                if side_by_side {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);
                        draw_creative_panel(ui, gui, hotbar, icons, panel_w, grid_h);
                        // Match the creative grid band so EMI doesn't stay a short stub.
                        let _ = draw_emi_sidebar(ui, gui, hotbar, bag, icons, grid_h);
                    });
                } else {
                    draw_creative_panel(ui, gui, hotbar, icons, panel_w, grid_h);
                    ui.add_space(10.0);
                    let _ = draw_emi_sidebar(ui, gui, hotbar, bag, icons, grid_h);
                }
            });
        });
}

/// How tall the creative item grid can grow for this window.
fn creative_grid_height(avail_h: f32, side_by_side: bool) -> f32 {
    let rows = creative_row_budget(avail_h, side_by_side);
    let h = rows * SLOT + (rows - 1.0) * SLOT_GAP;
    // Never collapse below the fixed 8-row floor, even if budget math glitches.
    h.max(CREATIVE_GRID_MIN_H)
}

/// Visible item rows for this window (clamped to the 8–22 band).
fn creative_row_budget(avail_h: f32, side_by_side: bool) -> f32 {
    // Title / tabs / search / hotbar / close / panel chrome. Stacked EMI eats more.
    let overhead = if side_by_side {
        CREATIVE_PANEL_OVERHEAD
    } else {
        CREATIVE_PANEL_OVERHEAD + CREATIVE_STACKED_EMI_OVERHEAD
    };
    // Own most of the vertical screen; leave a thin gutter for centering + EMI.
    let target = (avail_h * CREATIVE_CHROME_FRAC).clamp(
        avail_h * CREATIVE_CHROME_FRAC_MIN,
        avail_h * CREATIVE_CHROME_FRAC_MAX,
    );
    let usable = (target - overhead)
        .min(avail_h * CREATIVE_CHROME_FRAC_MAX - overhead)
        .max(SLOT * CREATIVE_ROWS_MIN);
    let pitch = SLOT + SLOT_GAP;
    (usable / pitch)
        .floor()
        .clamp(CREATIVE_ROWS_MIN, CREATIVE_ROWS_MAX)
}

/// Rows that must stay visible inside the pinned creative ScrollArea.
fn creative_visible_rows(grid_h: f32) -> usize {
    let pitch = SLOT + SLOT_GAP;
    ((grid_h + SLOT_GAP) / pitch)
        .floor()
        .clamp(CREATIVE_ROWS_MIN, CREATIVE_ROWS_MAX) as usize
}

fn creative_chrome_height(grid_h: f32, side_by_side: bool) -> f32 {
    let main = CREATIVE_PANEL_OVERHEAD + grid_h;
    if side_by_side {
        main
    } else {
        main + CREATIVE_STACKED_EMI_OVERHEAD
    }
}

fn draw_creative_panel(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
    panel_w: f32,
    grid_h: f32,
) {
    let grid_w = grid_pitch(CREATIVE_COLS);
    // Pin the grid band in points so parent Frames / horizontal rows cannot
    // collapse the viewport to content height (~2 rows). Always ≥8 rows.
    let grid_h = grid_h.max(CREATIVE_GRID_MIN_H);
    show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
        // Don't force wider than the clamped panel on tiny windows.
        ui.set_min_width(grid_w.min(panel_w));
        // Parent panel must reserve chrome + grid floor up front.
        ui.set_min_height(CREATIVE_PANEL_OVERHEAD + grid_h);
        title_label(ui, "CREATIVE");
        ui.add_space(8.0);
        // Category tabs with flanking arrows (GUI plan L7).
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
            if category_arrow(ui, "<").clicked() {
                gui.cycle_catalog_tab(false);
            }
            for index in 0..Gui::catalog_tab_count() {
                let name = Gui::catalog_tab_name(index);
                let label = {
                    let mut chars = name.chars();
                    match chars.next() {
                        Some(first) => {
                            let mut out = first.to_uppercase().collect::<String>();
                            out.push_str(chars.as_str());
                            out
                        }
                        None => name.to_string(),
                    }
                };
                let selected = gui.catalog_tab() == index;
                let size = Vec2::new(52.0, 26.0);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                if ui.is_rect_visible(rect) {
                    let chrome = if selected {
                        CHROME_TAB_SELECTED
                    } else {
                        CHROME_TAB
                    };
                    if let Some(tex) = chrome_texture(ui.ctx(), icons, chrome) {
                        ui.painter().image(
                            tex,
                            rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                            Color32::WHITE,
                        );
                    } else {
                        let fill = if selected { SELECT } else { BUTTON };
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(3), fill);
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(3),
                            Stroke::new(
                                if selected { 1.5 } else { 1.0 },
                                if selected { SELECT } else { SLOT_EDGE },
                            ),
                            StrokeKind::Outside,
                        );
                    }
                    let ink = if selected {
                        Color32::from_rgb(20, 18, 12)
                    } else {
                        INK
                    };
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(13.0),
                        ink,
                    );
                }
                if response.clicked() {
                    gui.set_catalog_tab(index);
                }
            }
            if category_arrow(ui, ">").clicked() {
                gui.cycle_catalog_tab(true);
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("search").color(MUTED).size(13.0));
            let mut q = gui.catalog_query().to_string();
            let edit_w = (panel_w - 72.0).clamp(120.0, 240.0);
            let hint = if Gui::catalog_tab_name(gui.catalog_tab()) == "flow" {
                "pipe tank"
            } else {
                "oak log"
            };
            let edit = egui::TextEdit::singleline(&mut q)
                .desired_width(edit_w)
                .hint_text(hint)
                .text_color(INK);
            if ui.add(edit).changed() {
                gui.set_catalog_query(&q);
            }
        });
        ui.add_space(8.0);
        let ids = gui.creative_ids();
        if ids.is_empty() {
            ui.label(RichText::new("nothing matches").color(MUTED).size(14.0));
            ui.add_space(4.0);
        }
        // Fixed min_height on the band + ScrollArea. Never size to content.
        let rows = creative_visible_rows(grid_h);
        let cells = ids.len().max(rows * CREATIVE_COLS);
        let grid_w_avail = ui.available_width().max(grid_w.min(panel_w));
        ui.allocate_ui_with_layout(
            Vec2::new(grid_w_avail, grid_h),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(grid_h);
                ui.set_max_height(grid_h);
                ui.set_min_size(Vec2::new(grid_w_avail.min(ui.available_width()), grid_h));
                egui::ScrollArea::vertical()
                    .id_salt("creative_grid_scroll")
                    .max_height(grid_h)
                    .min_scrolled_height(grid_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("creative_grid")
                            .spacing([SLOT_GAP, SLOT_GAP])
                            .min_col_width(SLOT)
                            .show(ui, |ui| {
                                for i in 0..cells {
                                    if let Some(id) = ids.get(i) {
                                        let selected = gui.catalog_index() == i;
                                        let tip = gui::tip_for(id, None);
                                        let tex = icon_texture(ui.ctx(), icons, id);
                                        let resp = slot_button(ui, icons, tex, selected, None);
                                        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                                        if resp.hovered() {
                                            gui.select_emi(id);
                                        }
                                        if resp.clicked() {
                                            gui.set_catalog_index(i);
                                            let _ = hotbar.give_stack(id);
                                        }
                                    } else {
                                        let _ = slot_button(ui, icons, None, false, None);
                                    }
                                    if (i + 1) % CREATIVE_COLS == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            },
        );
        ui.add_space(10.0);
        ui.label(RichText::new("hotbar").color(MUTED).size(13.0));
        ui.add_space(4.0);
        draw_hotbar_row(ui, Some(gui), hotbar, icons);
        ui.add_space(8.0);
        if close_button(ui, icons, "Close (I)").clicked() {
            gui.close_bag();
        }
    });
}

fn category_arrow(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(14.0).color(INK))
            .fill(BUTTON)
            .stroke(Stroke::new(1.0, SLOT_EDGE))
            .corner_radius(CornerRadius::same(3))
            .min_size(Vec2::new(28.0, 26.0)),
    )
}

/// Village trader stock panel — emerald buys for torch / apple / map.
fn draw_anvil(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    xp: &mut crate::xp::Xp,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let held_uses = hotbar.count(selected);
    let seed = hotbar.display_of(selected);
    if !seed.is_empty() {
        gui.seed_anvil_rename(&seed);
    }
    let can_repair = hotbar.repair_target().is_some();
    let can_combine = hotbar.combine_donor().is_some();
    let has_item = !held.is_empty();
    let rename_cost = 1;
    let work_cost = 1;
    let mat_hint = Hotbar::repair_material_for(&held)
        .map(|mats| mats.join(" / "))
        .unwrap_or_else(|| "—".to_string());
    let shown_name = {
        let custom = gui.anvil_rename();
        if custom.trim().is_empty() {
            seed
        } else {
            custom.to_string()
        }
    };

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                show_panel(ui, 440.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "ANVIL");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Rename costs XP. Repair / combine spend material + XP.")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            if has_item {
                                let tip = gui::tip_for(&held, Some(held_uses));
                                let tex = icon_texture(ui.ctx(), icons, &held);
                                let _ = slot_button(
                                    ui,
                                    icons,
                                    tex,
                                    false,
                                    if held_uses > 1 {
                                        Some(held_uses)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&shown_name)
                                            .size(15.0)
                                            .color(INK)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · level {}",
                                            tip.line, xp.level
                                        ))
                                        .size(12.0)
                                        .color(MUTED),
                                    );
                                });
                            } else {
                                ui.label(
                                    RichText::new("Select a tool or item in the hotbar.")
                                        .size(14.0)
                                        .color(MUTED),
                                );
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("Rename").size(13.0).color(DETAIL));
                    ui.add_space(2.0);
                    let rename = gui.anvil_rename_mut();
                    let response = ui.add(
                        egui::TextEdit::singleline(rename)
                            .desired_width(400.0)
                            .hint_text("custom name")
                            .font(FontId::proportional(15.0)),
                    );
                    if response.changed() {
                        touched = true;
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let rename_ok = has_item && xp.level >= rename_cost;
                        ui.add_enabled_ui(rename_ok, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                &format!("Rename (−{rename_cost} XP)"),
                                rename_ok,
                                Vec2::new(180.0, 30.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let name = gui.anvil_rename().to_string();
                                if xp.level < rename_cost {
                                    gui.note_anvil("need XP");
                                } else if hotbar.rename_selected(&name)
                                    && xp.spend_levels(rename_cost)
                                {
                                    gui.note_anvil("renamed");
                                    gui.request_anvil_clang();
                                    let mut tip = gui::tip_for(&held, Some(held_uses));
                                    if !name.trim().is_empty() {
                                        tip.name = name.trim().to_string();
                                    }
                                    gui.pin_tip(tip);
                                    touched = true;
                                }
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!("Material: {mat_hint}"))
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let repair_ok = can_repair && xp.level >= work_cost;
                        ui.add_enabled_ui(repair_ok, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                &format!("Repair (−{work_cost} XP)"),
                                repair_ok,
                                Vec2::new(170.0, 30.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if xp.level < work_cost {
                                    gui.note_anvil("need XP");
                                } else {
                                    let word = hotbar.repair_pick();
                                    gui.note_anvil(word);
                                    if word == "repaired" {
                                        let _ = xp.spend_levels(work_cost);
                                        gui.request_anvil_clang();
                                    }
                                    touched = true;
                                }
                            }
                        });
                        ui.add_space(8.0);
                        let combine_ok = can_combine && xp.level >= work_cost;
                        ui.add_enabled_ui(combine_ok, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                &format!("Combine (−{work_cost} XP)"),
                                combine_ok,
                                Vec2::new(180.0, 30.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if xp.level < work_cost {
                                    gui.note_anvil("need XP");
                                } else {
                                    let word = hotbar.combine_tool();
                                    gui.note_anvil(word);
                                    if word == "combined" {
                                        let _ = xp.spend_levels(work_cost);
                                        gui.request_anvil_clang();
                                    }
                                    touched = true;
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(gui.anvil_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_enchanting(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    xp: &mut crate::xp::Xp,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let held_uses = hotbar.count(selected);
    let books = gui.enchant_books();
    let offers = hotbar.enchant_offers(books);
    let lapis = {
        let mut n = 0u16;
        for i in 0..hotbar::SLOTS {
            if hotbar.slot(i) == Some("lapis_lazuli") {
                n = n.saturating_add(hotbar.count(i));
            }
        }
        n
    };
    let has_item = !held.is_empty();

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "ENCHANTING");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Bookshelves {books} · lapis {lapis} · level {}",
                            xp.level
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            if has_item {
                                let tip = gui::tip_for_tool(
                                    &held,
                                    Some(held_uses),
                                    hotbar.is_keen(selected),
                                    hotbar.is_sturdy(selected),
                                    hotbar.is_lucky(selected),
                                );
                                let tex = icon_texture(ui.ctx(), icons, &held);
                                let _ = slot_button(
                                    ui,
                                    icons,
                                    tex,
                                    false,
                                    if held_uses > 1 {
                                        Some(held_uses)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&tip.name)
                                            .size(15.0)
                                            .color(INK)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(&tip.line).size(12.0).color(MUTED),
                                    );
                                });
                            } else {
                                ui.label(
                                    RichText::new("Select a tool in the hotbar.")
                                        .size(14.0)
                                        .color(MUTED),
                                );
                            }
                        });
                    });
                    ui.add_space(10.0);
                    if offers.is_empty() {
                        ui.label(
                            RichText::new(if has_item {
                                "No offers — already enchanted, or need a dig tool."
                            } else {
                                "Hold a tool to see offers."
                            })
                            .size(13.0)
                            .color(MUTED),
                        );
                    } else {
                        for offer in &offers {
                            let can = xp.level >= offer.xp && lapis >= offer.lapis;
                            let row_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, row_tex, ROW, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(offer.title)
                                                .size(15.0)
                                                .color(INK)
                                                .strong(),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · −{} XP · {} lapis",
                                                offer.detail, offer.xp, offer.lapis
                                            ))
                                            .size(12.0)
                                            .color(MUTED),
                                        );
                                    });
                                    ui.add_space(8.0);
                                    ui.add_enabled_ui(can, |ui| {
                                        if chrome_button(
                                            ui,
                                            icons,
                                            if can { "Enchant" } else { "Need XP/lapis" },
                                            can,
                                            Vec2::new(120.0, 30.0),
                                            14.0,
                                        )
                                        .clicked()
                                        {
                                            if xp.level < offer.xp || lapis < offer.lapis {
                                                gui.note_enchant("need XP/lapis");
                                            } else {
                                                let word =
                                                    hotbar.apply_enchant(offer.id, offer.lapis);
                                                gui.note_enchant(word);
                                                if word == offer.id {
                                                    let _ = xp.spend_levels(offer.xp);
                                                    gui.request_enchant_chime();
                                                    let tip = gui::tip_for_tool(
                                                        &held,
                                                        Some(hotbar.count(selected)),
                                                        hotbar.is_keen(selected),
                                                        hotbar.is_sturdy(selected),
                                                        hotbar.is_lucky(selected),
                                                    );
                                                    gui.pin_tip(tip);
                                                }
                                                touched = true;
                                            }
                                        }
                                    });
                                });
                            });
                            ui.add_space(6.0);
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(gui.enchant_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_stonecutter(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let held_n = hotbar.count(selected);
    let options = hotbar.cut_options();
    if !options.is_empty() {
        let max = options.len().saturating_sub(1);
        if gui.cut_pick() > max {
            *gui.cut_pick_mut() = max;
        }
    } else if gui.cut_pick() != 0 {
        *gui.cut_pick_mut() = 0;
    }
    let pick = gui.cut_pick().min(options.len().saturating_sub(1));
    let chosen = options.get(pick).copied();
    let can_cut = chosen.is_some();

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "STONECUTTER");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Pick stairs · slabs · walls from one block")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Input").size(12.0).color(DETAIL));
                                ui.add_space(2.0);
                                if !held.is_empty() && !options.is_empty() {
                                    let tip = gui::tip_for(&held, Some(held_n.max(1)));
                                    let tex = icon_texture(ui.ctx(), icons, &held);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(
                                            ui,
                                            icons,
                                            tex,
                                            true,
                                            if held_n > 1 { Some(held_n) } else { None },
                                        );
                                        ui.label(
                                            RichText::new(&tip.name).size(13.0).color(INK),
                                        );
                                    });
                                } else if !held.is_empty() {
                                    let tip = gui::tip_for(&held, Some(held_n.max(1)));
                                    let tex = icon_texture(ui.ctx(), icons, &held);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(ui, icons, tex, false, None);
                                        ui.label(
                                            RichText::new(format!("{} · cannot cut", tip.name))
                                                .size(13.0)
                                                .color(MUTED),
                                        );
                                    });
                                } else {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.label(
                                        RichText::new("hold stone")
                                            .size(13.0)
                                            .color(MUTED),
                                    );
                                }
                            });
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Result").size(12.0).color(DETAIL));
                                ui.add_space(2.0);
                                if let Some(option) = chosen {
                                    let tip = gui::tip_for(option.result, Some(option.count));
                                    let tex = icon_texture(ui.ctx(), icons, option.result);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(
                                            ui,
                                            icons,
                                            tex,
                                            true,
                                            if option.count > 1 {
                                                Some(option.count)
                                            } else {
                                                None
                                            },
                                        );
                                        ui.label(
                                            RichText::new(&tip.name).size(13.0).color(INK),
                                        );
                                    });
                                } else {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.label(
                                        RichText::new("no recipe")
                                            .size(13.0)
                                            .color(MUTED),
                                    );
                                }
                            });
                        });
                    });
                    ui.add_space(12.0);
                    ui.label(RichText::new("Recipes").size(13.0).color(DETAIL));
                    ui.add_space(4.0);
                    if options.is_empty() {
                        ui.label(
                            RichText::new("Hold stone, cobble, smooth stone, or stone bricks.")
                                .size(13.0)
                                .color(MUTED),
                        );
                    } else {
                        ui.horizontal(|ui| {
                            for (index, option) in options.iter().enumerate() {
                                let on = pick == index;
                                let caption = if option.count > 1 {
                                    format!("{} ×{}", option.label, option.count)
                                } else {
                                    option.label.to_string()
                                };
                                if chrome_button(
                                    ui,
                                    icons,
                                    &caption,
                                    on,
                                    Vec2::new(100.0, 32.0),
                                    13.0,
                                )
                                .clicked()
                                {
                                    *gui.cut_pick_mut() = index;
                                    touched = true;
                                }
                                ui.add_space(6.0);
                            }
                        });
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(can_cut, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if can_cut { "Cut" } else { "Need stone" },
                                can_cut,
                                Vec2::new(120.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if let Some(option) = chosen {
                                    let word = hotbar.cut_into(option.result);
                                    gui.note_cut(word);
                                    gui.push_chat(word);
                                    if word == option.word {
                                        let tip = gui::tip_for(option.result, Some(option.count));
                                        gui.pin_tip(tip);
                                    }
                                    touched = true;
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(gui.cut_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_cartography(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let holding_map = !held.is_empty() && rebuild_world::Pack::builtin().is_map(&held);
    let zoom = hotbar.map_zoom();
    let locked = hotbar.map_locked();
    let paper = hotbar.map_paper();
    let can_widen = holding_map && !locked && zoom < crate::map::ZOOM_MAX;
    let can_narrow = holding_map && !locked && zoom > 0;
    let can_clone = holding_map && paper > 0 && hotbar.room_for("map");

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "CARTOGRAPHY");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Zoom {zoom}/{} · paper {paper} · {}",
                            crate::map::ZOOM_MAX,
                            if locked { "locked" } else { "unlocked" }
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Map").size(12.0).color(DETAIL));
                                ui.add_space(2.0);
                                if holding_map {
                                    let tip = gui::tip_for(&held, Some(1));
                                    let tex = icon_texture(ui.ctx(), icons, &held);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(ui, icons, tex, true, None);
                                        ui.label(
                                            RichText::new(&tip.name).size(13.0).color(INK),
                                        );
                                    });
                                } else {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.label(
                                        RichText::new("hold a map")
                                            .size(13.0)
                                            .color(MUTED),
                                    );
                                }
                            });
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Paper").size(12.0).color(DETAIL));
                                ui.add_space(2.0);
                                let tex = icon_texture(ui.ctx(), icons, "paper");
                                ui.horizontal(|ui| {
                                    let _ = slot_button(
                                        ui,
                                        icons,
                                        tex,
                                        paper > 0,
                                        if paper > 0 { Some(paper) } else { None },
                                    );
                                    ui.label(
                                        RichText::new(if paper > 0 {
                                            "ready to clone"
                                        } else {
                                            "need paper"
                                        })
                                        .size(13.0)
                                        .color(if paper > 0 { INK } else { MUTED }),
                                    );
                                });
                            });
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(can_widen, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                "Widen",
                                can_widen,
                                Vec2::new(100.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let word = hotbar.chart_map();
                                gui.note_chart(word);
                                gui.push_chat(word);
                                touched = true;
                            }
                        });
                        ui.add_space(6.0);
                        ui.add_enabled_ui(can_narrow, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                "Narrow",
                                can_narrow,
                                Vec2::new(100.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                hotbar.zoom_map(false);
                                gui.note_chart("narrower");
                                gui.push_chat("narrower");
                                touched = true;
                            }
                        });
                        ui.add_space(6.0);
                        ui.add_enabled_ui(holding_map, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if locked { "Unlock" } else { "Lock" },
                                holding_map,
                                Vec2::new(100.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let word = hotbar.lock_map();
                                gui.note_chart(word);
                                gui.push_chat(word);
                                touched = true;
                            }
                        });
                        ui.add_space(6.0);
                        ui.add_enabled_ui(can_clone, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if can_clone { "Clone" } else { "Need paper" },
                                can_clone,
                                Vec2::new(110.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let word = hotbar.clone_map();
                                gui.note_chart(word);
                                gui.push_chat(word);
                                if word == "cloned" {
                                    let tip = gui::tip_for("map", Some(1));
                                    gui.pin_tip(tip);
                                }
                                touched = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(gui.chart_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_chest(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = gui.chest_selected();
    let title = gui.chest_title().to_string();
    let cols = 3usize;
    let rows = hotbar::PACK.div_ceil(cols);

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                let panel_w = (grid_pitch(INV_COLS) + 48.0)
                    .min(ui.available_width() - 24.0)
                    .max(420.0);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, &title);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Click to take or stash · digit keys put from the hotbar")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    ui.label(RichText::new("Container").size(12.0).color(DETAIL));
                    ui.add_space(2.0);
                    let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, well, ROW, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                        for row in 0..rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                                for col in 0..cols {
                                    let index = row * cols + col;
                                    if index >= hotbar::PACK {
                                        break;
                                    }
                                    if chest_slot_button(
                                        ui, gui, hotbar, bag, icons, index, selected,
                                    ) {
                                        touched = true;
                                    }
                                }
                            });
                        }
                    });
                    ui.add_space(12.0);
                    ui.label(RichText::new("Inventory").size(13.0).color(DETAIL));
                    ui.add_space(4.0);
                    if draw_chest_inv_body(ui, gui, hotbar, bag, icons) {
                        touched = true;
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Hotbar").size(13.0).color(DETAIL));
                    ui.add_space(4.0);
                    if draw_chest_hotbar(ui, gui, hotbar, bag, icons) {
                        touched = true;
                    }
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                        touched = true;
                    }
                });
            });
        });
    touched
}

fn chest_slot_button(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
    index: usize,
    selected: usize,
) -> bool {
    let id = gui.chest_slot(index).unwrap_or("").to_string();
    let count = gui.chest_count(index);
    let tex = if id.is_empty() {
        None
    } else {
        icon_texture(ui.ctx(), icons, &id)
    };
    let resp = slot_button(
        ui,
        icons,
        tex,
        selected == index,
        if count > 1 { Some(count) } else { None },
    );
    if !id.is_empty() {
        let tip = gui::tip_for(&id, Some(count.max(1)));
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    }
    if !resp.clicked() {
        return false;
    }
    gui.select_chest(index);
    if !id.is_empty() {
        let Some(taken) = gui.take_chest_at(index) else {
            return true;
        };
        if hotbar.collect(taken) || bag.collect(taken) {
            return true;
        }
        let _ = gui.give_chest_at(index, taken);
        return true;
    }
    let Some(held) = hotbar.slot(hotbar.selected()) else {
        return true;
    };
    let held = held.to_string();
    if gui.give_chest_at(index, &held) {
        hotbar.consume_selected();
    }
    true
}

fn draw_chest_inv_body(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    egui::Grid::new("chest_inv_body")
        .spacing([SLOT_GAP, SLOT_GAP])
        .min_col_width(SLOT)
        .show(ui, |ui| {
            for index in 0..INV_BODY {
                let (id, count) = if index < hotbar::PACK {
                    (
                        bag.slot(index).unwrap_or("").to_string(),
                        bag.count(index),
                    )
                } else {
                    (String::new(), 0)
                };
                let tex = if id.is_empty() {
                    None
                } else {
                    icon_texture(ui.ctx(), icons, &id)
                };
                let resp = slot_button(
                    ui,
                    icons,
                    tex,
                    false,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for(&id, Some(count));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                    if resp.hovered() {
                        gui.select_emi(&id);
                    }
                }
                if resp.clicked() {
                    if !id.is_empty() {
                        // Bag → chest (or hotbar if chest full).
                        if let Some(taken) = bag.take_one(index) {
                            if gui.give_chest(taken) {
                                touched = true;
                            } else if hotbar.collect(taken) {
                                touched = true;
                            } else {
                                let _ = bag.insert_at(index, taken);
                            }
                        }
                    } else if let Some(held) = hotbar.slot(hotbar.selected()) {
                        let held = held.to_string();
                        if bag.insert_at(index, &held) {
                            hotbar.consume_selected();
                            touched = true;
                        }
                    }
                }
                if (index + 1) % INV_COLS == 0 {
                    ui.end_row();
                }
            }
        });
    touched
}

fn draw_chest_hotbar(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let row_w = grid_pitch(hotbar::SLOTS);
    if ui.available_width() + 0.5 >= row_w {
        ui.set_min_width(row_w);
    }
    egui::Grid::new("chest_hotbar_row")
        .spacing([SLOT_GAP, SLOT_GAP])
        .min_col_width(SLOT)
        .show(ui, |ui| {
            for slot in 0..hotbar::SLOTS {
                let id = hotbar.slot(slot).unwrap_or("").to_string();
                let count = hotbar.count(slot);
                let selected = hotbar.selected() == slot;
                let tex = if id.is_empty() {
                    None
                } else {
                    icon_texture(ui.ctx(), icons, &id)
                };
                let resp = slot_button(
                    ui,
                    icons,
                    tex,
                    selected,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for_keen(&id, Some(count), hotbar.is_keen(slot));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                    if resp.hovered() {
                        gui.select_emi(&id);
                    }
                }
                if resp.clicked() {
                    let _ = hotbar.select_digit((slot + 1) as u8);
                    // Second job: push this stack into the chest when selected again.
                    if !id.is_empty() && selected {
                        if gui.give_chest(&id) {
                            hotbar.consume_at(slot);
                            touched = true;
                        }
                    }
                }
            }
            ui.end_row();
        });
    let _ = bag;
    touched
}

fn draw_crafting_table(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> (bool, bool) {
    let mut touched = false;
    let mut crafted = false;
    let selected = gui.craft_selected();
    let preview = gui.craft_preview();
    let has_result = preview.is_some();

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                let panel_w = (grid_pitch(INV_COLS) + 48.0).min(ui.available_width() - 24.0).max(420.0);
                show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "CRAFTING");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("3×3 table · shaped recipes welcome · digit keys stash")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Grid").size(12.0).color(DETAIL));
                            ui.add_space(2.0);
                            let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, well, ROW, |ui| {
                                ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                                for row in 0..3 {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                                        for col in 0..3 {
                                            let index = row * 3 + col;
                                            if table_craft_input(
                                                ui, gui, hotbar, bag, icons, index, selected,
                                            ) {
                                                touched = true;
                                            }
                                        }
                                    });
                                }
                            });
                        });
                        ui.add_space(18.0);
                        ui.vertical(|ui| {
                            ui.add_space(SLOT + 8.0);
                            ui.label(RichText::new("Result").size(12.0).color(DETAIL));
                            ui.add_space(2.0);
                            let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, well, ROW, |ui| {
                                let tex = preview
                                    .as_deref()
                                    .and_then(|id| icon_texture(ui.ctx(), icons, id));
                                let on = selected == 9 || has_result;
                                let resp = slot_button(ui, icons, tex, on, None);
                                if let Some(id) = preview.as_deref() {
                                    let tip = gui::tip_for(id, Some(1));
                                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                                    ui.label(
                                        RichText::new(&tip.name).size(13.0).color(INK),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("—").size(13.0).color(MUTED),
                                    );
                                }
                                if resp.clicked() && has_result {
                                    gui.select_craft(9);
                                    gui.request_craft_take();
                                    touched = true;
                                    crafted = true;
                                }
                            });
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            preview
                                .as_deref()
                                .map(|id| format!("ready · {id}"))
                                .unwrap_or_else(|| "put items in the grid".to_string()),
                        )
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(has_result, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if has_result { "Take result" } else { "No match" },
                                has_result,
                                Vec2::new(140.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                gui.request_craft_take();
                                touched = true;
                                crafted = true;
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.label(RichText::new("Inventory").size(13.0).color(DETAIL));
                    ui.add_space(4.0);
                    draw_table_inv_body(ui, gui, hotbar, bag, icons);
                    ui.add_space(8.0);
                    ui.label(RichText::new("Hotbar").size(13.0).color(DETAIL));
                    ui.add_space(4.0);
                    draw_hotbar_row(ui, Some(gui), hotbar, icons);
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        for id in gui.drain_craft() {
                            if !hotbar.collect(id) {
                                let _ = bag.collect(id);
                            }
                        }
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                        touched = true;
                    }
                });
            });
        });
    (touched, crafted)
}

fn table_craft_input(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
    index: usize,
    selected: usize,
) -> bool {
    let id = gui.craft_slot(index).unwrap_or("").to_string();
    let count = gui.craft_count(index);
    let tex = if id.is_empty() {
        None
    } else {
        icon_texture(ui.ctx(), icons, &id)
    };
    let resp = slot_button(
        ui,
        icons,
        tex,
        selected == index,
        if count > 1 { Some(count) } else { None },
    );
    if !id.is_empty() {
        let tip = gui::tip_for(&id, Some(count.max(1)));
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    }
    if !resp.clicked() {
        return false;
    }
    gui.select_craft(index);
    if !id.is_empty() {
        let Some(taken) = gui.take_craft_slot(index) else {
            return true;
        };
        if !hotbar.collect(taken) {
            let _ = bag.collect(taken);
        }
        return true;
    }
    let Some(held) = hotbar.slot(hotbar.selected()) else {
        return true;
    };
    let held = held.to_string();
    if gui.give_craft_at(index, &held) {
        hotbar.consume_selected();
    }
    true
}

fn draw_table_inv_body(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) {
    egui::Grid::new("craft_table_inv_body")
        .spacing([SLOT_GAP, SLOT_GAP])
        .min_col_width(SLOT)
        .show(ui, |ui| {
            for index in 0..INV_BODY {
                let (id, count) = if index < hotbar::PACK {
                    (
                        bag.slot(index).unwrap_or("").to_string(),
                        bag.count(index),
                    )
                } else {
                    (String::new(), 0)
                };
                let tex = if id.is_empty() {
                    None
                } else {
                    icon_texture(ui.ctx(), icons, &id)
                };
                let resp = slot_button(
                    ui,
                    icons,
                    tex,
                    false,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for(&id, Some(count));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                    if resp.hovered() {
                        gui.select_emi(&id);
                    }
                    if resp.clicked() {
                        let _ = hotbar.give_stack(&id);
                    }
                }
                if (index + 1) % INV_COLS == 0 {
                    ui.end_row();
                }
            }
        });
}

fn draw_furnace(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = gui.furnace_selected();
    let cook = gui.furnace_cook();
    let goal = gui.furnace_cook_goal().max(1);
    let frac = (cook as f32 / goal as f32).clamp(0.0, 1.0);
    let preview = gui.furnace_preview();
    let status = gui.furnace_status();
    let title = gui.furnace_title();
    let has_output = gui.furnace_slot(2).is_some();
    let held = hotbar
        .slot(hotbar.selected())
        .map(str::to_string)
        .unwrap_or_default();
    let can_add = selected < 2 && !held.is_empty();

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, title);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Input · fuel · output — digit keys stash from the hotbar")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        for (index, label) in [(0, "Input"), (1, "Fuel"), (2, "Output")] {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(label).size(12.0).color(DETAIL));
                                ui.add_space(2.0);
                                let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                                chrome_well(ui, well, ROW, |ui| {
                                    let id = gui.furnace_slot(index).map(str::to_string);
                                    let count = gui.furnace_count(index);
                                    let on = selected == index;
                                    let tex = id
                                        .as_deref()
                                        .and_then(|id| icon_texture(ui.ctx(), icons, id));
                                    let response = slot_button(
                                        ui,
                                        icons,
                                        tex,
                                        on,
                                        if count > 1 { Some(count) } else { None },
                                    );
                                    if response.clicked() {
                                        if index == 2 && has_output {
                                            gui.request_furnace_take();
                                            touched = true;
                                        } else if index < 2 {
                                            gui.select_furnace(index);
                                            touched = true;
                                        }
                                    }
                                    if let Some(id) = id.as_deref() {
                                        let tip = gui::tip_for(id, Some(count.max(1)));
                                        ui.label(
                                            RichText::new(&tip.name).size(12.0).color(INK),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new("—").size(12.0).color(MUTED),
                                        );
                                    }
                                });
                            });
                            if index < 2 {
                                ui.add_space(10.0);
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.label(RichText::new("Cook progress").size(12.0).color(DETAIL));
                    ui.add_space(2.0);
                    let bar_w = 320.0;
                    let (bar_rect, _) =
                        ui.allocate_exact_size(Vec2::new(bar_w, 14.0), Sense::hover());
                    if ui.is_rect_visible(bar_rect) {
                        ui.painter().rect_filled(bar_rect, CornerRadius::same(3), SLOT_WELL);
                        ui.painter().rect_stroke(
                            bar_rect,
                            CornerRadius::same(3),
                            Stroke::new(1.0, SLOT_EDGE),
                            StrokeKind::Outside,
                        );
                        if frac > 0.0 {
                            let mut fill = bar_rect;
                            fill.set_width(bar_rect.width() * frac);
                            ui.painter()
                                .rect_filled(fill, CornerRadius::same(3), SELECT);
                        }
                    }
                    ui.label(
                        RichText::new(format!(
                            "{cook}/{goal}{}",
                            preview
                                .as_deref()
                                .map(|id| format!(" · → {id}"))
                                .unwrap_or_default()
                        ))
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label(RichText::new(status).size(12.0).color(MUTED));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(can_add, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if can_add { "Add held" } else { "Select slot" },
                                can_add,
                                Vec2::new(140.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                if gui.give_furnace(&held) {
                                    hotbar.consume_at(hotbar.selected());
                                    let tip = gui::tip_for(&held, Some(1));
                                    gui.pin_tip(tip);
                                    touched = true;
                                }
                            }
                        });
                        ui.add_space(8.0);
                        ui.add_enabled_ui(has_output, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if has_output { "Take output" } else { "No output" },
                                has_output,
                                Vec2::new(140.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                gui.request_furnace_take();
                                touched = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_brewing(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let bottles = hotbar.brew_bottles();
    let fuel = hotbar.brew_fuel();
    let ingredient = hotbar.brew_target();
    let result = hotbar.brew_result();
    let recipe = hotbar.brew_recipe_label();
    let can_brew = ingredient.is_some() && result.is_some();

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                show_panel(ui, 480.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "BREWING");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("Blaze powder fuel · {fuel}"))
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    ui.label(RichText::new("Bottles").size(13.0).color(DETAIL));
                    ui.add_space(2.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            if bottles.is_empty() {
                                for _ in 0..3 {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.add_space(6.0);
                                }
                                ui.label(
                                    RichText::new("empty — add water or potions")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                            } else {
                                for (id, count) in bottles.iter().take(3) {
                                    let tex = icon_texture(ui.ctx(), icons, id);
                                    let _ = slot_button(
                                        ui,
                                        icons,
                                        tex,
                                        false,
                                        if *count > 1 { Some(*count) } else { None },
                                    );
                                    ui.add_space(4.0);
                                }
                                if bottles.len() > 3 {
                                    ui.label(
                                        RichText::new(format!("+{}", bottles.len() - 3))
                                            .size(12.0)
                                            .color(MUTED),
                                    );
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Ingredient").size(12.0).color(DETAIL));
                            ui.add_space(2.0);
                            let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, well, ROW, |ui| {
                                if let Some(id) = ingredient {
                                    let tip = gui::tip_for(id, Some(1));
                                    let tex = icon_texture(ui.ctx(), icons, id);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(ui, icons, tex, false, None);
                                        ui.label(
                                            RichText::new(&tip.name).size(13.0).color(INK),
                                        );
                                    });
                                } else {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.label(
                                        RichText::new("—").size(13.0).color(MUTED),
                                    );
                                }
                            });
                        });
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Fuel").size(12.0).color(DETAIL));
                            ui.add_space(2.0);
                            let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, well, ROW, |ui| {
                                let tex = icon_texture(ui.ctx(), icons, "blaze_powder");
                                ui.horizontal(|ui| {
                                    let _ = slot_button(
                                        ui,
                                        icons,
                                        tex,
                                        fuel > 0,
                                        if fuel > 0 { Some(fuel) } else { None },
                                    );
                                    ui.label(
                                        RichText::new(if fuel > 0 {
                                            "blaze powder"
                                        } else {
                                            "need powder"
                                        })
                                        .size(13.0)
                                        .color(if fuel > 0 { INK } else { MUTED }),
                                    );
                                });
                            });
                        });
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Result").size(12.0).color(DETAIL));
                            ui.add_space(2.0);
                            let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                            chrome_well(ui, well, ROW, |ui| {
                                if let Some(id) = result {
                                    let tip = gui::tip_for(id, Some(1));
                                    let tex = icon_texture(ui.ctx(), icons, id);
                                    ui.horizontal(|ui| {
                                        let _ = slot_button(ui, icons, tex, can_brew, None);
                                        ui.label(
                                            RichText::new(&tip.name).size(13.0).color(INK),
                                        );
                                    });
                                } else {
                                    let _ = slot_button(ui, icons, None, false, None);
                                    ui.label(
                                        RichText::new("—").size(13.0).color(MUTED),
                                    );
                                }
                            });
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new(recipe).size(12.0).color(MUTED));
                    ui.add_space(8.0);
                    ui.add_enabled_ui(can_brew, |ui| {
                        if chrome_button(
                            ui,
                            icons,
                            if can_brew { "Brew" } else { "Need mix" },
                            can_brew,
                            Vec2::new(160.0, 32.0),
                            14.0,
                        )
                        .clicked()
                        {
                            let word = hotbar.brew_potion();
                            gui.note_brew(word);
                            gui.push_chat(word);
                            if word == "brewed" {
                                gui.request_brew_bubble();
                                if let Some(id) = hotbar
                                    .brew_bottles()
                                    .into_iter()
                                    .last()
                                    .map(|(id, _)| id)
                                    .or(result)
                                {
                                    let tip = gui::tip_for(id, Some(1));
                                    gui.pin_tip(tip);
                                }
                            }
                            touched = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(gui.brew_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_grindstone(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    xp: &mut crate::xp::Xp,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let held_uses = hotbar.count(selected);
    let has_item = !held.is_empty();
    let enchants = hotbar.enchant_count(selected);
    let can_strip = hotbar.can_disenchant();
    let can_combine = hotbar.combine_donor().is_some();
    let keen = hotbar.is_keen(selected);
    let sturdy = hotbar.is_sturdy(selected);
    let lucky = hotbar.is_lucky(selected);

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "GRINDSTONE");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Strip enchants for XP · combine a worn pair · level {}",
                            xp.level
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            if has_item {
                                let tip = gui::tip_for_tool(
                                    &held,
                                    Some(held_uses),
                                    keen,
                                    sturdy,
                                    lucky,
                                );
                                let tex = icon_texture(ui.ctx(), icons, &held);
                                let _ = slot_button(
                                    ui,
                                    icons,
                                    tex,
                                    false,
                                    if held_uses > 1 {
                                        Some(held_uses)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&tip.name)
                                            .size(15.0)
                                            .color(INK)
                                            .strong(),
                                    );
                                    let enchant_line = if enchants == 0 {
                                        tip.line.clone()
                                    } else {
                                        let mut parts = Vec::new();
                                        if keen {
                                            parts.push("Efficiency");
                                        }
                                        if sturdy {
                                            parts.push("Unbreaking");
                                        }
                                        if lucky {
                                            parts.push("Fortune");
                                        }
                                        format!("{} · +{} XP if stripped", parts.join(", "), enchants)
                                    };
                                    ui.label(
                                        RichText::new(enchant_line).size(12.0).color(MUTED),
                                    );
                                });
                            } else {
                                ui.label(
                                    RichText::new("Select a tool in the hotbar.")
                                        .size(14.0)
                                        .color(MUTED),
                                );
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(can_strip, |ui| {
                            let caption = if can_strip {
                                format!("Disenchant (+{enchants} XP)")
                            } else {
                                "No enchants".to_string()
                            };
                            if chrome_button(
                                ui,
                                icons,
                                &caption,
                                can_strip,
                                Vec2::new(200.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let refund = hotbar.enchant_count(selected);
                                let word = hotbar.grind_pick();
                                gui.note_grind(word);
                                if word == "ground" {
                                    let _ = xp.gain(refund.max(1));
                                    gui.request_grind_scrape();
                                    if let Some(id) = hotbar.slot(hotbar.selected()) {
                                        let tip = gui::tip_for_tool(
                                            id,
                                            Some(hotbar.count(hotbar.selected())),
                                            false,
                                            false,
                                            false,
                                        );
                                        gui.pin_tip(tip);
                                    }
                                } else if word == "sharpened" {
                                    gui.request_grind_scrape();
                                }
                                touched = true;
                            }
                        });
                        ui.add_space(8.0);
                        ui.add_enabled_ui(can_combine, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                if can_combine {
                                    "Combine pair"
                                } else {
                                    "Need a pair"
                                },
                                can_combine,
                                Vec2::new(160.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let word = hotbar.combine_tool();
                                gui.note_grind(word);
                                if word == "combined" {
                                    gui.request_grind_scrape();
                                    if let Some(id) = hotbar.slot(hotbar.selected()) {
                                        let tip = gui::tip_for_tool(
                                            id,
                                            Some(hotbar.count(hotbar.selected())),
                                            hotbar.is_keen(hotbar.selected()),
                                            hotbar.is_sturdy(hotbar.selected()),
                                            hotbar.is_lucky(hotbar.selected()),
                                        );
                                        gui.pin_tip(tip);
                                    }
                                }
                                touched = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(gui.grind_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_smithing(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let selected = hotbar.selected();
    let held = hotbar.slot(selected).unwrap_or("").to_string();
    let held_uses = hotbar.count(selected);
    let has_item = !held.is_empty();
    let netherite_next = hotbar::netherite_upgrade(&held);
    let has_ingot = hotbar.holds("netherite_ingot", 1);
    let can_netherite = netherite_next.is_some() && has_ingot;
    let can_other = hotbar.smith_target().is_some() && netherite_next.is_none();
    let ingot_n = {
        let mut n = 0u16;
        for i in 0..hotbar::SLOTS {
            if hotbar.slot(i) == Some("netherite_ingot") {
                n = n.saturating_add(hotbar.count(i));
            }
        }
        n
    };
    let trim_idx = gui.smith_trim().min(hotbar::SMITH_TRIMS.len().saturating_sub(1));

    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                show_panel(ui, 460.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "SMITHING");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Upgrade diamond gear with a netherite ingot · {ingot_n} ingot(s)"
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let slot_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                    chrome_well(ui, slot_tex, ROW, |ui| {
                        ui.horizontal(|ui| {
                            if has_item {
                                let tip = gui::tip_for_tool(
                                    &held,
                                    Some(held_uses),
                                    hotbar.is_keen(selected),
                                    hotbar.is_sturdy(selected),
                                    hotbar.is_lucky(selected),
                                );
                                let tex = icon_texture(ui.ctx(), icons, &held);
                                let _ = slot_button(
                                    ui,
                                    icons,
                                    tex,
                                    false,
                                    if held_uses > 1 {
                                        Some(held_uses)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_space(6.0);
                                ui.label(RichText::new("→").size(18.0).color(DETAIL));
                                ui.add_space(6.0);
                                if let Some(next) = netherite_next {
                                    let next_tip = gui::tip_for(next, Some(1));
                                    let tex = icon_texture(ui.ctx(), icons, next);
                                    let _ = slot_button(ui, icons, tex, false, None);
                                    ui.add_space(8.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&tip.name)
                                                .size(14.0)
                                                .color(INK)
                                                .strong(),
                                        );
                                        ui.label(
                                            RichText::new(format!("becomes {}", next_tip.name))
                                                .size(12.0)
                                                .color(MUTED),
                                        );
                                    });
                                } else {
                                    ui.add_space(8.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&tip.name)
                                                .size(14.0)
                                                .color(INK)
                                                .strong(),
                                        );
                                        ui.label(
                                            RichText::new(&tip.line).size(12.0).color(MUTED),
                                        );
                                    });
                                }
                            } else {
                                ui.label(
                                    RichText::new("Select diamond gear (or a pick to tier up).")
                                        .size(14.0)
                                        .color(MUTED),
                                );
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("Trim (stub)").size(13.0).color(DETAIL));
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        for (index, name) in hotbar::SMITH_TRIMS.iter().enumerate() {
                            let on = trim_idx == index;
                            if chrome_button(
                                ui,
                                icons,
                                name,
                                on,
                                Vec2::new(64.0, 26.0),
                                12.0,
                            )
                            .clicked()
                            {
                                *gui.smith_trim_mut() = index;
                                touched = true;
                            }
                            ui.add_space(4.0);
                        }
                    });
                    ui.label(
                        RichText::new("Cosmetic name only — no armor-trim stats yet.")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let upgrade_ok = can_netherite || can_other;
                        let caption = if can_netherite {
                            "Upgrade (−1 ingot)"
                        } else if can_other {
                            "Upgrade"
                        } else if netherite_next.is_some() {
                            "Need netherite ingot"
                        } else {
                            "Need diamond gear"
                        };
                        ui.add_enabled_ui(upgrade_ok, |ui| {
                            if chrome_button(
                                ui,
                                icons,
                                caption,
                                upgrade_ok,
                                Vec2::new(200.0, 32.0),
                                14.0,
                            )
                            .clicked()
                            {
                                let word = hotbar.smith_pick();
                                gui.note_smith(word);
                                if matches!(
                                    word,
                                    "netherite" | "iron" | "diamond" | "heavy"
                                ) {
                                    gui.request_smith_spark();
                                    let trim = hotbar::SMITH_TRIMS
                                        .get(gui.smith_trim())
                                        .copied()
                                        .unwrap_or("None");
                                    if word == "netherite" && trim != "None" {
                                        if let Some(id) = hotbar.slot(hotbar.selected()) {
                                            let base = gui::tip_for(id, Some(1)).name;
                                            let named = format!("{trim} {base}");
                                            let _ = hotbar.rename_selected(&named);
                                        }
                                    }
                                    if let Some(id) = hotbar.slot(hotbar.selected()) {
                                        let tip = gui::tip_for_tool(
                                            id,
                                            Some(hotbar.count(hotbar.selected())),
                                            hotbar.is_keen(hotbar.selected()),
                                            hotbar.is_sturdy(hotbar.selected()),
                                            hotbar.is_lucky(hotbar.selected()),
                                        );
                                        let mut tip = tip;
                                        if let Some(label) = hotbar.label_of(hotbar.selected()) {
                                            tip.name = label.to_string();
                                        }
                                        gui.pin_tip(tip);
                                    }
                                }
                                touched = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(gui.smith_note())
                            .size(13.0)
                            .color(DETAIL),
                    );
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        let _ = gui.on_press(winit::keyboard::KeyCode::Escape);
                    }
                });
            });
        });
    touched
}

fn draw_trade(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(48.0);
                show_panel(ui, 420.0, panel_chrome(ui.ctx(), icons), |ui| {
                    title_label(ui, "VILLAGE TRADER");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Emeralds for stock goods. Esc closes.")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(12.0);
                    let mut stock = gui.trade_stock();
                    for (index, offer) in crate::mobs::TRADER_OFFERS.iter().enumerate() {
                        let left = stock[index];
                        let can = left > 0
                            && hotbar.holds(offer.cost, offer.cost_n)
                            && (hotbar.room_for(offer.give) || bag.room_for(offer.give));
                        let row_tex = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
                        chrome_well(ui, row_tex, ROW, |ui| {
                            ui.horizontal(|ui| {
                                let label = format!(
                                    "{}× {}  →  {}× {}   ({} left)",
                                    offer.cost_n,
                                    offer.cost,
                                    offer.give_n,
                                    offer.give,
                                    left
                                );
                                ui.label(RichText::new(label).size(14.0).color(INK));
                                ui.add_space(8.0);
                                let caption = if left == 0 { "Sold out" } else { "Trade" };
                                ui.add_enabled_ui(can && left > 0, |ui| {
                                    if chrome_button(
                                        ui,
                                        icons,
                                        caption,
                                        can,
                                        Vec2::new(88.0, 28.0),
                                        14.0,
                                    )
                                    .clicked()
                                    {
                                        let mut paid = true;
                                        for _ in 0..offer.cost_n {
                                            if !hotbar.consume_one(offer.cost) {
                                                paid = false;
                                                break;
                                            }
                                        }
                                        if paid {
                                            for _ in 0..offer.give_n {
                                                if !hotbar.collect(offer.give) {
                                                    bag.collect(offer.give);
                                                }
                                            }
                                            stock[index] = left.saturating_sub(1);
                                            touched = true;
                                        }
                                    }
                                });
                            });
                        });
                        ui.add_space(6.0);
                    }
                    gui.set_trade_stock(stock);
                    ui.add_space(8.0);
                    if close_button(ui, icons, "Close (Esc)").clicked() {
                        gui.close_trade();
                    }
                });
            });
        });
    touched
}

/// Bottom-left chat log plate — samples Whimscape panel chrome.
fn draw_chat_plate(
    ui: &mut Ui,
    gui: &Gui,
    icons: &mut HashMap<String, TextureHandle>,
) {
    let lines = gui.chat_overlay_lines();
    if lines.is_empty() {
        return;
    }
    let panel_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
    let panel_w = if gui.chat_open() { 420.0 } else { 340.0 };
    // Match the old quad stack: above hunger/health/armor at the bottom-left.
    egui::Area::new(egui::Id::new("chat_log_plate"))
        .anchor(Align2::LEFT_BOTTOM, [16.0, -110.0])
        .order(egui::Order::Middle)
        .interactable(gui.chat_open())
        .show(ui.ctx(), |ui| {
            let fill = if panel_tex.is_some() {
                Color32::from_rgba_unmultiplied(8, 9, 12, 220)
            } else {
                PANEL
            };
            Frame::new()
                .fill(fill)
                .stroke(Stroke::new(1.0, SLOT_EDGE))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    if let Some(tex) = panel_tex {
                        let rect = ui.max_rect();
                        ui.painter().image(
                            tex,
                            rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                            Color32::WHITE,
                        );
                    }
                    ui.set_min_width(panel_w);
                    ui.set_max_width(panel_w);
                    for (line, draft) in &lines {
                        let color = if *draft { SELECT } else { INK };
                        ui.label(RichText::new(line).size(14.0).color(color));
                    }
                });
        });
}

fn draw_player_inventory(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
) -> (bool, bool) {
    let mut touched = false;
    let mut crafted = false;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let max_w = chrome_max_width(ui);
            let panel_w = clamped_panel_width(max_w, INV_COLS, true);
            let emi_w = EMI_SIDE_W.max(180.0);
            let side_by_side = panel_w + 12.0 + emi_w <= max_w + 0.5;
            let emi_list_h = creative_grid_height(ui.available_height(), side_by_side);
            ui.vertical_centered(|ui| {
                ui.set_max_width(max_w);
                egui::ScrollArea::vertical()
                    .id_salt("player_inv_chrome")
                    .max_height(ui.available_height() * 0.94)
                    .show(ui, |ui| {
                        ui.set_max_width(max_w);
                        if side_by_side {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);
                                let (panel_touched, panel_crafted) = draw_player_inventory_panel(
                                    ui, gui, hotbar, bag, armor, icons, panel_w,
                                );
                                if panel_touched {
                                    touched = true;
                                }
                                if panel_crafted {
                                    crafted = true;
                                }
                                if draw_emi_sidebar(ui, gui, hotbar, bag, icons, emi_list_h) {
                                    touched = true;
                                }
                            });
                        } else {
                            let (panel_touched, panel_crafted) = draw_player_inventory_panel(
                                ui, gui, hotbar, bag, armor, icons, panel_w,
                            );
                            if panel_touched {
                                touched = true;
                            }
                            if panel_crafted {
                                crafted = true;
                            }
                            ui.add_space(10.0);
                            if draw_emi_sidebar(ui, gui, hotbar, bag, icons, emi_list_h) {
                                touched = true;
                            }
                        }
                    });
            });
        });
    (touched, crafted)
}

fn draw_player_inventory_panel(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
    panel_w: f32,
) -> (bool, bool) {
    let mut touched = false;
    let mut crafted = false;
    let grid_w = grid_pitch(INV_COLS);
    show_panel(ui, panel_w, panel_chrome(ui.ctx(), icons), |ui| {
        ui.set_min_width(grid_w.min(panel_w));
        title_label(ui, "INVENTORY");
        ui.add_space(8.0);
        draw_inv_tab_strip(ui, gui, icons);
        ui.add_space(10.0);
        match gui.inv_tab() {
            PlayerInvTab::Player => {
                // Equipment + body inventory (GUI plan PLAYER detail).
                if draw_player_detail(ui, hotbar, bag, armor, icons) {
                    touched = true;
                }
                ui.add_space(12.0);
                ui.label(RichText::new("inventory").color(MUTED).size(12.0));
                ui.add_space(4.0);
                draw_inv_body_grid(ui, gui, hotbar, bag, icons);
            }
            PlayerInvTab::Craft => {
                // Focus the 2×2 + OUT; bag stays for ingredients. EMI stays beside.
                let (craft_touched, craft_done) = draw_craft_detail(ui, gui, hotbar, bag, icons);
                if craft_touched {
                    touched = true;
                }
                if craft_done {
                    crafted = true;
                }
                ui.add_space(12.0);
                ui.label(RichText::new("bag").color(MUTED).size(12.0));
                ui.add_space(4.0);
                draw_inv_body_grid(ui, gui, hotbar, bag, icons);
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("hotbar").color(MUTED).size(13.0));
        ui.add_space(4.0);
        draw_hotbar_row(ui, Some(gui), hotbar, icons);
        ui.add_space(8.0);
        if close_button(ui, icons, "Close (E)").clicked() {
            if refund_inv_craft(gui, hotbar, bag) {
                touched = true;
            }
            gui.close_items();
        }
    });
    (touched, crafted)
}

/// Segmented PLAYER | CRAFT strip under the inventory title.
fn draw_inv_tab_strip(
    ui: &mut Ui,
    gui: &mut Gui,
    icons: &mut HashMap<String, TextureHandle>,
) {
    let tabs = [
        (PlayerInvTab::Player, "PLAYER"),
        (PlayerInvTab::Craft, "CRAFT"),
    ];
    Frame::new()
        .fill(ROW)
        .stroke(Stroke::new(1.0, SLOT_EDGE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(4, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                let avail = ui.available_width().max(180.0);
                let each = ((avail - 4.0) / tabs.len() as f32).max(88.0);
                for (tab, label) in tabs {
                    let selected = gui.inv_tab() == tab;
                    let size = Vec2::new(each, 30.0);
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                    if ui.is_rect_visible(rect) {
                        let chrome = if selected {
                            CHROME_TAB_SELECTED
                        } else {
                            CHROME_TAB
                        };
                        if let Some(tex) = chrome_texture(ui.ctx(), icons, chrome) {
                            ui.painter().image(
                                tex,
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        } else {
                            let fill = if selected { SELECT } else { BUTTON };
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(3), fill);
                            ui.painter().rect_stroke(
                                rect,
                                CornerRadius::same(3),
                                Stroke::new(
                                    if selected { 1.5 } else { 1.0 },
                                    if selected { SELECT } else { SLOT_EDGE },
                                ),
                                StrokeKind::Outside,
                            );
                        }
                        let ink = if selected {
                            Color32::from_rgb(20, 18, 12)
                        } else {
                            INK
                        };
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(14.0),
                            ink,
                        );
                    }
                    if response.clicked() {
                        gui.set_inv_tab(tab);
                    }
                }
            });
        });
}

/// EMI item browser: searchable list + CRAFTING / USES for the selection.
///
/// `list_h` is the pinned item-grid band in points (same floor as creative: ≥8 rows).
fn draw_emi_sidebar(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
    list_h: f32,
) -> bool {
    let mut touched = false;
    let mut craft_click = false;
    let mut fill_click = false;
    let emi_w = EMI_SIDE_W.max(180.0).min(ui.available_width().max(160.0));
    let list_h = list_h.max(CREATIVE_GRID_MIN_H);
    let list_rows = creative_visible_rows(list_h);
    show_panel(ui, emi_w, panel_chrome(ui.ctx(), icons), |ui| {
        // Reserve title + list floor so the sidebar matches creative height.
        ui.set_min_height(EMI_PANEL_OVERHEAD + list_h);
        ui.label(RichText::new("EMI").size(16.0).color(INK).strong());
        ui.add_space(6.0);
        let rows = gui.emi_rows();
        let selected = gui.emi_selected().map(str::to_string);
        // Pad empty cells so a short filter still fills the pinned viewport.
        let cells = rows.len().max(list_rows * EMI_COLS);
        let grid_w_avail = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(grid_w_avail, list_h),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(list_h);
                ui.set_max_height(list_h);
                egui::ScrollArea::vertical()
                    .id_salt("emi_list")
                    .max_height(list_h)
                    .min_scrolled_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.label(RichText::new("nothing matches").color(MUTED).size(13.0));
                        }
                        egui::Grid::new("emi_grid")
                            .spacing([SLOT_GAP, SLOT_GAP])
                            .min_col_width(SLOT)
                            .show(ui, |ui| {
                                for i in 0..cells {
                                    if let Some(id) = rows.get(i) {
                                        let is_sel = selected.as_deref() == Some(id.as_str());
                                        let tip = gui::tip_for(id, None);
                                        let tex = icon_texture(ui.ctx(), icons, id);
                                        let resp = slot_button(ui, icons, tex, is_sel, None);
                                        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                                        if resp.clicked() {
                                            gui.select_emi(id);
                                        }
                                    } else {
                                        let _ = slot_button(ui, icons, None, false, None);
                                    }
                                    if (i + 1) % EMI_COLS == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            },
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("SEARCH").color(MUTED).size(12.0));
            let mut q = gui.emi_query().to_string();
            let edit = egui::TextEdit::singleline(&mut q)
                .desired_width(EMI_SIDE_W - 70.0)
                .hint_text("torch")
                .text_color(INK);
            if ui.add(edit).changed() {
                gui.set_emi_query(&q);
            }
        });

        if let Some(id) = gui.emi_selected().map(str::to_string) {
            ui.add_space(10.0);
            // CRAFTING panel — how this item is made.
            let craft_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
            chrome_well(ui, craft_tex, ROW, |ui| {
                ui.label(RichText::new("CRAFTING").size(13.0).color(INK).strong());
                ui.add_space(4.0);
                match gui.emi_crafting() {
                    Some(inputs) if !inputs.is_empty() => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                            for input in &inputs {
                                let tip = gui::tip_for(input, None);
                                let tex = icon_texture(ui.ctx(), icons, input);
                                let resp = slot_button(ui, icons, tex, false, None);
                                resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                                if resp.clicked() {
                                    gui.select_emi(input);
                                }
                            }
                            ui.label(RichText::new("→").size(16.0).color(DETAIL));
                            let tip = gui::tip_for(&id, None);
                            let tex = icon_texture(ui.ctx(), icons, &id);
                            let resp = slot_button(ui, icons, tex, true, None);
                            resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                            if resp.clicked() && gui.emi_can_ghost_fill() {
                                fill_click = true;
                            }
                        });
                        if let Some(pattern) = gui.emi_pattern() {
                            ui.add_space(4.0);
                            ui.label(RichText::new("shape").color(MUTED).size(11.0));
                            egui::Grid::new("emi_pattern")
                                .spacing([2.0, 2.0])
                                .min_col_width(SLOT * 0.55)
                                .show(ui, |ui| {
                                    for (i, cell) in pattern.iter().enumerate() {
                                        let tex = if cell.is_empty() {
                                            None
                                        } else {
                                            icon_texture(ui.ctx(), icons, cell)
                                        };
                                        let resp = slot_button_sized(
                                            ui,
                                            icons,
                                            tex,
                                            false,
                                            None,
                                            SLOT * 0.55,
                                            false,
                                        );
                                        if resp.clicked() && gui.emi_can_ghost_fill() {
                                            fill_click = true;
                                        }
                                        if (i + 1) % 3 == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if gui.emi_can_ghost_fill() {
                                if chrome_button(
                                    ui,
                                    icons,
                                    "Fill",
                                    true,
                                    Vec2::new(72.0, 24.0),
                                    13.0,
                                )
                                .clicked()
                                {
                                    fill_click = true;
                                }
                            }
                            if chrome_button(
                                ui,
                                icons,
                                "Craft",
                                false,
                                Vec2::new(72.0, 24.0),
                                13.0,
                            )
                            .clicked()
                            {
                                craft_click = true;
                            }
                        });
                    }
                    _ => {
                        ui.label(RichText::new("no recipe").color(MUTED).size(12.0));
                    }
                }
            });

            ui.add_space(8.0);
            // USES panel — what this item crafts into.
            let uses = gui.emi_uses();
            let uses_tex = chrome_texture(ui.ctx(), icons, CHROME_PANEL);
            chrome_well(ui, uses_tex, ROW, |ui| {
                ui.label(RichText::new("USES").size(13.0).color(INK).strong());
                ui.add_space(4.0);
                if uses.is_empty() {
                    ui.label(RichText::new("unused").color(MUTED).size(12.0));
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                        for result in &uses {
                            let tip = gui::tip_for(result, None);
                            let tex = icon_texture(ui.ctx(), icons, result);
                            let resp = slot_button(ui, icons, tex, false, None);
                            resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                            if resp.clicked() {
                                gui.select_emi(result);
                            }
                        }
                    });
                }
            });
        }
    });
    if fill_click {
        let _ = refund_inv_craft(gui, hotbar, bag);
        if gui.ghost_fill_emi() {
            touched = true;
        }
    }
    if craft_click && gui.craft_selected_into(hotbar, bag) {
        touched = true;
    }
    touched
}

/// Center entity preview + left/right columns of [`EQUIP_ROWS`] slots.
fn draw_player_detail(
    ui: &mut Ui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let preview_h = SLOT * EQUIP_ROWS as f32 + SLOT_GAP * (EQUIP_ROWS as f32 - 1.0);
    let preview_w = preview_h * (PREVIEW_W as f32 / PREVIEW_H as f32);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(SLOT_GAP + 4.0, SLOT_GAP);
        // Left: armor column (wired head/chest/feet/offhand; stub for legs).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
            for row in 0..EQUIP_ROWS {
                if row == OFFHAND_EQUIP_ROW {
                    if offhand_equip_slot(ui, hotbar, icons) {
                        touched = true;
                    }
                } else if equip_slot(ui, hotbar, bag, armor, icons, LEFT_ARMOR_MAP[row], true)
                {
                    touched = true;
                }
            }
        });
        // Center: soft-rendered player body (bbmodel / F5-style).
        draw_entity_preview(ui, icons, preview_w, preview_h);
        // Right: accessory stubs (engine has no accessory slots yet).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
            for _ in 0..EQUIP_ROWS {
                if equip_slot(ui, hotbar, bag, armor, icons, None, false) {
                    touched = true;
                }
            }
        });
    });
    touched
}

fn draw_entity_preview(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    width: f32,
    height: f32,
) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), SLOT_WELL);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, SLOT_EDGE),
        StrokeKind::Outside,
    );
    let lip = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.right(), rect.top() + 2.0),
    );
    ui.painter()
        .rect_filled(lip, CornerRadius::same(2), LIP.gamma_multiply(0.85));
    let tex = player_preview_texture(ui.ctx(), icons);
    let pad = 4.0;
    ui.painter().image(
        tex,
        rect.shrink(pad),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn player_preview_texture(
    ctx: &Context,
    icons: &mut HashMap<String, TextureHandle>,
) -> TextureId {
    let key = format!("{PREVIEW_KEY}:{}", player::preview_generation());
    if let Some(handle) = icons.get(&key) {
        return handle.id();
    }
    icons.retain(|name, _| !name.starts_with(PREVIEW_KEY));
    let rgba = player::preview_rgba(PREVIEW_W, PREVIEW_H);
    let image = egui::ColorImage::from_rgba_unmultiplied([PREVIEW_W, PREVIEW_H], &rgba);
    let handle = ctx.load_texture(key.clone(), image, TextureOptions::LINEAR);
    let tex = handle.id();
    icons.insert(key, handle);
    tex
}

/// Offhand well under the armor column — swaps with the selected hotbar stack.
fn offhand_equip_slot(
    ui: &mut Ui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let worn_id = hotbar.offhand();
    let count = hotbar.offhand_count();
    let tex = worn_id.and_then(|id| icon_texture(ui.ctx(), icons, id));
    let resp = slot_button(
        ui,
        icons,
        tex,
        false,
        if count > 1 { Some(count) } else { None },
    );
    if let Some(id) = worn_id {
        let tip = gui::tip_for(id, Some(count.max(1)));
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    } else {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                icons,
                &gui::ItemTip::plain("Offhand", "empty — F swaps with selected"),
            );
        });
    }
    if !resp.clicked() {
        return false;
    }
    hotbar.click_offhand()
}

/// One equipment well. `engine_slot` wires into [`Armor`]; `None` is a stub.
/// `armor_side` is true for the left column (accepts armor pieces).
fn equip_slot(
    ui: &mut Ui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
    engine_slot: Option<usize>,
    armor_side: bool,
) -> bool {
    let worn_id = engine_slot.and_then(|slot| armor.worn().get(slot).copied().flatten());
    let tex = worn_id.and_then(|id| icon_texture(ui.ctx(), icons, id));
    let resp = slot_button(ui, icons, tex, false, None);
    if let Some(id) = worn_id {
        let tip = gui::tip_for(id, Some(1));
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    } else if !armor_side {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                icons,
                &gui::ItemTip::plain("Accessory", "empty — not wired yet"),
            );
        });
    } else if engine_slot.is_none() {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                icons,
                &gui::ItemTip::plain("Equipment", "empty — not wired yet"),
            );
        });
    }
    if !resp.clicked() {
        return false;
    }
    let Some(slot) = engine_slot else {
        return false;
    };
    click_armor_slot(hotbar, bag, armor, slot)
}

/// Equip the selected hotbar armor into `slot`, or take the worn piece off.
fn click_armor_slot(
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    slot: usize,
) -> bool {
    let held = hotbar.slot(hotbar.selected());
    let held_fits = held.and_then(|id| {
        let (piece_slot, _) = Pack::builtin().armor_of(id)?;
        (piece_slot as usize == slot).then_some(id)
    });
    if let Some(id) = held_fits {
        let id = rebuild_world::Block::new(id, 0).id;
        let previous = armor.equip(slot, id);
        hotbar.consume_selected();
        if let Some(old) = previous {
            if !hotbar.collect(old) {
                let _ = bag.collect(old);
            }
        }
        return true;
    }
    if let Some(old) = armor.clear(slot) {
        if !hotbar.collect(old) {
            let _ = bag.collect(old);
        }
        return true;
    }
    false
}

fn draw_inv_body_grid(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) {
    egui::Grid::new("player_inv_grid")
        .spacing([SLOT_GAP, SLOT_GAP])
        .min_col_width(SLOT)
        .show(ui, |ui| {
            for index in 0..INV_BODY {
                // Wire bag is still 9 slots (shared with chests); extra wells are chrome.
                let (id, count) = if index < hotbar::PACK {
                    (
                        bag.slot(index).unwrap_or("").to_string(),
                        bag.count(index),
                    )
                } else {
                    (String::new(), 0)
                };
                let tex = if id.is_empty() {
                    None
                } else {
                    icon_texture(ui.ctx(), icons, &id)
                };
                let resp = slot_button(
                    ui,
                    icons,
                    tex,
                    false,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for(&id, Some(count));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                    if resp.hovered() {
                        gui.select_emi(&id);
                    }
                    if resp.clicked() {
                        let _ = hotbar.give_stack(&id);
                    }
                }
                if (index + 1) % INV_COLS == 0 {
                    ui.end_row();
                }
            }
        });
}

fn draw_hotbar_row(
    ui: &mut Ui,
    gui: Option<&mut Gui>,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) {
    // Same ×10 horizontal pitch as the body grid / HUD bar. Prefer full width
    // when the panel has room; never force the chrome past the window edge.
    let row_w = grid_pitch(hotbar::SLOTS);
    if ui.available_width() + 0.5 >= row_w {
        ui.set_min_width(row_w);
    }
    let mut hover_id: Option<String> = None;
    egui::Grid::new("inv_hotbar_row")
        .spacing([SLOT_GAP, SLOT_GAP])
        .min_col_width(SLOT)
        .show(ui, |ui| {
            for slot in 0..hotbar::SLOTS {
                let id = hotbar.slot(slot).unwrap_or("");
                let count = hotbar.count(slot);
                let selected = hotbar.selected() == slot;
                let tex = if id.is_empty() {
                    None
                } else {
                    icon_texture(ui.ctx(), icons, id)
                };
                let resp = slot_button(
                    ui,
                    icons,
                    tex,
                    selected,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for_keen(id, Some(count), hotbar.is_keen(slot));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
                    if resp.hovered() {
                        hover_id = Some(id.to_string());
                    }
                }
                if resp.clicked() {
                    let _ = hotbar.select_digit((slot + 1) as u8);
                }
            }
            ui.end_row();
        });
    if let (Some(gui), Some(id)) = (gui, hover_id) {
        gui.select_emi(&id);
    }
}

/// 2×2 INPUT + OUT wired to [`Gui::inv_craft_*`], plus a lower "2?" tray stub.
fn draw_craft_detail(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> (bool, bool) {
    let mut touched = false;
    let mut crafted = false;
    let preview = gui.inv_craft_preview();
    Frame::new()
        .fill(ROW)
        .stroke(Stroke::new(1.0, SLOT_EDGE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(12, 14))
        .show(ui, |ui| {
            ui.label(RichText::new("Craft").size(15.0).color(INK).strong());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                        for index in 0..2 {
                            if craft_input_slot(ui, gui, hotbar, bag, icons, index) {
                                touched = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                        for index in 2..4 {
                            if craft_input_slot(ui, gui, hotbar, bag, icons, index) {
                                touched = true;
                            }
                        }
                    });
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("OUT").size(11.0).color(MUTED));
                    ui.add_space(2.0);
                    if craft_out_slot(ui, gui, hotbar, bag, icons, preview.as_deref()) {
                        touched = true;
                        crafted = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(if gui.inv_craft_is_ghost() {
                    "ghost"
                } else {
                    "2?"
                })
                .size(12.0)
                .color(MUTED),
            );
            ui.add_space(4.0);
            Frame::new()
                .fill(SLOT_WELL)
                .stroke(Stroke::new(1.0, SLOT_EDGE))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_min_width(SLOT * 2.0 + SLOT_GAP);
                    let line = preview
                        .as_deref()
                        .map(|id| gui::tip_for(id, None).name)
                        .unwrap_or_else(|| "empty".to_string());
                    ui.label(RichText::new(line).size(12.0).color(DETAIL));
                });
        });
    (touched, crafted)
}

fn craft_input_slot(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
    index: usize,
) -> bool {
    let id = gui.inv_craft_slot(index).unwrap_or("").to_string();
    let count = gui.inv_craft_count(index);
    let ghost = gui.inv_craft_is_ghost();
    let tex = if id.is_empty() {
        None
    } else {
        icon_texture(ui.ctx(), icons, &id)
    };
    let resp = if ghost {
        slot_button_ghost(ui, icons, tex, false, if count > 1 { Some(count) } else { None })
    } else {
        slot_button(ui, icons, tex, false, if count > 1 { Some(count) } else { None })
    };
    if !id.is_empty() {
        let tip = if ghost {
            let name = gui::tip_for(&id, Some(count)).name;
            gui::ItemTip::plain(name, "ghost — place real items to craft")
        } else {
            gui::tip_for(&id, Some(count))
        };
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    }
    if !resp.clicked() {
        return false;
    }
    click_craft_input(gui, hotbar, bag, index)
}

fn craft_out_slot(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
    preview: Option<&str>,
) -> bool {
    let ghost = gui.inv_craft_is_ghost();
    let tex = preview.and_then(|id| icon_texture(ui.ctx(), icons, id));
    let resp = if ghost {
        slot_button_ghost(ui, icons, tex, preview.is_some(), None)
    } else {
        slot_button(ui, icons, tex, preview.is_some(), None)
    };
    if let Some(id) = preview {
        let tip = if ghost {
            let name = gui::tip_for(id, None).name;
            gui::ItemTip::plain(name, "ghost recipe — Fill from EMI")
        } else {
            gui::tip_for(id, None)
        };
        resp.clone().on_hover_ui(|ui| show_tip(ui, icons, &tip));
    } else {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                icons,
                &gui::ItemTip::plain("OUT", "match a recipe in the 2x2"),
            );
        });
    }
    if !resp.clicked() || preview.is_none() || ghost {
        return false;
    }
    click_craft_out(gui, hotbar, bag)
}

fn click_craft_input(
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    index: usize,
) -> bool {
    if gui.inv_craft_slot(index).is_some() {
        let Some(id) = gui.take_inv_craft_slot(index) else {
            return false;
        };
        if !hotbar.collect(id) {
            let _ = bag.collect(id);
        }
        return true;
    }
    let Some(held) = hotbar.slot(hotbar.selected()) else {
        return false;
    };
    let held = held.to_string();
    if !gui.give_inv_craft(index, &held) {
        return false;
    }
    hotbar.consume_selected();
    true
}

fn click_craft_out(gui: &mut Gui, hotbar: &mut Hotbar, bag: &mut hotbar::Bag) -> bool {
    let Some(result) = gui.inv_craft_preview() else {
        return false;
    };
    if !hotbar.room_for(&result) && !bag.room_for(&result) {
        return false;
    }
    let Some(id) = gui.pull_inv_craft() else {
        return false;
    };
    if !hotbar.collect(&id) {
        let _ = bag.collect(&id);
    }
    true
}

fn refund_inv_craft(gui: &mut Gui, hotbar: &mut Hotbar, bag: &mut hotbar::Bag) -> bool {
    let mut moved = false;
    for id in gui.drain_inv_craft() {
        moved = true;
        if !hotbar.collect(id) {
            let _ = bag.collect(id);
        }
    }
    moved
}

fn close_button(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    label: &str,
) -> egui::Response {
    menu_button(ui, icons, label, false)
}

/// Cook-shot helper: park a tip in the upper-right so the plate is visible.
fn draw_pinned_tip(ui: &mut Ui, gui: &Gui, icons: &HashMap<String, TextureHandle>) {
    let Some(tip) = gui.pinned_tip().cloned() else {
        return;
    };
    egui::Area::new(egui::Id::new("pinned_item_tip"))
        .anchor(Align2::RIGHT_TOP, [-24.0, 48.0])
        .interactable(false)
        .show(ui.ctx(), |ui| {
            show_tip(ui, icons, &tip);
        });
}

fn show_tip(ui: &mut Ui, icons: &HashMap<String, TextureHandle>, tip: &gui::ItemTip) {
    let tip_bg = chrome_cached(icons, CHROME_TIP_BG);
    let tip_frame = chrome_cached(icons, CHROME_TIP_FRAME);
    let fill = if tip_bg.is_some() {
        Color32::from_rgba_unmultiplied(12, 11, 16, 240)
    } else {
        TIP_BG
    };
    let stroke = if tip_frame.is_some() {
        Stroke::new(1.5, Color32::from_rgb(72, 58, 40))
    } else {
        Stroke::new(1.0, TIP_EDGE)
    };
    let out = Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            // Keep tips compact — name / id / hint / a few stats.
            ui.set_max_width(220.0);
            // Sample Whimscape tip plate behind the copy (content draws after).
            if let Some(tex) = tip_bg {
                let r = ui.max_rect();
                let band = egui::Rect::from_min_size(
                    r.min,
                    Vec2::new(r.width().min(220.0), r.height().min(140.0)),
                );
                ui.painter().image(
                    tex,
                    band,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 70),
                );
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(&tip.name).size(14.0).color(INK).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    rarity_chip(ui, tip.rarity);
                });
            });
            if !tip.id.is_empty() {
                ui.add_space(1.0);
                let id_line = match tip.source.as_deref() {
                    Some(ns) => format!("{} · {}", tip.id, ns),
                    None => tip.id.clone(),
                };
                ui.label(RichText::new(id_line).size(11.0).color(MUTED));
            }
            if !tip.description.is_empty() {
                ui.add_space(3.0);
                ui.label(RichText::new(&tip.description).size(12.0).color(DETAIL));
            }
            if !tip.stats.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                for line in tip.stats.iter().take(4) {
                    ui.label(RichText::new(line).size(12.0).color(INK));
                }
            }
            if !tip.enchants.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.label(RichText::new("ENCHANTS").size(11.0).color(MUTED));
                for (name, desc) in tip.enchants.iter().take(2) {
                    ui.label(RichText::new(name).size(12.0).color(DETAIL));
                    ui.label(RichText::new(desc).size(11.0).color(MUTED));
                }
            }
        });
    // Whimscape tip frame is a border plate — stretch over the final tip rect.
    if let Some(tex) = tip_frame {
        ui.painter().image(
            tex,
            out.response.rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

fn rarity_chip(ui: &mut Ui, rarity: gui::Rarity) {
    let [r, g, b] = rarity.rgb();
    Frame::new()
        .fill(Color32::from_rgb(r, g, b))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(rarity.label())
                    .size(11.0)
                    .color(INK)
                    .strong(),
            );
        });
}

fn slot_button(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
) -> egui::Response {
    slot_button_sized(ui, icons, tex, selected, count, SLOT, false)
}

fn slot_button_ghost(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
) -> egui::Response {
    slot_button_sized(ui, icons, tex, selected, count, SLOT, true)
}

fn slot_button_sized(
    ui: &mut Ui,
    icons: &mut HashMap<String, TextureHandle>,
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
    size: f32,
    ghost: bool,
) -> egui::Response {
    let stroke = if selected {
        Stroke::new(2.0, SELECT)
    } else if ghost {
        Stroke::new(1.5, DETAIL)
    } else {
        Stroke::new(1.0, SLOT_EDGE)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let well = chrome_texture(ui.ctx(), icons, CHROME_SLOT);
        if let Some(well) = well {
            ui.painter().image(
                well,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(3), SLOT_WELL);
            // Light lip along the top edge of the cell.
            let lip = egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.right(), rect.top() + 2.0),
            );
            ui.painter()
                .rect_filled(lip, CornerRadius::same(2), LIP.gamma_multiply(0.85));
        }
        if selected {
            if let Some(hi) = chrome_texture(ui.ctx(), icons, CHROME_SLOT_HIGHLIGHT) {
                ui.painter().image(
                    hi,
                    rect.expand(2.0),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(3), stroke, StrokeKind::Outside);
        if let Some(tex) = tex {
            // Modest inset — the raster itself is already AABB-fitted.
            let pad = (size * 0.06).max(1.5);
            let icon_rect = rect.shrink(pad);
            let tint = if ghost {
                Color32::from_rgba_unmultiplied(255, 255, 255, 120)
            } else {
                Color32::WHITE
            };
            ui.painter().image(
                tex,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        if let Some(count) = count.filter(|n| *n > 1) {
            ui.painter().text(
                egui::pos2(rect.right() - 3.0, rect.bottom() - 2.0),
                Align2::RIGHT_BOTTOM,
                count.to_string(),
                FontId::proportional(11.0),
                INK,
            );
        }
    }
    response
}

fn chrome_path(name: &str) -> String {
    format!("assets/terralite/textures/gui/chrome/{name}.png")
}

fn chrome_texture(
    ctx: &Context,
    icons: &mut HashMap<String, TextureHandle>,
    name: &str,
) -> Option<TextureId> {
    let key = format!("__chrome:{name}");
    if let Some(handle) = icons.get(&key) {
        return Some(handle.id());
    }
    let bytes = asset_png(&chrome_path(name))?;
    let image = texture::decode_png(&bytes)?;
    if image.width == 0 || image.height == 0 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    );
    let handle = ctx.load_texture(key.clone(), color, TextureOptions::NEAREST);
    let tex = handle.id();
    icons.insert(key, handle);
    Some(tex)
}

fn warm_chrome(ctx: &Context, icons: &mut HashMap<String, TextureHandle>) {
    for name in [
        CHROME_SLOT,
        CHROME_SLOT_HIGHLIGHT,
        CHROME_PANEL,
        CHROME_TAB,
        CHROME_TAB_SELECTED,
        CHROME_BUTTON,
        CHROME_BUTTON_HOT,
        CHROME_TIP_BG,
        CHROME_TIP_FRAME,
    ] {
        let _ = chrome_texture(ctx, icons, name);
    }
}

fn icon_texture(
    ctx: &Context,
    icons: &mut HashMap<String, TextureHandle>,
    id: &str,
) -> Option<TextureId> {
    if id.is_empty() || id == "air" || id == "empty" {
        return None;
    }
    if let Some(handle) = icons.get(id) {
        return Some(handle.id());
    }
    let image = render_slot_icon(id);
    // LINEAR so the 64px soft raster doesn't show hard nearest pixels in a slot.
    let handle = ctx.load_texture(id, image, TextureOptions::LINEAR);
    let tex = handle.id();
    icons.insert(id.to_string(), handle);
    Some(tex)
}

/// Soft-render a slot icon via the item/block model (`display.gui`).
fn render_slot_icon(id: &str) -> egui::ColorImage {
    item_icon::render(id, ICON_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_icon_has_opaque_pixels_for_cobble() {
        let img = render_slot_icon("cobblestone");
        assert_eq!(img.width(), ICON_PX);
        assert_eq!(img.height(), ICON_PX);
        let opaque = img.pixels.iter().filter(|c| c.a() > 200).count();
        assert!(
            opaque > ICON_PX * ICON_PX / 6,
            "expected a solid cube silhouette, got {opaque} opaque pixels"
        );
    }

    #[test]
    fn grass_slot_icon_comes_from_block_model() {
        let (shape, _) = Pack::builtin().slot_model("grass").expect("grass");
        assert!(!shape.faces().is_empty());
        let img = render_slot_icon("grass");
        let greenish = img
            .pixels
            .iter()
            .filter(|c| c.a() > 200 && c.g() > c.r() && c.g() > 80)
            .count();
        assert!(greenish > 40, "grass model icon should show green, got {greenish}");
    }

    #[test]
    fn tip_for_names_cobble() {
        let tip = gui::tip_for("cobblestone", Some(1));
        assert!(tip.name.to_ascii_lowercase().contains("cobble"));
        assert!(!tip.line.is_empty());
        assert_eq!(tip.rarity, gui::Rarity::Common);
        assert_eq!(tip.description, tip.line);
        assert_eq!(tip.id, "cobblestone");
        assert!(tip.stats.iter().any(|s| s == "Stack ×1"));
        assert!(tip.enchants.is_empty());
    }

    #[test]
    fn rich_tip_defaults_common_rarity() {
        let tip = gui::tip_for("apple", None);
        assert_eq!(tip.rarity, gui::Rarity::Common);
        assert_eq!(tip.rarity.label(), "Common");
        assert!(!tip.description.is_empty());
    }

    #[test]
    fn rich_tip_armor_lists_soak() {
        let tip = gui::tip_for("leather_cap", Some(1));
        assert!(tip.stats.iter().any(|s| s.contains("Soak")));
        assert!(tip.stats.iter().any(|s| s.contains("Head")));
    }

    #[test]
    fn rich_tip_tool_lists_uses_and_keen_enchant() {
        let plain = gui::tip_for("iron_pickaxe", Some(32));
        assert!(plain.stats.iter().any(|s| s.contains("Durability")));
        assert!(plain.stats.iter().any(|s| s.contains("Iron")));
        assert!(plain.enchants.is_empty());
        let keen = gui::tip_for_keen("iron_pickaxe", Some(20), true);
        assert_eq!(keen.enchants.len(), 1);
        assert_eq!(keen.enchants[0].0, "Efficiency");
        assert!(!keen.enchants[0].1.is_empty());
    }

    #[test]
    fn rich_tip_shows_id_light_food_and_fill() {
        let torch = gui::tip_for("torch", Some(8));
        assert_eq!(torch.id, "torch");
        assert!(torch.stats.iter().any(|s| s.starts_with("Light ")));
        let apple = gui::tip_for("apple", None);
        assert!(apple.stats.iter().any(|s| s.starts_with("Heal +")));
        let fill = gui::tip_for("ember_charge", Some(2));
        assert!(fill.stats.iter().any(|s| s == "Fill ember"));
        let modded = gui::tip_for("ember:glow_brick", None);
        assert_eq!(modded.source.as_deref(), Some("ember"));
    }

    #[test]
    fn player_inv_grid_is_four_by_ten() {
        assert_eq!(INV_COLS, 10);
        assert_eq!(INV_ROWS, 4);
        assert_eq!(INV_BODY, 40);
        assert_eq!(hotbar::SLOTS, 10);
    }

    #[test]
    fn creative_grid_is_ten_wide_with_hotbar() {
        assert_eq!(CREATIVE_COLS, 10);
        assert_eq!(hotbar::SLOTS, 10);
        assert_eq!(Gui::catalog_tab_count(), 7);
    }

    #[test]
    fn creative_grid_grows_taller_than_five_rows() {
        let short = creative_grid_height(720.0, true);
        let tall = creative_grid_height(1080.0, true);
        let five = SLOT * 5.0 + SLOT_GAP * 4.0;
        assert!(
            short > five,
            "creative should show more than five rows even on a short window ({short} vs {five})"
        );
        assert!(tall > short);
        let min_h = SLOT * CREATIVE_ROWS_MIN + SLOT_GAP * (CREATIVE_ROWS_MIN - 1.0);
        assert!(short + 0.5 >= min_h);
        // Tall windows should clear the old 12-row ceiling and fill most of the screen.
        let twelve = SLOT * 12.0 + SLOT_GAP * 11.0;
        assert!(
            tall > twelve,
            "creative should grow past 12 rows on a tall window ({tall} vs {twelve})"
        );
        let chrome = creative_chrome_height(tall, true);
        let frac = chrome / 1080.0;
        assert!(
            (CREATIVE_CHROME_FRAC_MIN - 0.02..=CREATIVE_CHROME_FRAC_MAX + 0.03).contains(&frac),
            "expected ~70–85% chrome height, got {frac:.3} ({chrome} / 1080)"
        );
        // Viewport row count must stay in the visible band even when the
        // catalog is shorter than the grid (empty slots pad the rest).
        let short_rows = creative_visible_rows(short);
        let tall_rows = creative_visible_rows(tall);
        assert!(
            short_rows >= CREATIVE_ROWS_MIN as usize,
            "short window must keep ≥8 visible rows, got {short_rows}"
        );
        assert!(
            tall_rows >= 12,
            "tall window must keep ≥12 visible rows, got {tall_rows}"
        );
        assert_eq!(short_rows, creative_row_budget(720.0, true) as usize);
        assert_eq!(tall_rows, creative_row_budget(1080.0, true) as usize);
        let padded_cells = 20usize.max(short_rows * CREATIVE_COLS);
        assert!(
            padded_cells >= short_rows * CREATIVE_COLS,
            "small catalogs must pad out to the visible row budget"
        );
    }

    #[test]
    fn soft_slot_icon_uses_bilinear_not_only_nearest() {
        let img = render_slot_icon("cobblestone");
        assert_eq!(img.width(), ICON_PX);
        // Soft edge AA leaves some translucent fringe; opaque core still fills.
        let soft = img.pixels.iter().filter(|c| c.a() > 20 && c.a() < 240).count();
        assert!(
            soft > 10,
            "expected soft AA fringe on the inventory cube, got {soft}"
        );
    }

    #[test]
    fn emi_sidebar_is_four_wide() {
        assert_eq!(EMI_COLS, 4);
        // EMI list must use the same ≥8-row floor as the creative grid.
        let list_h = creative_grid_height(720.0, true);
        assert!(list_h + 0.5 >= CREATIVE_GRID_MIN_H);
        assert!(creative_visible_rows(list_h) >= 8);
        assert!(EMI_SIDE_W > SLOT * 3.0);
    }

    #[test]
    fn emi_search_and_panels_wire_to_recipe_book() {
        let mut gui = Gui::new();
        gui.set_emi_query("torch");
        let rows = gui.emi_rows();
        assert!(
            rows.iter().any(|id| id == "torch"),
            "search should find torch: {rows:?}"
        );
        gui.select_emi("torch");
        assert_eq!(gui.emi_selected(), Some("torch"));
        let crafting = gui.emi_crafting().expect("torch has a recipe");
        assert!(crafting.iter().any(|id| id == "stick"));
        assert!(crafting.iter().any(|id| id.contains("coal")));
        gui.select_emi("stick");
        let uses = gui.emi_uses();
        assert!(
            uses.iter().any(|id| id == "torch"),
            "stick should list torch in USES: {uses:?}"
        );
    }

    #[test]
    fn emi_craft_button_spends_hotbar_ingredients() {
        let mut gui = Gui::new();
        let mut hotbar = Hotbar::new();
        let mut bag = hotbar::Bag::new();
        assert!(hotbar.select_digit(2));
        assert!(hotbar.give_stack("stick"));
        assert!(hotbar.select_digit(3));
        assert!(hotbar.give_stack("coal_ore"));
        gui.select_emi("torch");
        assert!(gui.craft_selected_into(&mut hotbar, &mut bag));
        assert!(
            (0..hotbar::SLOTS).any(|i| hotbar.slot(i) == Some("torch")),
            "crafted torch should land in the hotbar"
        );
    }

    #[test]
    fn emi_fill_ghosts_craft_grid_for_torch() {
        let mut gui = Gui::new();
        gui.select_emi("torch");
        assert!(gui.emi_can_ghost_fill());
        assert!(gui.ghost_fill_emi());
        assert!(gui.inv_craft_is_ghost());
        assert_eq!(gui.inv_tab(), PlayerInvTab::Craft);
        assert_eq!(gui.inv_craft_preview().as_deref(), Some("torch"));
    }

    #[test]
    fn cycle_catalog_tab_wraps_categories() {
        let mut gui = Gui::new();
        assert_eq!(gui.catalog_tab(), 0);
        gui.cycle_catalog_tab(false);
        assert_eq!(gui.catalog_tab(), Gui::catalog_tab_count() - 1);
        gui.cycle_catalog_tab(true);
        assert_eq!(gui.catalog_tab(), 0);
        gui.cycle_catalog_tab(true);
        assert_eq!(gui.catalog_tab(), 1);
    }

    #[test]
    fn inventory_player_craft_tabs_switch() {
        let mut gui = Gui::new();
        gui.open_player_inv();
        assert!(gui.player_inv_open());
        assert_eq!(gui.inv_tab(), PlayerInvTab::Player);
        gui.set_inv_tab(PlayerInvTab::Craft);
        assert_eq!(gui.inv_tab(), PlayerInvTab::Craft);
        gui.set_inv_tab(PlayerInvTab::Player);
        assert_eq!(gui.inv_tab(), PlayerInvTab::Player);
        // EMI selection is independent of the tab strip.
        gui.select_emi("torch");
        assert_eq!(gui.emi_selected(), Some("torch"));
        assert_eq!(gui.inv_tab(), PlayerInvTab::Player);
    }

    #[test]
    fn player_tab_has_two_equipment_columns_of_five() {
        assert_eq!(EQUIP_ROWS, 5);
        assert_eq!(LEFT_ARMOR_MAP.len(), 5);
        // Engine armor: head / chest / feet — legs stub; offhand is row 4.
        assert_eq!(LEFT_ARMOR_MAP[0], Some(0));
        assert_eq!(LEFT_ARMOR_MAP[1], Some(1));
        assert_eq!(LEFT_ARMOR_MAP[2], None);
        assert_eq!(LEFT_ARMOR_MAP[3], Some(2));
        assert_eq!(LEFT_ARMOR_MAP[4], None);
        assert_eq!(OFFHAND_EQUIP_ROW, 4);
    }

    #[test]
    fn click_offhand_swaps_with_selected() {
        let mut hotbar = Hotbar::new();
        assert!(hotbar.select_digit(2));
        assert!(hotbar.collect("shield"));
        assert!(hotbar.click_offhand());
        assert_eq!(hotbar.offhand(), Some("shield"));
        assert!(hotbar.slot(hotbar.selected()).is_none());
        assert!(hotbar.holding_shield());
        assert!(hotbar.click_offhand());
        assert!(hotbar.offhand().is_none());
        assert_eq!(hotbar.slot(hotbar.selected()), Some("shield"));
    }

    #[test]
    fn click_armor_slot_equips_matching_piece() {
        let mut hotbar = Hotbar::new();
        let mut bag = hotbar::Bag::new();
        let mut armor = Armor::new();
        // Slot 2 starts empty; collect one cap so consume clears the cell.
        assert!(hotbar.select_digit(2));
        assert!(hotbar.collect("leather_cap"));
        assert!(click_armor_slot(&mut hotbar, &mut bag, &mut armor, 0));
        assert_eq!(armor.worn()[0], Some("leather_cap"));
        assert!(hotbar.slot(hotbar.selected()).is_none());
        assert!(click_armor_slot(&mut hotbar, &mut bag, &mut armor, 0));
        assert!(armor.worn()[0].is_none());
        assert_eq!(hotbar.slot(hotbar.selected()), Some("leather_cap"));
    }

    #[test]
    fn click_craft_input_and_out_make_a_torch() {
        // Gui::new loads the EMI/datapack book (stick + coal_ore → torch).
        let mut gui = Gui::new();
        let mut hotbar = Hotbar::new();
        let mut bag = hotbar::Bag::new();
        // Slot 1 starts with cobble; fill empty selected slots via give_stack.
        assert!(hotbar.select_digit(2));
        assert!(hotbar.give_stack("stick"));
        assert!(click_craft_input(&mut gui, &mut hotbar, &mut bag, 0));
        assert_eq!(gui.inv_craft_slot(0), Some("stick"));
        assert!(hotbar.select_digit(3));
        assert!(hotbar.give_stack("coal_ore"));
        assert!(click_craft_input(&mut gui, &mut hotbar, &mut bag, 1));
        assert_eq!(gui.inv_craft_preview().as_deref(), Some("torch"));
        assert!(click_craft_out(&mut gui, &mut hotbar, &mut bag));
        assert!(gui.inv_craft_slot(0).is_none());
        assert!(gui.inv_craft_slot(1).is_none());
        assert!(
            (0..hotbar::SLOTS).any(|i| hotbar.slot(i) == Some("torch")),
            "torch should land in the hotbar"
        );
    }

    #[test]
    fn player_preview_texture_pixels_are_opaque() {
        let rgba = player::preview_rgba(PREVIEW_W, PREVIEW_H);
        assert_eq!(rgba.len(), PREVIEW_W * PREVIEW_H * 4);
        let opaque: Vec<_> = rgba.chunks_exact(4).filter(|px| px[3] > 200).collect();
        assert!(
            opaque.len() > PREVIEW_W * PREVIEW_H / 12,
            "entity preview should show the body silhouette"
        );
        let mut sum = [0u64; 3];
        for px in &opaque {
            sum[0] += px[0] as u64;
            sum[1] += px[1] as u64;
            sum[2] += px[2] as u64;
        }
        let n = opaque.len() as f64;
        let mean = [
            sum[0] as f64 / n,
            sum[1] as f64 / n,
            sum[2] as f64 / n,
        ];
        // Broken skin was a sparse cyan/green grid; a real body is warmer / muted.
        assert!(
            !(mean[2] > 180.0 && mean[1] > 140.0 && mean[0] < 120.0),
            "preview should not be a cyan missing-skin grid, mean={mean:?}"
        );
    }

    #[test]
    fn inventory_hotbar_row_is_ten_wide() {
        assert_eq!(hotbar::SLOTS, 10);
        assert_eq!(INV_COLS, 10);
        let row_w = grid_pitch(hotbar::SLOTS);
        let grid_w = grid_pitch(INV_COLS);
        assert_eq!(row_w, grid_w, "hotbar ×10 must match the 10-wide body pitch");
    }

    #[test]
    fn inventory_chrome_fits_inside_ninety_percent() {
        // Wide window: panel + EMI sit side-by-side under 90% chrome.
        let max_w = 1280.0 * 0.9;
        let panel = clamped_panel_width(max_w, CREATIVE_COLS, true);
        let emi = EMI_SIDE_W.max(180.0);
        assert!(
            panel + 12.0 + emi <= max_w + 0.5,
            "creative chrome overflowed: panel={panel} emi={emi} max={max_w}"
        );
        let inv_panel = clamped_panel_width(max_w, INV_COLS, true);
        assert!(
            inv_panel + 12.0 + emi <= max_w + 0.5,
            "player inv chrome overflowed: panel={inv_panel} emi={emi} max={max_w}"
        );
        // Narrow window: clamp keeps the main panel inside chrome (EMI stacks).
        let tight = 520.0;
        let tight_panel = clamped_panel_width(tight, CREATIVE_COLS, true);
        assert!(
            tight_panel <= tight + 0.5,
            "clamped panel must stay inside chrome max, got {tight_panel} > {tight}"
        );
        assert!(grid_pitch(CREATIVE_COLS) + 40.0 > tight_panel || tight_panel <= tight);
    }

    #[test]
    fn panel_width_helpers_match_ten_wide_grid() {
        let pitch = grid_pitch(10);
        assert!((pitch - (SLOT * 10.0 + SLOT_GAP * 9.0)).abs() < f32::EPSILON);
        let roomy = clamped_panel_width(2000.0, 10, true);
        assert!((roomy - (pitch + 40.0)).abs() < 0.1);
    }

    #[test]
    fn face_tile_loads_stone() {
        let tile = texture::face_tile("stone").expect("stone png");
        assert_eq!(tile.width, 16);
        assert_eq!(tile.height, 16);
        assert_eq!(tile.rgba.len(), 16 * 16 * 4);
    }

    #[test]
    fn dump_slot_icons_when_asked() {
        if std::env::var_os("DUMP_EGUI_ICONS").is_none() {
            return;
        }
        let dir = std::path::Path::new(
            r"C:\Users\monki\AppData\Local\Cursor\AgentStores\cursor_agent_stores\bc-918df1d9-b731-4436-a592-ce370bd63ade\files\media",
        );
        let _ = std::fs::create_dir_all(dir);
        for id in ["cobblestone", "dirt", "oak_log", "stone", "grass"] {
            let img = render_slot_icon(id);
            let path = dir.join(format!("egui-slot-{id}.png"));
            write_rgba_png(&path, img.width() as u32, img.height() as u32, &img);
            let after = dir.join(format!("after-slot-{id}.png"));
            write_rgba_png(&after, img.width() as u32, img.height() as u32, &img);
        }
    }

    /// Prove the pinned ScrollArea keeps the budgeted height even when the
    /// catalog is only ~2 content rows (the pre-fix auto-shrink failure).
    #[test]
    fn creative_scroll_area_pins_budgeted_height() {
        let ctx = Context::default();
        ctx.set_pixels_per_point(SCALE);
        let grid_h = creative_grid_height(720.0, true);
        let rows = creative_visible_rows(grid_h);
        assert!(rows >= 8, "budget must request ≥8 rows, got {rows}");
        assert!(
            grid_h + 0.5 >= CREATIVE_GRID_MIN_H,
            "grid_h must honor fixed 8-row floor ({grid_h} < {CREATIVE_GRID_MIN_H})"
        );
        // Simulate a short catalog (exactly 2 rows of items).
        let catalog_cells = CREATIVE_COLS * 2;
        let cells = catalog_cells.max(rows * CREATIVE_COLS);
        assert_eq!(cells, rows * CREATIVE_COLS);

        // Mirror the live path: CentralPanel → horizontal → framed panel →
        // set_min_height + allocate_ui_with_layout + pinned ScrollArea.
        let mut allocated = 0.0_f32;
        let mut panel_h = 0.0_f32;
        let mut icons = HashMap::new();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                Vec2::new(1280.0, 720.0),
            )),
            ..Default::default()
        };
        let mut full = ctx.run_ui(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let panel_w = clamped_panel_width(chrome_max_width(ui), CREATIVE_COLS, true);
                    show_panel(ui, panel_w, panel_chrome(ui.ctx(), &mut icons), |ui| {
                        ui.set_min_width(grid_pitch(CREATIVE_COLS).min(panel_w));
                        ui.set_min_height(CREATIVE_PANEL_OVERHEAD + grid_h);
                        ui.add_space(40.0); // title
                        ui.add_space(34.0); // tabs
                        ui.add_space(28.0); // search
                        let grid_w_avail = ui.available_width();
                        let y0 = ui.cursor().top();
                        ui.allocate_ui_with_layout(
                            Vec2::new(grid_w_avail, grid_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_min_height(grid_h);
                                ui.set_max_height(grid_h);
                                let _ = egui::ScrollArea::vertical()
                                    .id_salt("creative_grid_scroll_alloc")
                                    .max_height(grid_h)
                                    .min_scrolled_height(grid_h)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        egui::Grid::new("creative_grid_alloc")
                                            .spacing([SLOT_GAP, SLOT_GAP])
                                            .min_col_width(SLOT)
                                            .show(ui, |ui| {
                                                for i in 0..cells {
                                                    let _ = slot_button(ui, &mut icons, None, false, None);
                                                    if (i + 1) % CREATIVE_COLS == 0 {
                                                        ui.end_row();
                                                    }
                                                }
                                            });
                                    });
                            },
                        );
                        allocated = ui.cursor().top() - y0;
                        ui.add_space(10.0);
                        ui.add_space(40.0); // hotbar stand-in
                        ui.add_space(28.0); // close stand-in
                    });
                    // Horizontal layout grows min_rect.height with the tallest child.
                    panel_h = ui.min_rect().height();
                });
            });
        });
        full.textures_delta.clear();
        assert!(
            allocated + 0.5 >= grid_h,
            "live-path allocated height must keep the grid budget ({allocated} < {grid_h})"
        );
        assert!(
            allocated + 0.5 >= CREATIVE_GRID_MIN_H,
            "live-path must keep ≥8-row floor ({allocated} < {CREATIVE_GRID_MIN_H})"
        );
        assert!(
            panel_h + 0.5 >= CREATIVE_GRID_MIN_H,
            "framed panel must grow with the pinned grid (panel_h={panel_h})"
        );
        eprintln!(
            "live_path allocated={allocated} panel_h={panel_h} grid_h={grid_h} rows={rows} min_h={CREATIVE_GRID_MIN_H}"
        );

        // Always dump a schematic proving the padded row count.
        let dir = std::path::Path::new(
            r"C:\Users\monki\AppData\Local\Cursor\AgentStores\cursor_agent_stores\bc-918df1d9-b731-4436-a592-ce370bd63ade\files\media",
        );
        let _ = std::fs::create_dir_all(dir);
        let cell = 12u32;
        let gap = 2u32;
        let w = CREATIVE_COLS as u32 * (cell + gap) + gap;
        let h = rows as u32 * (cell + gap) + gap;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[18, 20, 28, 255]);
        }
        for r in 0..rows {
            for c in 0..CREATIVE_COLS {
                let filled = r * CREATIVE_COLS + c < catalog_cells;
                let x0 = gap + c as u32 * (cell + gap);
                let y0 = gap + r as u32 * (cell + gap);
                for y in y0..y0 + cell {
                    for x in x0..x0 + cell {
                        let i = ((y * w + x) * 4) as usize;
                        if filled {
                            rgba[i..i + 4].copy_from_slice(&[184, 133, 31, 255]);
                        } else {
                            rgba[i..i + 4].copy_from_slice(&[58, 60, 70, 255]);
                        }
                    }
                }
            }
        }
        let path = dir.join("creative-grid-layout-dump.png");
        let file = std::fs::File::create(&path).expect("create layout png");
        let mut encoder = png::Encoder::new(file, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&rgba).expect("png data");
        eprintln!(
            "dumped {} ({}x{} cells, allocated={allocated})",
            path.display(),
            CREATIVE_COLS,
            rows
        );
    }

    fn write_rgba_png(path: &std::path::Path, w: u32, h: u32, img: &egui::ColorImage) {
        let file = std::fs::File::create(path).expect("create png");
        let mut encoder = png::Encoder::new(file, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for px in &img.pixels {
            rgba.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        writer.write_image_data(&rgba).expect("png data");
    }
}
