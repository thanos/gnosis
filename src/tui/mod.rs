//! Ratatui TUI for live Gnosis scans.
//!
//! The UI observes [`crate::PipelineEvent`]s and supports deterministic
//! query commands while a scan is running.

#![allow(clippy::type_complexity)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]

use crate::KnowledgeStore;
use crate::{
    InventoryCounts, PipelineEvent, QueryEngine, ScanMetrics, StoredObject, UnderstandingStatus,
};
use anyhow::Result;
use crossbeam_channel::Receiver;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct TuiApp {
    store: Arc<Mutex<KnowledgeStore>>,
    metrics: Arc<ScanMetrics>,
    events: Receiver<PipelineEvent>,
    event_log: Vec<String>,
    event_cap: usize,
    objects: Vec<ObjectRow>,
    selected: usize,
    list_state: ListState,
    command: String,
    command_mode: bool,
    status_line: String,
    output_message: String,
    root: PathBuf,
    scan_done: bool,
    should_quit: bool,
    export_path: PathBuf,
    on_export: Option<Box<dyn Fn(&KnowledgeStore, &PathBuf) -> Result<()> + Send>>,
}

#[derive(Clone)]
struct ObjectRow {
    path: String,
    status: UnderstandingStatus,
    id: String,
}

impl TuiApp {
    pub fn new(
        root: PathBuf,
        store: Arc<Mutex<KnowledgeStore>>,
        metrics: Arc<ScanMetrics>,
        events: Receiver<PipelineEvent>,
        event_cap: usize,
        export_path: PathBuf,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            store,
            metrics,
            events,
            event_log: Vec::new(),
            event_cap,
            objects: Vec::new(),
            selected: 0,
            list_state,
            command: String::new(),
            command_mode: false,
            status_line: "scan running — press : for commands, q to quit".into(),
            output_message: String::new(),
            root,
            scan_done: false,
            should_quit: false,
            export_path,
            on_export: None,
        }
    }

    pub fn set_export_handler(
        &mut self,
        f: impl Fn(&KnowledgeStore, &PathBuf) -> Result<()> + Send + 'static,
    ) {
        self.on_export = Some(Box::new(f));
    }

    pub fn run(mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let tick = Duration::from_millis(100);
        let mut last_refresh = Instant::now();

        let result = loop {
            self.drain_events();
            if last_refresh.elapsed() > Duration::from_millis(250) {
                self.refresh_objects();
                last_refresh = Instant::now();
            }

            terminal.draw(|f| self.draw(f))?;

            if event::poll(tick)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }

            if self.should_quit {
                break Ok(());
            }
        };

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        result
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events.try_recv() {
            if matches!(ev, PipelineEvent::ScanCompleted { .. }) {
                self.scan_done = true;
                self.status_line =
                    "scan complete — :summary :unknown :export okf  q to quit".into();
            }
            self.event_log.push(ev.summary());
            if self.event_log.len() > self.event_cap {
                let overflow = self.event_log.len() - self.event_cap;
                self.event_log.drain(0..overflow);
            }
        }
    }

    fn refresh_objects(&mut self) {
        let store = self.store.lock().unwrap();
        let mut rows: Vec<ObjectRow> = store
            .objects()
            .map(|o| ObjectRow {
                path: o.descriptor.relative_path.display().to_string(),
                status: o.status,
                id: o.descriptor.id.to_string(),
            })
            .collect();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        self.objects = rows;
        if self.objects.is_empty() {
            self.list_state.select(None);
        } else {
            if self.selected >= self.objects.len() {
                self.selected = self.objects.len() - 1;
            }
            self.list_state.select(Some(self.selected));
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        if self.command_mode {
            match code {
                KeyCode::Esc => {
                    self.command_mode = false;
                    self.command.clear();
                }
                KeyCode::Enter => {
                    let cmd = self.command.clone();
                    self.command_mode = false;
                    self.command.clear();
                    self.run_command(&cmd);
                }
                KeyCode::Backspace => {
                    self.command.pop();
                }
                KeyCode::Char(c) => self.command.push(c),
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char(':') => {
                self.command_mode = true;
                self.command.clear();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.objects.is_empty() {
                    self.selected = (self.selected + 1).min(self.objects.len() - 1);
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.objects.is_empty() {
                    self.selected = self.selected.saturating_sub(1);
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Char('s') => self.run_command("summary"),
            KeyCode::Char('u') => self.run_command("unknown"),
            KeyCode::Char('e') => self.run_command("export okf"),
            _ => {}
        }
    }

    fn run_command(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if cmd == "quit" || cmd == "q" {
            self.should_quit = true;
            return;
        }
        if cmd == "help" {
            self.output_message =
                "commands: summary | objects | unknown | providers | stats | find <t> | explain <n> | graph <n> | export okf [path] | quit".into();
            return;
        }

        let store = self.store.lock().unwrap();
        let q = QueryEngine::new(&store);

        if cmd == "summary" {
            self.output_message = q.summary();
            return;
        }
        if cmd == "providers" {
            self.output_message = format!("providers:\n{}", q.providers().join("\n"));
            return;
        }
        if cmd == "stats" {
            let s = q.stats();
            self.output_message = format!("{s:?}");
            return;
        }
        if cmd == "unknown" {
            let rows: Vec<String> = q
                .unknown()
                .into_iter()
                .take(40)
                .map(|o| {
                    format!(
                        "[{}] {} — {}",
                        o.status,
                        o.descriptor.relative_path.display(),
                        o.classification_reason.as_deref().unwrap_or("")
                    )
                })
                .collect();
            self.output_message = if rows.is_empty() {
                "no unknown/partial objects".into()
            } else {
                rows.join("\n")
            };
            return;
        }
        if cmd == "objects" || cmd.starts_with("objects ") {
            let filter = cmd.strip_prefix("objects").unwrap_or("").trim();
            let status = match filter {
                "understood" => Some(UnderstandingStatus::Understood),
                "partial" => Some(UnderstandingStatus::PartiallyUnderstood),
                "unknown" => Some(UnderstandingStatus::Unknown),
                "failed" => Some(UnderstandingStatus::Failed),
                "" => None,
                other => {
                    let rows: Vec<String> = q
                        .objects(None, Some(other))
                        .into_iter()
                        .take(40)
                        .map(format_object)
                        .collect();
                    self.output_message = rows.join("\n");
                    return;
                }
            };
            let rows: Vec<String> = q
                .objects(status, None)
                .into_iter()
                .take(40)
                .map(format_object)
                .collect();
            self.output_message = rows.join("\n");
            return;
        }
        if let Some(text) = cmd.strip_prefix("find ") {
            let found = q.find(text.trim());
            let mut lines = Vec::new();
            for e in found.entities.iter().take(20) {
                lines.push(format!("entity {} {} ({})", e.kind, e.name, e.id));
            }
            for o in found.objects.iter().take(20) {
                lines.push(format!(
                    "object {} [{}]",
                    o.descriptor.relative_path.display(),
                    o.status
                ));
            }
            self.output_message = if lines.is_empty() {
                "no matches".into()
            } else {
                lines.join("\n")
            };
            return;
        }
        if let Some(name) = cmd.strip_prefix("explain ") {
            match q.explain(name.trim()) {
                Some(crate::ExplainResult::Entity {
                    entity,
                    neighborhood,
                }) => {
                    self.output_message = format!(
                        "{} {} ({})\nconfidence: {}\nsource: {}\nattrs: {:?}\nneighbors: {} nodes / {} edges",
                        entity.kind,
                        entity.name,
                        entity.id,
                        entity.confidence.as_str(),
                        entity.source_object,
                        entity.attributes,
                        neighborhood.nodes.len(),
                        neighborhood.edges.len()
                    );
                }
                Some(crate::ExplainResult::Object { object }) => {
                    self.output_message = format_object_detail(object);
                }
                None => self.output_message = "not found".into(),
            }
            return;
        }
        if let Some(name) = cmd.strip_prefix("graph ") {
            match q.graph(name.trim(), 1) {
                Some((entity, neighborhood)) => {
                    let mut lines = vec![format!("graph around {} {}", entity.kind, entity.name)];
                    for e in neighborhood.edges.iter().take(30) {
                        lines.push(format!("  {} --{}--> {}", e.from, e.kind, e.to));
                    }
                    self.output_message = lines.join("\n");
                }
                None => self.output_message = "not found".into(),
            }
            return;
        }
        if cmd == "export okf" || cmd.starts_with("export okf ") {
            let path = cmd
                .strip_prefix("export okf")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| self.export_path.clone());
            drop(store);
            let store = self.store.lock().unwrap();
            if let Some(ref export) = self.on_export {
                match export(&store, &path) {
                    Ok(()) => {
                        self.output_message = format!("exported OKF to {}", path.display());
                        self.status_line = self.output_message.clone();
                    }
                    Err(e) => self.output_message = format!("export failed: {e}"),
                }
            } else {
                self.output_message = "export handler not configured".into();
            }
            return;
        }

        self.output_message = format!("unknown command: {cmd} (try help)");
    }

    fn draw(&mut self, f: &mut ratatui::Frame<'_>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(f.area());

        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[0]);

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main[0]);

        let bottom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main[1]);

        self.draw_sources(f, top[0]);
        self.draw_activity(f, top[1]);
        self.draw_inventory(f, bottom[0]);
        self.draw_current(f, bottom[1]);
        self.draw_command(f, chunks[1]);
    }

    fn draw_sources(&self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let snap = self.metrics.snapshot(0);
        let store = self.store.lock().unwrap();
        let branch = store.git_branch().unwrap_or("n/a");
        let providers = store.enabled_providers().join(", ");
        let text = format!(
            "Root: {}\nConnector: filesystem\nGit: {}\nDiscovered: {}\nQueue: {}\nBytes: {}\nProviders: {}\nScan: {}",
            self.root.display(),
            branch,
            snap.objects_discovered,
            snap.queue_depth,
            snap.bytes_considered,
            providers,
            if self.scan_done { "complete" } else { "running" }
        );
        let widget = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Sources"))
            .wrap(Wrap { trim: true });
        f.render_widget(widget, area);
    }

    fn draw_activity(&self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let snap = self.metrics.snapshot(0);
        let header = format!(
            "done u/p/u/f: {}/{}/{}/{}  elapsed {}ms",
            snap.understood, snap.partial, snap.unknown, snap.failed, snap.elapsed_ms
        );
        let items: Vec<ListItem> = self
            .event_log
            .iter()
            .rev()
            .take(area.height.saturating_sub(3) as usize)
            .map(|e| ListItem::new(e.as_str()))
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Activity — {header}")),
        );
        f.render_widget(list, area);
    }

    fn draw_inventory(&self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let store = self.store.lock().unwrap();
        let inv: InventoryCounts = store.inventory();
        let text = format!(
            "Objects: {}\nModules/namespaces: {}\nTypes: {}\nFunctions: {}\nDocuments: {}\nDatasets: {}\nRelationships: {}\n\nUnderstood: {}\nPartial: {}\nUnknown: {}\nFailed: {}",
            inv.source_objects,
            inv.modules,
            inv.types,
            inv.functions,
            inv.documents,
            inv.datasets,
            inv.relationships,
            inv.understood,
            inv.partial,
            inv.unknown,
            inv.failed
        );
        let widget = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Knowledge Inventory"),
        );
        f.render_widget(widget, area);
    }

    fn draw_current(&mut self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let store = self.store.lock().unwrap();
        let body = if let Some(row) = self.objects.get(self.selected) {
            if let Some(obj) = store.objects().find(|o| o.descriptor.id.as_str() == row.id) {
                format_object_detail(obj)
            } else {
                format!("{}\n[{}]", row.path, row.status)
            }
        } else if !self.output_message.is_empty() {
            self.output_message.clone()
        } else {
            "select an object…".into()
        };

        // Prefer command output when present and recent.
        let text = if !self.output_message.is_empty() && !self.command_mode {
            // Show object detail in panel; command output also in status — show both split
            if self.objects.get(self.selected).is_some() && !body.contains("Gnosis summary") {
                if self.output_message.len() > 20
                    && (self.output_message.starts_with("Gnosis")
                        || self.output_message.starts_with("entity")
                        || self.output_message.starts_with("graph")
                        || self.output_message.starts_with("[")
                        || self.output_message.starts_with("providers")
                        || self.output_message.starts_with("exported")
                        || self.output_message.starts_with("commands"))
                {
                    self.output_message.clone()
                } else {
                    body
                }
            } else {
                self.output_message.clone()
            }
        } else {
            body
        };

        let items: Vec<ListItem> = self
            .objects
            .iter()
            .map(|o| ListItem::new(format!("[{}] {}", short_status(o.status), o.path)))
            .collect();

        // Split current panel: list + detail
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Objects"))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, panes[0], &mut self.list_state);

        let detail = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Current Object / Output"),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(detail, panes[1]);
    }

    fn draw_command(&self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let content = if self.command_mode {
            Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Cyan)),
                Span::raw(self.command.clone()),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ])
        } else {
            Line::from(vec![
                Span::raw(self.status_line.clone()),
                Span::raw("  "),
                Span::styled(
                    "[s]ummary [u]nknown [e]xport [:]cmd [q]uit",
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        };
        let widget =
            Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Command"));
        f.render_widget(widget, area);
    }
}

fn short_status(s: UnderstandingStatus) -> &'static str {
    match s {
        UnderstandingStatus::Understood => "U",
        UnderstandingStatus::PartiallyUnderstood => "P",
        UnderstandingStatus::Unknown => "?",
        UnderstandingStatus::Failed => "F",
    }
}

fn format_object(o: &StoredObject) -> String {
    format!(
        "[{}] {} ({})",
        o.status,
        o.descriptor.relative_path.display(),
        o.classification_reason.as_deref().unwrap_or("")
    )
}

fn format_object_detail(o: &StoredObject) -> String {
    let mut lines = vec![
        format!("id: {}", o.descriptor.id),
        format!("path: {}", o.descriptor.relative_path.display()),
        format!("media: {}", o.descriptor.media_type),
        format!("size: {}", o.descriptor.size),
        format!("status: {}", o.status),
        format!(
            "provider: {}",
            o.provider
                .as_ref()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "(none)".into())
        ),
        format!(
            "reason: {}",
            o.classification_reason.as_deref().unwrap_or("(none)")
        ),
        format!(
            "fingerprint: {}",
            o.proto.fingerprint.as_deref().unwrap_or("-")
        ),
        format!("entities: {}", o.entity_ids.len()),
    ];
    if let Some(git) = &o.proto.git {
        lines.push(format!(
            "git: branch={:?} tracked={:?} commit={:?}",
            git.branch, git.tracked, git.last_commit_id
        ));
    }
    if !o.diagnostics.is_empty() {
        lines.push(format!("diagnostics: {}", o.diagnostics.join("; ")));
    }
    lines.join("\n")
}

/// Headless helper used by CLI when --no-tui is set: drain events to stderr-like prints.
pub fn drain_events_headless(events: Receiver<PipelineEvent>, quiet: bool) {
    while let Ok(ev) = events.recv() {
        if !quiet {
            println!("· {}", ev.summary());
        }
        if matches!(ev, PipelineEvent::ScanCompleted { .. }) {
            // Continue draining any trailing events briefly.
            while let Ok(ev) = events.try_recv() {
                if !quiet {
                    println!("· {}", ev.summary());
                }
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Confidence, Entity, EntityId, ObjectDescriptor, ObjectId, ProtoData, ProviderId,
        Relationship, RelationshipId, StoredObject,
    };
    use crossbeam_channel::bounded;
    use ratatui::backend::TestBackend;
    use std::sync::atomic::Ordering;

    fn seeded_store() -> Arc<Mutex<KnowledgeStore>> {
        let mut store = KnowledgeStore::new();
        store.set_root(PathBuf::from("/repo"));
        store.set_git_branch(Some("main".into()));
        store.set_enabled_providers(vec!["test".into()]);
        let id = ObjectId::new("obj:a.rs");
        let ent = Entity {
            id: EntityId::new("ent:fn:foo"),
            kind: "function".into(),
            name: "foo".into(),
            attributes: Default::default(),
            evidence: Vec::new(),
            confidence: Confidence::High,
            source_object: id.clone(),
        };
        store.upsert_object(StoredObject {
            descriptor: ObjectDescriptor {
                id: id.clone(),
                path: PathBuf::from("/repo/a.rs"),
                relative_path: PathBuf::from("a.rs"),
                is_dir: false,
                size: 1,
                modified: None,
                media_type: "text/x-rust".into(),
                extension: Some("rs".into()),
            },
            proto: ProtoData {
                fingerprint: Some("fp".into()),
                ..ProtoData::default()
            },
            status: UnderstandingStatus::Understood,
            provider: Some(ProviderId::new("test")),
            classification_reason: Some("ok".into()),
            entity_ids: vec![ent.id.clone()],
            diagnostics: vec!["note".into()],
        });
        store.add_entity(ent.clone());
        store.add_relationship(Relationship {
            id: RelationshipId::generate("defines", ent.id.as_str(), ent.id.as_str()),
            kind: "defines".into(),
            from: ent.id.clone(),
            to: ent.id,
            attributes: Default::default(),
            evidence: Vec::new(),
            confidence: Confidence::Low,
        });
        Arc::new(Mutex::new(store))
    }

    fn app_with(
        store: Arc<Mutex<KnowledgeStore>>,
    ) -> (TuiApp, crossbeam_channel::Sender<PipelineEvent>) {
        let (tx, rx) = bounded(32);
        let metrics = Arc::new(ScanMetrics::new());
        metrics.start();
        metrics.objects_discovered.store(1, Ordering::Relaxed);
        let app = TuiApp::new(
            PathBuf::from("/repo"),
            store,
            metrics,
            rx,
            50,
            PathBuf::from("/tmp/out.okf"),
        );
        (app, tx)
    }

    #[test]
    fn keys_and_commands_drive_state() {
        let store = seeded_store();
        let (mut app, tx) = app_with(Arc::clone(&store));
        app.set_export_handler(|_store, path| {
            std::fs::create_dir_all(path).ok();
            std::fs::write(path.join("index.md"), "# ok").ok();
            Ok(())
        });

        let _ = tx.send(PipelineEvent::ScanCompleted {
            objects: 1,
            elapsed_ms: 1,
        });
        app.drain_events();
        assert!(app.scan_done);
        app.refresh_objects();
        assert_eq!(app.objects.len(), 1);

        app.handle_key(KeyCode::Char(':'));
        assert!(app.command_mode);
        app.handle_key(KeyCode::Char('h'));
        app.handle_key(KeyCode::Char('e'));
        app.handle_key(KeyCode::Char('l'));
        app.handle_key(KeyCode::Char('p'));
        app.handle_key(KeyCode::Enter);
        assert!(app.output_message.contains("commands:"));

        app.handle_key(KeyCode::Char('s'));
        assert!(app.output_message.contains("Gnosis summary"));

        app.run_command("providers");
        assert!(app.output_message.contains("test"));
        app.run_command("stats");
        assert!(!app.output_message.is_empty());
        app.run_command("objects");
        assert!(app.output_message.contains("a.rs"));
        app.run_command("objects understood");
        app.run_command("objects rs");
        app.run_command("unknown");
        app.run_command("find foo");
        assert!(app.output_message.contains("entity") || app.output_message.contains("foo"));
        app.run_command("explain foo");
        assert!(app.output_message.contains("function") || app.output_message.contains("foo"));
        app.run_command("graph foo");
        assert!(app.output_message.contains("graph"));
        app.run_command("export okf");
        assert!(app.output_message.contains("exported") || app.output_message.contains("export"));
        app.run_command("nope");
        assert!(app.output_message.contains("unknown command"));

        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char('k'));
        app.handle_key(KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn draw_with_test_backend() {
        let store = seeded_store();
        let (mut app, _tx) = app_with(store);
        app.refresh_objects();
        app.run_command("summary");
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let flat: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(flat.contains("Sources") || flat.contains("Activity") || flat.contains("Command"));
    }

    #[test]
    fn drain_events_headless_stops_on_completion() {
        let (tx, rx) = bounded(8);
        tx.send(PipelineEvent::ObjectDiscovered {
            id: ObjectId::new("obj:x"),
            path: PathBuf::from("x"),
        })
        .unwrap();
        tx.send(PipelineEvent::ScanCompleted {
            objects: 1,
            elapsed_ms: 1,
        })
        .unwrap();
        drop(tx);
        drain_events_headless(rx, true);
    }
}
