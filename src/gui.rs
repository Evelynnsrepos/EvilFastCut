//! Evil Fast Cut's window: pick or drop a video, set how to cut, click Cut.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui;
use evil_fastcut::{Outputs, cut, default_outputs, detect::Settings};

const VIDEO_TYPES: [&str; 12] = ["mp4", "mov", "mkv", "avi", "webm", "m4v", "mxf", "mts", "wav", "mp3", "m4a", "flac"];

enum Msg {
    Line(String),
    Done(Result<(), String>),
}

struct App {
    video: Option<PathBuf>,
    s: Settings,
    xml: bool,
    fcpxml: bool,
    trimmed: bool,
    /// None = all sound tracks mixed.
    track: Option<usize>,
    log: Vec<String>,
    running: Option<Receiver<Msg>>,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 600.0]).with_min_inner_size([420.0, 480.0]).with_app_id("io.github.evelynnsrepos.EvilFastCut"),
        ..Default::default()
    };
    let video = std::env::args_os().nth(1).map(PathBuf::from); // "Open with" and drag onto the app icon
    eframe::run_native(
        "Evil Fast Cut",
        options,
        Box::new(move |_| Ok(Box::new(App { video, s: Settings::default(), xml: true, fcpxml: false, trimmed: false, track: None, log: Vec::new(), running: None }))),
    )
}

impl App {
    fn start(&mut self, ctx: &egui::Context) {
        let Some(video) = self.video.clone() else { return };
        let all = default_outputs(&video);
        let out = Outputs { xml: all.xml.filter(|_| self.xml), fcpxml: all.fcpxml.filter(|_| self.fcpxml), video: all.video.filter(|_| self.trimmed) };
        let (s, track) = (self.s, self.track);
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        self.log = vec![format!("Listening to {}…", video.file_name().unwrap_or_default().to_string_lossy())];
        self.running = Some(rx);
        std::thread::spawn(move || {
            let result = cut(&video, &s, track, &out, &mut |line| {
                let _ = tx.send(Msg::Line(line));
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if let Some(rx) = &self.running {
            for msg in rx.try_iter() {
                match msg {
                    Msg::Line(l) => self.log.push(l),
                    Msg::Done(Ok(())) => {
                        self.log.push(if self.xml { "Done! In FilmCraft, open the .xml with File ▸ Import." } else { "Done!" }.into());
                        self.running = None;
                        break;
                    }
                    Msg::Done(Err(e)) => {
                        self.log.push(format!("Couldn't cut: {e}"));
                        self.running = None;
                        break;
                    }
                }
            }
        }
        let busy = self.running.is_some();
        if let Some(f) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())).filter(|_| !busy) {
            self.video = Some(f);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Evil Fast Cut");
            ui.label("Cuts the silent parts (and breaths) out of a video.");
            ui.add_space(8.0);

            let label = match &self.video {
                Some(v) => format!("🎬 {}\n(click to choose another, or drop one here)", v.file_name().unwrap_or_default().to_string_lossy()),
                None => "Drop a video here, or click to choose one".into(),
            };
            let pick = ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(label).size(16.0)).min_size(egui::vec2(ui.available_width(), 70.0)));
            if pick.clicked()
                && let Some(f) = rfd::FileDialog::new().add_filter("Videos and sound", &VIDEO_TYPES).add_filter("All files", &["*"]).pick_file()
            {
                self.video = Some(f);
            }
            ui.add_space(8.0);

            ui.add_enabled_ui(!busy, |ui| {
                egui::Grid::new("settings").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    ui.label("Silence is quieter than");
                    ui.add(egui::Slider::new(&mut self.s.threshold_db, -70.0..=-10.0).suffix(" dB").step_by(1.0))
                        .on_hover_text("Noisy room or hum: try -35. Very quiet voice: try -50.");
                    ui.end_row();
                    ui.label("Only cut silences longer than");
                    ui.add(egui::Slider::new(&mut self.s.min_silence, 0.1..=3.0).suffix(" s").step_by(0.05))
                        .on_hover_text("Lower it for a snappier, jump-cut style.");
                    ui.end_row();
                    ui.label("Keep around each cut");
                    ui.add(egui::Slider::new(&mut self.s.padding, 0.0..=0.5).suffix(" s").step_by(0.01))
                        .on_hover_text("Raise it if the starts or ends of words get clipped.");
                    ui.end_row();
                    ui.checkbox(&mut self.s.breaths, "Also cut breaths");
                    ui.add_enabled(self.s.breaths, egui::Slider::new(&mut self.s.breath_drop_db, 4.0..=24.0).suffix(" dB quieter").step_by(1.0))
                        .on_hover_text("How much quieter than your voice a breath is. Lower it if loud breaths stay in.");
                    ui.end_row();
                    ui.label("Listen to");
                    let name = |t: Option<usize>| t.map_or("All sound tracks".to_string(), |t| format!("Sound track {}", t + 1));
                    egui::ComboBox::from_id_salt("track").selected_text(name(self.track)).show_ui(ui, |ui| {
                        for t in std::iter::once(None).chain((0..8).map(Some)) {
                            ui.selectable_value(&mut self.track, t, name(t));
                        }
                    })
                    .response
                    .on_hover_text("Recorded with OBS and the mic on its own track? Pick the mic's track, so game sound or music doesn't count as talking.");
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.label("Save next to the video:");
                ui.checkbox(&mut self.xml, "Timeline for FilmCraft, DaVinci Resolve and Premiere Pro (.xml)");
                ui.checkbox(&mut self.fcpxml, "Timeline for Final Cut Pro (.fcpxml)");
                ui.checkbox(&mut self.trimmed, "Trimmed video (slower: re-encodes the video)");
            });
            ui.add_space(10.0);

            let ready = self.video.is_some() && (self.xml || self.fcpxml || self.trimmed) && !busy;
            ui.horizontal(|ui| {
                if ui.add_enabled(ready, egui::Button::new(egui::RichText::new("✂ Cut").size(18.0)).min_size(egui::vec2(120.0, 36.0))).clicked() {
                    self.start(ctx);
                }
                if busy {
                    ui.spinner();
                }
                if ui.button("Defaults").clicked() && !busy {
                    self.s = Settings::default();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for l in &self.log {
                    ui.label(l);
                }
            });
        });
    }
}
