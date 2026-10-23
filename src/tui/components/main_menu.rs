//! Upstream `MainMenu.tsx`.

use super::list::{Entry, Item, List, Props};
use crate::tui::ink::layout::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainMenuOption {
    Lines,
    Colors,
    Powerline,
    TerminalConfig,
    GlobalOverrides,
    Install,
    ManageInstallation,
    CheckUpdates,
    ConfigureStatusLine,
    ExportConfig,
    ImportConfig,
    StarGithub,
    Save,
    Exit,
}

/// `installation.method`, the part of the installation metadata the menu reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallMethod {
    AutoUpdate,
    Pinned,
    SelfManaged,
    Unknown,
}

fn uses_manage_installation(m: Option<InstallMethod>) -> bool {
    matches!(m, Some(InstallMethod::Pinned) | Some(InstallMethod::SelfManaged))
}

fn installation_item(installed: bool, method: Option<InstallMethod>) -> Entry<MainMenuOption> {
    if !installed {
        return Entry::new("📦 Install to Claude Code", MainMenuOption::Install)
            .description("Add ccstatusline to your Claude Code settings for automatic status line rendering");
    }
    if uses_manage_installation(method) {
        return Entry::new("🧰 Manage Installation", MainMenuOption::ManageInstallation)
            .description("Check pinned global package updates or uninstall ccstatusline");
    }
    Entry::new("🔌 Uninstall from Claude Code", MainMenuOption::Install)
        .description("Remove ccstatusline from your Claude Code settings")
}

pub fn build_items(installed: bool, has_changes: bool, method: Option<InstallMethod>) -> Vec<Item<MainMenuOption>> {
    use MainMenuOption::*;
    let e = |label: &str, v: MainMenuOption, d: &str| Item::Entry(Entry::new(label, v).description(d));
    let mut items = vec![
        e(
            "📝 Edit Lines",
            Lines,
            "Configure any number of status lines with various widgets like model info, git status, and token usage",
        ),
        e(
            "🎨 Edit Colors",
            Colors,
            "Customize colors for each widget including foreground, background, and bold styling",
        ),
        e(
            "⚡ Powerline Setup",
            Powerline,
            "Install Powerline fonts for enhanced visual separators and symbols in your status line",
        ),
        Item::Separator,
        e("💻 Terminal Options", TerminalConfig, "Configure terminal-specific settings for optimal display"),
        e(
            "🌐 Global Overrides",
            GlobalOverrides,
            "Set global padding, separators, and color overrides that apply to all widgets",
        ),
        Item::Entry(
            Entry::new("🔧 Configure Status Line", ConfigureStatusLine)
                .sublabel(if installed { None } else { Some("(install first)".into()) })
                .disabled(!installed)
                .description("Configure Claude Code status line settings like refresh interval"),
        ),
        Item::Separator,
        e("📤 Export Config", ExportConfig, "Save your current configuration to a JSON file for backup or sharing"),
        e("📥 Import Config", ImportConfig, "Load configuration from a previously exported JSON file"),
        Item::Separator,
        Item::Entry(installation_item(installed, method)),
    ];
    let star = e(
        "⭐ Like ccstatusline? Star us on GitHub",
        StarGithub,
        "Open the ccstatusline GitHub repository in your browser so you can star the project",
    );
    if has_changes {
        items.extend([
            Item::Separator,
            e("💾 Save & Exit", Save, "Save all changes and exit the configuration tool"),
            e("❌ Exit without saving", Exit, "Exit without saving your changes"),
            Item::Separator,
            star,
        ]);
    } else {
        items.extend([Item::Separator, e("🚪 Exit", Exit, "Exit the configuration tool"), Item::Separator, star]);
    }
    items
}

/// `getMainMenuSelectionIndex`.
pub fn selection_index(items: &[Item<MainMenuOption>], option: MainMenuOption) -> usize {
    let mut index = 0;
    for item in items {
        if let Item::Entry(e) = item {
            if e.value == option {
                return index;
            }
            if !e.disabled {
                index += 1;
            }
        }
    }
    0
}

/// `getMainMenuInstallSelectionIndex`.
pub fn install_selection_index(installed: bool, method: Option<InstallMethod>) -> usize {
    let option = if installed && uses_manage_installation(method) {
        MainMenuOption::ManageInstallation
    } else {
        MainMenuOption::Install
    };
    selection_index(&build_items(installed, false, method), option)
}

pub fn view(list: &List, items: &[Item<MainMenuOption>], initial: usize, show_truncation_warning: bool) -> El {
    let mut children = Vec::new();
    if show_truncation_warning {
        children.push(boxed(
            BoxStyle::default().margin_bottom(1.0),
            vec![text(
                TextStyle::color("yellow"),
                vec![s("⚠ Some lines are truncated, see Terminal Options → Terminal Width for info")],
            )],
        ));
    }
    children.push(text(TextStyle::bold(), vec![s("Main Menu")]));
    let mut p = Props::new(items);
    p.initial_selection = initial;
    p.margin_top = 1.0;
    children.push(list.view(&p));
    col(children)
}

pub fn props(items: &[Item<MainMenuOption>], initial: usize) -> Props<'_, MainMenuOption> {
    let mut p = Props::new(items);
    p.initial_selection = initial;
    p.margin_top = 1.0;
    p
}
