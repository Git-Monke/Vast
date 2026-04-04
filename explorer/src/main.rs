use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use egui::{Color32, Pos2, Sense, Vec2};
use spacetimedb_sdk::{DbContext, Table};
use vast_bindings::{
    buildingQueryTableAccess, empireQueryTableAccess, register_empire, shipQueryTableAccess,
    spawn_starter_ship, DbConnection, Empire, EmpireTableAccess, Ship, ShipLocation,
    ShipTableAccess,
};
use universe::generator::{generate_star, star_info_at, PlanetType, StarSystem, StarType};
use universe::settings::{grid_to_ly, COORD_UNITS_PER_LY};
use universe::ships::{compute_cost, ShipStats};

const ZOOM_MIN: f64 = 0.1; // pixels per light-year (max zoom out)
const ZOOM_MAX: f64 = 400.0; // pixels per light-year (max zoom in)
/// Assumed max screen half-width in pixels, used to size the star-scan range.
/// Raise this if stars are cut off at low zoom on a very large monitor.
const MAX_SCREEN_HALF_PX: f64 = 2000.0;
/// Chunk side length in **grid units** (tenths of a ly). 640 = 64 ly per chunk side.
const CHUNK_SIZE: i32 = 64 * COORD_UNITS_PER_LY;

#[derive(Clone, Copy)]
struct CachedStar {
    x: i32,
    y: i32,
    star_type: StarType,
    size_solar_radii: f64,
}

/// Floor division for negative `a` (chunk index), with `b > 0`.
fn floor_div(a: i32, b: i32) -> i32 {
    debug_assert!(b > 0);
    if a >= 0 {
        a / b
    } else {
        (a - b + 1) / b
    }
}

fn collect_chunk_stars(cx: i32, cy: i32, chunk_size: i32) -> Vec<CachedStar> {
    let x0 = cx * chunk_size;
    let y0 = cy * chunk_size;
    let mut out = Vec::new();
    for ly in y0..y0 + chunk_size {
        for lx in x0..x0 + chunk_size {
            if let Some((star_type, size_solar_radii)) = star_info_at(lx, ly) {
                out.push(CachedStar {
                    x: lx,
                    y: ly,
                    star_type,
                    size_solar_radii,
                });
            }
        }
    }
    out
}

fn star_color(star_type: StarType) -> Color32 {
    match star_type {
        StarType::Red => Color32::from_rgb(255, 70, 40),
        StarType::Orange => Color32::from_rgb(255, 160, 30),
        StarType::Yellow => Color32::from_rgb(255, 240, 80),
        StarType::YellowWhite => Color32::from_rgb(255, 255, 200),
        StarType::White => Color32::from_rgb(240, 245, 255),
        StarType::BlueWhite => Color32::from_rgb(180, 210, 255),
        StarType::Blue => Color32::from_rgb(100, 140, 255),
        StarType::NeutronStar => Color32::from_rgb(210, 90, 255),
    }
}

/// Map solar radii → visual pixel radius using log scale,
/// capped so stars never overlap their grid cell.
fn visual_radius(size_solar_radii: f64, zoom: f64) -> f32 {
    let lo = 0.00001_f64.ln(); // neutron stars are tiny
    let hi = 20.0_f64.ln();
    let t = ((size_solar_radii.ln().clamp(lo, hi)) - lo) / (hi - lo); // 0..1
    let max_r = (zoom * 0.38).min(14.0_f64); // never wider than 38% of grid cell
    let r = 2.0 + (max_r - 2.0) * t;
    r as f32
}

fn format_large_number(num: u64) -> String {
    if num >= 1_000_000_000_000 {
        format!("{:.2}T", num as f64 / 1_000_000_000_000.0)
    } else if num >= 1_000_000_000 {
        format!("{:.2}B", num as f64 / 1_000_000_000.0)
    } else if num >= 1_000_000 {
        format!("{:.2}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.2}k", num as f64 / 1_000.0)
    } else {
        format!("{}", num)
    }
}

fn format_time(minutes: u64) -> String {
    if minutes >= 24 * 60 * 7 {
        format!(
            "{} weeks {} days",
            minutes / (24 * 60 * 7),
            (minutes % (24 * 60 * 7)) / (24 * 60)
        )
    } else if minutes >= 24 * 60 {
        format!(
            "{} days {} hours",
            minutes / (24 * 60),
            (minutes % (24 * 60)) / 60
        )
    } else if minutes >= 60 {
        format!("{} hours {} mins", minutes / 60, minutes % 60)
    } else {
        format!("{} mins", minutes)
    }
}

fn explorer_token_path() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    base.join("vast").join("explorer_token.txt")
}

#[derive(PartialEq)]
enum Tab {
    Universe,
    ShipBuilder,
}

struct ExplorerApp {
    current_tab: Tab,
    camera_x: f64,
    camera_y: f64,
    zoom: f64, // pixels per light-year
    selected: Option<StarSystem>,
    ship_stats: ShipStats,
    /// Lazy-filled map: chunk (cx, cy) → stars whose integer coords lie in that chunk.
    star_chunks: HashMap<(i32, i32), Vec<CachedStar>>,
    /// SpacetimeDB connection; [`Self::sync_session`] advances it via [`DbConnection::frame_tick`].
    conn: Option<DbConnection>,
    connect_error: Option<String>,
    bootstrap_error: Option<String>,
    empire_name_input: String,
    my_empire: Option<Empire>,
    my_ships: Vec<Ship>,
    did_center_camera: bool,
}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_session(ctx);
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::Universe, "🌌 Universe Explorer");
                ui.selectable_value(&mut self.current_tab, Tab::ShipBuilder, "🛠 Ship Builder");
            });
        });

        match self.current_tab {
            Tab::Universe => self.show_universe(ctx),
            Tab::ShipBuilder => self.show_ship_builder(ctx),
        }
    }
}

impl ExplorerApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        let mut app = Self {
            current_tab: Tab::Universe,
            camera_x: 0.0,
            camera_y: 0.0,
            zoom: 14.0,
            selected: None,
            ship_stats: ShipStats::default(),
            star_chunks: HashMap::new(),
            conn: None,
            connect_error: None,
            bootstrap_error: None,
            empire_name_input: String::new(),
            my_empire: None,
            my_ships: Vec::new(),
            did_center_camera: false,
        };
        app.start_connection(cc);
        app
    }

    fn start_connection(&mut self, cc: &eframe::CreationContext) {
        let host =
            std::env::var("SPACETIMEDB_HOST").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let db = std::env::var("SPACETIMEDB_DB_NAME").unwrap_or_else(|_| "vast".into());
        let token_path = explorer_token_path();
        let saved = std::fs::read_to_string(&token_path)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let egui_ctx = cc.egui_ctx.clone();
        let path_for_save = token_path.clone();
        let result = DbConnection::builder()
            .with_uri(host)
            .with_database_name(db)
            .with_token(saved)
            .on_connect(move |conn, _identity, token| {
                let _ = std::fs::create_dir_all(
                    path_for_save
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                );
                let _ = std::fs::write(&path_for_save, token);
                let egui_ctx = egui_ctx.clone();
                conn.subscription_builder()
                    .on_applied(move |_ctx| {
                        egui_ctx.request_repaint();
                    })
                    .on_error(|_ctx, e| {
                        eprintln!("subscription error: {e}");
                    })
                    .add_query(|q| q.from.empire())
                    .add_query(|q| q.from.building())
                    .add_query(|q| q.from.ship())
                    .subscribe();
            })
            .on_connect_error(|_ctx, e| {
                eprintln!("connection error: {e:?}");
            })
            .build();
        match result {
            Ok(c) => self.conn = Some(c),
            Err(e) => self.connect_error = Some(format!("{e:?}")),
        }
    }

    fn game_ready(&self) -> bool {
        self.my_empire.is_some() && !self.my_ships.is_empty()
    }

    fn sync_session(&mut self, _ctx: &egui::Context) {
        let Some(conn) = &self.conn else {
            return;
        };
        let _ = conn.frame_tick();
        let Some(id) = conn.try_identity() else {
            return;
        };
        self.my_empire = conn.db().empire().iter().find(|e| e.identity == id);
        self.my_ships.clear();
        for s in conn.db().ship().iter() {
            if s.owner == id {
                self.my_ships.push(s);
            }
        }
        if !self.did_center_camera {
            if let Some(ship) = self.my_ships.first() {
                if let ShipLocation::AtPlanet(loc) = &ship.location {
                    self.camera_x = grid_to_ly(loc.star_x);
                    self.camera_y = grid_to_ly(loc.star_y);
                    self.did_center_camera = true;
                }
            }
        }
    }

    fn show_universe(&mut self, ctx: &egui::Context) {
        if !self.game_ready() {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::from_rgb(4, 4, 12)))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading(egui::RichText::new("VAST").size(28.0));
                        ui.add_space(12.0);
                        ui.label("Enter your empire name (unique on this database). No password yet.");
                        ui.add_space(8.0);
                        if let Some(e) = &self.connect_error {
                            ui.colored_label(Color32::RED, format!("Connection: {e}"));
                        }
                        if let Some(e) = &self.bootstrap_error {
                            ui.colored_label(Color32::from_rgb(255, 180, 120), e.as_str());
                        }
                        ui.add_space(8.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.empire_name_input)
                                .desired_width(280.0),
                        );
                        ui.add_space(12.0);
                        if ui.button("Start").clicked() {
                            self.bootstrap_error = None;
                            let name = self.empire_name_input.trim().to_string();
                            if name.is_empty() {
                                self.bootstrap_error = Some("Enter an empire name.".into());
                            } else if let Some(conn) = &self.conn {
                                if let Err(e) = conn.reducers().register_empire(name) {
                                    self.bootstrap_error =
                                        Some(format!("Could not send register_empire: {e:?}"));
                                } else if let Err(e) = conn.reducers().spawn_starter_ship() {
                                    self.bootstrap_error =
                                        Some(format!("Could not send spawn_starter_ship: {e:?}"));
                                }
                            } else {
                                self.bootstrap_error =
                                    Some("Not connected to SpacetimeDB.".into());
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "SPACETIMEDB_HOST / SPACETIMEDB_DB_NAME (default: vast)",
                            )
                            .small()
                            .weak(),
                        );
                    });
                });
            return;
        }

        // ── Side panel: selected system info ────────────────────────────────
        if self.selected.is_some() {
            egui::SidePanel::right("info")
                .min_width(260.0)
                .show(ctx, |ui| {
                    let sys = self.selected.as_ref().unwrap();
                    ui.add_space(6.0);
                    ui.heading(format!(
                        "({:.1}, {:.1}) ly",
                        grid_to_ly(sys.x),
                        grid_to_ly(sys.y)
                    ));
                    ui.label(format!("Type:  {:?}", sys.star_type));
                    ui.label(format!("Temp:  {:.0} K", sys.star_type.temperature_k()));
                    ui.label(format!("Size:  {:.4} R☉", sys.star_size_solar_radii));
                    ui.separator();
                    ui.label(format!("{} planet(s)", sys.planets.len()));
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for p in &sys.planets {
                            ui.add_space(4.0);
                            let type_label = match p.planet_type {
                                PlanetType::Solid => "Solid",
                                PlanetType::Ocean => "Ocean",
                                PlanetType::Gas => "Gas",
                            };
                            ui.label(
                                egui::RichText::new(format!("▸ Planet {}", p.index + 1)).strong(),
                            );
                            ui.label(format!("  Type:     {}", type_label));
                            ui.label(format!("  Distance: {:.2} AU", p.distance_au));
                            ui.label(format!("  Temp:     {:.0} K", p.temperature_k));
                            ui.label(format!("  Slots:    {}", p.size));
                            ui.label(format!("  Richness: {:.2}×", p.richness));
                            for res in &p.resources {
                                ui.label(format!("  {}:  {:.2}×", res.name(), res.multiplier()));
                            }
                        }
                    });
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() {
                        self.selected = None;
                    }
                });
        }

        // ── Main star map ────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(4, 4, 12)))
            .show(ctx, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = response.rect;

                // Drag to pan
                if response.dragged() {
                    let delta = response.drag_delta();
                    self.camera_x -= delta.x as f64 / self.zoom;
                    self.camera_y -= delta.y as f64 / self.zoom;
                }

                // Scroll to zoom, centred on cursor
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let factor = (scroll as f64 * 0.004).exp();
                    self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                }

                // Arrow / WASD pan
                let pan_speed = 120.0 / self.zoom; // ly per second ≈ constant screen speed
                let dt = ctx.input(|i| i.stable_dt) as f64;
                ctx.input(|i| {
                    if i.key_down(egui::Key::ArrowLeft) || i.key_down(egui::Key::A) {
                        self.camera_x -= pan_speed * dt;
                    }
                    if i.key_down(egui::Key::ArrowRight) || i.key_down(egui::Key::D) {
                        self.camera_x += pan_speed * dt;
                    }
                    if i.key_down(egui::Key::ArrowUp) || i.key_down(egui::Key::W) {
                        self.camera_y -= pan_speed * dt;
                    }
                    if i.key_down(egui::Key::ArrowDown) || i.key_down(egui::Key::S) {
                        self.camera_y += pan_speed * dt;
                    }
                });

                let w = rect.width() as f64;
                let h = rect.height() as f64;
                let cx = rect.center();

                let u = COORD_UNITS_PER_LY as f64;
                // Camera in light-years; grid coordinates are tenths of a ly.
                let to_screen = |gx: i32, gy: i32| -> Pos2 {
                    let lx = gx as f64 / u;
                    let ly = gy as f64 / u;
                    Pos2::new(
                        cx.x + ((lx - self.camera_x) * self.zoom) as f32,
                        cx.y + ((ly - self.camera_y) * self.zoom) as f32,
                    )
                };

                // Visible grid range (capped in tenths of a ly)
                let half_w_ly = w / (2.0 * self.zoom);
                let half_h_ly = h / (2.0 * self.zoom);
                let mut x_min = ((self.camera_x - half_w_ly) * u).floor() as i32;
                let mut x_max = ((self.camera_x + half_w_ly) * u).ceil() as i32;
                let mut y_min = ((self.camera_y - half_h_ly) * u).floor() as i32;
                let mut y_max = ((self.camera_y + half_h_ly) * u).ceil() as i32;
                let scan_cap = (MAX_SCREEN_HALF_PX / ZOOM_MIN).ceil() as i32 * COORD_UNITS_PER_LY;
                let cx_t = (self.camera_x * u).round() as i32;
                let cy_t = (self.camera_y * u).round() as i32;
                x_min = x_min.max(cx_t - scan_cap);
                x_max = x_max.min(cx_t + scan_cap);
                y_min = y_min.max(cy_t - scan_cap);
                y_max = y_max.min(cy_t + scan_cap);

                let cx_min = floor_div(x_min, CHUNK_SIZE);
                let cx_max = floor_div(x_max, CHUNK_SIZE);
                let cy_min = floor_div(y_min, CHUNK_SIZE);
                let cy_max = floor_div(y_max, CHUNK_SIZE);

                // Render stars; track nearest for click (iterate cached stars only, not every grid cell)
                let click_pos = if response.clicked() {
                    response.interact_pointer_pos()
                } else {
                    None
                };
                let mut nearest: Option<(f32, i32, i32)> = None; // (dist², lx, ly)

                for cy in cy_min..=cy_max {
                    for cx in cx_min..=cx_max {
                        let stars = self
                            .star_chunks
                            .entry((cx, cy))
                            .or_insert_with(|| collect_chunk_stars(cx, cy, CHUNK_SIZE));
                        for cs in stars.iter() {
                            let sp = to_screen(cs.x, cs.y);
                            if !rect.contains(sp) {
                                continue;
                            }

                            let color = star_color(cs.star_type);
                            let r = visual_radius(cs.size_solar_radii, self.zoom);

                            // Sphere: base fill + soft ambient rim + specular highlight
                            painter.circle_filled(sp, r, color);
                            // Dim outer glow to give volume
                            painter.circle_filled(
                                sp,
                                r * 1.25,
                                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
                            );
                            // Specular highlight: small bright spot upper-left
                            let hl_offset = Vec2::new(-r * 0.28, -r * 0.28);
                            painter.circle_filled(
                                sp + hl_offset,
                                r * 0.32,
                                Color32::from_rgba_unmultiplied(255, 255, 255, 140),
                            );

                            if let Some(cp) = click_pos {
                                let d2 = (sp.x - cp.x).powi(2) + (sp.y - cp.y).powi(2);
                                if nearest.map_or(true, |(nd, _, _)| d2 < nd) {
                                    nearest = Some((d2, cs.x, cs.y));
                                }
                            }
                        }
                    }
                }

                // Handle click
                if click_pos.is_some() {
                    if let Some((d2, lx, ly)) = nearest {
                        let threshold = (visual_radius(1.0, self.zoom) + 6.0).powi(2);
                        if d2 <= threshold {
                            self.selected = generate_star(lx, ly);
                        } else {
                            self.selected = None;
                        }
                    } else {
                        self.selected = None;
                    }
                }

                // Highlight selected star
                if let Some(sys) = &self.selected {
                    let sp = to_screen(sys.x, sys.y);
                    let r = visual_radius(sys.star_size_solar_radii, self.zoom);
                    painter.circle_stroke(
                        sp,
                        r + 4.0,
                        egui::Stroke::new(1.5, Color32::from_rgb(255, 255, 100)),
                    );
                }

                // Unique stars = every star in chunk caches (each cell at most one star; chunks don't overlap).
                let stars_discovered: usize = self
                    .star_chunks
                    .values()
                    .map(|chunk| chunk.len())
                    .sum();

                // HUD
                let hud_origin = rect.left_top() + Vec2::new(10.0, 10.0);
                painter.text(
                    hud_origin,
                    egui::Align2::LEFT_TOP,
                    format!(
                        "({:.1}, {:.1}) ly   zoom {:.1}×   drag/WASD to pan   scroll to zoom   click star for info",
                        self.camera_x, self.camera_y, self.zoom
                    ),
                    egui::FontId::monospace(11.0),
                    Color32::from_gray(130),
                );
                let mut hud_y = 16.0;
                if let Some(e) = &self.my_empire {
                    painter.text(
                        hud_origin + Vec2::new(0.0, hud_y),
                        egui::Align2::LEFT_TOP,
                        format!("Empire: {} — {} credits", e.name, e.credits),
                        egui::FontId::monospace(11.0),
                        Color32::from_rgb(180, 220, 255),
                    );
                    hud_y += 16.0;
                }
                painter.text(
                    hud_origin + Vec2::new(0.0, hud_y),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "Stars discovered: {}  (unique; grows as you pan into new regions)",
                        stars_discovered
                    ),
                    egui::FontId::monospace(11.0),
                    Color32::from_gray(130),
                );

                let pan_keys = ctx.input(|i| {
                    i.key_down(egui::Key::ArrowLeft)
                        || i.key_down(egui::Key::A)
                        || i.key_down(egui::Key::ArrowRight)
                        || i.key_down(egui::Key::D)
                        || i.key_down(egui::Key::ArrowUp)
                        || i.key_down(egui::Key::W)
                        || i.key_down(egui::Key::ArrowDown)
                        || i.key_down(egui::Key::S)
                });
                if response.dragged()
                    || scroll != 0.0
                    || pan_keys
                    || ctx.input(|i| i.pointer.any_pressed())
                {
                    ctx.request_repaint();
                }
            });
    }

    fn show_ship_builder(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🛠 Ship Builder");
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Basic Scout").clicked() {
                    self.ship_stats = ShipStats {
                        size_kt: 1,
                        speed_lys: 0.1,
                        defense: 10,
                        attack: 0,
                        battery_ly: 50,
                        radar_ly: 5,
                    };
                }
                if ui.button("Medium Freighter").clicked() {
                    self.ship_stats = ShipStats {
                        size_kt: 100,
                        speed_lys: 0.1,
                        defense: 50,
                        attack: 0,
                        battery_ly: 100,
                        radar_ly: 10,
                    };
                }
                if ui.button("Destroyer").clicked() {
                    self.ship_stats = ShipStats {
                        size_kt: 50,
                        speed_lys: 5.0,
                        defense: 200,
                        attack: 500,
                        battery_ly: 100,
                        radar_ly: 15,
                    };
                }
                if ui.button("Empire Supertanker").clicked() {
                    self.ship_stats = ShipStats {
                        size_kt: 10_000,
                        speed_lys: 0.1,
                        defense: 1000,
                        attack: 0,
                        battery_ly: 20,
                        radar_ly: 5,
                    };
                }
            });
            ui.add_space(10.0);

            ui.columns(2, |columns| {
                // Left Column: Stats Input
                columns[0].group(|ui| {
                    ui.heading("Ship Stats");
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Size (kt):");
                        ui.add(egui::DragValue::new(&mut self.ship_stats.size_kt).speed(1));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Speed (ly/s):");
                        ui.add(
                            egui::DragValue::new(&mut self.ship_stats.speed_lys)
                                .speed(0.01)
                                .range(0.1..=1000.0),
                        );
                        if self.ship_stats.speed_lys < 0.1 {
                            self.ship_stats.speed_lys = 0.1;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Attack:");
                        ui.add(egui::DragValue::new(&mut self.ship_stats.attack).speed(1));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Defense:");
                        ui.add(egui::DragValue::new(&mut self.ship_stats.defense).speed(1));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Battery (ly):");
                        ui.add(egui::DragValue::new(&mut self.ship_stats.battery_ly).speed(1));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Radar (ly):");
                        ui.add(egui::DragValue::new(&mut self.ship_stats.radar_ly).speed(1));
                    });
                });

                // Right Column: Cost Breakdown
                columns[1].group(|ui| {
                    ui.heading("Cost Breakdown");
                    ui.add_space(8.0);

                    match compute_cost(&self.ship_stats) {
                        Ok(cost) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Development Time: {}",
                                    format_time(cost.total_dev_minutes)
                                ))
                                .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "Daily Maintenance: {} credits",
                                    format_large_number(cost.total_maint_credits)
                                ))
                                .strong(),
                            );
                            ui.add_space(10.0);

                            egui::Grid::new("cost_breakdown_grid")
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label("Component");
                                    ui.label("Dev C");
                                    ui.label("Maint C");
                                    ui.label("Base Maint");
                                    ui.label("Mult");
                                    ui.end_row();

                                    ui.label("Size");
                                    ui.label(format_large_number(cost.size_dev_credits));
                                    ui.label(format_large_number(cost.size_maint_credits));
                                    ui.label(format_large_number(cost.size_maint_base_credits));
                                    ui.label("-");
                                    ui.end_row();

                                    ui.label("Speed");
                                    ui.label(format_large_number(cost.speed_dev_credits));
                                    ui.label(format_large_number(cost.speed_maint_credits));
                                    ui.label(format_large_number(cost.speed_maint_base_credits));
                                    ui.label(format!("{:.2}x", cost.speed_maint_mult));
                                    ui.end_row();

                                    ui.label("Attack");
                                    ui.label(format_large_number(cost.attack_dev_credits));
                                    ui.label(format_large_number(cost.attack_maint_credits));
                                    ui.label(format_large_number(cost.attack_maint_base_credits));
                                    ui.label(format!("{:.2}x", cost.attack_maint_mult));
                                    ui.end_row();

                                    ui.label("Defense");
                                    ui.label(format_large_number(cost.defense_dev_credits));
                                    ui.label(format_large_number(cost.defense_maint_credits));
                                    ui.label(format_large_number(cost.defense_maint_base_credits));
                                    ui.label(format!("{:.2}x", cost.defense_maint_mult));
                                    ui.end_row();

                                    ui.label("Battery");
                                    ui.label(format_large_number(cost.battery_dev_credits));
                                    ui.label(format_large_number(cost.battery_maint_credits));
                                    ui.label(format_large_number(cost.battery_maint_base_credits));
                                    ui.label(format!("{:.2}x", cost.battery_maint_mult));
                                    ui.end_row();

                                    ui.label("Radar");
                                    ui.label(format_large_number(cost.radar_dev_credits));
                                    ui.label(format_large_number(cost.radar_maint_credits));
                                    ui.label(format_large_number(cost.radar_maint_base_credits));
                                    ui.label("-");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("TOTAL").strong());
                                    ui.label(
                                        egui::RichText::new(format_large_number(
                                            cost.total_dev_credits,
                                        ))
                                        .strong(),
                                    );
                                    ui.label(
                                        egui::RichText::new(format_large_number(
                                            cost.total_maint_credits,
                                        ))
                                        .strong(),
                                    );
                                    ui.label("-");
                                    ui.label("-");
                                    ui.end_row();
                                });
                        }
                        Err(e) => {
                            ui.colored_label(Color32::RED, format!("Invalid Configuration: {}", e));
                        }
                    }
                });
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VAST — Galaxy Explorer")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "VAST Explorer",
        options,
        Box::new(|cc| Ok(Box::new(ExplorerApp::new(cc)))),
    )
}
