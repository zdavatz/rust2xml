//! egui-based desktop UI for rust2xml.
//!
//! Two big buttons (`-e (Extended)` and `-b (Firstbase)`) trigger a
//! download/extract/SQLite-write pipeline in a worker thread, with a
//! live status line and a tabbed table viewer for the resulting DB.

use crate::cli::Cli;
use crate::options::{Options, PriceSource};
use crate::sqlite_export;
use crate::util;
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::thread;

/// Worker → UI events.
enum Event {
    Log(String),
    Progress(f32, String),
    Done(PathBuf),
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Extended,
    Firstbase,
}

impl RunMode {
    fn flag(&self) -> char {
        match self {
            RunMode::Extended => 'e',
            RunMode::Firstbase => 'b',
        }
    }
    fn label(&self) -> &'static str {
        match self {
            RunMode::Extended => "-e (Extended)",
            RunMode::Firstbase => "-b (Firstbase)",
        }
    }
    fn apply_to(&self, opts: &mut Options) {
        // GUI always runs against the FOPH/BAG FHIR NDJSON feed —
        // single source of truth for the new EPL data model.
        opts.fhir = true;
        match self {
            RunMode::Extended => {
                opts.extended = true;
                opts.nonpharma = true;
                opts.calc = true;
                opts.price = Some(PriceSource::ZurRose);
            }
            RunMode::Firstbase => {
                opts.firstbase = true;
                opts.nonpharma = true;
                opts.calc = true;
            }
        }
    }
}

#[derive(Default)]
struct TableData {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// One lowercased haystack per row (all column values joined with
    /// `\u{1f}`).  Built once at load time so the search box can filter
    /// 180 K-row tables on every keystroke without re-lowercasing.
    haystacks: Vec<String>,
}

/// Embedded icon PNG used both for the window header and for the
/// clickable mail-link badge in the top-right of the controls panel.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

pub struct GuiApp {
    rx: Option<Receiver<Event>>,
    running_mode: Option<RunMode>,
    log: Vec<String>,
    progress: f32,
    progress_label: String,
    sqlite_path: Option<PathBuf>,
    table_names: Vec<String>,
    selected_table: Option<String>,
    table_cache: Option<TableData>,
    last_error: Option<String>,
    icon_texture: Option<egui::TextureHandle>,
    /// Substring search over every column of the selected tab.  Empty
    /// string means "show all rows".
    search_query: String,
    /// Snapshot of `search_query` at the time `filtered_rows` was last
    /// rebuilt.  We diff every frame so the filter never drifts even if
    /// `Response::changed()` misses an event.
    last_filter_query: String,
    /// Indices into `table_cache.rows` that the current `search_query`
    /// matches.  Recomputed whenever query or tab changes.
    filtered_rows: Vec<usize>,
}

impl Default for GuiApp {
    fn default() -> Self {
        Self {
            rx: None,
            running_mode: None,
            log: Vec::new(),
            progress: 0.0,
            progress_label: String::new(),
            sqlite_path: None,
            table_names: Vec::new(),
            selected_table: None,
            table_cache: None,
            last_error: None,
            icon_texture: None,
            search_query: String::new(),
            last_filter_query: String::new(),
            filtered_rows: Vec::new(),
        }
    }
}

impl GuiApp {
    /// Lazily decode the embedded icon into an egui texture on the
    /// first frame.  Cached for subsequent frames.
    fn ensure_icon_texture(&mut self, ctx: &egui::Context) {
        if self.icon_texture.is_some() {
            return;
        }
        let img = match image::load_from_memory(ICON_PNG) {
            Ok(i) => i,
            Err(_) => return,
        };
        let resized = img.resize_exact(64, 64, image::imageops::FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let (w, h) = rgba.dimensions();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        );
        let texture = ctx.load_texture("app-icon", color_image, egui::TextureOptions::LINEAR);
        self.icon_texture = Some(texture);
    }
}

impl GuiApp {
    fn start_run(&mut self, mode: RunMode, ctx: egui::Context) {
        if self.running_mode.is_some() {
            return;
        }
        let (tx, rx): (Sender<Event>, Receiver<Event>) = unbounded();
        self.rx = Some(rx);
        self.running_mode = Some(mode);
        self.log.clear();
        self.progress = 0.0;
        self.progress_label.clear();
        self.last_error = None;
        self.table_cache = None;
        self.selected_table = None;
        self.table_names.clear();

        let now = chrono::Local::now();
        let filename = sqlite_export::timestamped_filename(mode.flag(), now);
        // Anchor every write under `~/rust2xml/sqlite/` so the user
        // always finds output in the same place regardless of where
        // the GUI was launched from.  In a sandboxed Mac App Store
        // build, `dirs::home_dir()` returns the per-app container
        // path automatically, so this is also the sandbox-safe
        // destination.
        let sqlite_path = util::home_sqlite_dir().join(&filename);

        let _ = tx.send(Event::Log(format!("Starting {} run...", mode.label())));
        let _ = tx.send(Event::Log(format!("Output: {}", sqlite_path.display())));

        let tx_thread = tx.clone();
        let ctx_thread = ctx.clone();
        thread::spawn(move || {
            let mut opts = Options::default();
            opts.log = true;
            mode.apply_to(&mut opts);

            // Wire util::log() into the GUI log channel so every
            // download/extract step shows up live.
            let tx_for_log = tx_thread.clone();
            let ctx_for_log = ctx_thread.clone();
            util::set_log_sink(Some(Box::new(move |line| {
                let _ = tx_for_log.send(Event::Log(line));
                ctx_for_log.request_repaint();
            })));

            // Wire pipeline progress events into the GUI progress bar.
            let tx_for_progress = tx_thread.clone();
            let ctx_for_progress = ctx_thread.clone();
            util::set_progress_sink(Some(Box::new(move |frac, label| {
                let _ = tx_for_progress.send(Event::Progress(frac, label));
                ctx_for_progress.request_repaint();
            })));

            let _ = tx_thread.send(Event::Log("Downloading sources...".into()));
            ctx_thread.request_repaint();

            let cli = Cli::new(opts);
            let result = cli.run_to_sqlite(&sqlite_path);

            // Detach sinks before signalling completion so any late
            // stragglers don't sneak in after we mark the run done.
            util::set_log_sink(None);
            util::set_progress_sink(None);

            match result {
                Ok(()) => {
                    let _ = tx_thread.send(Event::Log("Done.".into()));
                    let _ = tx_thread.send(Event::Done(sqlite_path));
                }
                Err(e) => {
                    let _ = tx_thread.send(Event::Error(format!("{e:#}")));
                }
            }
            ctx_thread.request_repaint();
        });
    }

    fn drain_events(&mut self) {
        let rx = match self.rx.as_ref() {
            Some(rx) => rx.clone(),
            None => return,
        };
        let mut finished = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Log(line) => self.log.push(line),
                Event::Progress(frac, label) => {
                    self.progress = frac;
                    self.progress_label = label;
                }
                Event::Done(path) => {
                    self.log.push(format!("SQLite written: {}", path.display()));
                    self.sqlite_path = Some(path);
                    self.load_table_names();
                    finished = true;
                }
                Event::Error(err) => {
                    self.log.push(format!("ERROR: {err}"));
                    self.last_error = Some(err);
                    finished = true;
                }
            }
        }
        if finished {
            self.running_mode = None;
            self.rx = None;
        }
    }

    fn load_table_names(&mut self) {
        let path = match &self.sqlite_path {
            Some(p) => p.clone(),
            None => return,
        };
        let conn = match Connection::open(&path) {
            Ok(c) => c,
            Err(e) => {
                self.last_error = Some(format!("opening db: {e}"));
                return;
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name != 'meta' ORDER BY name",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.last_error = Some(format!("listing tables: {e}"));
                return;
            }
        };
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map(|iter| iter.flatten().collect())
            .unwrap_or_default();
        self.table_names = names;
        if let Some(first) = self.table_names.first().cloned() {
            self.select_table(&first);
        }
    }

    fn select_table(&mut self, name: &str) {
        self.selected_table = Some(name.to_string());
        // Switching tabs resets the search — column sets differ between
        // articles/products/limitations/etc., so a stale query is rarely
        // what the user wants.
        self.search_query.clear();
        let path = match &self.sqlite_path {
            Some(p) => p.clone(),
            None => return,
        };
        let data = load_table(&path, name).unwrap_or_else(|e| {
            self.last_error = Some(format!("loading {name}: {e}"));
            TableData::default()
        });
        self.table_cache = Some(data);
        self.recompute_filter();
    }

    /// Rebuild `filtered_rows` from `search_query` against the current
    /// `table_cache`.  Empty query → all rows.
    fn recompute_filter(&mut self) {
        self.last_filter_query = self.search_query.clone();
        let data = match self.table_cache.as_ref() {
            Some(d) => d,
            None => {
                self.filtered_rows.clear();
                return;
            }
        };
        let needle = self.search_query.trim().to_lowercase();
        if needle.is_empty() {
            self.filtered_rows = (0..data.rows.len()).collect();
            return;
        }
        self.filtered_rows = data
            .haystacks
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if h.contains(needle.as_str()) { Some(i) } else { None })
            .collect();
    }
}

fn load_table(path: &Path, name: &str) -> rusqlite::Result<TableData> {
    let conn = Connection::open(path)?;
    let cols_sql = format!("PRAGMA table_info(\"{}\");", name.replace('"', "\"\""));
    let mut stmt = conn.prepare(&cols_sql)?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .flatten()
        .collect();
    if columns.is_empty() {
        return Ok(TableData::default());
    }

    let select = format!(
        "SELECT {} FROM \"{}\";",
        columns
            .iter()
            .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", "),
        name.replace('"', "\"\"")
    );
    let mut stmt = conn.prepare(&select)?;
    let n = columns.len();
    let rows: Vec<Vec<String>> = stmt
        .query_map([], |row| {
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let v: rusqlite::types::Value = row.get(i)?;
                out.push(value_to_string(&v));
            }
            Ok(out)
        })?
        .flatten()
        .collect();
    let haystacks: Vec<String> = rows
        .iter()
        .map(|r| r.join("\u{1f}").to_lowercase())
        .collect();
    Ok(TableData {
        columns,
        rows,
        haystacks,
    })
}

fn value_to_string(v: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    let raw = match v {
        Value::Null => String::new(),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Text(t) => t.clone(),
        Value::Blob(b) => format!("<{} bytes>", b.len()),
    };
    // Collapse line breaks + tabs to single spaces.  Long German
    // limitation descriptions ship with embedded newlines; in a 18-px
    // table row those force egui to either shrink the label out of
    // view or skip drawing it entirely, which is why the limitations
    // tab looked blank before.
    let mut out = String::with_capacity(raw.len());
    let mut prev_space = false;
    for ch in raw.chars() {
        let normalized = match ch {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        };
        if normalized == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(normalized);
            prev_space = false;
        }
    }
    out
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        // Continuous repaint while a worker is running so log lines flow.
        if self.running_mode.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        self.ensure_icon_texture(ctx);

        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let running = self.running_mode.is_some();
                if ui
                    .add_enabled(!running, egui::Button::new(RichText::new("Run -e (Extended)").size(16.0)).min_size(egui::vec2(220.0, 36.0)))
                    .clicked()
                {
                    let ctx_clone = ctx.clone();
                    self.start_run(RunMode::Extended, ctx_clone);
                }
                if ui
                    .add_enabled(!running, egui::Button::new(RichText::new("Run -b (Firstbase)").size(16.0)).min_size(egui::vec2(220.0, 36.0)))
                    .clicked()
                {
                    let ctx_clone = ctx.clone();
                    self.start_run(RunMode::Firstbase, ctx_clone);
                }

                // "Open Data Folder" — reveals `~/rust2xml/` in the
                // platform's file manager so the user always knows
                // where their SQLite snapshots and XML output live.
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("📂 Open Data Folder").size(14.0),
                        )
                        .min_size(egui::vec2(180.0, 36.0)),
                    )
                    .on_hover_text(format!(
                        "Open {} in {}",
                        util::home_data_root().display(),
                        if cfg!(target_os = "macos") {
                            "Finder"
                        } else if cfg!(target_os = "windows") {
                            "Explorer"
                        } else {
                            "your file manager"
                        }
                    ))
                    .clicked()
                {
                    if let Err(e) = open_in_file_manager(&util::home_data_root()) {
                        self.log.push(format!("Open data folder failed: {e}"));
                    }
                }

                if running {
                    ui.spinner();
                    ui.label(format!(
                        "Running {}...",
                        self.running_mode.unwrap().label()
                    ));
                }

                // Top-right: clickable app icon → mailto link.  The
                // right-to-left layout pushes the badge to the trailing
                // edge of the row regardless of window width.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(tex) = &self.icon_texture {
                        let resp = ui
                            .add(
                                egui::Image::new(tex)
                                    .max_width(40.0)
                                    .max_height(40.0)
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text("Contact: zdavatz@ywesee.com");
                        if resp.clicked() {
                            ctx.open_url(egui::OpenUrl::new_tab(
                                "mailto:zdavatz@ywesee.com",
                            ));
                        }
                    }
                });
            });
            if running_or_progressed(self.running_mode.is_some(), self.progress) {
                ui.add_space(4.0);
                let label = if self.progress_label.is_empty() {
                    format!("{:.0}%", self.progress * 100.0)
                } else {
                    format!("{:.0}% — {}", self.progress * 100.0, self.progress_label)
                };
                ui.add(
                    egui::ProgressBar::new(self.progress)
                        .text(label)
                        .desired_width(ui.available_width()),
                );
            }
            if let Some(p) = &self.sqlite_path {
                ui.horizontal(|ui| {
                    ui.label("DB:");
                    ui.monospace(p.display().to_string());
                });
            }
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("log").resizable(true).default_height(140.0).show(ctx, |ui| {
            ui.label(RichText::new("Log").strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for line in &self.log {
                    ui.monospace(line);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.table_names.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Press a button above to download data and build the SQLite DB.").size(14.0));
                });
                return;
            }

            // Tab strip — selecting a tab loads & resets the search.
            let names = self.table_names.clone();
            let mut new_select: Option<String> = None;
            ui.horizontal_wrapped(|ui| {
                for name in &names {
                    let selected = self.selected_table.as_deref() == Some(name.as_str());
                    if ui.selectable_label(selected, name).clicked() && !selected {
                        new_select = Some(name.clone());
                    }
                }
            });
            if let Some(n) = new_select {
                self.select_table(&n);
            }

            // Search box — case-insensitive substring across every column
            // of the currently-selected tab.  We diff against
            // `last_filter_query` every frame so a missed `changed()`
            // event can never leave the filter stale.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Search:").strong());
                let avail = ui.available_width();
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("substring across all columns (case-insensitive)")
                        .desired_width(avail - 90.0),
                );
                if ui.button("Clear").clicked() {
                    self.search_query.clear();
                }
            });
            if self.search_query != self.last_filter_query {
                self.recompute_filter();
            }
            ui.separator();

            let data = match self.table_cache.as_ref() {
                Some(d) => d,
                None => return,
            };
            if data.columns.is_empty() {
                ui.label("(empty table)");
                return;
            }

            let total = data.rows.len();
            let visible = self.filtered_rows.len();
            if self.search_query.trim().is_empty() {
                ui.label(format!("{total} rows × {} cols", data.columns.len()));
            } else {
                ui.label(format!(
                    "{visible} of {total} rows match × {} cols",
                    data.columns.len()
                ));
            }

            let filtered = &self.filtered_rows;

            // Horizontal+vertical scroll wraps a TableBuilder.  Each column
            // is `Column::initial(160.0).resizable(true).clip(true)` so a
            // user can widen columns with long values.
            egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                let mut tb = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
                for _ in &data.columns {
                    tb = tb.column(Column::initial(160.0).at_least(60.0).clip(true).resizable(true));
                }
                tb.header(22.0, |mut header| {
                    for name in &data.columns {
                        header.col(|ui| {
                            ui.strong(name);
                        });
                    }
                })
                .body(|body| {
                    body.rows(18.0, filtered.len(), |mut row| {
                        let row_idx = filtered[row.index()];
                        let r = &data.rows[row_idx];
                        for value in r {
                            row.col(|ui| {
                                ui.add(
                                    egui::Label::new(value)
                                        .truncate()
                                        .selectable(true),
                                )
                                .on_hover_text(value);
                            });
                        }
                    });
                });
            });
        });
    }
}

/// Show progress bar while running, or when a completed run left a
/// final value on screen (so the user sees "Done" briefly before the
/// next click).
fn running_or_progressed(running: bool, progress: f32) -> bool {
    running || progress > 0.0
}

/// Reveal `path` in the platform's native file manager.  On macOS
/// uses `open`, on Windows `explorer`, on Linux `xdg-open`.
fn open_in_file_manager(path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    cmd.arg(path).spawn().map(|_| ())
}

impl Clone for TableData {
    fn clone(&self) -> Self {
        Self {
            columns: self.columns.clone(),
            rows: self.rows.clone(),
            haystacks: self.haystacks.clone(),
        }
    }
}
