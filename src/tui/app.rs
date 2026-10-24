//! Upstream `App.tsx`: the editor's state, the screens it switches between, the global keys, and the frame
//! every screen shares (title, notices, preview).
//!
//! Settings are held as the JSON object upstream holds (zod's output, keys in insertion order) and edited the
//! way the TSX edits it, so a save writes the same bytes; the typed `Settings` is parsed from it for the
//! preview.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::claude;
use super::components::confirm::{Answer, Confirm};
use super::components::flow_notice::FlowNotice;
use super::components::items_editor::{self, ItemsEditor};
use super::components::line_selector::{self, LineSelector};
use super::components::list::{Event, List};
use super::components::main_menu::{self, MainMenuOption};
use super::components::powerline_setup::fonts;
use super::components::preview;
use super::ink::keys::Input;
use super::ink::layout::*;
use super::screens::{self, Action};
use crate::settings::{ConfigStore, Settings};

const GITHUB_REPO_URL: &str = "https://github.com/sirmalloc/ccstatusline";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Main,
    Lines,
    Items,
    ColorLines,
    Colors,
    Install,
    ManageInstallation,
    TerminalConfig,
    TerminalWidth,
    GlobalOverrides,
    Powerline,
    RefreshInterval,
    ExportConfig,
    ImportConfig,
    Confirm,
    FlowNotice,
    /// A screen this build does not draw yet (its PR has not landed): blank, ESC returns to the main menu.
    Pending,
}

#[derive(Clone, Debug)]
struct Flash {
    text: String,
    color: &'static str,
    until: Instant,
}

#[derive(Clone, Debug)]
enum ConfirmAction {
    /// `buildInvalidConfigSaveConfirm`: save (and exit for Save & Exit) after the user agrees.
    SaveGuard {
        return_screen: Screen,
        then_exit: bool,
    },
    StarGithub,
    /// `handleInstallSelection`'s action: write the status line command.
    Install {
        command: String,
    },
    /// `handleUninstallSelection`'s action.
    Uninstall,
}

#[derive(Clone, Debug)]
struct ConfirmState {
    message: String,
    action: ConfirmAction,
    cancel_screen: Option<Screen>,
}

/// When the App calls Ink's `exit()`: at once (the last frame stays as it was) or after one more frame, when
/// upstream re-renders before an awaited save finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    No,
    Now,
    AfterRender,
}

pub struct App {
    store: ConfigStore,
    settings: Value,
    original: String,
    has_changes: bool,
    config_load_error: Option<&'static str>,
    screen: Screen,
    menu_selections: HashMap<&'static str, usize>,
    confirm: Option<ConfirmState>,
    confirm_ui: Confirm,
    installed: bool,
    existing_status_line: Option<String>,
    /// `currentRefreshInterval`: Claude's `statusLine.refreshInterval`, read at mount.
    refresh_interval: Option<Value>,
    /// `supportsRefreshInterval`: `claude --version` is at least 2.1.97, checked once at mount.
    supports_refresh_interval: bool,
    /// `flowNotice` with its `continueScreen`.
    flow_notice: Option<(FlowNotice, Screen)>,
    pub terminal_width: usize,
    flash: Option<Flash>,
    main_list: List,
    selected_line: usize,
    line_selector: LineSelector,
    items_editor: ItemsEditor,
    /// The screen hosted through `screens::Screen` (colors, terminal, overrides, powerline, ...).
    hosted: Option<Box<dyn screens::Screen>>,
    install_menu: super::components::install_menu::InstallMenu,
    manage: super::components::install_menu::ManageInstallation,
    pub exit: Exit,
    messages: Vec<String>,
    /// `powerlineFontStatus.installed`.
    fonts_installed: bool,
    /// A `checkPowerlineFontsAsync()` still running.
    font_check: Option<Receiver<bool>>,
    installing_fonts: bool,
    /// When the install runs: upstream defers it 50 ms so the "Installing..." frame is drawn first.
    font_install_at: Option<Instant>,
    font_install_message: Option<String>,
}

/// `void checkPowerlineFontsAsync().then(setPowerlineFontStatus)`, off the event loop.
fn start_font_check() -> Receiver<bool> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fonts::check_async());
    });
    rx
}

/// `JSON.stringify(v)`.
fn json(v: &Value) -> String {
    crate::jsobj::stringify(v)
}

impl App {
    /// The App's mount: load the settings and Claude's status line state.
    pub fn new(mut store: ConfigStore, terminal_width: usize) -> App {
        let loaded = store.load();
        let config_load_error = store.load_error;
        let messages = store.captured.take().unwrap_or_default();
        crate::colors::set_chalk_level(loaded.color_level);
        let settings = crate::settings::settings_json(&loaded);
        let original = json(&settings);
        let installed = claude::is_installed();
        let existing_status_line = claude::existing_status_line();
        let refresh_interval = claude::refresh_interval();
        let supports_refresh_interval = claude::is_claude_code_version_at_least("2.1.97");
        App {
            store,
            settings,
            original,
            has_changes: false,
            config_load_error,
            screen: Screen::Main,
            menu_selections: HashMap::new(),
            confirm: None,
            confirm_ui: Confirm::default(),
            installed,
            existing_status_line,
            refresh_interval,
            supports_refresh_interval,
            flow_notice: None,
            terminal_width,
            flash: None,
            main_list: List::new(0),
            selected_line: 0,
            line_selector: LineSelector::new(),
            items_editor: ItemsEditor::new(),
            hosted: None,
            install_menu: super::components::install_menu::InstallMenu::new(),
            manage: super::components::install_menu::ManageInstallation::new(),
            exit: Exit::No,
            messages,
            // The sync check on startup, then the async one.
            fonts_installed: fonts::check(),
            font_check: Some(start_font_check()),
            installing_fonts: false,
            font_install_at: None,
            font_install_message: None,
        }
    }

    /// What loading printed, for the caller to show above the first frame.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    fn typed(&self) -> Settings {
        crate::settings::parse_settings(&crate::jsobj::strip_undefined(&self.settings)).unwrap_or_default()
    }

    fn set_settings(&mut self, v: Value) {
        self.settings = v;
        self.has_changes = json(&self.settings) != self.original;
    }

    fn set_flash(&mut self, text: impl Into<String>, color: &'static str) {
        self.flash = Some(Flash { text: text.into(), color, until: Instant::now() + Duration::from_millis(2000) });
    }

    /// How long until a timer fires (the flash message clearing), for the event loop's wait.
    pub fn next_timer(&self) -> Option<Duration> {
        let now = Instant::now();
        [
            self.flash.as_ref().map(|f| f.until.saturating_duration_since(now)),
            self.font_install_at.map(|at| at.saturating_duration_since(now)),
            // Poll a running font check.
            self.font_check.as_ref().map(|_| Duration::from_millis(20)),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn tick(&mut self) {
        if self.flash.as_ref().is_some_and(|f| Instant::now() >= f.until) {
            self.flash = None;
        }
        if let Some(rx) = &self.font_check {
            match rx.try_recv() {
                Ok(installed) => {
                    self.fonts_installed = installed;
                    self.font_check = None;
                }
                Err(TryRecvError::Disconnected) => self.font_check = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.font_install_at.is_some_and(|at| Instant::now() >= at) {
            self.font_install_at = None;
            let (_, message) = fonts::install();
            self.installing_fonts = false;
            self.font_install_message = Some(message);
            // Refresh font status
            self.font_check = Some(start_font_check());
        }
    }

    fn installation_method(&self) -> main_menu::InstallMethod {
        claude::current_installation(
            self.installed,
            self.existing_status_line.as_deref(),
            self.settings.get("installation"),
        )
    }

    /// `saveSettings(settings)`: `{...settings, version}` written atomically, then the widget hooks synced.
    fn save(&mut self) -> bool {
        let mut v = self.settings.clone();
        if let Some(o) = v.as_object_mut() {
            o.insert("version".into(), crate::jsjson::num(crate::settings::CURRENT_VERSION));
        }
        match self.store.write_settings_value(&crate::jsobj::strip_undefined(&v)) {
            Ok(()) => {
                crate::hooks::sync_widget_hooks(&self.typed());
                true
            }
            Err(_) => false,
        }
    }

    fn perform_save(&mut self) {
        if self.save() {
            self.original = json(&self.settings);
            self.has_changes = false;
            self.config_load_error = None;
            self.set_flash("✓ Configuration saved", "green");
        } else {
            self.set_flash("✗ Could not save configuration", "red");
        }
    }

    fn save_and_exit(&mut self, exit: Exit) {
        if self.save() {
            self.exit = exit;
        } else {
            self.set_flash("✗ Could not save configuration", "red");
        }
    }

    fn invalid_config_guard(&self, return_screen: Screen, then_exit: bool) -> Option<ConfirmState> {
        let err = self.config_load_error?;
        Some(ConfirmState {
            message: format!(
                "{err} and is preserved on disk. Saving replaces it with the current configuration. Continue?"
            ),
            action: ConfirmAction::SaveGuard { return_screen, then_exit },
            cancel_screen: Some(return_screen),
        })
    }

    fn open_confirm(&mut self, c: ConfirmState) {
        self.confirm = Some(c);
        self.confirm_ui = Confirm::default();
        self.screen = Screen::Confirm;
    }

    /// One stdin chunk, delivered to every active `useInput` handler.
    pub fn handle(&mut self, ev: &Input) {
        if ev.is_ctrl_c() {
            // Ink's App exits on Ctrl+C before (and instead of) the handlers.
            self.exit = Exit::Now;
            return;
        }
        let screen_before = self.screen;
        match self.screen {
            Screen::Main => self.handle_main(ev),
            Screen::Lines | Screen::ColorLines => self.handle_lines(ev),
            Screen::Items => self.handle_items(ev),
            Screen::Install => self.handle_install(ev),
            Screen::ManageInstallation => self.handle_manage(ev),
            Screen::FlowNotice => {
                if let Some((notice, next)) = self.flow_notice.as_mut() {
                    let next = *next;
                    if notice.handle(ev) {
                        self.flow_notice = None;
                        self.go(next);
                    }
                }
            }
            s if is_hosted(s) => self.handle_hosted(ev),
            Screen::Confirm => self.handle_confirm(ev),
            Screen::Pending => {
                if ev.key.escape {
                    self.screen = Screen::Main;
                }
            }
            _ => self.handle_hosted(ev),
        }
        // The App's own handler: Ctrl+S saves from any screen but the confirm dialog.
        if ev.key.ctrl && ev.input == "s" && screen_before != Screen::Confirm && self.exit == Exit::No {
            let return_screen = screen_before;
            match self.invalid_config_guard(return_screen, false) {
                Some(guard) => self.open_confirm(guard),
                None => self.perform_save(),
            }
        }
    }

    fn main_items(&self) -> Vec<super::components::list::Item<MainMenuOption>> {
        main_menu::build_items(self.installed, self.has_changes, Some(self.installation_method()))
    }

    fn handle_main(&mut self, ev: &Input) {
        let items = self.main_items();
        let initial = self.menu_selections.get("main").copied().unwrap_or(0);
        let p = main_menu::props(&items, initial);
        self.main_list.sync(&p);
        let Event::Selected(value, index) = self.main_list.handle(&p, ev) else { return };
        if value != MainMenuOption::Save && value != MainMenuOption::Exit {
            self.menu_selections.insert("main", index);
        }
        match value {
            MainMenuOption::StarGithub => self.open_confirm(ConfirmState {
                message: format!("Open the ccstatusline GitHub repository in your browser?\n\n{GITHUB_REPO_URL}"),
                action: ConfirmAction::StarGithub,
                cancel_screen: None,
            }),
            MainMenuOption::Save => match self.invalid_config_guard(Screen::Main, true) {
                Some(guard) => self.open_confirm(guard),
                None => self.save_and_exit(Exit::Now),
            },
            MainMenuOption::Exit => self.exit = Exit::Now,
            MainMenuOption::Lines => self.go(Screen::Lines),
            MainMenuOption::Colors => self.go(Screen::ColorLines),
            MainMenuOption::TerminalConfig => self.go(Screen::TerminalConfig),
            MainMenuOption::GlobalOverrides => self.go(Screen::GlobalOverrides),
            MainMenuOption::Powerline => self.go(Screen::Powerline),
            MainMenuOption::ConfigureStatusLine => self.go(Screen::RefreshInterval),
            MainMenuOption::ExportConfig => self.go(Screen::ExportConfig),
            MainMenuOption::ImportConfig => self.go(Screen::ImportConfig),
            MainMenuOption::Install => {
                if self.installed {
                    self.open_uninstall_confirm(Screen::Main);
                } else {
                    self.install_menu = super::components::install_menu::InstallMenu::new();
                    self.screen = Screen::Install;
                }
            }
            MainMenuOption::ManageInstallation => {
                self.manage = super::components::install_menu::ManageInstallation::new();
                self.screen = Screen::ManageInstallation;
            }
            _ => self.screen = Screen::Pending,
        }
    }

    /// Switching screens mounts the new screen's components afresh.
    fn go(&mut self, screen: Screen) {
        self.hosted = None;
        match screen {
            Screen::Lines | Screen::ColorLines => self.line_selector = LineSelector::new(),
            Screen::Items => self.items_editor = ItemsEditor::new(),
            s if is_hosted(s) => {
                let typed = self.typed();
                let ctx = self.ctx(&typed);
                let hosted: Box<dyn screens::Screen> = match s {
                    Screen::Colors => Box::new(super::components::color_menu::ColorMenu::new(&ctx)),
                    Screen::TerminalConfig => {
                        Box::new(super::components::terminal_options::TerminalOptionsMenu::new(&ctx))
                    }
                    Screen::TerminalWidth => Box::new(super::components::terminal_width::TerminalWidthMenu::new(&ctx)),
                    Screen::GlobalOverrides => {
                        Box::new(super::components::global_overrides::GlobalOverridesMenu::new(&ctx))
                    }
                    Screen::Powerline => Box::new(super::components::powerline_setup::PowerlineSetup::new(&ctx)),
                    Screen::RefreshInterval => Box::new(super::components::refresh_interval::RefreshIntervalMenu::new(
                        &ctx,
                        self.refresh_interval.clone(),
                        self.supports_refresh_interval,
                    )),
                    Screen::ExportConfig => Box::new(super::components::export_config::ExportConfigDialog::new(&ctx)),
                    _ => Box::new(super::components::import_config::ImportConfigDialog::new(&ctx)),
                };
                self.hosted = Some(hosted);
            }
            _ => {}
        }
        self.screen = screen;
    }

    fn ctx<'a>(&'a self, typed: &'a Settings) -> screens::Ctx<'a> {
        screens::Ctx {
            settings: &self.settings,
            typed,
            terminal_width: self.terminal_width,
            selected_line: self.selected_line,
            powerline_fonts_installed: self.fonts_installed,
            installing_fonts: self.installing_fonts,
            font_install_message: self.font_install_message.as_deref(),
        }
    }

    fn handle_hosted(&mut self, ev: &Input) {
        let typed = self.typed();
        let actions = match self.hosted.as_mut() {
            Some(h) => {
                let ctx = screens::Ctx {
                    settings: &self.settings,
                    typed: &typed,
                    terminal_width: self.terminal_width,
                    selected_line: self.selected_line,
                    powerline_fonts_installed: self.fonts_installed,
                    installing_fonts: self.installing_fonts,
                    font_install_message: self.font_install_message.as_deref(),
                };
                h.handle(ev, &ctx)
            }
            None => Vec::new(),
        };
        for action in actions {
            self.apply(action);
        }
    }

    /// Where each hosted screen's `onBack` leads, with the main menu selection the App records on the way.
    fn back_from_hosted(&mut self) {
        match self.screen {
            Screen::Colors => self.go(Screen::ColorLines),
            Screen::TerminalWidth => self.go(Screen::TerminalConfig),
            Screen::TerminalConfig => {
                self.menu_selections.insert("main", 3);
                self.go(Screen::Main);
            }
            Screen::GlobalOverrides => {
                self.menu_selections.insert("main", 4);
                self.go(Screen::Main);
            }
            _ => self.go(Screen::Main),
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SetSettings(v) => self.set_settings(v),
            Action::SetLineWidgets(widgets) => {
                let mut lines = self.lines();
                if self.selected_line < lines.len() {
                    lines[self.selected_line] = Value::Array(widgets);
                    self.set_lines(lines);
                }
            }
            Action::Back => self.back_from_hosted(),
            Action::OpenTerminalWidth => self.go(Screen::TerminalWidth),
            Action::Flash(text, color) => self.set_flash(text, color),
            Action::SetRefreshInterval(interval) => {
                let previous = self.refresh_interval.take();
                self.refresh_interval = interval.map(crate::jsjson::num);
                if claude::set_refresh_interval(interval).is_ok() {
                    self.set_flash("✓ Refresh interval updated", "green");
                } else {
                    self.refresh_interval = previous;
                    self.set_flash("✗ Failed to save refresh interval", "red");
                }
                self.go(Screen::Main);
            }
            Action::FlowNotice { title, message, color } => {
                self.flow_notice = Some((FlowNotice::new(title, message, color), Screen::Main));
                self.go(Screen::FlowNotice);
            }
            Action::InstallFonts => {
                self.installing_fonts = true;
                self.font_install_at = Some(Instant::now() + Duration::from_millis(50));
            }
            Action::ClearFontMessage => self.font_install_message = None,
        }
    }

    fn lines(&self) -> Vec<Value> {
        crate::jsobj::get(&self.settings, "lines").and_then(Value::as_array).cloned().unwrap_or_default()
    }

    /// `updateLines`: `{...settings, lines}`.
    fn set_lines(&mut self, lines: Vec<Value>) {
        let next = crate::jsobj::with(&self.settings, "lines", Value::Array(lines));
        self.set_settings(next);
    }

    fn line_config(&self) -> (Option<&'static str>, bool, Option<String>) {
        if self.screen == Screen::ColorLines {
            let t = self.typed();
            let managed = (t.powerline.enabled
                && t.powerline.theme.as_deref().is_some_and(|th| !th.is_empty() && th != "custom"))
            .then(|| t.powerline.theme.clone().unwrap_or_default());
            (Some("Select Line to Edit Colors"), false, managed)
        } else {
            (Some("Select Line to Edit Items"), true, None)
        }
    }

    fn handle_lines(&mut self, ev: &Input) {
        let lines = self.lines();
        let initial = self.menu_selections.get("lines").copied().unwrap_or(0);
        self.line_selector.sync(&lines, initial);
        let (title, allow_editing, managed) = self.line_config();
        let cfg = line_selector::Config { title, allow_editing, managed_theme: managed.as_deref() };
        match self.line_selector.handle(ev, &cfg) {
            line_selector::Outcome::Select(line) => {
                self.menu_selections.insert("lines", line);
                self.selected_line = line;
                let next = if self.screen == Screen::ColorLines { Screen::Colors } else { Screen::Items };
                self.go(next);
            }
            line_selector::Outcome::Back => {
                self.menu_selections.insert("main", if self.screen == Screen::ColorLines { 1 } else { 0 });
                self.screen = Screen::Main;
            }
            line_selector::Outcome::LinesUpdated(lines) => self.set_lines(lines),
            line_selector::Outcome::None => {}
        }
    }

    fn handle_items(&mut self, ev: &Input) {
        let typed = self.typed();
        let lines = self.lines();
        let widgets: Vec<Value> = lines.get(self.selected_line).and_then(Value::as_array).cloned().unwrap_or_default();
        match self.items_editor.handle(ev, &widgets, &typed) {
            items_editor::Outcome::Update(widgets) => {
                // `updateLine`: newLines[selectedLine] = widgets.
                let mut lines = lines;
                if self.selected_line < lines.len() {
                    lines[self.selected_line] = Value::Array(widgets);
                } else {
                    lines.resize(self.selected_line, crate::jsobj::undefined());
                    lines.push(Value::Array(widgets));
                }
                self.set_lines(lines);
            }
            items_editor::Outcome::Back => {
                self.menu_selections.insert("lines", self.selected_line);
                self.go(Screen::Lines);
            }
            items_editor::Outcome::None => {}
        }
    }

    fn handle_confirm(&mut self, ev: &Input) {
        let Some(c) = self.confirm.clone() else { return };
        match self.confirm_ui.handle(ev, false) {
            Answer::Yes => self.run_confirm_action(c.action),
            Answer::No => {
                self.screen = c.cancel_screen.unwrap_or(Screen::Main);
                self.confirm = None;
            }
            Answer::None => {}
        }
    }

    fn run_confirm_action(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::SaveGuard { return_screen, then_exit } => {
                self.confirm = None;
                self.screen = return_screen;
                if then_exit {
                    // Upstream awaits the save, so the main menu is drawn once before it exits.
                    self.save_and_exit(Exit::AfterRender);
                } else {
                    self.perform_save();
                }
            }
            ConfirmAction::Install { command } => self.run_install(&command),
            ConfirmAction::Uninstall => self.run_uninstall(),
            ConfirmAction::StarGithub => {
                if super::open_url::open_external_url(GITHUB_REPO_URL) {
                    self.set_flash("✓ Opened GitHub repository in browser", "green");
                } else {
                    self.set_flash(format!("✗ Could not open browser. Visit: {GITHUB_REPO_URL}"), "red");
                }
                self.screen = Screen::Main;
                self.confirm = None;
            }
        }
    }

    fn install_command(&self) -> String {
        let custom = self.store.is_custom_path().then(|| self.store.path.clone());
        super::install::status_line_command(&super::install::base_command(), custom.as_deref())
    }

    fn handle_install(&mut self, ev: &Input) {
        let command = self.install_command();
        match self.install_menu.handle(ev, &command) {
            super::components::install_menu::Outcome::Select => self.open_install_confirm(command),
            super::components::install_menu::Outcome::Cancel => self.screen = Screen::Main,
            super::components::install_menu::Outcome::None => {}
        }
    }

    fn handle_manage(&mut self, ev: &Input) {
        match self.manage.handle(ev) {
            super::components::install_menu::Outcome::Select => self.open_uninstall_confirm(Screen::ManageInstallation),
            super::components::install_menu::Outcome::Cancel => {
                let index = main_menu::install_selection_index(true, Some(self.installation_method()));
                self.menu_selections.insert("main", index);
                self.screen = Screen::Main;
            }
            super::components::install_menu::Outcome::None => {}
        }
    }

    /// `handleInstallSelection`: describe the side effects, then confirm.
    fn open_install_confirm(&mut self, command: String) {
        let existing = claude::existing_status_line();
        let already = existing.as_deref().is_some_and(claude::is_known_command);
        let path = crate::claude_config::settings_path().display().to_string();
        let side_effects = [
            format!("Claude settings path: {path}"),
            format!("Final statusLine.command: {command}"),
            format!("Hook command behavior: hook-enabled widgets run {command} --hook"),
        ]
        .join("\n");
        let message = match &existing {
            Some(e) if !already => {
                format!("A status line is already configured: \"{e}\"\n\n{side_effects}\n\nReplace it?")
            }
            _ if already => format!("ccstatusline is already installed.\n\n{side_effects}\n\nUpdate it?"),
            _ => format!("{side_effects}\n\nContinue?"),
        };
        self.open_confirm(ConfirmState {
            message,
            action: ConfirmAction::Install { command },
            cancel_screen: Some(Screen::Install),
        });
    }

    fn open_uninstall_confirm(&mut self, cancel: Screen) {
        let path = crate::claude_config::settings_path().display().to_string();
        self.open_confirm(ConfirmState {
            message: format!("This will remove ccstatusline from {path}. Continue?"),
            action: ConfirmAction::Uninstall,
            cancel_screen: Some(cancel),
        });
    }

    fn run_install(&mut self, command: &str) {
        let supports = claude::is_claude_code_version_at_least("2.1.97");
        match super::install::install(&crate::claude_config::settings_path(), command, supports) {
            Ok(()) => {
                // Hooks follow the saved config, as upstream re-reads it after installing.
                if let Some(saved) = self.saved_settings() {
                    crate::hooks::sync_widget_hooks(&saved);
                }
                self.installed = true;
                self.existing_status_line = claude::existing_status_line().or_else(|| Some(command.to_string()));
                let index = main_menu::install_selection_index(true, Some(self.installation_method()));
                self.menu_selections.insert("main", index);
                self.screen = Screen::Main;
                self.set_flash("✓ Installed to Claude Code", "green");
            }
            Err(message) => {
                self.flash = None;
                self.flow_notice = Some((
                    FlowNotice::new("Install Failed".into(), message, "red"),
                    Screen::Install,
                ));
                self.screen = Screen::FlowNotice;
            }
        }
        self.confirm = None;
    }

    fn run_uninstall(&mut self) {
        if !super::install::uninstall(&crate::claude_config::settings_path()) {
            self.messages.push("Warning: Could not read existing Claude settings.".into());
        }
        // saveInstallationMetadata(undefined): the config file without its installation record.
        if self.store.path.exists() {
            if let Some(saved) = self.saved_settings_json() {
                let mut v = crate::jsobj::without(&saved, "installation");
                v = crate::jsobj::with(&v, "version", crate::jsjson::num(crate::settings::CURRENT_VERSION));
                let _ = self.store.write_settings_value(&crate::jsobj::strip_undefined(&v));
            }
        }
        crate::hooks::remove_managed_hooks();
        self.installed = false;
        self.existing_status_line = None;
        self.settings = crate::jsobj::without(&self.settings, "installation");
        self.original =
            json(&crate::jsobj::without(&serde_json::from_str(&self.original).unwrap_or(Value::Null), "installation"));
        self.has_changes = json(&self.settings) != self.original;
        let index = main_menu::install_selection_index(false, None);
        self.menu_selections.insert("main", index);
        self.set_flash("✓ Uninstalled from Claude Code", "green");
        self.screen = Screen::Main;
        self.confirm = None;
    }

    /// The config as saved on disk (`loadSettings`), when it loads without error.
    fn saved_settings_json(&self) -> Option<Value> {
        let mut store = crate::settings::ConfigStore::new(Some(&self.store.path.to_string_lossy()));
        store.captured = Some(Vec::new());
        let st = store.load();
        store.load_error.is_none().then(|| crate::settings::settings_json(&st))
    }

    fn saved_settings(&self) -> Option<Settings> {
        let v = self.saved_settings_json()?;
        crate::settings::parse_settings(&v).ok()
    }

    pub fn view(&mut self) -> El {
        let typed = self.typed();
        let rendered = preview::render_lines(&typed, self.terminal_width);

        let mut header = vec![
            text(
                TextStyle::bold(),
                vec![Inline::Nested(
                    Transform::Gradient(super::ink::gradient::RETRO),
                    vec![s("CCStatusline Configuration")],
                )],
            ),
            text(TextStyle::bold(), vec![s(format!(" | v{}", crate::UPSTREAM_VERSION))]),
        ];
        if let Some(f) = &self.flash {
            header.push(text(TextStyle::color(f.color).with_bold(true), vec![s(format!("  {}", f.text))]));
        }
        let mut children = vec![boxed(BoxStyle::default().margin_bottom(1.0), header)];
        if let Some(err) = self.config_load_error {
            children.push(text(
                TextStyle::color("red"),
                vec![s(format!("⚠ {err} — showing defaults; saving here overwrites the file."))],
            ));
        }
        if self.store.is_custom_path() {
            children.push(text(TextStyle::dim(), vec![s(format!("Config: {}", self.store.path.display()))]));
        }
        children.push(preview::view(&rendered, self.terminal_width));

        let screen = match self.screen {
            Screen::Main => {
                let items = self.main_items();
                let initial = self.menu_selections.get("main").copied().unwrap_or(0);
                let p = main_menu::props(&items, initial);
                self.main_list.sync(&p);
                let warning = rendered.any_truncated && typed.flex_mode == "full-minus-40";
                Some(main_menu::view(&self.main_list, &items, initial, warning))
            }
            Screen::Confirm => {
                let message = self.confirm.as_ref().map(|c| c.message.clone());
                message.map(|m| self.confirm_ui.view(Some(&m), false, false))
            }
            Screen::Lines | Screen::ColorLines => {
                let lines = self.lines();
                let initial = self.menu_selections.get("lines").copied().unwrap_or(0);
                self.line_selector.sync(&lines, initial);
                let (title, allow_editing, managed) = self.line_config();
                let cfg = line_selector::Config { title, allow_editing, managed_theme: managed.as_deref() };
                Some(self.line_selector.view(&cfg))
            }
            Screen::Items => {
                let widgets: Vec<Value> =
                    self.lines().get(self.selected_line).and_then(Value::as_array).cloned().unwrap_or_default();
                Some(self.items_editor.view(&widgets, self.selected_line + 1, &typed))
            }
            s if is_hosted(s) => {
                let ctx = screens::Ctx {
                    settings: &self.settings,
                    typed: &typed,
                    terminal_width: self.terminal_width,
                    selected_line: self.selected_line,
                    powerline_fonts_installed: self.fonts_installed,
                    installing_fonts: self.installing_fonts,
                    font_install_message: self.font_install_message.as_deref(),
                };
                self.hosted.as_mut().map(|h| h.view(&ctx))
            }
            Screen::Install => {
                let command = self.install_command();
                let path = crate::claude_config::settings_path().display().to_string();
                let existing = self.existing_status_line.clone();
                Some(self.install_menu.view(&command, existing.as_deref(), &path))
            }
            Screen::ManageInstallation => {
                let current = match self.installation_method() {
                    main_menu::InstallMethod::Pinned => "Pinned global install",
                    main_menu::InstallMethod::SelfManaged => "Self-managed/global install",
                    main_menu::InstallMethod::AutoUpdate => "Auto-update",
                    main_menu::InstallMethod::Unknown => "Unknown installation",
                };
                Some(self.manage.view(current))
            }
            Screen::FlowNotice => self.flow_notice.as_mut().map(|(n, _)| n.view()),
            Screen::Pending => None,
            _ => None,
        };
        children.push(boxed(BoxStyle::default().margin_top(1.0), screen.into_iter().collect()));
        col(children)
    }
}

fn is_hosted(s: Screen) -> bool {
    matches!(
        s,
        Screen::Colors
            | Screen::TerminalConfig
            | Screen::TerminalWidth
            | Screen::GlobalOverrides
            | Screen::Powerline
            | Screen::RefreshInterval
            | Screen::ExportConfig
            | Screen::ImportConfig
    )
}
