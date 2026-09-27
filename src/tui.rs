use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::action::Status;
use crate::catalog;
use crate::engine;
use crate::mas;
use crate::schema::{AppGroup, Pack};
use crate::select::{Selection, Tier};
use crate::sys;
use crate::winget::{self, AppResult};

const RESET: &str = "\x1b[0m";
const STATUS_BUDGET: Duration = Duration::from_millis(120);

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Pack(usize),
    Declutter,
    Apps,
    Activate,
}

enum Modal {
    Quit,
    Oneway {
        normal: Vec<Target>,
        oneway: Vec<Target>,
    },
}

struct Target {
    pack: usize,
    ids: Vec<String>,
}

enum Row {
    Header,
    Group,
    Tweak(usize, usize),
    AppEntry(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    List,
}

enum Hover {
    Tweak(usize, String),
    AppEntry(String),
}

struct App {
    packs: Arc<Vec<Pack>>,
    normal: Vec<usize>,
    oneway: Vec<usize>,
    groups: Vec<AppGroup>,
    app_results: HashMap<String, String>,
    winget_ready: Option<bool>,
    screen: Screen,
    cursor: usize,
    focus: Focus,
    sidebar_cursor: usize,
    selection: Selection,
    statuses: HashMap<String, Vec<Status>>,
    sidebar_state: ListState,
    list_state: ListState,
    modal: Option<Modal>,
    should_quit: bool,
    busy: Option<&'static str>,
    job_rx: Option<Receiver<JobMsg>>,
    pending_apply: Option<Vec<Target>>,
    pending_reset: Option<Vec<Target>>,
    version: String,
    build: String,
    admin: bool,
}

pub fn run() {
    sys::enable_vt();
    let packs = catalog::packs();
    if packs.is_empty() {
        println!("{}", warn("winforge: no tweak packs found"));
        pause();
        return;
    }
    let mut app = App::new(packs);
    let mut terminal = match create_terminal() {
        Ok(terminal) => terminal,
        Err(err) => {
            eprintln!("winforge: terminal setup failed: {err}");
            return;
        }
    };
    loop {
        drain_job(&mut app);
        app.ensure_current_statuses();
        if terminal.draw(|frame| draw(frame, &mut app)).is_err() {
            break;
        }
        let ready = match event::poll(Duration::from_millis(200)) {
            Ok(ready) => ready,
            Err(_) => break,
        };
        if ready {
            let ev = match event::read() {
                Ok(ev) => ev,
                Err(_) => break,
            };
            handle_event(&mut app, ev);
            if app.should_quit {
                break;
            }
        }
        if let Some(targets) = app.pending_apply.take() {
            app.busy = Some("applying");
            app.job_rx = Some(spawn_job(
                Arc::clone(&app.packs),
                Job::Apply { targets },
            ));
        }
        if let Some(targets) = app.pending_reset.take() {
            app.busy = Some("resetting");
            app.job_rx = Some(spawn_job(
                Arc::clone(&app.packs),
                Job::Reset { targets },
            ));
        }
    }
    let _ = suspend(&mut terminal);
}

enum Job {
    Apply { targets: Vec<Target> },
    Reset { targets: Vec<Target> },
    Apps { apps: Vec<crate::schema::App> },
}

enum JobMsg {
    AppResult { id: String, label: String },
    Done,
}

fn spawn_job(packs: Arc<Vec<Pack>>, job: Job) -> Receiver<JobMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        match job {
            Job::Apply { targets } => {
                for target in targets {
                    let pack = &packs[target.pack];
                    let report = engine::apply_tweaks(pack, &target.ids);
                    sys::log(&format!(
                        "apply {}: {} done, {} skipped, {} failed",
                        pack.meta.id, report.done, report.skipped, report.failed
                    ));
                    for failure in &report.failures {
                        sys::log(&format!("failed: {failure}"));
                    }
                }
                run_refresh();
            }
            Job::Reset { targets } => {
                for target in targets {
                    let pack = &packs[target.pack];
                    let report = engine::reset_tweaks(pack, &target.ids);
                    sys::log(&format!(
                        "reset {}: {} done, {} skipped, {} failed",
                        pack.meta.id, report.done, report.skipped, report.failed
                    ));
                    for failure in &report.failures {
                        sys::log(&format!("failed: {failure}"));
                    }
                }
                run_refresh();
            }
            Job::Apps { apps } => {
                if !winget::available() {
                    sys::log("apps: winget not found; bootstrapping (needs internet)");
                    if let Err(message) = winget::ensure() {
                        sys::log(&format!(
                            "apps: bootstrap failed: {message}; running forced repair"
                        ));
                    }
                    if !winget::available() {
                        if let Err(message) = winget::repair() {
                            sys::log(&format!("apps: repair failed: {message}"));
                        }
                    }
                }
                if !winget::available() {
                    sys::log("apps: winget is still unavailable");
                }
                for app in &apps {
                    let label = match winget::install_app_quiet(app) {
                        AppResult::Installed => String::from("installed"),
                        AppResult::AlreadyPresent => String::from("already present"),
                        AppResult::InstalledRebootRequired => {
                            String::from("installed, reboot required")
                        }
                        AppResult::Failed(code) => format!("FAILED ({code})"),
                    };
                    sys::log(&format!("apps: {} ({}): {}", app.name, app.id, label));
                    let _ = tx.send(JobMsg::AppResult {
                        id: app.id.clone(),
                        label,
                    });
                }
            }
        }
        let _ = tx.send(JobMsg::Done);
    });
    return rx;
}

fn drain_job(app: &mut App) {
    if app.job_rx.is_none() {
        return;
    }
    loop {
        let msg = {
            let Some(rx) = app.job_rx.as_ref() else {
                return;
            };
            rx.try_recv()
        };
        match msg {
            Ok(JobMsg::AppResult { id, label }) => {
                app.app_results.insert(id, label);
            }
            Ok(JobMsg::Done) | Err(TryRecvError::Disconnected) => {
                app.busy = None;
                app.job_rx = None;
                app.invalidate_statuses();
                app.winget_ready = None;
                sys::log("job finished; rescanning statuses");
                return;
            }
            Err(TryRecvError::Empty) => {
                return;
            }
        }
    }
}

impl App {
    fn new(packs: Vec<Pack>) -> App {
        let normal: Vec<usize> = packs
            .iter()
            .enumerate()
            .filter(|(_, pack)| !pack.meta.oneway())
            .map(|(index, _)| index)
            .collect();
        let oneway: Vec<usize> = packs
            .iter()
            .enumerate()
            .filter(|(_, pack)| pack.meta.oneway())
            .map(|(index, _)| index)
            .collect();
        let build = match sys::windows_build() {
            Ok(number) => number.to_string(),
            Err(_) => String::from("?"),
        };
        let mut app = App {
            packs: Arc::new(packs),
            normal,
            oneway,
            groups: catalog::app_groups(),
            app_results: HashMap::new(),
            winget_ready: None,
            screen: Screen::Declutter,
            cursor: 0,
            focus: Focus::List,
            sidebar_cursor: 0,
            selection: Selection::new(),
            statuses: HashMap::new(),
            sidebar_state: ListState::default(),
            list_state: ListState::default(),
            modal: None,
            should_quit: false,
            busy: None,
            job_rx: None,
            pending_apply: None,
            pending_reset: None,
            version: concat!("v", env!("CARGO_PKG_VERSION")).to_string(),
            build,
            admin: sys::is_admin(),
        };
        if let Some(&first) = app.normal.first() {
            app.screen = Screen::Pack(first);
        }
        app.sidebar_state.select(Some(app.screen_to_sidebar()));
        app.set_screen(app.screen);
        return app;
    }

    fn sidebar_len(&self) -> usize {
        return self.normal.len() + 3;
    }

    fn screen_to_sidebar(&self) -> usize {
        match self.screen {
            Screen::Pack(p) => {
                return self.normal.iter().position(|&x| x == p).unwrap_or(0);
            }
            Screen::Declutter => return self.normal.len(),
            Screen::Apps => return self.normal.len() + 1,
            Screen::Activate => return self.normal.len() + 2,
        }
    }

    fn sidebar_to_screen(&self, index: usize) -> Screen {
        if index < self.normal.len() {
            return Screen::Pack(self.normal[index]);
        }
        if index == self.normal.len() {
            return Screen::Declutter;
        }
        if index == self.normal.len() + 1 {
            return Screen::Apps;
        }
        return Screen::Activate;
    }

    fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.cursor = 0;
        self.list_state.select(Some(0));
        self.sidebar_state.select(Some(self.screen_to_sidebar()));
        self.ensure_current_statuses();
        if let Some(pos) = self
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Tweak(..)))
        {
            self.cursor = pos;
            self.list_state.select(Some(pos));
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        match self.screen {
            Screen::Pack(p) => {
                for t in 0..self.packs[p].tweak.len() {
                    rows.push(Row::Tweak(p, t));
                }
            }
            Screen::Declutter => {
                for &p in &self.oneway {
                    rows.push(Row::Header);
                    for t in 0..self.packs[p].tweak.len() {
                        rows.push(Row::Tweak(p, t));
                    }
                }
            }
            Screen::Apps => {
                for (g, group) in self.groups.iter().enumerate() {
                    rows.push(Row::Group);
                    for a in 0..group.app.len() {
                        rows.push(Row::AppEntry(g, a));
                    }
                }
            }
            Screen::Activate => {}
        }
        return rows;
    }

    fn ensure_current_statuses(&mut self) {
        match self.screen {
            Screen::Pack(p) => self.load_status(p),
            Screen::Declutter => {
                let indices: Vec<usize> = self.oneway.clone();
                for p in indices {
                    self.load_status(p);
                }
            }
            Screen::Apps => {
                if self.winget_ready.is_none() {
                    let ready = winget::available();
                    self.winget_ready = Some(ready);
                }
            }
            Screen::Activate => {}
        }
    }

    fn load_status(&mut self, index: usize) {
        let total = self.packs[index].tweak.len();
        let id = self.packs[index].meta.id.clone();
        let loaded = self.statuses.get(&id).map(Vec::len).unwrap_or(0);
        if loaded >= total && self.statuses.contains_key(&id) {
            return;
        }
        let deadline = Instant::now() + STATUS_BUDGET;
        let mut statuses = self.statuses.remove(&id).unwrap_or_default();
        while statuses.len() < total {
            let t = statuses.len();
            let status = engine::single_status(&self.packs[index], t);
            statuses.push(status);
            if Instant::now() >= deadline {
                break;
            }
        }
        self.statuses.insert(id, statuses);
    }

    fn invalidate_statuses(&mut self) {
        self.statuses.clear();
    }

    fn focus_sidebar(&mut self) {
        self.focus = Focus::Sidebar;
        self.sidebar_cursor = self.screen_to_sidebar();
        self.sidebar_state.select(Some(self.sidebar_cursor));
    }

    fn focus_list(&mut self) {
        self.focus = Focus::List;
        let screen = self.sidebar_to_screen(self.sidebar_cursor);
        if screen != self.screen {
            self.set_screen(screen);
        }
    }

    fn move_sidebar_cursor(&mut self, delta: i32) {
        let len = self.sidebar_len();
        if len == 0 {
            return;
        }
        let next = (self.sidebar_cursor as i64 + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.sidebar_cursor = next;
        self.set_screen(self.sidebar_to_screen(next));
    }

    fn move_cursor(&mut self, delta: i32) {
        let rows = self.rows();
        let tweak_positions: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Tweak(..) | Row::AppEntry(..)))
            .map(|(index, _)| index)
            .collect();
        if tweak_positions.is_empty() {
            self.cursor = 0;
            self.list_state.select(Some(0));
            return;
        }
        let current = self.cursor.min(rows.len() - 1);
        let list_pos = tweak_positions
            .iter()
            .position(|&pos| pos >= current)
            .unwrap_or(tweak_positions.len() - 1);
        let new_pos =
            (list_pos as i64 + delta as i64).clamp(0, tweak_positions.len() as i64 - 1) as usize;
        self.cursor = tweak_positions[new_pos];
        self.list_state.select(Some(self.cursor));
    }

    fn hovered(&self) -> Option<Hover> {
        let rows = self.rows();
        let row = rows.get(self.cursor)?;
        return match row {
            Row::Tweak(p, t) => {
                let tweak = &self.packs[*p].tweak[*t];
                Some(Hover::Tweak(*p, tweak.id.clone()))
            }
            Row::AppEntry(g, a) => {
                let app = &self.groups[*g].app[*a];
                Some(Hover::AppEntry(app.id.clone()))
            }
            Row::Header | Row::Group => None,
        };
    }

    fn toggle_hovered(&mut self) {
        let Some(hover) = self.hovered() else {
            return;
        };
        let id = match hover {
            Hover::Tweak(_, id) => id,
            Hover::AppEntry(id) => id,
        };
        self.selection.toggle(&id);
    }

    fn select_category(&mut self, value: bool) {
        match self.screen {
            Screen::Pack(p) => {
                let id = self.packs[p].meta.id.clone();
                self.selection.select_category(&self.packs, &id, value);
            }
            Screen::Declutter => {
                for &p in &self.oneway {
                    let id = self.packs[p].meta.id.clone();
                    self.selection.select_category(&self.packs, &id, value);
                }
            }
            Screen::Apps | Screen::Activate => {}
        }
    }

    fn targets_for(&self, pack_indices: &[usize]) -> Vec<Target> {
        let mut targets = Vec::new();
        if self.selection.is_empty() {
            let Some(Hover::Tweak(p, id)) = self.hovered() else {
                return targets;
            };
            if pack_indices.contains(&p) {
                targets.push(Target {
                    pack: p,
                    ids: vec![id],
                });
            }
            return targets;
        }
        let selected = self.selection.ids();
        for &p in pack_indices {
            let ids: Vec<String> = self.packs[p]
                .tweak
                .iter()
                .filter(|tweak| selected.contains(&tweak.id))
                .map(|tweak| tweak.id.clone())
                .collect();
            if !ids.is_empty() {
                targets.push(Target { pack: p, ids });
            }
        }
        return targets;
    }

    fn request_apply(&mut self) {
        match self.screen {
            Screen::Apps => self.apps_install(),
            Screen::Activate => {
                let _ = mas::run();
            }
            Screen::Pack(_) => {
                let normal = self.targets_for(&self.normal);
                let oneway = self.targets_for(&self.oneway);
                if !oneway.is_empty() {
                    self.modal = Some(Modal::Oneway { normal, oneway });
                    return;
                }
                if !normal.is_empty() {
                    self.pending_apply = Some(normal);
                }
            }
            Screen::Declutter => {
                let oneway = self.targets_for(&self.oneway);
                if !oneway.is_empty() {
                    self.modal = Some(Modal::Oneway {
                        normal: Vec::new(),
                        oneway,
                    });
                }
            }
        }
    }

    fn request_reset(&mut self) {
        let targets = self.targets_for(&self.normal);
        if !targets.is_empty() {
            self.pending_reset = Some(targets);
        }
    }

    fn request_quit(&mut self) {
        if self.selection.is_empty() {
            self.should_quit = true;
            return;
        }
        self.modal = Some(Modal::Quit);
    }

    fn apps_install(&mut self) {
        let mut apps: Vec<crate::schema::App> = Vec::new();
        let selected = self.selection.ids();
        for group in &self.groups {
            for app in &group.app {
                if selected.contains(&app.id) {
                    apps.push(app.clone());
                }
            }
        }
        if apps.is_empty() {
            let rows = self.rows();
            let hovered = match rows.get(self.cursor) {
                Some(Row::AppEntry(g, a)) => Some(self.groups[*g].app[*a].clone()),
                _ => None,
            };
            match hovered {
                Some(app) => apps.push(app),
                None => {
                    sys::log("apps: nothing selected to install");
                    return;
                }
            }
        }
        self.winget_ready = None;
        self.busy = Some("installing");
        self.job_rx = Some(spawn_job(Arc::clone(&self.packs), Job::Apps { apps }));
    }
}

fn handle_event(app: &mut App, ev: Event) {
    match ev {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.request_quit();
                return;
            }
            if app.modal.is_some() {
                handle_modal_key(app, key.code);
            } else {
                handle_key(app, key.code);
            }
        }
        _ => {}
    }
}

fn handle_modal_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let Some(modal) = app.modal.take() else {
                return;
            };
            match modal {
                Modal::Quit => app.should_quit = true,
                Modal::Oneway { normal, oneway } => {
                    let mut targets = normal;
                    targets.extend(oneway);
                    if !targets.is_empty() {
                        app.pending_apply = Some(targets);
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.modal = None,
        _ => {}
    }
}

fn handle_key(app: &mut App, code: KeyCode) {
    if app.busy.is_some() {
        match code {
            KeyCode::Char('h') | KeyCode::Left => {
                if app.focus == Focus::List {
                    app.focus_sidebar();
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if app.focus == Focus::Sidebar {
                    app.focus_list();
                }
            }
            KeyCode::Tab => match app.focus {
                Focus::Sidebar => app.focus_list(),
                Focus::List => app.focus_sidebar(),
            },
            KeyCode::Char('j') | KeyCode::Down => match app.focus {
                Focus::Sidebar => app.move_sidebar_cursor(1),
                Focus::List => app.move_cursor(1),
            },
            KeyCode::Char('k') | KeyCode::Up => match app.focus {
                Focus::Sidebar => app.move_sidebar_cursor(-1),
                Focus::List => app.move_cursor(-1),
            },
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Char('h') | KeyCode::Left => {
            if app.focus == Focus::List {
                app.focus_sidebar();
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if app.focus == Focus::Sidebar {
                app.focus_list();
            }
        }
        KeyCode::Tab => match app.focus {
            Focus::Sidebar => app.focus_list(),
            Focus::List => app.focus_sidebar(),
        },
        KeyCode::Char('j') | KeyCode::Down => match app.focus {
            Focus::Sidebar => app.move_sidebar_cursor(1),
            Focus::List => app.move_cursor(1),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focus {
            Focus::Sidebar => app.move_sidebar_cursor(-1),
            Focus::List => app.move_cursor(-1),
        },
        KeyCode::Enter => match app.focus {
            Focus::Sidebar => app.focus_list(),
            Focus::List => app.request_apply(),
        },
        KeyCode::Char(' ') if app.focus == Focus::List => app.toggle_hovered(),
        KeyCode::Char('a') if app.focus == Focus::List => app.select_category(true),
        KeyCode::Char('x') if app.focus == Focus::List => app.select_category(false),
        KeyCode::Char('A') => {
            let packs = &app.packs;
            app.selection.select_all(packs, true);
        }
        KeyCode::Char('X') | KeyCode::Char('c') => {
            let packs = &app.packs;
            app.selection.select_all(packs, false);
        }
        KeyCode::Char('1') => {
            let packs = &app.packs;
            app.selection.apply_preset(packs, Tier::Min, true);
        }
        KeyCode::Char('2') => {
            let packs = &app.packs;
            app.selection.apply_preset(packs, Tier::Std, true);
        }
        KeyCode::Char('3') => {
            let packs = &app.packs;
            app.selection.apply_preset(packs, Tier::Max, true);
        }
        KeyCode::Char('u') => app.request_reset(),
        KeyCode::Char('q') | KeyCode::Esc => app.request_quit(),
        _ => {}
    }
}

fn create_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    return Terminal::new(backend);
}

fn suspend(terminal: &mut Tui) -> io::Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    return terminal.show_cursor();
}

fn draw(frame: &mut Frame, app: &mut App) {
    let root = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(10),
        Constraint::Length(1),
    ])
    .split(frame.area());
    draw_header(frame, root[0], app);
    let body = Layout::horizontal([Constraint::Percentage(25), Constraint::Min(1)]).split(root[1]);
    draw_sidebar(frame, body[0], app);
    match app.screen {
        Screen::Pack(p) => draw_pack(frame, body[1], app, p),
        Screen::Declutter => draw_declutter(frame, body[1], app),
        Screen::Apps => draw_apps(frame, body[1], app),
        Screen::Activate => draw_console_placeholder(
            frame,
            body[1],
            "activate windows",
            "ENTER opens massgravel MAS in a new window",
        ),
    }
    draw_log(frame, root[2]);
    draw_footer(frame, root[3], app.modal.is_some(), app.busy);
    if let Some(modal) = &app.modal {
        let selected = app.selection.ids().len();
        draw_modal(frame, frame.area(), modal, selected);
    }
}

fn draw_log(frame: &mut Frame, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = sys::log_tail(inner_height.max(1))
        .into_iter()
        .map(Line::from)
        .collect();
    let paragraph = Paragraph::new(lines).block(Block::bordered().title(" log "));
    frame.render_widget(paragraph, area);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let admin = if app.admin {
        Span::styled("admin", Style::default().fg(Color::Green))
    } else {
        Span::styled("no admin", Style::default().fg(Color::Red))
    };
    let selected = app.selection.ids().len();
    let line = Line::from(vec![
        Span::styled(
            "winforge",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(app.version.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw("  build "),
        Span::styled(app.build.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        admin,
        Span::raw(format!("  selected: {selected}")),
        Span::styled(
            "   (enter apply, u undo, q quit)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut items: Vec<ListItem> = Vec::new();
    for &p in &app.normal {
        items.push(ListItem::new(Line::from(Span::raw(
            app.packs[p].meta.title.clone(),
        ))));
    }
    items.push(ListItem::new(Line::from(Span::raw("Declutter"))));
    items.push(ListItem::new(Line::from(Span::raw("Apps (winget)"))));
    items.push(ListItem::new(Line::from(Span::raw("Activate Windows"))));
    let highlight = if matches!(app.focus, Focus::Sidebar) {
        Style::default()
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let border = if matches!(app.focus, Focus::Sidebar) {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(Block::bordered().title(" categories ").border_style(border))
        .highlight_style(highlight)
        .highlight_symbol(if matches!(app.focus, Focus::Sidebar) {
            "> "
        } else {
            "  "
        });
    frame.render_stateful_widget(list, area, &mut app.sidebar_state);
}

fn draw_pack(frame: &mut Frame, area: Rect, app: &mut App, p: usize) {
    let mut items: Vec<ListItem> = Vec::new();
    for t in 0..app.packs[p].tweak.len() {
        items.push(ListItem::new(tweak_line(app, p, t)));
    }
    let focused = matches!(app.focus, Focus::List);
    let title = pack_title(app, p);
    let border = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let highlight = if focused {
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(Block::bordered().title(title).border_style(border))
        .highlight_style(highlight)
        .highlight_symbol(if focused { "> " } else { "  " });
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_declutter(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    let banner = Paragraph::new(Line::from(Span::styled(
        "ONE-WAY - CANNOT BE UNDONE",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(banner, chunks[0]);
    let mut items: Vec<ListItem> = Vec::new();
    for &p in &app.oneway {
        items.push(ListItem::new(pack_title(app, p)));
        for t in 0..app.packs[p].tweak.len() {
            items.push(ListItem::new(tweak_line(app, p, t)));
        }
    }
    let list = List::new(items)
        .block(Block::bordered().title(" declutter "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

fn draw_apps(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    let (banner, color) = if app.winget_ready == Some(true) {
        (
            "winget ready - space select, enter install (results in log)",
            Color::Green,
        )
    } else {
        (
            "winget NOT found - enter bootstraps or repairs it (internet)",
            Color::Yellow,
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            banner.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );
    let mut items: Vec<ListItem> = Vec::new();
    for group in app.groups.iter() {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{}  [{} apps]", group.meta.title, group.app.len()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))));
        for app_item in group.app.iter() {
            let mark = if app.selection.contains(&app_item.id) {
                "[x]"
            } else {
                "[ ]"
            };
            let result = app
                .app_results
                .get(&app_item.id)
                .map(|text| format!("  - {text}"))
                .unwrap_or_default();
            items.push(ListItem::new(Line::from(Span::raw(format!(
                "{mark} {} ({}){result}",
                app_item.name, app_item.id
            )))));
        }
    }
    let focused = matches!(app.focus, Focus::List);
    let border = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let highlight = if focused {
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" apps (winget) ")
                .border_style(border),
        )
        .highlight_style(highlight)
        .highlight_symbol(if focused { "> " } else { "  " });
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

fn draw_console_placeholder(frame: &mut Frame, area: Rect, title: &str, hint: &str) {
    let text = vec![
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::raw(hint.to_string())),
    ];
    frame.render_widget(Paragraph::new(text).block(Block::bordered()), area);
}

fn draw_footer(frame: &mut Frame, area: Rect, modal: bool, busy: Option<&'static str>) {
    let text = if let Some(label) = busy {
        format!("working: {label}")
    } else if modal {
        String::from("y = confirm  n/Esc = cancel")
    } else {
        String::from(
            "h/l focus pane  j/k move  space toggle  a/A select all  x/X clear  1/2/3 preset  enter apply  u undo  q quit",
        )
    };
    let style = if busy.is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(Span::styled(text, style)), area);
}

fn draw_modal(frame: &mut Frame, area: Rect, modal: &Modal, selected: usize) {
    let (title, lines, color) = match modal {
        Modal::Quit => (
            " confirm quit ",
            vec![
                Line::from(format!(
                    "selection holds {selected} tweak(s); nothing has been applied"
                )),
                Line::default(),
                Line::from(Span::styled(
                    "quit anyway? y/n",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
            ],
            Color::Yellow,
        ),
        Modal::Oneway { normal, oneway } => {
            let count: usize = normal
                .iter()
                .chain(oneway.iter())
                .map(|t| t.ids.len())
                .sum();
            (
                " one-way apply ",
                vec![
                    Line::from(Span::styled(
                        "ONE-WAY - CANNOT BE UNDONE",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(format!(
                        "about to apply {count} tweak(s) that cannot be reset"
                    )),
                    Line::default(),
                    Line::from(Span::styled(
                        "continue? y/n",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )),
                ],
                Color::Red,
            )
        }
    };
    let width = 60.min(area.width.saturating_sub(2));
    let height = 7.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect {
        x,
        y,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered().title(Span::styled(
        title,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        rect,
    );
}

fn pack_title(app: &App, p: usize) -> Line<'static> {
    let pack = &app.packs[p];
    let mut spans = vec![Span::styled(
        pack.meta.title.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    let total = pack.tweak.len();
    match app.statuses.get(&pack.meta.id) {
        Some(statuses) if statuses.len() >= total => {
            let summary = engine::summarize(statuses);
            spans.push(Span::raw(format!(
                "  applied {}/{}",
                summary.applied, summary.total
            )));
            let letters = [
                Status::Applied,
                Status::Default,
                Status::Mixed,
                Status::Unknown,
                Status::Absent,
            ];
            for (count, status) in summary.counts.iter().zip(letters) {
                let (letter, color) = status_letter(status);
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!("{letter}{count}"),
                    Style::default().fg(color),
                ));
            }
        }
        Some(statuses) => spans.push(Span::styled(
            format!("  loading {}/{}", statuses.len(), total),
            Style::default().fg(Color::Yellow),
        )),
        None => spans.push(Span::styled(
            format!("  loading 0/{total}"),
            Style::default().fg(Color::Yellow),
        )),
    }
    return Line::from(spans);
}

fn tweak_line(app: &App, pack_index: usize, tweak_index: usize) -> Line<'static> {
    let pack = &app.packs[pack_index];
    let tweak = &pack.tweak[tweak_index];
    let (mark, mark_color) = if app.selection.contains(&tweak.id) {
        ("[x]", Color::Green)
    } else {
        ("[ ]", Color::DarkGray)
    };
    let (letter, letter_color) = match app
        .statuses
        .get(&pack.meta.id)
        .and_then(|statuses| statuses.get(tweak_index))
        .copied()
    {
        Some(status) => status_letter(status),
        None => ("?", Color::DarkGray),
    };
    let mut spans = vec![
        Span::styled(mark.to_string(), Style::default().fg(mark_color)),
        Span::raw(" "),
        Span::styled(
            letter.to_string(),
            Style::default()
                .fg(letter_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::raw(tweak.name.clone()),
    ];
    if let Some(desc) = &tweak.desc {
        spans.push(Span::styled(
            format!(" ({desc})"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    return Line::from(spans);
}

fn status_letter(status: Status) -> (&'static str, Color) {
    match status {
        Status::Applied => ("A", Color::Green),
        Status::Default => ("D", Color::Gray),
        Status::Mixed => ("M", Color::Red),
        Status::Unknown => ("U", Color::Blue),
        Status::Absent => ("X", Color::DarkGray),
    }
}

fn run_refresh() {
    let Some(script) = catalog::script("refresh.ps1") else {
        return;
    };
    sys::log("running refresh.ps1...");
    match sys::run_script(script, sys::TIMEOUT_SCRIPT) {
        Ok(out) if out.status.success() => {
            sys::log("refresh.ps1 done");
        }
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1);
            sys::log(&format!("refresh.ps1 exit {code}"));
            sys::log(&format!("winforge: refresh exited {code}"));
        }
        Err(err) => {
            sys::log(&format!("refresh.ps1 failed: {err}"));
            sys::log(&format!("winforge: refresh failed: {err}"));
        }
    }
}

fn warn(text: &str) -> String {
    return colored("33", text);
}

fn dim(text: &str) -> String {
    return colored("90", text);
}

fn colored(code: &str, text: &str) -> String {
    return format!("\x1b[{code}m{text}{RESET}");
}

fn prompt(text: &str) -> Option<String> {
    print!("{text} ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    return match io::stdin().lock().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim().to_string()),
        Err(_) => None,
    };
}

fn pause() {
    let _ = prompt(&dim("ENTER = back"));
}
