//! egui menus on top of the existing wgpu world pass.
//!
//! Pause / settings / main / worlds / creative / items list draw here at
//! [`SCALE`] (~1.5×). RoseGold GUIAPI screens and block stations stay on the
//! legacy quad path (`gui::Gui::paint`) during the transition.
//!
//! Visual language matches the legacy HUD: dark well, light top lip, warm
//! select — not default egui purple/gray.

use std::collections::HashMap;

use egui::{
    Align2, Color32, Context, CornerRadius, FontId, Frame, FullOutput, Margin, RichText, Sense,
    Stroke, StrokeKind, TextureHandle, TextureId, TextureOptions, Ui, Vec2, ViewportId,
};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as WinitState;
use rebuild_world::Pack;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::gui::{self, Gui, Menu, MenuAction};
use crate::hotbar::{self, Hotbar};
use crate::palette;
use crate::texture::{self, Image};

/// Overall egui zoom — matches the old `GUI_SCALE` target.
pub const SCALE: f32 = 1.5;

/// Square inventory / creative cell in egui points (before [`SCALE`]).
const SLOT: f32 = 36.0;
const SLOT_GAP: f32 = 5.0;
const ICON_PX: usize = 48;
const CREATIVE_COLS: usize = 9;

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
            icons: HashMap::new(),
        }
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.winit.on_window_event(window, event).consumed
    }

    pub fn take_action(&mut self) -> MenuAction {
        std::mem::replace(&mut self.pending, MenuAction::None)
    }

    /// True when egui should own pointer/keyboard instead of the world.
    pub fn wants_input(menu: &Menu, gui: &Gui) -> bool {
        menu.is_open() || gui.bag_open() || gui.items_list_open()
    }

    /// Build menus for this frame. Returns tessellated output ready to paint.
    pub fn run(
        &mut self,
        window: &Window,
        menu: &mut Menu,
        gui: &mut Gui,
        hotbar: &mut Hotbar,
        bag: &mut hotbar::Bag,
    ) -> FullOutput {
        let raw = self.winit.take_egui_input(window);
        let mut pending = MenuAction::None;
        let icons = &mut self.icons;
        let output = self.ctx.run_ui(raw, |ui| {
            apply_theme(ui.ctx());
            if menu.is_open() {
                pending = draw_menu(ui, menu);
            } else if gui.bag_open() {
                draw_creative(ui, gui, hotbar, bag, icons);
            } else if gui.items_list_open() {
                draw_items(ui, gui, hotbar, bag, icons);
            }
        });
        self.pending = pending;
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
                ui.set_width(width);
                add_contents(ui);
            });
        });
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
    let tabs = ["all", "build", "nature", "ore", "light", "more"];
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                let panel_w = SLOT * CREATIVE_COLS as f32
                    + SLOT_GAP * (CREATIVE_COLS as f32 - 1.0)
                    + 28.0;
                show_panel(ui, panel_w.max(520.0), |ui| {
                    title_label(ui, "CREATIVE");
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for (index, name) in tabs.iter().enumerate() {
                            let selected = gui.catalog_tab() == index;
                            let fill = if selected { SELECT } else { BUTTON };
                            let tab = egui::Button::new(
                                RichText::new(*name).size(13.0).color(if selected {
                                    Color32::from_rgb(20, 18, 12)
                                } else {
                                    INK
                                }),
                            )
                            .fill(fill)
                            .stroke(Stroke::new(1.0, if selected { SELECT } else { SLOT_EDGE }))
                            .corner_radius(CornerRadius::same(3))
                            .min_size(Vec2::new(56.0, 26.0));
                            if ui.add(tab).clicked() {
                                gui.set_catalog_tab(index);
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("search").color(MUTED).size(13.0));
                        let mut q = gui.catalog_query().to_string();
                        let edit = egui::TextEdit::singleline(&mut q)
                            .desired_width(240.0)
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
                    ui.label(RichText::new("bag").color(MUTED).size(13.0));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(SLOT_GAP);
                        for slot in 0..hotbar::PACK {
                            let id = bag.slot(slot).unwrap_or("");
                            let count = bag.count(slot);
                            let tex = if id.is_empty() {
                                None
                            } else {
                                icon_texture(ui.ctx(), icons, id)
                            };
                            let resp = slot_button(
                                ui,
                                tex,
                                false,
                                if count > 1 { Some(count) } else { None },
                            );
                            if !id.is_empty() {
                                let tip = gui::tip_for(id, Some(count));
                                resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                                if resp.clicked() {
                                    let _ = hotbar.give_stack(id);
                                }
                            }
                        }
                    });
                    ui.add_space(8.0);
                    if close_button(ui, "Close (I)").clicked() {
                        gui.close_bag();
                    }
                });
            });
        });
}

fn draw_items(
    ui: &mut Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
    icons: &mut HashMap<String, TextureHandle>,
) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(backdrop()))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                show_panel(ui, 460.0, |ui| {
                    title_label(ui, "ITEMS");
                    ui.add_space(10.0);
                    let rows = gui.item_rows();
                    let selected = gui.selected_row();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (index, id) in rows.iter().enumerate() {
                            let tip = gui::tip_for(id, None);
                            let detail = gui.item_detail(id);
                            let chosen = index == selected;
                            let fill = if chosen { SELECT_SOFT } else { ROW };
                            let stroke = if chosen {
                                Stroke::new(1.5, SELECT)
                            } else {
                                Stroke::new(1.0, SLOT_EDGE)
                            };
                            let tex = icon_texture(ui.ctx(), icons, id);
                            let height = 40.0;
                            let width = ui.available_width();
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
                            if ui.is_rect_visible(rect) {
                                ui.painter().rect(
                                    rect,
                                    CornerRadius::same(4),
                                    fill,
                                    stroke,
                                    StrokeKind::Inside,
                                );
                                let icon_rect = egui::Rect::from_min_size(
                                    egui::pos2(rect.left() + 6.0, rect.center().y - 14.0),
                                    Vec2::splat(28.0),
                                );
                                ui.painter().rect_filled(
                                    icon_rect,
                                    CornerRadius::same(2),
                                    SLOT_WELL,
                                );
                                if let Some(tex) = tex {
                                    ui.painter().image(
                                        tex,
                                        icon_rect.shrink(2.0),
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                }
                                let name_pos = egui::pos2(rect.left() + 42.0, rect.top() + 6.0);
                                ui.painter().text(
                                    name_pos,
                                    Align2::LEFT_TOP,
                                    &tip.name,
                                    FontId::proportional(14.0),
                                    INK,
                                );
                                let line = if detail.is_empty() {
                                    tip.line.as_str()
                                } else {
                                    detail.as_str()
                                };
                                if !line.is_empty() {
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 42.0, rect.top() + 22.0),
                                        Align2::LEFT_TOP,
                                        line,
                                        FontId::proportional(12.0),
                                        MUTED,
                                    );
                                }
                            }
                            resp.clone().on_hover_ui(|ui| show_tip(ui, &tip));
                            if resp.clicked() {
                                gui.select_item_row(index);
                                let _ = gui.craft_selected_into(hotbar, bag);
                            }
                            ui.add_space(4.0);
                        }
                    });
                    ui.add_space(6.0);
                    if close_button(ui, "Close (E)").clicked() {
                        gui.close_items();
                    }
                });
            });
        });
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
            ui.set_max_width(220.0);
            ui.label(RichText::new(&tip.name).size(14.0).color(INK).strong());
            if !tip.line.is_empty() {
                ui.label(RichText::new(&tip.line).size(12.0).color(DETAIL));
            }
        });
}

fn slot_button(
    ui: &mut Ui,
    tex: Option<TextureId>,
    selected: bool,
    count: Option<u16>,
) -> egui::Response {
    let stroke = if selected {
        Stroke::new(2.0, SELECT)
    } else {
        Stroke::new(1.0, SLOT_EDGE)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SLOT), Sense::click());
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
            let pad = 4.0;
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
