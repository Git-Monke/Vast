mod building_economy;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use egui::{Color32, Pos2, Sense, Vec2};
use spacetimedb_sdk::{DbContext, Identity, Table};
use spacetimedb_sdk::Timestamp;
use vast_bindings::{
    buildingQueryTableAccess, collect_star_resources, empireQueryTableAccess, order_warp,
    place_building, register_empire, sell_ship_cargo, sell_star_warehouse, shipQueryTableAccess,
    spawn_starter_ship, upgrade_building, star_system_stockQueryTableAccess,
    Building, BuildingKind, BuildingTableAccess, DbConnection, Empire, EmpireTableAccess, Material,
    Ship, ShipAttackMode, ShipTableAccess, StarSystemStock, StarSystemStockTableAccess,
};
use universe::generator::{generate_star, star_info_at, PlanetType, StarSystem, StarType};
use universe::material_stock::get_amount;
use universe::credits_for_materials_sale;
use universe::{
    Material as UniverseMaterial, MaterialKind, BASELINE_CREDITS_PER_KT_HELIUM,
    BASELINE_CREDITS_PER_KT_IRON,
};
use universe::parse_star_id;
use universe::star_location_id;
use universe::settings::{grid_to_ly, COORD_UNITS_PER_LY};
use universe::star_display_id;
use universe::ships::{compute_cost, ShipStats};

const ZOOM_MIN: f64 = 0.1; // pixels per light-year (max zoom out)
const ZOOM_MAX: f64 = 400.0; // pixels per light-year (max zoom in)
/// Assumed max screen half-width in pixels, used to size the star-scan range.
/// Raise this if stars are cut off at low zoom on a very large monitor.
const MAX_SCREEN_HALF_PX: f64 = 2000.0;
/// Chunk side length in **grid units** (tenths of a ly). 640 = 64 ly per chunk side.
const CHUNK_SIZE: i32 = 64 * COORD_UNITS_PER_LY;

fn max_stationed_kt(ships: &[Ship], owner: Identity, sx: i32, sy: i32) -> u32 {
    ships
        .iter()
        .filter(|s| s.owner == owner)
        .filter(|s| !s.in_transit && s.star_x == sx && s.star_y == sy)
        .map(|s| s.stats.size_kt)
        .max()
        .unwrap_or(0)
}

fn planet_has_enemy_garrison(buildings: &[Building], planet_index: u8, me: Identity) -> bool {
    buildings.iter().any(|b| {
        b.planet_index == planet_index
            && b.kind == BuildingKind::MilitaryGarrison
            && b.owner != Some(me)
    })
}

fn count_my_sales_depots(conn: &DbConnection, me: Identity) -> u32 {
    conn.db()
        .building()
        .iter()
        .filter(|b| b.kind == BuildingKind::SalesDepot && b.owner == Some(me))
        .count() as u32
}

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

/// True if the ship is tied to this star (in this system, or in transit from/to here).
fn my_ship_at_star_coords(ship: &Ship, star_x: i32, star_y: i32) -> bool {
    if !ship.in_transit {
        ship.star_x == star_x && ship.star_y == star_y
    } else {
        (ship.transit_from_x == star_x && ship.transit_from_y == star_y)
            || (ship.transit_to_x == star_x && ship.transit_to_y == star_y)
    }
}

fn format_material_line(m: &Material) -> String {
    match m {
        Material::Iron(q) => format!("Iron {:.2}", q),
        Material::Helium(q) => format!("Helium {:.2}", q),
    }
}

fn jump_ready_line(ship: &Ship) -> Option<String> {
    if ship.in_transit {
        return None;
    }
    let now = Timestamp::now();
    let n = now.to_micros_since_unix_epoch();
    let r = ship.jump_ready_at.to_micros_since_unix_epoch();
    if n >= r {
        Some("Battery: ready to warp".to_string())
    } else {
        let sec = ((r - n) / 1_000_000).max(1);
        Some(format!("Battery: charging (~{sec}s to jump)"))
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

fn explorer_token_dir() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    base.join("vast").join("explorer_tokens")
}

/// Token file for a given empire name (trimmed). Same name → same file → same SpacetimeDB identity.
fn explorer_token_path_for_empire_name(name: &str) -> PathBuf {
    let trimmed = name.trim();
    let mut safe: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else if c.is_whitespace() {
                '_'
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        safe = "unnamed".into();
    }
    safe.truncate(200);
    explorer_token_dir().join(format!("{safe}.txt"))
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
    bootstrap_err_tx: mpsc::Sender<String>,
    bootstrap_err_rx: mpsc::Receiver<String>,
    /// Async messages from reducers (e.g. warp), shown in the star panel.
    toast_tx: mpsc::Sender<String>,
    toast_rx: mpsc::Receiver<String>,
    toast_message: Option<String>,
    /// Destination star ID for [`order_warp`] (paste or "Use for warp" from current selection).
    warp_star_id_input: String,
    /// egui context for [`Self::start_connection`] (subscriptions need `request_repaint`).
    egui_ctx: egui::Context,
    empire_name_input: String,
    /// Set after a successful **Connect** — `register_empire` uses this so it matches the token file.
    session_empire_name: Option<String>,
    my_empire: Option<Empire>,
    my_ships: Vec<Ship>,
    did_center_camera: bool,
    /// Selected ship in the star info panel (for highlighting and "Center on map").
    selected_ship_id: Option<u64>,
    /// Clears [`Self::selected_ship_id`] when the user selects a different star system.
    prev_selected_star: Option<(i32, i32)>,
    /// Construction form (selected star panel).
    build_planet_index: u8,
    /// Slot index for construction (`egui` sliders use `u32`).
    build_slot: u32,
    build_kind: BuildingKind,
    build_level: u32,
    build_mining_resource_index: u8,
    build_garrison_mode: ShipAttackMode,
    /// Per [`MaterialKind::ALL`] — kt to load when collecting ([`collect_star_resources`]).
    collect_pickup_kt: Vec<f64>,
    /// Per [`MaterialKind::ALL`] — kt to sell from ship at a Sales Depot ([`sell_ship_cargo`]).
    sell_ship_kt: Vec<f64>,
    /// Per [`MaterialKind::ALL`] — kt to sell from star warehouse ([`sell_star_warehouse`]).
    sell_warehouse_kt: Vec<f64>,
    /// Throttled DB scans for the selected-star side panel ([`Self::refresh_star_panel_cache_if_needed`]).
    star_panel_cache_star: Option<(i32, i32)>,
    star_panel_cache_time_secs: f64,
    star_panel_cached_buildings: Vec<Building>,
    star_panel_cached_stock: Option<StarSystemStock>,
}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut bootstrap_msgs = 0u32;
        while let Ok(msg) = self.bootstrap_err_rx.try_recv() {
            self.bootstrap_error = Some(msg);
            bootstrap_msgs += 1;
        }
        if bootstrap_msgs > 0 {
            ctx.request_repaint();
        }
        let mut toast_msgs = 0u32;
        while let Ok(msg) = self.toast_rx.try_recv() {
            self.toast_message = Some(msg);
            toast_msgs += 1;
        }
        if toast_msgs > 0 {
            ctx.request_repaint();
        }
        self.sync_session(ctx);
        // Keep `sync_session` / transit / star-economy math advancing while idle (not only on map input).
        if matches!(self.current_tab, Tab::Universe) && self.conn.is_some() {
            ctx.request_repaint();
        }
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
        let (bootstrap_err_tx, bootstrap_err_rx) = mpsc::channel();
        let (toast_tx, toast_rx) = mpsc::channel();
        let app = Self {
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
            bootstrap_err_tx,
            bootstrap_err_rx,
            toast_tx,
            toast_rx,
            toast_message: None,
            warp_star_id_input: String::new(),
            egui_ctx: cc.egui_ctx.clone(),
            empire_name_input: String::new(),
            session_empire_name: None,
            my_empire: None,
            my_ships: Vec::new(),
            did_center_camera: false,
            selected_ship_id: None,
            prev_selected_star: None,
            build_planet_index: 0,
            build_slot: 0,
            build_kind: BuildingKind::MiningDepot,
            build_level: 1,
            build_mining_resource_index: 0,
            build_garrison_mode: ShipAttackMode::Defend,
            collect_pickup_kt: {
                let mut v = vec![0.0_f64; MaterialKind::ALL.len()];
                if !v.is_empty() {
                    v[0] = 1.0;
                }
                v
            },
            sell_ship_kt: vec![0.0_f64; MaterialKind::ALL.len()],
            sell_warehouse_kt: vec![0.0_f64; MaterialKind::ALL.len()],
            star_panel_cache_star: None,
            star_panel_cache_time_secs: 0.0,
            star_panel_cached_buildings: Vec::new(),
            star_panel_cached_stock: None,
        };
        app
    }

    /// Connect using the saved token for `empire_name` if present; otherwise a new anonymous identity
    /// (then register with **Start**). Token is written on successful connect.
    fn start_connection(&mut self, empire_name: &str) {
        let host =
            std::env::var("SPACETIMEDB_HOST").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let db = std::env::var("SPACETIMEDB_DB_NAME").unwrap_or_else(|_| "vast".into());
        let token_path = explorer_token_path_for_empire_name(empire_name);
        let saved = std::fs::read_to_string(&token_path)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let egui_ctx = self.egui_ctx.clone();
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
                    .add_query(|q| q.from.star_system_stock())
                    .subscribe();
            })
            .on_connect_error(|_ctx, e| {
                eprintln!("connection error: {e:?}");
            })
            .build();
        match result {
            Ok(c) => {
                self.session_empire_name = Some(empire_name.trim().to_string());
                self.conn = Some(c);
            }
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
        #[cfg(debug_assertions)]
        let _frame_tick_t0 = std::time::Instant::now();
        let _ = conn.frame_tick();
        #[cfg(debug_assertions)]
        {
            let ms = _frame_tick_t0.elapsed().as_secs_f64() * 1000.0;
            if ms > 16.0 {
                eprintln!("[vast explorer] frame_tick took {ms:.1} ms");
            }
        }
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
                if !ship.in_transit {
                    self.camera_x = grid_to_ly(ship.star_x);
                    self.camera_y = grid_to_ly(ship.star_y);
                    self.did_center_camera = true;
                }
            }
        }
    }

    /// Star side panel: `star_system_stock` row every frame (small table); full `building` scan throttled.
    fn refresh_star_panel_cache_if_needed(
        &mut self,
        star_x: i32,
        star_y: i32,
        ctx: &egui::Context,
    ) {
        const BUILDINGS_POLL_SECS: f64 = 0.15;
        let now_secs = ctx.input(|i| i.time);
        let star_key = (star_x, star_y);
        let Some(conn) = &self.conn else {
            self.star_panel_cached_buildings.clear();
            self.star_panel_cached_stock = None;
            self.star_panel_cache_star = None;
            return;
        };
        let sid_loc = star_location_id(star_x, star_y);
        self.star_panel_cached_stock = conn
            .db()
            .star_system_stock()
            .iter()
            .find(|s| s.star_location_id == sid_loc);

        let need_buildings = self.star_panel_cache_star != Some(star_key)
            || now_secs - self.star_panel_cache_time_secs >= BUILDINGS_POLL_SECS;
        if need_buildings {
            self.star_panel_cached_buildings = conn
                .db()
                .building()
                .iter()
                .filter(|b| b.star_x == star_x && b.star_y == star_y)
                .collect();
            self.star_panel_cache_star = Some(star_key);
            self.star_panel_cache_time_secs = now_secs;
        }
    }

    fn show_universe(&mut self, ctx: &egui::Context) {
        let cur_star = self.selected.as_ref().map(|s| (s.x, s.y));
        if self.prev_selected_star != cur_star {
            self.selected_ship_id = None;
            self.prev_selected_star = cur_star;
            self.star_panel_cache_star = None;
        }

        if !self.game_ready() {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::from_rgb(4, 4, 12)))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading(egui::RichText::new("VAST").size(28.0));
                        ui.add_space(12.0);
                        ui.label("Enter your empire name, then Connect.");
                        ui.label(
                            "Same name as before loads that empire; a new name creates a new one.",
                        );
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
                        if self.conn.is_none() {
                            if ui.button("Connect").clicked() {
                                self.bootstrap_error = None;
                                self.connect_error = None;
                                let name = self.empire_name_input.trim().to_string();
                                if name.is_empty() {
                                    self.bootstrap_error = Some("Enter an empire name.".into());
                                } else {
                                    self.start_connection(&name);
                                }
                            }
                        } else if ui.button("Start").clicked() {
                            self.bootstrap_error = None;
                            let name = self
                                .session_empire_name
                                .clone()
                                .unwrap_or_else(|| self.empire_name_input.trim().to_string());
                            if name.is_empty() {
                                self.bootstrap_error = Some("Enter an empire name.".into());
                            } else if let Some(conn) = &self.conn {
                                let tx = self.bootstrap_err_tx.clone();
                                if let Err(e) = conn.reducers().register_empire_then(name, move |ctx, res| {
                                    match res {
                                        Ok(Ok(())) => {
                                            let tx2 = tx.clone();
                                            let _ = ctx.reducers().spawn_starter_ship_then(
                                                move |_ctx2, res2| match res2 {
                                                    Ok(Ok(())) => {}
                                                    Ok(Err(msg)) => {
                                                        let _ = tx2.send(format!("Spawn: {msg}"));
                                                    }
                                                    Err(err) => {
                                                        let _ = tx2.send(format!(
                                                            "Spawn failed: {err:?}"
                                                        ));
                                                    }
                                                },
                                            );
                                        }
                                        Ok(Err(msg)) => {
                                            let _ = tx.send(format!("Register: {msg}"));
                                        }
                                        Err(err) => {
                                            let _ = tx.send(format!("Register failed: {err:?}"));
                                        }
                                    }
                                }) {
                                    self.bootstrap_error = Some(format!(
                                        "Could not send register_empire: {e:?}"
                                    ));
                                }
                            } else {
                                self.bootstrap_error =
                                    Some("Not connected to SpacetimeDB.".into());
                            }
                        }
                        if self.conn.is_some() && !self.game_ready() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Connected — Start registers your empire and ship (first time only).")
                                    .small()
                                    .weak(),
                            );
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
                .min_width(300.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("vast_star_sidepanel")
                        .show(ui, |ui| {
                    let (star_x, star_y) = {
                        let s = self.selected.as_ref().unwrap();
                        (s.x, s.y)
                    };
                    self.refresh_star_panel_cache_if_needed(star_x, star_y, ctx);
                    let sys = self.selected.as_ref().unwrap();
                    ui.add_space(6.0);
                    ui.heading(format!(
                        "({:.1}, {:.1}) ly",
                        grid_to_ly(sys.x),
                        grid_to_ly(sys.y)
                    ));
                    let sid = star_display_id(sys.x, sys.y);
                    ui.horizontal(|ui| {
                        ui.label("Star ID:");
                        ui.monospace(&sid);
                        if ui.button("Copy").clicked() {
                            ctx.copy_text(sid.clone());
                        }
                        if ui.button("Use for warp").clicked() {
                            self.warp_star_id_input = sid.clone();
                        }
                    });
                    if let Some(ref t) = self.toast_message {
                        ui.colored_label(Color32::from_rgb(200, 220, 255), t.as_str());
                    }
                    ui.label(format!("Type:  {:?}", sys.star_type));
                    ui.label(format!("Temp:  {:.0} K", sys.star_type.temperature_k()));
                    ui.label(format!("Size:  {:.4} R☉", sys.star_size_solar_radii));
                    ui.separator();
                    ui.heading("Your ships");
                    let mut ships_here: Vec<&Ship> = self
                        .my_ships
                        .iter()
                        .filter(|s| my_ship_at_star_coords(s, sys.x, sys.y))
                        .collect();
                    ships_here.sort_by_key(|s| s.id);
                    if ships_here.is_empty() {
                        ui.label(
                            egui::RichText::new("None at this system.")
                                .italics()
                                .weak(),
                        );
                    } else {
                        for ship in ships_here {
                                    let is_sel = self.selected_ship_id == Some(ship.id);
                                    egui::Frame::group(ui.style())
                                        .stroke(egui::Stroke::new(
                                            if is_sel { 1.5 } else { 1.0 },
                                            if is_sel {
                                                Color32::from_rgb(120, 200, 255)
                                            } else {
                                                Color32::from_gray(60)
                                            },
                                        ))
                                        .inner_margin(egui::Margin::same(8.0))
                                        .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .selectable_label(
                                                    is_sel,
                                                    egui::RichText::new(format!(
                                                        "Ship #{}",
                                                        ship.id
                                                    ))
                                                    .strong(),
                                                )
                                                .clicked()
                                            {
                                                self.selected_ship_id = Some(ship.id);
                                            }
                                        });
                                        let st = &ship.stats;
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Size {} kt  ·  {:.2} ly/s  ·  Def {}  ·  Atk {}",
                                                st.size_kt,
                                                st.speed_lys,
                                                st.defense,
                                                st.attack
                                            ))
                                            .monospace()
                                            .size(11.0),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Battery {} ly  ·  Radar {} ly",
                                                st.battery_ly, st.radar_ly
                                            ))
                                            .monospace()
                                            .size(11.0),
                                        );
                                        let mode_s = match ship.attack_mode {
                                            ShipAttackMode::Defend => "Defend",
                                            ShipAttackMode::StrikeFirst => "Strike first",
                                        };
                                        ui.label(format!("Attack mode: {mode_s}"));
                                        if !ship.in_transit {
                                            ui.label("Location: in star system");
                                            if let Some(j) = jump_ready_line(ship) {
                                                ui.label(
                                                    egui::RichText::new(j)
                                                        .small()
                                                        .color(Color32::from_rgb(180, 200, 140)),
                                                );
                                            }
                                        } else {
                                            ui.label(format!(
                                                "In transit: ({:.1},{:.1}) → ({:.1},{:.1}) ly",
                                                grid_to_ly(ship.transit_from_x),
                                                grid_to_ly(ship.transit_from_y),
                                                grid_to_ly(ship.transit_to_x),
                                                grid_to_ly(ship.transit_to_y),
                                            ));
                                        }
                                        if !ship.cargo.is_empty() {
                                            ui.label("Cargo:");
                                            for m in &ship.cargo {
                                                ui.label(format!(
                                                    "  · {}",
                                                    format_material_line(m)
                                                ));
                                            }
                                        } else {
                                            ui.label(
                                                egui::RichText::new("Cargo: empty")
                                                    .weak()
                                                    .small(),
                                            );
                                        }
                                        ui.horizontal(|ui| {
                                            if ui.button("Center on map").clicked() {
                                                self.selected_ship_id = Some(ship.id);
                                                if !ship.in_transit {
                                                    self.camera_x = grid_to_ly(ship.star_x);
                                                    self.camera_y = grid_to_ly(ship.star_y);
                                                } else {
                                                    self.camera_x = (grid_to_ly(ship.transit_from_x)
                                                        + grid_to_ly(ship.transit_to_x))
                                                        * 0.5;
                                                    self.camera_y = (grid_to_ly(ship.transit_from_y)
                                                        + grid_to_ly(ship.transit_to_y))
                                                        * 0.5;
                                                }
                                            }
                                        });
                                        });
                        }
                    }
                    ui.separator();
                    ui.heading("Warp");
                    ui.label("Select a ship in the list above, paste a destination Star ID, then order warp.");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.warp_star_id_input)
                            .desired_width(f32::INFINITY)
                            .hint_text("e.g. AA-1000-1000 or !500000,-300000"),
                    );
                    ui.horizontal(|ui| {
                        let can_warp = self.selected_ship_id.is_some() && self.conn.is_some();
                        if ui
                            .add_enabled(can_warp, egui::Button::new("Warp to this ID"))
                            .clicked()
                        {
                            self.toast_message = None;
                            if let Some(ship_id) = self.selected_ship_id {
                                if let Some((dx, dy)) = parse_star_id(self.warp_star_id_input.trim())
                                {
                                    if let Some(conn) = &self.conn {
                                        let tx = self.toast_tx.clone();
                                        if let Err(e) =
                                            conn.reducers().order_warp_then(ship_id, dx, dy, {
                                                move |_, res| {
                                                    let msg = match res {
                                                        Ok(Ok(())) => {
                                                            "Warp ordered.".to_string()
                                                        }
                                                        Ok(Err(err)) => format!("Warp: {err}"),
                                                        Err(err) => {
                                                            format!("Warp failed: {err:?}")
                                                        }
                                                    };
                                                    let _ = tx.send(msg);
                                                }
                                            })
                                        {
                                            self.toast_message =
                                                Some(format!("Could not send warp: {e:?}"));
                                        }
                                    }
                                } else {
                                    self.toast_message = Some(
                                        "Invalid star ID — check format.".to_string(),
                                    );
                                }
                            }
                        }
                    });
                    ui.separator();
                    ui.label(format!("{} planet(s)", sys.planets.len()));

                    let buildings_at_star: &[Building] = &self.star_panel_cached_buildings;
                    let me = self.conn.as_ref().and_then(|c| c.try_identity());

                            for p in &sys.planets {
                                ui.add_space(4.0);
                                let type_label = match p.planet_type {
                                    PlanetType::Solid => "Solid",
                                    PlanetType::Ocean => "Ocean",
                                    PlanetType::Gas => "Gas",
                                };
                                ui.label(
                                    egui::RichText::new(format!("▸ Planet {}", p.index + 1))
                                        .strong(),
                                );
                                ui.label(format!("  Type:     {}", type_label));
                                ui.label(format!("  Distance: {:.2} AU", p.distance_au));
                                ui.label(format!("  Temp:     {:.0} K", p.temperature_k));
                                ui.label(format!("  Slots:    {}", p.size));
                                ui.label(format!("  Richness: {:.2}×", p.richness));
                                for res in &p.resources {
                                    ui.label(format!(
                                        "  {}:  {:.2}×",
                                        res.name(),
                                        res.multiplier()
                                    ));
                                }
                                let on_planet: Vec<&Building> = buildings_at_star
                                    .iter()
                                    .filter(|b| b.planet_index == p.index)
                                    .collect();
                                if !on_planet.is_empty() {
                                    ui.label("  Structures:");
                                    for b in on_planet {
                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "    {:?}  L{}  slot {}",
                                                b.kind, b.level, b.slot_index
                                            ));
                                            if let Some(o) = b.owner {
                                                let short = format!("{:?}", o);
                                                let short = short.chars().take(12).collect::<String>();
                                                ui.label(egui::RichText::new(short).small());
                                            }
                                            if b.kind != BuildingKind::SalesDepot
                                                && me.is_some()
                                                && b.level
                                                    < building_economy::MAX_BUILDING_LEVEL as u32
                                            {
                                                if ui.small_button("Upgrade +1").clicked() {
                                                    if let Some(conn) = &self.conn {
                                                        let tx = self.toast_tx.clone();
                                                        let id = b.id;
                                                        let nl = b.level + 1;
                                                        if let Err(e) =
                                                            conn.reducers().upgrade_building_then(
                                                                id,
                                                                nl,
                                                                move |_, res| {
                                                                    let msg = match res {
                                                                        Ok(Ok(())) => {
                                                                            "Upgraded.".into()
                                                                        }
                                                                        Ok(Err(err)) => {
                                                                            format!("Upgrade: {err}")
                                                                        }
                                                                        Err(err) => format!(
                                                                            "Upgrade failed: {err:?}"
                                                                        ),
                                                                    };
                                                                    let _ = tx.send(msg);
                                                                },
                                                            )
                                                        {
                                                            self.toast_message = Some(format!(
                                                                "Could not send upgrade: {e:?}"
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                                if let Some(id) = me {
                                    if planet_has_enemy_garrison(&buildings_at_star, p.index, id) {
                                        ui.colored_label(
                                            Color32::from_rgb(255, 120, 120),
                                            "  Enemy garrison — you cannot build or upgrade on this planet.",
                                        );
                                    }
                                }
                            }

                    ui.separator();
                    ui.heading("Star economy");
                    {
                        let cap_struct = building_economy::capacity_kt_at_star(&buildings_at_star);
                        let rates =
                            building_economy::mining_rates_kt_s(sys, &buildings_at_star);
                        let mp = building_economy::military_power_at_star(&buildings_at_star);
                        let ship_slots =
                            building_economy::ship_build_slots_at_star(&buildings_at_star);
                        ui.label(format!(
                            "Warehouse capacity (from structures): {:.2} kt",
                            cap_struct
                        ));
                        let rate_parts: Vec<String> = MaterialKind::ALL
                            .iter()
                            .map(|&k| {
                                let r = rates.get(&k).copied().unwrap_or(0.0);
                                let name = match k {
                                    MaterialKind::Iron => "Iron",
                                    MaterialKind::Helium => "Helium",
                                };
                                format!("{} {:.4}", name, r)
                            })
                            .collect();
                        ui.label(format!(
                            "Mining rates (kt/s): {}",
                            rate_parts.join(", ")
                        ));
                        ui.label(format!("Military power (garrisons): {:.1}", mp));
                        ui.label(format!("Concurrent ship build slots: {}", ship_slots));
                        if let Some(st) = self.star_panel_cached_stock.as_ref() {
                            let now = Timestamp::now();
                            let stored_lines: Vec<String> = st
                                .settled
                                .iter()
                                .map(|m| {
                                    let (name, q) = match m {
                                        Material::Iron(q) => ("Iron", *q),
                                        Material::Helium(q) => ("Helium", *q),
                                    };
                                    format!("{} {:.2}", name, q)
                                })
                                .collect();
                            let stored_txt = if stored_lines.is_empty() {
                                "(empty)".to_string()
                            } else {
                                stored_lines.join(", ")
                            };
                            let tot =
                                building_economy::theoretical_materials(
                                    now,
                                    st.last_settled_at,
                                    &st.settled,
                                    st.capacity_kt,
                                    &rates,
                                );
                            let avail_lines: Vec<String> = tot
                                .iter()
                                .map(|m| {
                                    format!("{} {:.2}", m.name(), m.multiplier())
                                })
                                .collect();
                            let avail_txt = if avail_lines.is_empty() {
                                "0.00 total".to_string()
                            } else {
                                avail_lines.join(", ")
                            };
                            ui.label(format!(
                                "Stored (settled): {} | Row capacity {:.2} kt | total stored {:.2} kt",
                                stored_txt,
                                st.capacity_kt,
                                building_economy::total_settled_kt_bindings(&st.settled),
                            ));
                            ui.label(format!(
                                "Available (with accrual): {}",
                                avail_txt
                            ));
                        } else {
                            ui.label(
                                egui::RichText::new(
                                    "No stock row yet — place a building to run settlement.",
                                )
                                .weak(),
                            );
                        }
                    }

                    let has_sales_depot = buildings_at_star
                        .iter()
                        .any(|b| b.kind == BuildingKind::SalesDepot);
                    if has_sales_depot && self.game_ready() {
                        if let Some(identity) = me {
                            ui.separator();
                            ui.heading("Sales (Sales Depot)");
                            ui.label(format!(
                                "Government sink: {} cr/kt Iron, {} cr/kt Helium",
                                BASELINE_CREDITS_PER_KT_IRON, BASELINE_CREDITS_PER_KT_HELIUM
                            ));
                            let ships_docked: Vec<&Ship> = self
                                .my_ships
                                .iter()
                                .filter(|s| {
                                    s.owner == identity
                                        && !s.in_transit
                                        && s.star_x == sys.x
                                        && s.star_y == sys.y
                                })
                                .collect();
                            if ships_docked.is_empty() {
                                ui.label(
                                    egui::RichText::new("Station a ship to sell cargo.")
                                        .weak(),
                                );
                            } else {
                                let ship_for_sale = self
                                    .selected_ship_id
                                    .filter(|sid| ships_docked.iter().any(|s| s.id == *sid))
                                    .or_else(|| ships_docked.first().map(|s| s.id));
                                if let Some(ship_id) = ship_for_sale {
                                    if let Some(ship) =
                                        ships_docked.iter().copied().find(|s| s.id == ship_id)
                                    {
                                        let cargo_u: Vec<UniverseMaterial> = ship
                                            .cargo
                                            .iter()
                                            .map(building_economy::binding_to_universe)
                                            .collect();
                                        ui.label("Sell from ship cargo");
                                        for (i, &kind) in MaterialKind::ALL.iter().enumerate() {
                                            let max_k = get_amount(&cargo_u, kind);
                                            let hi = max_k.max(1e-9);
                                            let label = match kind {
                                                MaterialKind::Iron => "Iron kt",
                                                MaterialKind::Helium => "Helium kt",
                                            };
                                            if let Some(amt) = self.sell_ship_kt.get_mut(i) {
                                                if *amt > max_k {
                                                    *amt = max_k;
                                                }
                                                ui.add(
                                                    egui::Slider::new(amt, 0.0..=hi).text(label),
                                                );
                                            }
                                        }
                                        let pickup_s = building_economy::pickup_vec_for_reducer(
                                            &self.sell_ship_kt,
                                        );
                                        let partial_u: Vec<UniverseMaterial> = pickup_s
                                            .iter()
                                            .map(building_economy::binding_to_universe)
                                            .collect();
                                        let est_ship = credits_for_materials_sale(&partial_u);
                                        let est_all_ship = credits_for_materials_sale(&cargo_u);
                                        ui.label(format!(
                                            "Estimated (sliders): {} cr | Sell all: {} cr",
                                            format_large_number(est_ship),
                                            format_large_number(est_all_ship)
                                        ));
                                        ui.horizontal(|ui| {
                                            if ui
                                                .button("Sell ship cargo (from sliders)")
                                                .clicked()
                                            {
                                                if let Some(conn) = &self.conn {
                                                    let tx = self.toast_tx.clone();
                                                    if let Err(e) = conn
                                                        .reducers()
                                                        .sell_ship_cargo_then(
                                                            ship_id,
                                                            pickup_s,
                                                            move |_, res| {
                                                                let msg = match res {
                                                                    Ok(Ok(())) => {
                                                                        "Sold (ship).".into()
                                                                    }
                                                                    Ok(Err(err)) => {
                                                                        format!("Sell ship: {err}")
                                                                    }
                                                                    Err(err) => format!(
                                                                        "Sell ship failed: {err:?}"
                                                                    ),
                                                                };
                                                                let _ = tx.send(msg);
                                                            },
                                                        )
                                                    {
                                                        self.toast_message = Some(format!(
                                                            "Could not send sell_ship_cargo: {e:?}"
                                                        ));
                                                    }
                                                }
                                            }
                                            if ui.button("Sell all ship cargo").clicked() {
                                                if let Some(conn) = &self.conn {
                                                    let tx = self.toast_tx.clone();
                                                    if let Err(e) = conn
                                                        .reducers()
                                                        .sell_ship_cargo_then(
                                                            ship_id,
                                                            vec![],
                                                            move |_, res| {
                                                                let msg = match res {
                                                                    Ok(Ok(())) => {
                                                                        "Sold all (ship).".into()
                                                                    }
                                                                    Ok(Err(err)) => {
                                                                        format!("Sell ship: {err}")
                                                                    }
                                                                    Err(err) => format!(
                                                                        "Sell ship failed: {err:?}"
                                                                    ),
                                                                };
                                                                let _ = tx.send(msg);
                                                            },
                                                        )
                                                    {
                                                        self.toast_message = Some(format!(
                                                            "Could not send sell_ship_cargo: {e:?}"
                                                        ));
                                                    }
                                                }
                                            }
                                        });

                                        ui.separator();
                                        ui.label("Sell from star warehouse");
                                        if let Some(st) = self.star_panel_cached_stock.as_ref() {
                                            let now = Timestamp::now();
                                            let rates = building_economy::mining_rates_kt_s(
                                                sys,
                                                &buildings_at_star,
                                            );
                                            let tot = building_economy::theoretical_materials(
                                                now,
                                                st.last_settled_at,
                                                &st.settled,
                                                st.capacity_kt,
                                                &rates,
                                            );
                                            for (i, &kind) in MaterialKind::ALL.iter().enumerate()
                                            {
                                                let max_k = get_amount(&tot, kind);
                                                let hi = max_k.max(1e-9);
                                                let label = match kind {
                                                    MaterialKind::Iron => "Iron kt",
                                                    MaterialKind::Helium => "Helium kt",
                                                };
                                                if let Some(amt) =
                                                    self.sell_warehouse_kt.get_mut(i)
                                                {
                                                    if *amt > max_k {
                                                        *amt = max_k;
                                                    }
                                                    ui.add(
                                                        egui::Slider::new(amt, 0.0..=hi)
                                                            .text(label),
                                                    );
                                                }
                                            }
                                            let pickup_w = building_economy::pickup_vec_for_reducer(
                                                &self.sell_warehouse_kt,
                                            );
                                            let partial_w: Vec<UniverseMaterial> = pickup_w
                                                .iter()
                                                .map(building_economy::binding_to_universe)
                                                .collect();
                                            let est_wh = credits_for_materials_sale(&partial_w);
                                            let est_all_wh = credits_for_materials_sale(&tot);
                                            ui.label(format!(
                                                "Estimated (sliders): {} cr | Sell all: {} cr",
                                                format_large_number(est_wh),
                                                format_large_number(est_all_wh)
                                            ));
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .button("Sell warehouse (from sliders)")
                                                    .clicked()
                                                {
                                                    if let Some(conn) = &self.conn {
                                                        let tx = self.toast_tx.clone();
                                                        if let Err(e) = conn
                                                            .reducers()
                                                            .sell_star_warehouse_then(
                                                                ship_id,
                                                                pickup_w,
                                                                move |_, res| {
                                                                    let msg = match res {
                                                                        Ok(Ok(())) => {
                                                                            "Sold (warehouse)."
                                                                                .into()
                                                                        }
                                                                        Ok(Err(err)) => {
                                                                            format!(
                                                                                "Sell warehouse: {err}"
                                                                            )
                                                                        }
                                                                        Err(err) => format!(
                                                                            "Sell warehouse failed: {err:?}"
                                                                        ),
                                                                    };
                                                                    let _ = tx.send(msg);
                                                                },
                                                            )
                                                        {
                                                            self.toast_message = Some(format!(
                                                                "Could not send sell_star_warehouse: {e:?}"
                                                            ));
                                                        }
                                                    }
                                                }
                                                if ui.button("Sell all warehouse stock").clicked()
                                                {
                                                    if let Some(conn) = &self.conn {
                                                        let tx = self.toast_tx.clone();
                                                        if let Err(e) = conn
                                                            .reducers()
                                                            .sell_star_warehouse_then(
                                                                ship_id,
                                                                vec![],
                                                                move |_, res| {
                                                                    let msg = match res {
                                                                        Ok(Ok(())) => {
                                                                            "Sold all (warehouse)."
                                                                                .into()
                                                                        }
                                                                        Ok(Err(err)) => {
                                                                            format!(
                                                                                "Sell warehouse: {err}"
                                                                            )
                                                                        }
                                                                        Err(err) => format!(
                                                                            "Sell warehouse failed: {err:?}"
                                                                        ),
                                                                    };
                                                                    let _ = tx.send(msg);
                                                                },
                                                            )
                                                        {
                                                            self.toast_message = Some(format!(
                                                                "Could not send sell_star_warehouse: {e:?}"
                                                            ));
                                                        }
                                                    }
                                                }
                                            });
                                        } else {
                                            ui.label(
                                                egui::RichText::new(
                                                    "No warehouse stock row — settlement has not run here.",
                                                )
                                                .weak(),
                                            );
                                        }
                                    }
                                } else {
                                    ui.label("Select a ship at this star in the list above.");
                                }
                            }
                        }
                    }

                    if self.game_ready() {
                        if let Some(identity) = me {
                            ui.separator();
                            ui.heading("Collect from warehouse");
                            let ships_docked: Vec<&Ship> = self
                                .my_ships
                                .iter()
                                .filter(|s| {
                                    s.owner == identity
                                        && !s.in_transit
                                        && s.star_x == sys.x
                                        && s.star_y == sys.y
                                })
                                .collect();
                            if ships_docked.is_empty() {
                                ui.label(
                                    egui::RichText::new("Station a ship here to load cargo.")
                                        .weak(),
                                );
                            } else {
                                for (i, &kind) in MaterialKind::ALL.iter().enumerate() {
                                    let label = match kind {
                                        MaterialKind::Iron => "Iron kt to load",
                                        MaterialKind::Helium => "Helium kt to load",
                                    };
                                    if let Some(amt) = self.collect_pickup_kt.get_mut(i) {
                                        ui.add(egui::Slider::new(
                                            amt,
                                            0.0..=1_000_000.0,
                                        )
                                        .text(label));
                                    }
                                }
                                let ship_for_collect = self
                                    .selected_ship_id
                                    .filter(|sid| ships_docked.iter().any(|s| s.id == *sid))
                                    .or_else(|| ships_docked.first().map(|s| s.id));
                                if let Some(ship_id) = ship_for_collect {
                                    if ui.button("Collect onto ship (uses ship selected above)")
                                        .clicked()
                                    {
                                        if let Some(conn) = &self.conn {
                                            let tx = self.toast_tx.clone();
                                            let pickup = building_economy::pickup_vec_for_reducer(
                                                &self.collect_pickup_kt,
                                            );
                                            if let Err(e) =
                                                conn.reducers().collect_star_resources_then(
                                                    ship_id,
                                                    pickup,
                                                    move |_, res| {
                                                        let msg = match res {
                                                            Ok(Ok(())) => "Collected.".into(),
                                                            Ok(Err(err)) => {
                                                                format!("Collect: {err}")
                                                            }
                                                            Err(err) => {
                                                                format!("Collect failed: {err:?}")
                                                            }
                                                        };
                                                        let _ = tx.send(msg);
                                                    },
                                                )
                                            {
                                                self.toast_message =
                                                    Some(format!("Could not send collect: {e:?}"));
                                            }
                                        }
                                    }
                                } else {
                                    ui.label("Select a ship at this star in the list above.");
                                }
                            }
                        }
                    }

                    ui.separator();
                    ui.heading("Construction");
                    if !self.game_ready() {
                        ui.label(
                            egui::RichText::new("Register and spawn a ship to place buildings.")
                                .weak(),
                        );
                    } else if let (Some(conn), Some(identity)) = (&self.conn, me) {
                        let max_kt =
                            max_stationed_kt(&self.my_ships, identity, sys.x, sys.y);
                        ui.label(format!(
                            "Your largest ship at this star: {} kt (stationed only)",
                            max_kt
                        ));
                        egui::ComboBox::from_id_salt("vast_bp")
                            .selected_text(format!("Planet {}", self.build_planet_index + 1))
                            .show_ui(ui, |ui| {
                                for p in &sys.planets {
                                    ui.selectable_value(
                                        &mut self.build_planet_index,
                                        p.index,
                                        format!("Planet {} ({} slots)", p.index + 1, p.size),
                                    );
                                }
                            });
                        if let Some(planet) =
                            sys.planets.iter().find(|p| p.index == self.build_planet_index)
                        {
                            let max_slot = planet.size.saturating_sub(1) as u32;
                            if self.build_slot > max_slot {
                                self.build_slot = max_slot;
                            }
                            if planet_has_enemy_garrison(
                                &buildings_at_star,
                                planet.index,
                                identity,
                            ) {
                                ui.colored_label(
                                    Color32::from_rgb(255, 120, 120),
                                    "Enemy garrison blocks construction on this planet.",
                                );
                            } else {
                                ui.add(
                                    egui::Slider::new(&mut self.build_slot, 0..=max_slot)
                                        .text("Slot index"),
                                );
                                egui::ComboBox::from_id_salt("vast_bk")
                                    .selected_text(format!("{:?}", self.build_kind))
                                    .show_ui(ui, |ui| {
                                        for k in [
                                            BuildingKind::MiningDepot,
                                            BuildingKind::Warehouse,
                                            BuildingKind::MilitaryGarrison,
                                            BuildingKind::SalesDepot,
                                            BuildingKind::ShipDepot,
                                        ] {
                                            ui.selectable_value(
                                                &mut self.build_kind,
                                                k,
                                                format!("{k:?}"),
                                            );
                                        }
                                    });
                                if self.build_kind == BuildingKind::SalesDepot {
                                    let n = count_my_sales_depots(conn, identity);
                                    let c = building_economy::sales_depot_next_cost(n);
                                    ui.label(format!(
                                        "Next Sales Depot cost: {} cr (you already own {})",
                                        format_large_number(c),
                                        n
                                    ));
                                } else {
                                    ui.add(egui::Slider::new(
                                        &mut self.build_level,
                                        1..=building_economy::MAX_BUILDING_LEVEL as u32,
                                    )
                                    .text("Level"));
                                    let need = building_economy::min_ship_kt_for_level(
                                        self.build_level,
                                    );
                                    ui.label(format!(
                                        "Requires stationed ship >= {} kt",
                                        need
                                    ));
                                    if let Some(cost) = building_economy::credits_for_leveled_place(
                                        self.build_kind,
                                        self.build_level,
                                    ) {
                                        ui.label(format!(
                                            "Cost: {} cr",
                                            format_large_number(cost)
                                        ));
                                    }
                                }
                                if self.build_kind == BuildingKind::MiningDepot
                                    && !planet.resources.is_empty()
                                {
                                    let ri_max =
                                        (planet.resources.len() - 1).min(u8::MAX as usize) as u8;
                                    ui.add(
                                        egui::Slider::new(
                                            &mut self.build_mining_resource_index,
                                            0..=ri_max,
                                        )
                                        .text("Resource index"),
                                    );
                                }
                                if self.build_kind == BuildingKind::MilitaryGarrison {
                                    egui::ComboBox::from_id_salt("vast_bg")
                                        .selected_text(format!("{:?}", self.build_garrison_mode))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.build_garrison_mode,
                                                ShipAttackMode::Defend,
                                                "Defend",
                                            );
                                            ui.selectable_value(
                                                &mut self.build_garrison_mode,
                                                ShipAttackMode::StrikeFirst,
                                                "StrikeFirst",
                                            );
                                        });
                                }
                                if ui.button("Place building").clicked() {
                                    self.toast_message = None;
                                    let tx = self.toast_tx.clone();
                                    let sx = sys.x;
                                    let sy = sys.y;
                                    let pi = self.build_planet_index;
                                    let sl = self.build_slot as u8;
                                    let k = self.build_kind;
                                    let lvl = if k == BuildingKind::SalesDepot {
                                        1
                                    } else {
                                        self.build_level
                                    };
                                    let mi = if k == BuildingKind::MiningDepot {
                                        Some(self.build_mining_resource_index)
                                    } else {
                                        None
                                    };
                                    let ga = if k == BuildingKind::MilitaryGarrison {
                                        Some(self.build_garrison_mode)
                                    } else {
                                        None
                                    };
                                    if let Err(e) = conn.reducers().place_building_then(
                                        sx,
                                        sy,
                                        pi,
                                        sl,
                                        k,
                                        lvl,
                                        mi,
                                        ga,
                                        move |_, res| {
                                            let msg = match res {
                                                Ok(Ok(())) => "Building placed.".to_string(),
                                                Ok(Err(err)) => format!("Build: {err}"),
                                                Err(err) => format!("Build failed: {err:?}"),
                                            };
                                            let _ = tx.send(msg);
                                        },
                                    ) {
                                        self.toast_message =
                                            Some(format!("Could not send build: {e:?}"));
                                    }
                                }
                            }
                        }
                    } else {
                        ui.label("Connect to place buildings.");
                    }
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() {
                        self.selected = None;
                    }
                        });
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

                // Scroll to zoom only when the pointer is over the map (not side/top panels).
                let map_scroll = if response.hovered() {
                    ctx.input(|i| i.smooth_scroll_delta.y)
                } else {
                    0.0
                };
                if map_scroll != 0.0 {
                    let factor = (map_scroll as f64 * 0.004).exp();
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

                // Owned ships: marker at star; dim line when in transit
                let ship_marker_r = (self.zoom * 0.12).clamp(3.0, 14.0) as f32;
                let ship_accent = Color32::from_rgb(0, 220, 255);
                let ship_line = Color32::from_rgba_unmultiplied(0, 200, 255, 120);
                for ship in &self.my_ships {
                    if !ship.in_transit {
                        let sp = to_screen(ship.star_x, ship.star_y);
                        painter.circle_stroke(
                            sp,
                            ship_marker_r,
                            egui::Stroke::new(1.8, ship_accent),
                        );
                        painter.circle_filled(sp, ship_marker_r * 0.35, ship_accent);
                    } else {
                        let a = to_screen(ship.transit_from_x, ship.transit_from_y);
                        let b = to_screen(ship.transit_to_x, ship.transit_to_y);
                        painter.line_segment(
                            [a, b],
                            egui::Stroke::new(1.0, ship_line),
                        );
                        let now = Timestamp::now();
                        let n = now.to_micros_since_unix_epoch();
                        let d0 = ship.transit_depart_at.to_micros_since_unix_epoch();
                        let d1 = ship.transit_arrive_at.to_micros_since_unix_epoch();
                        let frac = if d1 <= d0 {
                            1.0_f32
                        } else {
                            (((n - d0) as f64 / (d1 - d0) as f64) as f32).clamp(0.0, 1.0)
                        };
                        let p = a.lerp(b, frac);
                        painter.circle_stroke(
                            p,
                            ship_marker_r,
                            egui::Stroke::new(1.8, ship_accent),
                        );
                        painter.circle_filled(p, ship_marker_r * 0.35, ship_accent);
                    }
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
                if !self.my_ships.is_empty() {
                    painter.text(
                        hud_origin + Vec2::new(0.0, hud_y),
                        egui::Align2::LEFT_TOP,
                        format!("Ships: {}", self.my_ships.len()),
                        egui::FontId::monospace(11.0),
                        Color32::from_rgb(160, 240, 255),
                    );
                    hud_y += 14.0;
                    for s in self.my_ships.iter().take(5) {
                        let line = if !s.in_transit {
                            format!(
                                "  #{}  ({:.1}, {:.1}) ly",
                                s.id,
                                grid_to_ly(s.star_x),
                                grid_to_ly(s.star_y),
                            )
                        } else {
                            format!(
                                "  #{}  transit  ({:.1},{:.1})→({:.1},{:.1}) ly",
                                s.id,
                                grid_to_ly(s.transit_from_x),
                                grid_to_ly(s.transit_from_y),
                                grid_to_ly(s.transit_to_x),
                                grid_to_ly(s.transit_to_y),
                            )
                        };
                        painter.text(
                            hud_origin + Vec2::new(0.0, hud_y),
                            egui::Align2::LEFT_TOP,
                            line,
                            egui::FontId::monospace(10.0),
                            Color32::from_rgb(140, 200, 220),
                        );
                        hud_y += 13.0;
                    }
                    if self.my_ships.len() > 5 {
                        painter.text(
                            hud_origin + Vec2::new(0.0, hud_y),
                            egui::Align2::LEFT_TOP,
                            format!("  … +{} more", self.my_ships.len() - 5),
                            egui::FontId::monospace(10.0),
                            Color32::from_rgb(140, 200, 220),
                        );
                        hud_y += 13.0;
                    }
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
                let any_scroll = ctx.input(|i| i.smooth_scroll_delta.y != 0.0);
                if response.dragged()
                    || any_scroll
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
