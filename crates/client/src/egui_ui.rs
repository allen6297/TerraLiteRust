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
use rebuild_world::Pack;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::body::Armor;
use crate::gui::{self, Gui, Menu, MenuAction, PlayerInvTab};
use crate::hotbar::{self, Hotbar};
use crate::palette;
use crate::player;
use crate::texture::{self, Image};

/// Overall egui zoom — matches the old `GUI_SCALE` target.
pub const SCALE: f32 = 1.5;

/// Square inventory / creative cell in egui points (before [`SCALE`]).
const SLOT: f32 = 36.0;
const SLOT_GAP: f32 = 5.0;
const ICON_PX: usize = 48;
/// Creative catalog grid width (GUI plan: 10-wide).
const CREATIVE_COLS: usize = 10;
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

/// Left-column UI row → engine armor slot (`None` = stub).
/// Order is top→bottom: head, chest, legs(stub), feet, offhand(stub).
const LEFT_ARMOR_MAP: [Option<usize>; EQUIP_ROWS] = [Some(0), Some(1), None, Some(2), None];

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
const TIP_BG: Color32 = Color32::from_rgb(18, 20, 28);
const TIP_EDGE: Color32 = Color32::from_rgb(237, 199, 107);
const DETAIL: Color32 = Color32::from_rgb(255, 235, 140);

pub struct EguiHost {
    ctx: Context,
    winit: WinitState,
    renderer: Renderer,
    /// Pending menu action from the last `run` (Play, Quit, …).
    pending: MenuAction,
    /// True when the last `run` mutated hotbar / bag / armor.
    touched: bool,
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

    /// True when egui should own pointer/keyboard instead of the world.
    pub fn wants_input(menu: &Menu, gui: &Gui) -> bool {
        menu.is_open() || gui.bag_open() || gui.player_inv_open()
    }

    /// Build menus for this frame. Returns tessellated output ready to paint.
    pub fn run(
        &mut self,
        window: &Window,
        menu: &mut Menu,
        gui: &mut Gui,
        hotbar: &mut Hotbar,
        bag: &mut hotbar::Bag,
        armor: &mut Armor,
    ) -> FullOutput {
        let raw = self.winit.take_egui_input(window);
        let mut pending = MenuAction::None;
        let mut touched = false;
        let icons = &mut self.icons;
        let output = self.ctx.run_ui(raw, |ui| {
            apply_theme(ui.ctx());
            if menu.is_open() {
                pending = draw_menu(ui, menu);
            } else if gui.bag_open() {
                draw_creative(ui, gui, hotbar, bag, icons);
            } else if gui.player_inv_open() {
                touched = draw_player_inventory(ui, gui, hotbar, bag, armor, icons);
            }
        });
        self.pending = pending;
        self.touched = touched;
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

fn show_panel(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    // Outer lip (light strip) then dark well — matches legacy HUD panels.
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
            panel_frame().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.set_max_width(width);
                    add_contents(ui);
                });
            });
        });
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

fn menu_button(ui: &mut Ui, text: &str, selected: bool) -> egui::Response {
    let fill = if selected { SELECT_SOFT } else { BUTTON };
    let stroke = if selected {
        Stroke::new(1.5, SELECT)
    } else {
        Stroke::new(1.0, SLOT_EDGE)
    };
    let btn = egui::Button::new(RichText::new(text).size(15.0).color(INK))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(4))
        .min_size(Vec2::new(ui.available_width().max(280.0), 34.0));
    ui.add(btn)
}

fn draw_menu(ui: &mut Ui, menu: &mut Menu) -> MenuAction {
    let mut action = MenuAction::None;
    let title = menu.page_title();
    let rows = menu.menu_rows();
    let selected = menu.selected_row();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.14);
                show_panel(ui, 420.0, |ui| {
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
                        if menu_button(ui, &text, index == selected).clicked() {
                            action = menu.activate_row(index);
                        }
                    }
                });
            });
        });
    action
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
            let panel_w = clamped_panel_width(max_w, CREATIVE_COLS, true);
            let emi_w = EMI_SIDE_W.max(180.0);
            let side_by_side = panel_w + 12.0 + emi_w <= max_w + 0.5;
            ui.vertical_centered(|ui| {
                ui.set_max_width(max_w);
                egui::ScrollArea::vertical()
                    .id_salt("creative_chrome")
                    .max_height(ui.available_height() * 0.94)
                    .show(ui, |ui| {
                        ui.set_max_width(max_w);
                        if side_by_side {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);
                                draw_creative_panel(ui, gui, hotbar, icons, panel_w);
                                let _ = draw_emi_sidebar(ui, gui, hotbar, bag, icons);
                            });
                        } else {
                            draw_creative_panel(ui, gui, hotbar, icons, panel_w);
                            ui.add_space(10.0);
                            let _ = draw_emi_sidebar(ui, gui, hotbar, bag, icons);
                        }
                    });
            });
        });
}

fn draw_creative_panel(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
    panel_w: f32,
) {
    let tabs = ["all", "build", "nature", "ore", "light", "more"];
    let grid_w = grid_pitch(CREATIVE_COLS);
    show_panel(ui, panel_w, |ui| {
        // Don't force wider than the clamped panel on tiny windows.
        ui.set_min_width(grid_w.min(panel_w));
        title_label(ui, "CREATIVE");
        ui.add_space(8.0);
        // Category tabs with flanking arrows (GUI plan L7).
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
            if category_arrow(ui, "<").clicked() {
                gui.cycle_catalog_tab(false);
            }
            for (index, name) in tabs.iter().enumerate() {
                let selected = gui.catalog_tab() == index;
                let fill = if selected { SELECT } else { BUTTON };
                let tab = egui::Button::new(RichText::new(*name).size(13.0).color(
                    if selected {
                        Color32::from_rgb(20, 18, 12)
                    } else {
                        INK
                    },
                ))
                .fill(fill)
                .stroke(Stroke::new(
                    1.0,
                    if selected { SELECT } else { SLOT_EDGE },
                ))
                .corner_radius(CornerRadius::same(3))
                .min_size(Vec2::new(52.0, 26.0));
                if ui.add(tab).clicked() {
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
            let edit = egui::TextEdit::singleline(&mut q)
                .desired_width(edit_w)
                .hint_text("oak log")
                .text_color(INK);
            if ui.add(edit).changed() {
                gui.set_catalog_query(&q);
            }
        });
        ui.add_space(8.0);
        let ids = gui.creative_ids();
        if ids.is_empty() {
            ui.label(RichText::new("nothing matches").color(MUTED).size(14.0));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("creative_grid_scroll")
                .max_height(SLOT * 5.0 + SLOT_GAP * 4.0)
                .show(ui, |ui| {
                    egui::Grid::new("creative_grid")
                        .spacing([SLOT_GAP, SLOT_GAP])
                        .min_col_width(SLOT)
                        .show(ui, |ui| {
                            for (i, id) in ids.iter().enumerate() {
                                let selected = gui.catalog_index() == i;
                                let tip = gui::tip_for(id, None);
                                let tex = icon_texture(ui.ctx(), icons, id);
                                let resp = slot_button(ui, tex, selected, None);
                                resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                                if resp.clicked() {
                                    gui.set_catalog_index(i);
                                    let _ = hotbar.give_stack(id);
                                }
                                if (i + 1) % CREATIVE_COLS == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
        }
        ui.add_space(10.0);
        ui.label(RichText::new("hotbar").color(MUTED).size(13.0));
        ui.add_space(4.0);
        draw_hotbar_row(ui, hotbar, icons);
        ui.add_space(8.0);
        if close_button(ui, "Close (I)").clicked() {
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

fn draw_player_inventory(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            let max_w = chrome_max_width(ui);
            let panel_w = clamped_panel_width(max_w, INV_COLS, true);
            let emi_w = EMI_SIDE_W.max(180.0);
            let side_by_side = panel_w + 12.0 + emi_w <= max_w + 0.5;
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
                                if draw_player_inventory_panel(
                                    ui, gui, hotbar, bag, armor, icons, panel_w,
                                ) {
                                    touched = true;
                                }
                                if draw_emi_sidebar(ui, gui, hotbar, bag, icons) {
                                    touched = true;
                                }
                            });
                        } else {
                            if draw_player_inventory_panel(
                                ui, gui, hotbar, bag, armor, icons, panel_w,
                            ) {
                                touched = true;
                            }
                            ui.add_space(10.0);
                            if draw_emi_sidebar(ui, gui, hotbar, bag, icons) {
                                touched = true;
                            }
                        }
                    });
            });
        });
    touched
}

fn draw_player_inventory_panel(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    armor: &mut Armor,
    icons: &mut HashMap<String, TextureHandle>,
    panel_w: f32,
) -> bool {
    let mut touched = false;
    let grid_w = grid_pitch(INV_COLS);
    show_panel(ui, panel_w, |ui| {
        ui.set_min_width(grid_w.min(panel_w));
        title_label(ui, "INVENTORY");
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (PlayerInvTab::Player, "PLAYER"),
                (PlayerInvTab::Craft, "Craft"),
            ] {
                let selected = gui.inv_tab() == tab;
                let fill = if selected { SELECT } else { BUTTON };
                let btn = egui::Button::new(RichText::new(label).size(14.0).color(
                    if selected {
                        Color32::from_rgb(20, 18, 12)
                    } else {
                        INK
                    },
                ))
                .fill(fill)
                .stroke(Stroke::new(
                    1.0,
                    if selected { SELECT } else { SLOT_EDGE },
                ))
                .corner_radius(CornerRadius::same(3))
                .min_size(Vec2::new(88.0, 28.0));
                if ui.add(btn).clicked() {
                    gui.set_inv_tab(tab);
                }
            }
        });
        ui.add_space(10.0);
        match gui.inv_tab() {
            PlayerInvTab::Player => {
                if draw_player_detail(ui, hotbar, bag, armor, icons) {
                    touched = true;
                }
                ui.add_space(12.0);
                draw_inv_body_grid(ui, hotbar, bag, icons);
            }
            PlayerInvTab::Craft => {
                if draw_craft_detail(ui, gui, hotbar, bag, icons) {
                    touched = true;
                }
                ui.add_space(12.0);
                draw_inv_body_grid(ui, hotbar, bag, icons);
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("hotbar").color(MUTED).size(13.0));
        ui.add_space(4.0);
        draw_hotbar_row(ui, hotbar, icons);
        ui.add_space(8.0);
        if close_button(ui, "Close (E)").clicked() {
            if refund_inv_craft(gui, hotbar, bag) {
                touched = true;
            }
            gui.close_items();
        }
    });
    touched
}

/// EMI item browser: searchable list + CRAFTING / USES for the selection.
fn draw_emi_sidebar(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
    let mut craft_click = false;
    let emi_w = EMI_SIDE_W.max(180.0).min(ui.available_width().max(160.0));
    show_panel(ui, emi_w, |ui| {
        ui.label(RichText::new("EMI").size(16.0).color(INK).strong());
        ui.add_space(6.0);
        let rows = gui.emi_rows();
        let selected = gui.emi_selected().map(str::to_string);
        if rows.is_empty() {
            ui.label(RichText::new("nothing matches").color(MUTED).size(13.0));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("emi_list")
                .max_height(SLOT * 6.0 + SLOT_GAP * 5.0)
                .show(ui, |ui| {
                    egui::Grid::new("emi_grid")
                        .spacing([SLOT_GAP, SLOT_GAP])
                        .min_col_width(SLOT)
                        .show(ui, |ui| {
                            for (i, id) in rows.iter().enumerate() {
                                let is_sel = selected.as_deref() == Some(id.as_str());
                                let tip = gui::tip_for(id, None);
                                let tex = icon_texture(ui.ctx(), icons, id);
                                let resp = slot_button(ui, tex, is_sel, None);
                                resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                                if resp.clicked() {
                                    gui.select_emi(id);
                                }
                                if (i + 1) % EMI_COLS == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
        }
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
            Frame::new()
                .fill(ROW)
                .stroke(Stroke::new(1.0, SLOT_EDGE))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new("CRAFTING").size(13.0).color(INK).strong());
                    ui.add_space(4.0);
                    match gui.emi_crafting() {
                        Some(inputs) if !inputs.is_empty() => {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                                for input in &inputs {
                                    let tip = gui::tip_for(input, None);
                                    let tex = icon_texture(ui.ctx(), icons, input);
                                    let resp = slot_button(ui, tex, false, None);
                                    resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                                    if resp.clicked() {
                                        gui.select_emi(input);
                                    }
                                }
                                ui.label(RichText::new("→").size(16.0).color(DETAIL));
                                let tip = gui::tip_for(&id, None);
                                let tex = icon_texture(ui.ctx(), icons, &id);
                                let resp = slot_button(ui, tex, true, None);
                                resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
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
                                            let _ = slot_button_sized(
                                                ui,
                                                tex,
                                                false,
                                                None,
                                                SLOT * 0.55,
                                            );
                                            if (i + 1) % 3 == 0 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            }
                            ui.add_space(4.0);
                            let craft = egui::Button::new(
                                RichText::new("Craft").size(13.0).color(INK),
                            )
                            .fill(BUTTON)
                            .stroke(Stroke::new(1.0, SLOT_EDGE))
                            .corner_radius(CornerRadius::same(3))
                            .min_size(Vec2::new(72.0, 24.0));
                            if ui.add(craft).clicked() {
                                craft_click = true;
                            }
                        }
                        _ => {
                            ui.label(RichText::new("no recipe").color(MUTED).size(12.0));
                        }
                    }
                });

            ui.add_space(8.0);
            // USES panel — what this item crafts into.
            let uses = gui.emi_uses();
            Frame::new()
                .fill(ROW)
                .stroke(Stroke::new(1.0, SLOT_EDGE))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 8))
                .show(ui, |ui| {
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
                                let resp = slot_button(ui, tex, false, None);
                                resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                                if resp.clicked() {
                                    gui.select_emi(result);
                                }
                            }
                        });
                    }
                });
        }
    });
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
        // Left: armor column (wired head/chest/feet; stubs for legs + offhand).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
            for row in 0..EQUIP_ROWS {
                if equip_slot(ui, hotbar, bag, armor, icons, LEFT_ARMOR_MAP[row], true) {
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
    if let Some(handle) = icons.get(PREVIEW_KEY) {
        return handle.id();
    }
    let rgba = player::preview_rgba(PREVIEW_W, PREVIEW_H);
    let image = egui::ColorImage::from_rgba_unmultiplied([PREVIEW_W, PREVIEW_H], &rgba);
    let handle = ctx.load_texture(PREVIEW_KEY, image, TextureOptions::LINEAR);
    let tex = handle.id();
    icons.insert(PREVIEW_KEY.to_string(), handle);
    tex
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
    let resp = slot_button(ui, tex, false, None);
    if let Some(id) = worn_id {
        let tip = gui::tip_for(id, Some(1));
        resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
    } else if !armor_side {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                &gui::ItemTip::plain("Accessory", "empty — not wired yet"),
            );
        });
    } else if engine_slot.is_none() {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
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
                    tex,
                    false,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for(&id, Some(count));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
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
    hotbar: &mut Hotbar,
    icons: &mut HashMap<String, TextureHandle>,
) {
    // Same ×10 horizontal pitch as the body grid / HUD bar. Prefer full width
    // when the panel has room; never force the chrome past the window edge.
    let row_w = grid_pitch(hotbar::SLOTS);
    if ui.available_width() + 0.5 >= row_w {
        ui.set_min_width(row_w);
    }
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
                    tex,
                    selected,
                    if count > 1 { Some(count) } else { None },
                );
                if !id.is_empty() {
                    let tip = gui::tip_for_keen(id, Some(count), hotbar.is_keen(slot));
                    resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                }
                if resp.clicked() {
                    let _ = hotbar.select_digit((slot + 1) as u8);
                }
            }
            ui.end_row();
        });
}

/// 2×2 INPUT + OUT wired to [`Gui::inv_craft_*`], plus a lower "2?" tray stub.
fn draw_craft_detail(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) -> bool {
    let mut touched = false;
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
                    }
                });
            });
            ui.add_space(10.0);
            ui.label(RichText::new("2?").size(12.0).color(MUTED));
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
    touched
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
    let tex = if id.is_empty() {
        None
    } else {
        icon_texture(ui.ctx(), icons, &id)
    };
    let resp = slot_button(ui, tex, false, if count > 1 { Some(count) } else { None });
    if !id.is_empty() {
        let tip = gui::tip_for(&id, Some(count));
        resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
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
    let tex = preview.and_then(|id| icon_texture(ui.ctx(), icons, id));
    let resp = slot_button(ui, tex, preview.is_some(), None);
    if let Some(id) = preview {
        let tip = gui::tip_for(id, None);
        resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
    } else {
        resp.clone().on_hover_ui(|ui| {
            show_tip(
                ui,
                &gui::ItemTip::plain("OUT", "match a recipe in the 2x2"),
            );
        });
    }
    if !resp.clicked() || preview.is_none() {
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

fn close_button(ui: &mut Ui, label: &str) -> egui::Response {
    menu_button(ui, label, false)
}

fn show_tip(ui: &mut Ui, tip: &gui::ItemTip) {
    Frame::new()
        .fill(TIP_BG)
        .stroke(Stroke::new(1.0, TIP_EDGE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_max_width(240.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&tip.name).size(14.0).color(INK).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    rarity_chip(ui, tip.rarity);
                });
            });
            if !tip.description.is_empty() {
                ui.add_space(2.0);
                ui.label(RichText::new(&tip.description).size(12.0).color(DETAIL));
            }
            if !tip.stats.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.label(RichText::new("STATS").size(11.0).color(MUTED));
                for line in &tip.stats {
                    ui.label(RichText::new(line).size(12.0).color(INK));
                }
            }
            if !tip.enchants.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.label(RichText::new("ENCHANTS").size(11.0).color(MUTED));
                for (name, desc) in &tip.enchants {
                    ui.label(RichText::new(name).size(12.0).color(DETAIL));
                    Frame::new()
                        .fill(SLOT_WELL)
                        .stroke(Stroke::new(1.0, SLOT_EDGE))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_max_width(220.0);
                            ui.label(RichText::new(desc).size(11.0).color(MUTED));
                        });
                }
            }
        });
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
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
) -> egui::Response {
    slot_button_sized(ui, tex, selected, count, SLOT)
}

fn slot_button_sized(
    ui: &mut Ui,
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
    size: f32,
) -> egui::Response {
    let stroke = if selected {
        Stroke::new(2.0, SELECT)
    } else {
        Stroke::new(1.0, SLOT_EDGE)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), SLOT_WELL);
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(3), stroke, StrokeKind::Outside);
        // Light lip along the top edge of the cell.
        let lip = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right(), rect.top() + 2.0),
        );
        ui.painter()
            .rect_filled(lip, CornerRadius::same(2), LIP.gamma_multiply(0.85));
        if let Some(tex) = tex {
            let pad = (size * 0.11).max(2.0);
            let icon_rect = rect.shrink(pad);
            ui.painter().image(
                tex,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
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
    let handle = ctx.load_texture(id, image, TextureOptions::NEAREST);
    let tex = handle.id();
    icons.insert(id.to_string(), handle);
    Some(tex)
}

/// Soft-render a 3D-ish block (or flat item face) for egui slots.
fn render_slot_icon(id: &str) -> egui::ColorImage {
    let pack = Pack::builtin();
    let tile = texture::face_tile(id);
    let base = palette::color(id, 0);
    let is_block = pack.contains_block(id)
        || pack
            .placed_block(id)
            .is_some_and(|placed| pack.contains_block(&placed));
    if is_block {
        return isometric_icon(tile.as_ref(), base);
    }
    flat_icon(tile.as_ref(), base)
}

fn flat_icon(tile: Option<&Image>, base: [f32; 3]) -> egui::ColorImage {
    let size = ICON_PX;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let color = sample_or_base(
                tile,
                x as f32 / size as f32,
                y as f32 / size as f32,
                base,
                1.0,
            );
            put(&mut rgba, size, x, y, color);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([size, size], &rgba)
}

/// Classic inventory cube: top diamond + shaded left/right sides, atlas UVs.
fn isometric_icon(tile: Option<&Image>, base: [f32; 3]) -> egui::ColorImage {
    let size = ICON_PX;
    let mut rgba = vec![0u8; size * size * 4];
    let cx = (size as f32 - 1.0) * 0.5;
    let mid_y = size as f32 * 0.38;
    let hw = size as f32 * 0.42;
    let hh = size as f32 * 0.21;
    let depth = size as f32 * 0.42;

    // Left face (darker).
    fill_parallelogram(
        &mut rgba,
        size,
        [cx - hw, mid_y],
        [cx, mid_y + hh],
        [cx, mid_y + hh + depth],
        [cx - hw, mid_y + depth],
        tile,
        base,
        0.72,
        false,
    );
    // Right face.
    fill_parallelogram(
        &mut rgba,
        size,
        [cx + hw, mid_y],
        [cx, mid_y + hh],
        [cx, mid_y + hh + depth],
        [cx + hw, mid_y + depth],
        tile,
        base,
        0.90,
        false,
    );
    // Top face (bright) — painted last so it covers the seam.
    fill_parallelogram(
        &mut rgba,
        size,
        [cx, mid_y - hh],
        [cx + hw, mid_y],
        [cx, mid_y + hh],
        [cx - hw, mid_y],
        tile,
        base,
        1.12,
        true,
    );

    egui::ColorImage::from_rgba_unmultiplied([size, size], &rgba)
}

fn fill_parallelogram(
    rgba: &mut [u8],
    size: usize,
    a: [f32; 2],
    b: [f32; 2],
    c: [f32; 2],
    d: [f32; 2],
    tile: Option<&Image>,
    base: [f32; 3],
    shade: f32,
    overwrite: bool,
) {
    let min_x = a[0].min(b[0]).min(c[0]).min(d[0]).floor().max(0.0) as usize;
    let max_x = a[0]
        .max(b[0])
        .max(c[0])
        .max(d[0])
        .ceil()
        .min(size as f32 - 1.0) as usize;
    let min_y = a[1].min(b[1]).min(c[1]).min(d[1]).floor().max(0.0) as usize;
    let max_y = a[1]
        .max(b[1])
        .max(c[1])
        .max(d[1])
        .ceil()
        .min(size as f32 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            if let Some((u, v)) = bilinear_uv(p, a, b, c, d) {
                let color = sample_or_base(
                    tile,
                    u.clamp(0.0, 1.0),
                    v.clamp(0.0, 1.0),
                    base,
                    shade,
                );
                let i = (y * size + x) * 4;
                if overwrite || rgba[i + 3] == 0 {
                    put(rgba, size, x, y, color);
                }
            }
        }
    }
}

/// Map point inside quad ABCD to UV. A=(0,0), B=(1,0), C=(1,1), D=(0,1).
fn bilinear_uv(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> Option<(f32, f32)> {
    // Triangle A-B-C with UV (0,0)-(1,0)-(1,1)
    if let Some((w0, w1, w2)) = bary(p, a, b, c) {
        let u = w1 + w2;
        let v = w2;
        let _ = w0;
        return Some((u, v));
    }
    // Triangle A-C-D with UV (0,0)-(1,1)-(0,1)
    if let Some((w0, w1, w2)) = bary(p, a, c, d) {
        let u = w1;
        let v = w1 + w2;
        let _ = w0;
        return Some((u, v));
    }
    None
}

fn bary(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<(f32, f32, f32)> {
    let v0 = [b[0] - a[0], b[1] - a[1]];
    let v1 = [c[0] - a[0], c[1] - a[1]];
    let v2 = [p[0] - a[0], p[1] - a[1]];
    let den = v0[0] * v1[1] - v1[0] * v0[1];
    if den.abs() < 1e-6 {
        return None;
    }
    let w1 = (v2[0] * v1[1] - v1[0] * v2[1]) / den;
    let w2 = (v0[0] * v2[1] - v2[0] * v0[1]) / den;
    let w0 = 1.0 - w1 - w2;
    if w0 < -0.02 || w1 < -0.02 || w2 < -0.02 {
        return None;
    }
    Some((w0, w1, w2))
}

fn sample_or_base(
    tile: Option<&Image>,
    u: f32,
    v: f32,
    base: [f32; 3],
    shade: f32,
) -> [u8; 4] {
    let (r, g, b, a) = if let Some(tile) = tile {
        let tx = ((u * TILE_F).floor() as usize).min(15);
        let ty = ((v * TILE_F).floor() as usize).min(15);
        let i = (ty * 16 + tx) * 4;
        (
            tile.rgba[i],
            tile.rgba[i + 1],
            tile.rgba[i + 2],
            tile.rgba[i + 3],
        )
    } else {
        (
            (base[0] * 255.0) as u8,
            (base[1] * 255.0) as u8,
            (base[2] * 255.0) as u8,
            255,
        )
    };
    [
        ((r as f32 * shade).clamp(0.0, 255.0)) as u8,
        ((g as f32 * shade).clamp(0.0, 255.0)) as u8,
        ((b as f32 * shade).clamp(0.0, 255.0)) as u8,
        a,
    ]
}

const TILE_F: f32 = 16.0;

fn put(rgba: &mut [u8], size: usize, x: usize, y: usize, color: [u8; 4]) {
    let i = (y * size + x) * 4;
    rgba[i..i + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isometric_icon_has_opaque_pixels_for_cobble() {
        let img = render_slot_icon("cobblestone");
        assert_eq!(img.width(), ICON_PX);
        assert_eq!(img.height(), ICON_PX);
        let opaque = img.pixels.iter().filter(|c| c.a() > 200).count();
        assert!(
            opaque > ICON_PX * ICON_PX / 4,
            "expected a solid cube silhouette, got {opaque} opaque pixels"
        );
    }

    #[test]
    fn tip_for_names_cobble() {
        let tip = gui::tip_for("cobblestone", Some(1));
        assert!(tip.name.to_ascii_lowercase().contains("cobble"));
        assert!(!tip.line.is_empty());
        assert_eq!(tip.rarity, gui::Rarity::Common);
        assert_eq!(tip.description, tip.line);
        assert!(tip.stats.is_empty());
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
        assert!(plain.stats.iter().any(|s| s.contains("Uses")));
        assert!(plain.enchants.is_empty());
        let keen = gui::tip_for_keen("iron_pickaxe", Some(20), true);
        assert_eq!(keen.enchants.len(), 1);
        assert_eq!(keen.enchants[0].0, "Efficiency");
        assert!(!keen.enchants[0].1.is_empty());
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
        assert_eq!(Gui::catalog_tab_count(), 6);
    }

    #[test]
    fn emi_sidebar_is_four_wide() {
        assert_eq!(EMI_COLS, 4);
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
    fn player_tab_has_two_equipment_columns_of_five() {
        assert_eq!(EQUIP_ROWS, 5);
        assert_eq!(LEFT_ARMOR_MAP.len(), 5);
        // Engine armor: head / chest / feet — legs + offhand stay stubs.
        assert_eq!(LEFT_ARMOR_MAP[0], Some(0));
        assert_eq!(LEFT_ARMOR_MAP[1], Some(1));
        assert_eq!(LEFT_ARMOR_MAP[2], None);
        assert_eq!(LEFT_ARMOR_MAP[3], Some(2));
        assert_eq!(LEFT_ARMOR_MAP[4], None);
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
        }
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
