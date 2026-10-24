//! OS notification copy and state badges. Pattern: title = state word, then
//! `repo#123`; body = PR title, then at most one short reason. Copy is rendered
//! at send time from the live queue item and never persisted.

use super::{Category, Destination, Frame, Notice};
use crate::queue;
use std::path::{Path, PathBuf};

/// Longest title: fits one macOS banner line and one Windows toast title line.
pub const TITLE_MAX: usize = 48;
/// Longest body: within two macOS banner lines and two Windows toast lines.
pub const BODY_MAX: usize = 90;

const SEPARATOR: &str = " · ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Success,
    Attention,
    Ready,
    Failure,
}

impl Badge {
    pub const ALL: [Self; 4] = [Self::Success, Self::Attention, Self::Ready, Self::Failure];

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Success => "success.png",
            Self::Attention => "attention.png",
            Self::Ready => "ready.png",
            Self::Failure => "failure.png",
        }
    }

    /// Embedded at compile time, so every build ships every badge.
    pub fn png(self) -> &'static [u8] {
        match self {
            Self::Success => include_bytes!("../../icons/notifications/success.png"),
            Self::Attention => include_bytes!("../../icons/notifications/attention.png"),
            Self::Ready => include_bytes!("../../icons/notifications/ready.png"),
            Self::Failure => include_bytes!("../../icons/notifications/failure.png"),
        }
    }

    /// Writes this badge to `directory/name`, rewriting only missing or changed bytes.
    pub fn write(self, directory: &Path, name: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(directory)?;
        let path = directory.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(self.png()) {
            std::fs::write(&path, self.png())?;
        }
        Ok(path)
    }
}

/// Badges live beside, not inside, saved state; they hold no user data.
pub fn badge_directory(root: &Path) -> PathBuf {
    root.join("notification-badges")
}

impl Category {
    /// First word(s) of every title for this category.
    pub fn state(self) -> &'static str {
        match self {
            Self::Confirmation => "Confirm",
            Self::HumanInput => "Needs you",
            Self::Ready => "Ready",
            Self::Failure => "Failed",
            Self::Test => "Notifications on",
            Self::Approved => "Approved",
            Self::Merged => "Merged",
            Self::FinalReviewCleared => "Cleared",
        }
    }

    pub fn badge(self) -> Badge {
        match self {
            Self::Approved | Self::Merged => Badge::Success,
            Self::Confirmation | Self::HumanInput => Badge::Attention,
            Self::Ready | Self::FinalReviewCleared | Self::Test => Badge::Ready,
            Self::Failure => Badge::Failure,
        }
    }

    /// Reason shown after the PR title when the state word alone could mislead.
    fn reason(self) -> Option<&'static str> {
        match self {
            // Automated outcomes never replace the person's own review.
            Self::Ready | Self::FinalReviewCleared | Self::Approved => Some("Needs your review"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub repository: String,
    pub number: u64,
    pub title: String,
    /// Typed failure cause from the queue item, never inferred from text.
    pub failure_cause: Option<&'static str>,
}

impl Subject {
    pub fn of(item: &queue::Item) -> Self {
        Self {
            repository: item.job.repository_name.clone(),
            number: item.job.number,
            title: item.job.title.clone(),
            failure_cause: item.failure_cause,
        }
    }

    /// The live item behind a queue notice, if it is still in the snapshot.
    pub fn find(snapshot: &queue::Snapshot, notice: &Notice) -> Option<Self> {
        let Destination::QueueItem { item_id } = &notice.event.destination else {
            return None;
        };
        snapshot
            .items
            .iter()
            .find(|item| &item.id == item_id || item.aliases.contains(item_id))
            .map(Self::of)
    }

    /// [`Self::find`] for a saved notice. The item holds only its latest failure
    /// cause, so the reason shows only while the notice is still the item's
    /// current event (`current` = `frames` of the same snapshot).
    pub fn for_history(
        snapshot: &queue::Snapshot,
        current: &[Frame],
        notice: &Notice,
    ) -> Option<Self> {
        let mut subject = Self::find(snapshot, notice)?;
        if !current.iter().any(|frame| {
            frame.source == notice.source && frame.event.as_ref() == Some(&notice.event)
        }) {
            subject.failure_cause = None;
        }
        Some(subject)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub body: String,
    pub badge: Badge,
}

fn failure_reason(cause: &str) -> Option<&'static str> {
    match cause {
        "publication_failed" => Some("Publish failed"),
        "publication_unresolved" => Some("Publish unconfirmed"),
        queue::CAUSE_PANIC => Some("Review crashed"),
        queue::CAUSE_TOSSED => Some("Retries exhausted"),
        queue::CAUSE_EXIT_FAILED => Some("Review failed"),
        _ => None,
    }
}

/// Single-line text without control, bidi-control or XML-invalid characters, so a
/// provider-supplied title cannot reorder or spoof the notification, or make a
/// Windows toast's XML unloadable.
fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| {
            !c.is_control()
                // Every Unicode Bidi_Control character.
                && !matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
                // The only XML 1.0-invalid chars left after `is_control`.
                && !matches!(c, '\u{FFFE}' | '\u{FFFF}')
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.into();
    }
    let kept: String = value.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

pub fn render(notice: &Notice, subject: Option<&Subject>) -> Message {
    let category = notice.event.category;
    let state = category.state();
    let badge = category.badge();
    let Some(subject) = subject else {
        let (title, body) = match (category, &notice.event.destination) {
            (Category::Test, _) => (state.to_string(), "PR alerts show here"),
            (Category::Failure, Destination::Settings) => {
                (format!("{state}{SEPARATOR}Repo check"), "Open Settings")
            }
            _ => (state.to_string(), "Open the queue"),
        };
        return Message {
            title,
            body: body.into(),
            badge,
        };
    };
    let repository = clean(
        subject
            .repository
            .rsplit('/')
            .next()
            .unwrap_or(&subject.repository),
    );
    let number = format!("#{}", subject.number);
    let room = TITLE_MAX
        .saturating_sub(state.chars().count() + SEPARATOR.chars().count() + number.chars().count());
    let title = format!("{state}{SEPARATOR}{}{number}", truncate(&repository, room));
    let reason = match category {
        Category::Failure => subject.failure_cause.and_then(failure_reason),
        _ => category.reason(),
    };
    let pr_title = clean(&subject.title);
    let body = match (pr_title.is_empty(), reason) {
        (true, reason) => reason.unwrap_or("Open the queue").into(),
        (false, Some(reason)) => {
            let room = BODY_MAX - reason.chars().count() - SEPARATOR.chars().count();
            format!("{}{SEPARATOR}{reason}", truncate(&pr_title, room))
        }
        (false, None) => truncate(&pr_title, BODY_MAX),
    };
    Message { title, body, badge }
}

/// `file:///` URI for a local Windows path, as toast `<image src>` requires.
/// Canonical paths carry a verbatim `\\?\` prefix that a URI must not.
#[cfg(any(windows, test))]
pub fn windows_file_uri(path: &str) -> String {
    let (prefix, rest) = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        ("file://", unc)
    } else if let Some(local) = path.strip_prefix(r"\\?\") {
        ("file:///", local)
    } else if let Some(unc) = path.strip_prefix(r"\\") {
        ("file://", unc)
    } else {
        ("file:///", path)
    };
    let mut uri = String::from(prefix);
    for byte in rest.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}
