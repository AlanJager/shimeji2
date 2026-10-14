//! Which windows mischief may touch.
//!
//! App-name rules always apply. Title rules apply only when the caller passes
//! a title, which it does only while Accessibility is granted. An immutable
//! set cannot be unblocked. Editable defaults start blocked. Anything else is
//! allowed until the user adds a block, or an allow rule, which then limits
//! windows to those that match an allow.

use serde::{Deserialize, Serialize};

/// Apps mischief never moves. Compared without case, after trimming.
pub const IMMUTABLE_APPS: &[&str] = &[
    "loginwindow",
    "SecurityAgent",
    "System Settings",
    "System Preferences",
    "coreautha",
];

/// Apps blocked until the user unblocks that exact name.
pub const DEFAULT_BLOCKED_APPS: &[&str] =
    &["1Password", "Bitwarden", "Keychain Access", "Terminal"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Allow,
    Block,
}

/// `App` matches the application name. `Title` matches the window title and
/// is ignored when no title is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    App,
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRule {
    pub effect: Effect,
    pub target: Target,
    pub text: String,
}

/// The part of the rules stored in `settings.json`.
///
/// Missing from an older file, this is the default: nothing unblocked, no
/// user rules. The immutable list and the editable defaults live in code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WindowRules {
    /// Canonical names from [`DEFAULT_BLOCKED_APPS`] the user has allowed.
    pub unblocked: Vec<String>,
    pub rules: Vec<UserRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    Immutable,
    DefaultApp,
    UserBlock,
    NotOnAllowlist,
}

impl WindowRules {
    /// Allow one editable default. Returns false for an unknown or immutable name.
    pub fn unblock_default(&mut self, app: &str) -> bool {
        let Some(canonical) = canonical_default(app) else {
            return false;
        };
        if !self
            .unblocked
            .iter()
            .any(|name| name.eq_ignore_ascii_case(canonical))
        {
            self.unblocked.push(canonical.to_string());
        }
        true
    }

    /// Put an editable default back on the block list.
    pub fn block_default(&mut self, app: &str) -> bool {
        let Some(canonical) = canonical_default(app) else {
            return false;
        };
        self.unblocked
            .retain(|name| !name.eq_ignore_ascii_case(canonical));
        true
    }

    /// Whether an editable default is currently allowed.
    pub fn default_allowed(&self, app: &str) -> bool {
        let Some(canonical) = canonical_default(app) else {
            return false;
        };
        self.unblocked
            .iter()
            .any(|name| name.eq_ignore_ascii_case(canonical))
    }

    /// Allow or block one editable default. Immutable and unknown names return false.
    pub fn set_default_allowed(&mut self, app: &str, allowed: bool) -> bool {
        if allowed {
            self.unblock_default(app)
        } else {
            self.block_default(app)
        }
    }

    /// Append one user rule. Empty text is refused. The text is trimmed.
    pub fn add_rule(&mut self, effect: Effect, target: Target, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        self.rules.push(UserRule {
            effect,
            target,
            text: text.to_string(),
        });
        true
    }

    /// Remove the user rule at `index`. Out of range returns false.
    pub fn remove_rule(&mut self, index: usize) -> bool {
        if index >= self.rules.len() {
            return false;
        }
        self.rules.remove(index);
        true
    }

    pub fn decision(&self, app_name: &str, title: Option<&str>) -> Decision {
        let app = app_name.trim();
        if listed(app, IMMUTABLE_APPS) {
            return Decision::Deny(DenyReason::Immutable);
        }
        let title = title.map(str::trim).filter(|text| !text.is_empty());
        if self
            .rules
            .iter()
            .any(|rule| rule.effect == Effect::Block && rule_matches(rule, app, title))
        {
            return Decision::Deny(DenyReason::UserBlock);
        }
        if let Some(canonical) = canonical_default(app) {
            let unblocked = self
                .unblocked
                .iter()
                .any(|name| name.eq_ignore_ascii_case(canonical));
            let allowed_by_name = self.rules.iter().any(|rule| {
                rule.effect == Effect::Allow
                    && rule.target == Target::App
                    && contains_rule(app, &rule.text)
            });
            if !unblocked && !allowed_by_name {
                return Decision::Deny(DenyReason::DefaultApp);
            }
        }
        let mut allow_in_effect = false;
        let mut allow_hit = false;
        for rule in &self.rules {
            if rule.effect != Effect::Allow || !target_in_effect(rule, title) {
                continue;
            }
            allow_in_effect = true;
            if rule_matches(rule, app, title) {
                allow_hit = true;
            }
        }
        if allow_in_effect && !allow_hit {
            return Decision::Deny(DenyReason::NotOnAllowlist);
        }
        Decision::Allow
    }

    pub fn allows(&self, app_name: &str, title: Option<&str>) -> bool {
        matches!(self.decision(app_name, title), Decision::Allow)
    }

    /// Drop empty patterns, and keep only real editable defaults in `unblocked`.
    pub fn sanitize(&mut self) {
        let mut kept = Vec::new();
        for name in self.unblocked.drain(..) {
            let Some(canonical) = canonical_default(&name) else {
                continue;
            };
            if !kept.iter().any(|have| have == canonical) {
                kept.push(canonical.to_string());
            }
        }
        self.unblocked = kept;
        self.rules.retain(|rule| !rule.text.trim().is_empty());
        for rule in &mut self.rules {
            let trimmed = rule.text.trim();
            if trimmed.len() != rule.text.len() {
                rule.text = trimmed.to_string();
            }
        }
    }
}

/// One line for a rule list: `Allow app "TextEdit"`.
pub fn describe_rule(rule: &UserRule) -> String {
    let effect = match rule.effect {
        Effect::Allow => "Allow",
        Effect::Block => "Block",
    };
    let target = match rule.target {
        Target::App => "app",
        Target::Title => "title",
    };
    format!("{effect} {target} \"{}\"", rule.text)
}

/// Shimeji-ee `InteractiveWindows` and `InteractiveWindowsBlacklist`.
///
/// Both are `/`-separated title substrings, as in `settings.properties`.
/// Empty pieces are dropped. Block entries are stored first.
pub fn parse_ee_interactive_windows(allow: &str, block: &str) -> Vec<UserRule> {
    let mut rules = Vec::new();
    for text in split_ee(block) {
        rules.push(UserRule {
            effect: Effect::Block,
            target: Target::Title,
            text,
        });
    }
    for text in split_ee(allow) {
        rules.push(UserRule {
            effect: Effect::Allow,
            target: Target::Title,
            text,
        });
    }
    rules
}

fn split_ee(list: &str) -> Vec<String> {
    list.split('/')
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect()
}

fn canonical_default(app: &str) -> Option<&'static str> {
    let app = app.trim();
    DEFAULT_BLOCKED_APPS
        .iter()
        .copied()
        .find(|name| name.eq_ignore_ascii_case(app))
}

fn listed(app: &str, names: &[&str]) -> bool {
    names.iter().any(|name| name.eq_ignore_ascii_case(app))
}

fn target_in_effect(rule: &UserRule, title: Option<&str>) -> bool {
    match rule.target {
        Target::App => true,
        Target::Title => title.is_some(),
    }
}

fn rule_matches(rule: &UserRule, app: &str, title: Option<&str>) -> bool {
    if !target_in_effect(rule, title) {
        return false;
    }
    let haystack = match rule.target {
        Target::App => app,
        Target::Title => title.unwrap_or(""),
    };
    contains_rule(haystack, &rule.text)
}

fn contains_rule(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    !needle.is_empty() && haystack.contains(needle)
}
