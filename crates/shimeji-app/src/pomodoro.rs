//! A session-only focus clock driven by injected monotonic milliseconds.
//!
//! The clock is independent of mascot animation, wall-clock changes, and timer
//! callback frequency. Completing a stage never starts another stage.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Persisted preferences; an active timer is deliberately never serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub focus_minutes: u16,
    pub short_break_minutes: u16,
    pub long_break_minutes: u16,
    pub notifications: bool,
    pub sound: bool,
    pub show_badge: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            notifications: true,
            sound: false,
            show_badge: false,
        }
    }
}

impl Preferences {
    pub fn normalize(&mut self) {
        self.focus_minutes = self.focus_minutes.clamp(1, 180);
        self.short_break_minutes = self.short_break_minutes.clamp(1, 180);
        self.long_break_minutes = self.long_break_minutes.clamp(1, 180);
    }

    fn duration_ms(&self, phase: Phase) -> u64 {
        let minutes = match phase {
            Phase::Focus => self.focus_minutes,
            Phase::ShortBreak => self.short_break_minutes,
            Phase::LongBreak => self.long_break_minutes,
        };
        u64::from(minutes.clamp(1, 180)) * 60_000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn title(self) -> &'static str {
        match self {
            Self::Focus => "专注",
            Self::ShortBreak => "短休息",
            Self::LongBreak => "长休息",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interruption {
    Locked,
    Sleep,
    Session,
}

impl Interruption {
    fn bit(self) -> u8 {
        match self {
            Self::Locked => 1,
            Self::Sleep => 2,
            Self::Session => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    pub phase: Phase,
}

/// Color bands of the remaining stage time, shared by both native shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatienceColor {
    Green,
    Yellow,
    Orange,
    Red,
}

impl PatienceColor {
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Green => [73, 181, 90],
            Self::Yellow => [224, 189, 58],
            Self::Orange => [233, 139, 52],
            Self::Red => [220, 79, 79],
        }
    }
}

/// Passive head label. The integer fill avoids floating-point comparisons in
/// render caches; colors use the exact remaining-time ratio, before rounding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patience {
    pub label: String,
    /// Remaining fraction, from 0 (empty) through 1000 (full).
    pub per_mille: u16,
    pub color: PatienceColor,
}

/// One timer shared by all pets in an application session.
#[derive(Debug, Default)]
pub struct Pomodoro {
    status: Status,
    phase: Option<Phase>,
    remaining: u64,
    /// Original stage length, retained when a pause replaces `remaining`.
    duration_ms: u64,
    started_at: u64,
    last_seen: u64,
    completed_focus: u32,
    interruptions: u8,
    completions: VecDeque<Completion>,
}

impl Pomodoro {
    pub fn start_next(&mut self, now_ms: u64, preferences: &Preferences) -> bool {
        self.settle(now_ms);
        if self.interruptions != 0 || matches!(self.status, Status::Running | Status::Paused) {
            return false;
        }
        let phase = self.next_phase();
        self.phase = Some(phase);
        self.remaining = preferences.duration_ms(phase);
        self.duration_ms = self.remaining;
        self.started_at = self.last_seen;
        self.status = Status::Running;
        true
    }

    pub fn pause(&mut self, now_ms: u64) {
        self.settle(now_ms);
        if self.status == Status::Running {
            self.remaining = self.remaining_ms(self.last_seen);
            self.status = Status::Paused;
        }
    }

    pub fn resume(&mut self, now_ms: u64) -> bool {
        self.settle(now_ms);
        if !self.can_resume() {
            return false;
        }
        self.started_at = self.last_seen;
        self.status = Status::Running;
        true
    }

    /// Discard the current stage, preserving completed rounds and blockers.
    pub fn cancel(&mut self) {
        self.status = Status::Idle;
        self.phase = None;
        self.remaining = 0;
        self.duration_ms = 0;
        self.completions.clear();
    }

    /// Interruption causes are independent. Clearing a cause never resumes.
    pub fn set_interruption(&mut self, reason: Interruption, active: bool, now_ms: u64) {
        if active {
            self.pause(now_ms);
            self.interruptions |= reason.bit();
        } else {
            self.settle(now_ms);
            self.interruptions &= !reason.bit();
        }
    }

    pub fn update(&mut self, now_ms: u64) -> Option<Completion> {
        self.settle(now_ms);
        self.completions.pop_front()
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn phase(&self) -> Option<Phase> {
        self.phase
    }

    pub fn next_phase(&self) -> Phase {
        match self.phase {
            Some(Phase::Focus) if self.status == Status::Completed => {
                if self.completed_focus > 0 && self.completed_focus % 4 == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            _ => Phase::Focus,
        }
    }

    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        if self.status == Status::Running {
            self.remaining
                .saturating_sub(now_ms.max(self.last_seen).saturating_sub(self.started_at))
        } else {
            self.remaining
        }
    }

    pub fn completed_focus(&self) -> u32 {
        self.completed_focus
    }

    pub fn quiet(&self) -> bool {
        self.phase == Some(Phase::Focus) && matches!(self.status, Status::Running | Status::Paused)
    }

    pub fn running(&self) -> bool {
        self.status == Status::Running
    }

    pub fn interrupted(&self) -> bool {
        self.interruptions != 0
    }

    pub fn can_resume(&self) -> bool {
        self.status == Status::Paused && self.interruptions == 0
    }

    pub fn label(&self, now_ms: u64) -> String {
        let phase = self.phase.map(Phase::title).unwrap_or("番茄钟");
        let seconds = self.remaining_ms(now_ms).div_ceil(1000);
        let remaining = format!("{:02}:{:02}", seconds / 60, seconds % 60);
        match self.status {
            Status::Idle => "番茄钟 · 待开始".to_string(),
            Status::Running => format!("{phase} · {remaining}"),
            Status::Paused => format!("已暂停 · {remaining}"),
            Status::Completed => format!("{phase}完成 · 准备好再开始"),
        }
    }

    pub fn patience(&self, now_ms: u64) -> Option<Patience> {
        let phase = self.phase?;
        if self.status == Status::Idle || self.duration_ms == 0 {
            return None;
        }
        let remaining = self.remaining_ms(now_ms).min(self.duration_ms);
        let duration = u128::from(self.duration_ms);
        let scaled = u128::from(remaining) * 4;
        let color = if scaled > duration * 3 {
            PatienceColor::Green
        } else if scaled > duration * 2 {
            PatienceColor::Yellow
        } else if scaled > duration {
            PatienceColor::Orange
        } else {
            PatienceColor::Red
        };
        let seconds = remaining.div_ceil(1000);
        let suffix = match self.status {
            Status::Paused => " · 已暂停",
            Status::Completed => " · 已完成",
            _ => match phase {
                Phase::Focus => "",
                Phase::ShortBreak => " · 短休息",
                Phase::LongBreak => " · 长休息",
            },
        };
        Some(Patience {
            label: format!("耐心：{:02}:{:02}{suffix}", seconds / 60, seconds % 60),
            per_mille: ((u128::from(remaining) * 1000) / duration) as u16,
            color,
        })
    }

    fn settle(&mut self, now_ms: u64) {
        self.last_seen = self.last_seen.max(now_ms);
        if self.status != Status::Running || self.remaining_ms(self.last_seen) != 0 {
            return;
        }
        self.remaining = 0;
        self.status = Status::Completed;
        if let Some(phase) = self.phase {
            if phase == Phase::Focus {
                self.completed_focus = self.completed_focus.saturating_add(1);
            }
            self.completions.push_back(Completion { phase });
        }
    }
}
