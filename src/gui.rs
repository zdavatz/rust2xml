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
}

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
        }
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
        let sqlite_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("sqlite");
        let sqlite_path = sqlite_dir.join(&filename);

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
        let path = match &self.sqlite_path {
            Some(p) => p.clone(),
            None => return,
        };
        let data = load_table(&path, name).unwrap_or_else(|e| {
            self.last_error = Some(format!("loading {name}: {e}"));
            TableData::default()
        });
        self.table_cache = Some(data);
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
    Ok(TableData { columns, rows })
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

        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("rust2xml");
                ui.label(crate::version::VERSION);
            });
            ui.add_space(4.0);
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
                if running {
                    ui.spinner();
                    ui.label(format!(
                        "Running {}...",
                        self.running_mode.unwrap().label()
                    ));
                }
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

            let names = self.table_names.clone();
            ui.horizontal_wrapped(|ui| {
                for name in &names {
                    let selected = self.selected_table.as_deref() == Some(name.as_str());
                    if ui.selectable_label(selected, name).clicked() && !selected {
                        self.select_table(name);
                    }
                }
            });
            ui.separator();

            let data = match self.table_cache.as_ref() {
                Some(d) => d.clone(),
                None => return,
            };
            if data.columns.is_empty() {
                ui.label("(empty table)");
                return;
            }

            ui.label(format!("{} rows × {} cols", data.rows.len(), data.columns.len()));

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
                    body.rows(18.0, data.rows.len(), |mut row| {
                        let idx = row.index();
                        let r = &data.rows[idx];
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

impl Clone for TableData {
    fn clone(&self) -> Self {
        Self {
            columns: self.columns.clone(),
            rows: self.rows.clone(),
        }
    }
}
