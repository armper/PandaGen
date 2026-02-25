//! Shared command surface specification.
//!
//! This module is the single source of truth for command invocation patterns,
//! aliases, argument requirements, and palette metadata used by parser,
//! command palette registry, and prompt validation/suggestions.

use crate::help::HelpCategory;
use crate::ComponentType;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaletteDescriptorSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub tags: &'static [&'static str],
    pub category: &'static str,
    pub keybinding: Option<&'static str>,
    pub prompt_pattern: Option<&'static str>,
    pub requires_args: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaunchCommandSpec {
    pub token: &'static str,
    pub component_type: ComponentType,
    pub required_usage: Option<&'static str>,
    pub palette: PaletteDescriptorSpec,
}

pub(crate) const LAUNCH_COMMAND_SPECS: &[LaunchCommandSpec] = &[
    LaunchCommandSpec {
        token: "editor",
        component_type: ComponentType::Editor,
        required_usage: None,
        palette: PaletteDescriptorSpec {
            id: "open_editor",
            name: "Open Editor",
            description: "Open a file in the text editor",
            tags: &["editor", "open", "file"],
            category: "Workspace",
            keybinding: Some("Ctrl+O"),
            prompt_pattern: Some("open editor "),
            requires_args: false,
        },
    },
    LaunchCommandSpec {
        token: "cli",
        component_type: ComponentType::Cli,
        required_usage: None,
        palette: PaletteDescriptorSpec {
            id: "open_cli",
            name: "Open CLI",
            description: "Open an interactive CLI component",
            tags: &["cli", "open", "console"],
            category: "Workspace",
            keybinding: None,
            prompt_pattern: Some("open cli "),
            requires_args: false,
        },
    },
    LaunchCommandSpec {
        token: "pipeline",
        component_type: ComponentType::PipelineExecutor,
        required_usage: None,
        palette: PaletteDescriptorSpec {
            id: "open_pipeline",
            name: "Open Pipeline",
            description: "Open an interactive pipeline executor component",
            tags: &["pipeline", "open", "executor"],
            category: "Workspace",
            keybinding: None,
            prompt_pattern: Some("open pipeline "),
            requires_args: false,
        },
    },
    LaunchCommandSpec {
        token: "custom",
        component_type: ComponentType::Custom,
        required_usage: Some("Usage: open custom <entry> [args...]"),
        palette: PaletteDescriptorSpec {
            id: "open_custom",
            name: "Open Custom Host",
            description: "Open a custom host by entry name",
            tags: &["custom", "open", "entry"],
            category: "Workspace",
            keybinding: Some("Ctrl+Shift+O"),
            prompt_pattern: Some("open custom "),
            requires_args: true,
        },
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelperCommandKind {
    RecentFiles,
    OpenFilePicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelperCommandSpec {
    pub kind: HelperCommandKind,
    pub aliases: &'static [&'static [&'static str]],
    pub usage: &'static str,
    pub palette: PaletteDescriptorSpec,
}

const RECENT_ALIAS_1: &[&str] = &["recent"];
const RECENT_ALIAS_2: &[&str] = &["recent", "files"];
const RECENT_ALIAS_3: &[&str] = &["open", "recent"];
const RECENT_ALIAS_4: &[&str] = &["open", "recent", "files"];
const OPEN_FILE_ALIAS_1: &[&str] = &["open", "file"];
const OPEN_FILE_ALIAS_2: &[&str] = &["open", "file-picker"];

pub(crate) const HELPER_COMMAND_SPECS: &[HelperCommandSpec] = &[
    HelperCommandSpec {
        kind: HelperCommandKind::RecentFiles,
        aliases: &[RECENT_ALIAS_1, RECENT_ALIAS_2, RECENT_ALIAS_3, RECENT_ALIAS_4],
        usage: "Usage: recent",
        palette: PaletteDescriptorSpec {
            id: "recent",
            name: "Recent Files",
            description: "Show recently opened files",
            tags: &["recent", "history", "files"],
            category: "Workspace",
            keybinding: None,
            prompt_pattern: Some("recent"),
            requires_args: false,
        },
    },
    HelperCommandSpec {
        kind: HelperCommandKind::OpenFilePicker,
        aliases: &[OPEN_FILE_ALIAS_1, OPEN_FILE_ALIAS_2],
        usage: "Usage: open file",
        palette: PaletteDescriptorSpec {
            id: "open_file_picker",
            name: "Open File Picker",
            description: "Open the file picker component",
            tags: &["file", "picker", "open"],
            category: "Workspace",
            keybinding: None,
            prompt_pattern: Some("open file"),
            requires_args: false,
        },
    },
];

pub(crate) const NON_LAUNCH_PALETTE_SPECS: &[PaletteDescriptorSpec] = &[
    PaletteDescriptorSpec {
        id: "list",
        name: "List Components",
        description: "List all active components",
        tags: &["list", "components"],
        category: "Workspace",
        keybinding: Some("Ctrl+L"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "focus_next",
        name: "Next Component",
        description: "Focus the next component",
        tags: &["next", "focus"],
        category: "Workspace",
        keybinding: Some("Alt+Tab"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "focus_prev",
        name: "Previous Component",
        description: "Focus the previous component",
        tags: &["prev", "previous", "focus"],
        category: "Workspace",
        keybinding: Some("Alt+Shift+Tab"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "close",
        name: "Close Component",
        description: "Close a component by ID",
        tags: &["close", "kill"],
        category: "Workspace",
        keybinding: None,
        prompt_pattern: Some("close "),
        requires_args: true,
    },
    PaletteDescriptorSpec {
        id: "help",
        name: "Help",
        description: "Show help overview",
        tags: &["help", "?"],
        category: "Workspace",
        keybinding: Some("?"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "help_workspace",
        name: "Help: Workspace",
        description: "Show workspace commands help",
        tags: &["help", "workspace"],
        category: "Workspace",
        keybinding: None,
        prompt_pattern: Some("help workspace"),
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "help_editor",
        name: "Help: Editor",
        description: "Show editor commands help",
        tags: &["help", "editor"],
        category: "Editor",
        keybinding: None,
        prompt_pattern: Some("help editor"),
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "help_keys",
        name: "Help: Keyboard Shortcuts",
        description: "Show keyboard shortcuts reference",
        tags: &["help", "keys", "keyboard", "shortcuts"],
        category: "Workspace",
        keybinding: None,
        prompt_pattern: Some("help keys"),
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "help_system",
        name: "Help: System",
        description: "Show system commands help",
        tags: &["help", "system"],
        category: "System",
        keybinding: None,
        prompt_pattern: Some("help system"),
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "save",
        name: "Save File",
        description: "Save the current file",
        tags: &["save", "write"],
        category: "Editor",
        keybinding: Some("Ctrl+S"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "quit",
        name: "Quit Workspace",
        description: "Exit the workspace",
        tags: &["quit", "exit"],
        category: "System",
        keybinding: Some("Ctrl+Q"),
        prompt_pattern: None,
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "boot_profile_show",
        name: "Boot Profile: Show",
        description: "Show current boot profile configuration",
        tags: &["boot", "profile", "show"],
        category: "System",
        keybinding: None,
        prompt_pattern: Some("boot profile show"),
        requires_args: false,
    },
    PaletteDescriptorSpec {
        id: "boot_profile_set",
        name: "Boot Profile: Set",
        description: "Set boot profile (workspace/editor/kiosk)",
        tags: &["boot", "profile", "set"],
        category: "System",
        keybinding: None,
        prompt_pattern: Some("boot profile set "),
        requires_args: true,
    },
    PaletteDescriptorSpec {
        id: "boot_profile_save",
        name: "Boot Profile: Save",
        description: "Persist current boot profile configuration",
        tags: &["boot", "profile", "save"],
        category: "System",
        keybinding: None,
        prompt_pattern: Some("boot profile save"),
        requires_args: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComponentIdCommandSpec {
    pub token: &'static str,
    pub usage: &'static str,
}

pub(crate) const COMPONENT_ID_COMMAND_SPECS: &[ComponentIdCommandSpec] = &[
    ComponentIdCommandSpec {
        token: "focus",
        usage: "Usage: focus <component_id>",
    },
    ComponentIdCommandSpec {
        token: "close",
        usage: "Usage: close <component_id>",
    },
    ComponentIdCommandSpec {
        token: "status",
        usage: "Usage: status <component_id>",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelpTopicSpec {
    pub topic: HelpCategory,
    pub aliases: &'static [&'static str],
}

pub(crate) const HELP_TOPIC_SPECS: &[HelpTopicSpec] = &[
    HelpTopicSpec {
        topic: HelpCategory::Workspace,
        aliases: &["workspace"],
    },
    HelpTopicSpec {
        topic: HelpCategory::Editor,
        aliases: &["editor"],
    },
    HelpTopicSpec {
        topic: HelpCategory::Keys,
        aliases: &["keys", "keyboard", "shortcuts"],
    },
    HelpTopicSpec {
        topic: HelpCategory::System,
        aliases: &["system"],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenInvocationValidation {
    ValidPrefix,
    ValidComplete,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandInvocationValidation {
    ValidPrefix,
    ValidComplete,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SuggestionSpec {
    pub pattern: &'static str,
    pub description: &'static str,
}

pub(crate) const DEFAULT_SUGGESTIONS: &[SuggestionSpec] = &[
    SuggestionSpec {
        pattern: "open editor <path>",
        description: "Open file in editor",
    },
    SuggestionSpec {
        pattern: "open file",
        description: "Open file picker",
    },
    SuggestionSpec {
        pattern: "list",
        description: "List all components",
    },
    SuggestionSpec {
        pattern: "help",
        description: "Show help overview",
    },
    SuggestionSpec {
        pattern: "recent",
        description: "Show recent files",
    },
];

pub(crate) const OPEN_PREFIX_SUGGESTIONS: &[SuggestionSpec] = &[
    SuggestionSpec {
        pattern: "open editor <path>",
        description: "Open file in editor",
    },
    SuggestionSpec {
        pattern: "open file",
        description: "Open file picker",
    },
    SuggestionSpec {
        pattern: "open recent",
        description: "Show recent files",
    },
];

pub(crate) const RECENT_PREFIX_SUGGESTIONS: &[SuggestionSpec] = &[
    SuggestionSpec {
        pattern: "recent",
        description: "Show recent files",
    },
    SuggestionSpec {
        pattern: "open recent",
        description: "Show recent files",
    },
];

pub(crate) const HELP_PREFIX_SUGGESTIONS: &[SuggestionSpec] = &[
    SuggestionSpec {
        pattern: "help",
        description: "Overview",
    },
    SuggestionSpec {
        pattern: "help workspace",
        description: "Workspace commands",
    },
    SuggestionSpec {
        pattern: "help editor",
        description: "Editor commands",
    },
    SuggestionSpec {
        pattern: "help keys",
        description: "Keyboard shortcuts",
    },
    SuggestionSpec {
        pattern: "help system",
        description: "System commands",
    },
];

pub(crate) const VALID_PREFIX_GROUPS: &[&[&str]] = &[
    &["op", "ope"],
    &["li", "lis"],
    &["ne", "nex"],
    &["pr", "pre"],
    &["cl", "clo", "clos"],
    &["fo", "foc", "focu"],
    &["st", "sta", "stat", "statu"],
    &["he", "hel"],
    &["re", "rec", "rece", "recen"],
];

pub(crate) fn launch_command_by_token(token: &str) -> Option<&'static LaunchCommandSpec> {
    LAUNCH_COMMAND_SPECS.iter().find(|spec| spec.token == token)
}

pub(crate) fn helper_command_by_alias(parts: &[&str]) -> Option<&'static HelperCommandSpec> {
    HELPER_COMMAND_SPECS.iter().find(|spec| {
        spec.aliases
            .iter()
            .any(|alias_parts| *alias_parts == parts)
    })
}

pub(crate) fn helper_command_by_open_token(token: &str) -> Option<&'static HelperCommandSpec> {
    HELPER_COMMAND_SPECS.iter().find(|spec| {
        spec.aliases.iter().any(|alias_parts| {
            alias_parts.len() >= 2 && alias_parts[0] == "open" && alias_parts[1] == token
        })
    })
}

pub(crate) fn component_id_command_by_token(token: &str) -> Option<&'static ComponentIdCommandSpec> {
    COMPONENT_ID_COMMAND_SPECS
        .iter()
        .find(|spec| spec.token == token)
}

pub(crate) fn validate_component_id_invocation(parts: &[&str]) -> CommandInvocationValidation {
    if parts.is_empty() {
        return CommandInvocationValidation::Invalid;
    }

    if component_id_command_by_token(parts[0]).is_none() {
        return CommandInvocationValidation::Invalid;
    }

    if parts.len() == 1 {
        return CommandInvocationValidation::ValidPrefix;
    }
    if parts.len() == 2 && parts[1].starts_with("comp:") {
        return CommandInvocationValidation::ValidComplete;
    }

    CommandInvocationValidation::Invalid
}

pub(crate) fn parse_help_topic(topic_token: Option<&str>) -> Option<HelpCategory> {
    match topic_token {
        None => Some(HelpCategory::Overview),
        Some(token) => {
            if token.eq_ignore_ascii_case("overview") {
                return Some(HelpCategory::Overview);
            }
            HELP_TOPIC_SPECS
                .iter()
                .find(|spec| {
                    spec.aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(token))
                })
                .map(|spec| spec.topic)
        }
    }
}

pub(crate) fn validate_help_invocation(parts: &[&str]) -> CommandInvocationValidation {
    if parts.is_empty() || parts[0] != "help" {
        return CommandInvocationValidation::Invalid;
    }

    match parts.len() {
        1 => CommandInvocationValidation::ValidComplete,
        2 => {
            if parse_help_topic(parts.get(1).copied()).is_some() {
                CommandInvocationValidation::ValidComplete
            } else {
                CommandInvocationValidation::Invalid
            }
        }
        _ => CommandInvocationValidation::Invalid,
    }
}

pub(crate) fn help_usage_pattern() -> String {
    let topics: Vec<&str> = HELP_TOPIC_SPECS
        .iter()
        .filter_map(|spec| spec.aliases.first().copied())
        .collect();
    format!("help [{}]", topics.join("|"))
}

pub(crate) fn validate_open_invocation(parts: &[&str]) -> OpenInvocationValidation {
    if parts.is_empty() || parts[0] != "open" {
        return OpenInvocationValidation::Invalid;
    }

    if parts.len() == 1 {
        return OpenInvocationValidation::ValidPrefix;
    }

    if helper_command_by_alias(parts).is_some() {
        return OpenInvocationValidation::ValidComplete;
    }

    let Some(spec) = launch_command_by_token(parts[1]) else {
        return OpenInvocationValidation::Invalid;
    };

    if spec.required_usage.is_some() && parts.len() < 3 {
        OpenInvocationValidation::ValidPrefix
    } else {
        OpenInvocationValidation::ValidComplete
    }
}

pub(crate) fn is_known_command_prefix(cmd: &str) -> bool {
    VALID_PREFIX_GROUPS
        .iter()
        .any(|prefix_group| prefix_group.contains(&cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helper_alias_resolution() {
        assert_eq!(
            helper_command_by_alias(&["recent"]).map(|s| s.kind),
            Some(HelperCommandKind::RecentFiles)
        );
        assert_eq!(
            helper_command_by_alias(&["open", "file-picker"]).map(|s| s.kind),
            Some(HelperCommandKind::OpenFilePicker)
        );
        assert!(helper_command_by_alias(&["open", "file", "extra"]).is_none());
    }

    #[test]
    fn test_open_validation() {
        assert_eq!(
            validate_open_invocation(&["open", "custom"]),
            OpenInvocationValidation::ValidPrefix
        );
        assert_eq!(
            validate_open_invocation(&["open", "custom", "dashboard"]),
            OpenInvocationValidation::ValidComplete
        );
        assert_eq!(
            validate_open_invocation(&["open", "recent", "files"]),
            OpenInvocationValidation::ValidComplete
        );
        assert_eq!(
            validate_open_invocation(&["open", "unknown"]),
            OpenInvocationValidation::Invalid
        );
    }

    #[test]
    fn test_component_id_validation() {
        assert_eq!(
            validate_component_id_invocation(&["close"]),
            CommandInvocationValidation::ValidPrefix
        );
        assert_eq!(
            validate_component_id_invocation(&["focus", "comp:123"]),
            CommandInvocationValidation::ValidComplete
        );
        assert_eq!(
            validate_component_id_invocation(&["status", "invalid"]),
            CommandInvocationValidation::Invalid
        );
    }

    #[test]
    fn test_help_validation_and_parse() {
        assert_eq!(
            parse_help_topic(None),
            Some(HelpCategory::Overview)
        );
        assert_eq!(
            parse_help_topic(Some("keyboard")),
            Some(HelpCategory::Keys)
        );
        assert_eq!(
            validate_help_invocation(&["help", "workspace"]),
            CommandInvocationValidation::ValidComplete
        );
        assert_eq!(
            validate_help_invocation(&["help", "invalid"]),
            CommandInvocationValidation::Invalid
        );
        assert_eq!(help_usage_pattern(), "help [workspace|editor|keys|system]");
    }
}
