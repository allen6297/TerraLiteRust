//! egui menus on top of the existing wgpu world pass.
//!
//! Pause / settings / main / worlds / creative / items list draw here at
//! [`SCALE`] (~1.5×). RoseGold GUIAPI screens and block stations stay on the
//! legacy quad path (`gui::Gui::paint`) during the transition.

use egui::{Context, FontId, FullOutput, RichText, Vec2, ViewportId};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as WinitState;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::gui::{self, Gui, Menu, MenuAction};
use crate::hotbar::{self, Hotbar};

/// Overall egui zoom — matches the old `GUI_SCALE` target.
pub const SCALE: f32 = 1.5;

pub struct EguiHost {
    ctx: Context,
    winit: WinitState,
    renderer: Renderer,
    /// Pending menu action from the last `run` (Play, Quit, …).
    pending: MenuAction,
}

impl EguiHost {
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
    ) -> Self {
        let ctx = Context::default();
        ctx.set_pixels_per_point(SCALE);
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
        let output = self.ctx.run_ui(raw, |ui| {
            if menu.is_open() {
                pending = draw_menu(ui, menu);
            } else if gui.bag_open() {
                draw_creative(ui, gui, hotbar, bag);
            } else if gui.items_list_open() {
                draw_items(ui, gui, hotbar, bag);
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
            textures_delta: mut textures_delta,
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

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(18, 20, 28))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 74, 90)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(14))
}

fn draw_menu(ui: &mut egui::Ui, menu: &mut Menu) -> MenuAction {
    let mut action = MenuAction::None;
    let title = menu.page_title();
    let rows = menu.menu_rows();
    let selected = menu.selected_row();
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_black_alpha(160)))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.18);
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(12, 14, 20))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 94, 110)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.label(
                            RichText::new(title)
                                .font(FontId::proportional(28.0))
                                .color(egui::Color32::WHITE),
                        );
                        ui.add_space(10.0);
                        for (index, label) in rows.iter().enumerate() {
                            let detail = menu.row_detail(label);
                            let text = if detail.is_empty() {
                                gui::title_case_label(label)
                            } else {
                                format!("{} — {}", gui::title_case_label(label), detail)
                            };
                            let filled = index == selected;
                            let btn = egui::Button::new(RichText::new(text).size(16.0))
                                .fill(if filled {
                                    egui::Color32::from_rgb(70, 90, 50)
                                } else {
                                    egui::Color32::from_rgb(40, 44, 56)
                                })
                                .min_size(Vec2::new(280.0, 28.0));
                            if ui.add(btn).clicked() {
                                action = menu.activate_row(index);
                            }
                        }
                    });
            });
        });
    action
}

fn draw_creative(
    ui: &mut egui::Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
) {
    let tabs = ["all", "build", "nature", "ore", "light", "more"];
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_black_alpha(140)))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(
                        RichText::new("CREATIVE")
                            .font(FontId::proportional(26.0))
                            .color(egui::Color32::WHITE),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (index, name) in tabs.iter().enumerate() {
                            let selected = gui.catalog_tab() == index;
                            if ui
                                .selectable_label(selected, RichText::new(*name).size(14.0))
                                .clicked()
                            {
                                gui.set_catalog_tab(index);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("search");
                        let mut q = gui.catalog_query().to_string();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut q)
                                    .desired_width(200.0)
                                    .hint_text("oak log"),
                            )
                            .changed()
                        {
                            gui.set_catalog_query(&q);
                        }
                    });
                    ui.add_space(6.0);
                    let ids = gui.creative_ids();
                    if ids.is_empty() {
                        ui.label(RichText::new("nothing matches").color(egui::Color32::LIGHT_GRAY));
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(260.0)
                            .show(ui, |ui| {
                                egui::Grid::new("creative_grid")
                                    .spacing([6.0, 6.0])
                                    .show(ui, |ui| {
                                        for (i, id) in ids.iter().enumerate() {
                                            let label = crate::recipes::display_name(id);
                                            let selected = gui.catalog_index() == i;
                                            let btn = egui::Button::new(
                                                RichText::new(gui::title_case_label(&label))
                                                    .size(12.0),
                                            )
                                            .fill(if selected {
                                                egui::Color32::from_rgb(180, 160, 40)
                                            } else {
                                                egui::Color32::from_rgb(48, 52, 64)
                                            })
                                            .min_size(Vec2::new(96.0, 28.0));
                                            if ui.add(btn).clicked() {
                                                gui.set_catalog_index(i);
                                                let _ = hotbar.give_stack(id);
                                            }
                                            if (i + 1) % 5 == 0 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("bag").strong());
                    ui.horizontal_wrapped(|ui| {
                        for slot in 0..hotbar::PACK {
                            let id = bag.slot(slot).unwrap_or("");
                            let text = if id.is_empty() {
                                "·".to_string()
                            } else {
                                gui::title_case_label(&crate::recipes::display_name(id))
                            };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(text).size(11.0))
                                        .min_size(Vec2::new(72.0, 24.0)),
                                )
                                .clicked()
                                && !id.is_empty()
                            {
                                let _ = hotbar.give_stack(id);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    if ui.button("Close (I)").clicked() {
                        gui.close_bag();
                    }
                });
            });
        });
}

fn draw_items(
    ui: &mut egui::Ui,
    gui: &mut Gui,
    hotbar: &mut Hotbar,
    bag: &mut hotbar::Bag,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_black_alpha(140)))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.label(
                        RichText::new("ITEMS")
                            .font(FontId::proportional(26.0))
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(8.0);
                    let rows = gui.item_rows();
                    let selected = gui.selected_row();
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (index, id) in rows.iter().enumerate() {
                            let name = crate::recipes::display_name(id);
                            let detail = gui.item_detail(id);
                            let text = if detail.is_empty() {
                                gui::title_case_label(&name)
                            } else {
                                format!("{} — {detail}", gui::title_case_label(&name))
                            };
                            let btn = egui::Button::new(RichText::new(text).size(14.0))
                                .fill(if index == selected {
                                    egui::Color32::from_rgb(70, 90, 50)
                                } else {
                                    egui::Color32::from_rgb(40, 44, 56)
                                })
                                .min_size(Vec2::new(360.0, 26.0));
                            if ui.add(btn).clicked() {
                                gui.select_item_row(index);
                                let _ = gui.craft_selected_into(hotbar, bag);
                            }
                        }
                    });
                    if ui.button("Close (E)").clicked() {
                        gui.close_items();
                    }
                });
            });
        });
}
