//! Menu Bar layout model, port of the macOS `MenuBarLayout*.swift` family
//! (tokens, conditionals, display modes) reduced to what a StatusNotifierItem
//! can show: an icon pixmap plus a title string (KDE shows the title, most
//! other hosts only the icon).
//!
//! Persistence lives in its own file, `~/.config/codexbar/menubar-layout.json`
//! (versioned, atomic write). It is deliberately NOT part of `linux.json`,
//! whose Qt reader rejects unknown keys.
//!
//! The default layout is `[[icon]]` with a static title: it reproduces the
//! tray exactly as it behaved before the layout existed. (macOS defaults to
//! icon + percent, but on Linux that would change the tray title.)
//!
//! Unknown values never render as 0: they render as [`UNKNOWN`].

use crate::model::{CostEntry, Entry, QuotaWindow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
pub const UNKNOWN: &str = "\u{2014}";
pub const MAX_LINES: usize = 2;
pub const MAX_CLAUSES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Window {
    Session,
    Weekly,
    ScopedWeekly,
    Automatic,
}

impl Window {
    pub const ALL: [Window; 4] = [
        Window::Session,
        Window::Weekly,
        Window::ScopedWeekly,
        Window::Automatic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Window::Session => "Session",
            Window::Weekly => "Weekly",
            Window::ScopedWeekly => "Scoped weekly",
            Window::Automatic => "Auto",
        }
    }

    /// Session = `primary`, Weekly = `secondary`, Scoped = first per-model cap,
    /// Automatic = the first window (what the tray meters already read).
    pub fn pick(self, entry: &Entry) -> Option<&QuotaWindow> {
        match self {
            Window::Session => entry.windows.iter().find(|w| w.key == "primary"),
            Window::Weekly => entry.windows.iter().find(|w| w.key == "secondary"),
            Window::ScopedWeekly => entry.windows.iter().find(|w| w.scoped),
            Window::Automatic => entry.windows.first(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Glyph {
    Dot,
    Pipe,
    Slash,
    Hyphen,
    Bullet,
    Diamond,
    HollowDiamond,
    Tilde,
    DoubleBar,
}

impl Glyph {
    pub const ALL: [Glyph; 9] = [
        Glyph::Dot,
        Glyph::Pipe,
        Glyph::Slash,
        Glyph::Hyphen,
        Glyph::Bullet,
        Glyph::Diamond,
        Glyph::HollowDiamond,
        Glyph::Tilde,
        Glyph::DoubleBar,
    ];

    pub fn glyph(self) -> &'static str {
        match self {
            Glyph::Dot => "\u{00b7}",
            Glyph::Pipe => "|",
            Glyph::Slash => "/",
            Glyph::Hyphen => "-",
            Glyph::Bullet => "\u{2022}",
            Glyph::Diamond => "\u{25c6}",
            Glyph::HollowDiamond => "\u{25c7}",
            Glyph::Tilde => "~",
            Glyph::DoubleBar => "\u{2016}",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Token {
    /// The tray icon (meters or themed icon, exactly as before layouts existed).
    Icon,
    ProviderName,
    Percent {
        window: Window,
    },
    Pace {
        window: Window,
    },
    /// A graphical bar drawn into the icon pixmap (max 2, like the meters).
    ProgressBar {
        window: Window,
    },
    ResetCountdown {
        window: Window,
    },
    ResetAbsolute {
        window: Window,
    },
    /// Credits balance.
    Balance,
    CostToday,
    /// Text chosen by the layout's [`DisplayMode`].
    Display,
    Separator {
        glyph: Glyph,
    },
    Space,
    Hidden,
    Conditional {
        id: String,
    },
}

impl Token {
    pub fn label(&self) -> String {
        match self {
            Token::Icon => "Icon".into(),
            Token::ProviderName => "Provider name".into(),
            Token::Percent { window } => format!("Percent \u{00b7} {}", window.label()),
            Token::Pace { window } => format!("Pace \u{00b7} {}", window.label()),
            Token::ProgressBar { window } => format!("Bar \u{00b7} {}", window.label()),
            Token::ResetCountdown { window } => format!("Resets in \u{00b7} {}", window.label()),
            Token::ResetAbsolute { window } => format!("Resets at \u{00b7} {}", window.label()),
            Token::Balance => "Balance".into(),
            Token::CostToday => "Cost today".into(),
            Token::Display => "Display mode text".into(),
            Token::Separator { glyph } => format!("Separator {}", glyph.glyph()),
            Token::Space => "Space".into(),
            Token::Hidden => "Hidden".into(),
            Token::Conditional { id } => format!("If: {id}"),
        }
    }

    /// Tokens offered by the editor palette (conditionals are added separately).
    pub fn palette() -> Vec<Token> {
        let mut out = vec![Token::Icon, Token::ProviderName];
        for window in Window::ALL {
            out.push(Token::Percent { window });
        }
        for window in [Window::Session, Window::Weekly, Window::Automatic] {
            out.push(Token::Pace { window });
        }
        for window in [Window::Session, Window::Weekly] {
            out.push(Token::ProgressBar { window });
        }
        for window in [Window::Session, Window::Weekly, Window::Automatic] {
            out.push(Token::ResetCountdown { window });
        }
        out.push(Token::ResetAbsolute {
            window: Window::Automatic,
        });
        out.extend([Token::Balance, Token::CostToday, Token::Display]);
        for glyph in Glyph::ALL {
            out.push(Token::Separator { glyph });
        }
        out.push(Token::Space);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    Percent,
    Pace,
    Both,
    ResetTime,
}

impl DisplayMode {
    pub const ALL: [DisplayMode; 4] = [
        DisplayMode::Percent,
        DisplayMode::Pace,
        DisplayMode::Both,
        DisplayMode::ResetTime,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Percent => "Percent",
            DisplayMode::Pace => "Pace",
            DisplayMode::Both => "Percent and pace",
            DisplayMode::ResetTime => "Reset time",
        }
    }
}

/// What the SNI title carries. `Static` is the pre-layout behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleMode {
    Static,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    Session,
    Weekly,
    ScopedWeekly,
    Automatic,
    SessionResetsIn,
    WeeklyResetsIn,
    AutomaticResetsIn,
    SessionPace,
    WeeklyPace,
    AutomaticPace,
    Balance,
    CostToday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Percent,
    SignedPercent,
    Hours,
    Usd,
}

impl Metric {
    pub const ALL: [Metric; 12] = [
        Metric::Session,
        Metric::Weekly,
        Metric::ScopedWeekly,
        Metric::Automatic,
        Metric::SessionResetsIn,
        Metric::WeeklyResetsIn,
        Metric::AutomaticResetsIn,
        Metric::SessionPace,
        Metric::WeeklyPace,
        Metric::AutomaticPace,
        Metric::Balance,
        Metric::CostToday,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Metric::Session => "Session %",
            Metric::Weekly => "Weekly %",
            Metric::ScopedWeekly => "Scoped weekly %",
            Metric::Automatic => "Auto %",
            Metric::SessionResetsIn => "Session resets in",
            Metric::WeeklyResetsIn => "Weekly resets in",
            Metric::AutomaticResetsIn => "Auto resets in",
            Metric::SessionPace => "Session pace",
            Metric::WeeklyPace => "Weekly pace",
            Metric::AutomaticPace => "Auto pace",
            Metric::Balance => "Balance",
            Metric::CostToday => "Cost today",
        }
    }

    pub fn kind(self) -> MetricKind {
        match self {
            Metric::Session | Metric::Weekly | Metric::ScopedWeekly | Metric::Automatic => {
                MetricKind::Percent
            }
            Metric::SessionResetsIn | Metric::WeeklyResetsIn | Metric::AutomaticResetsIn => {
                MetricKind::Hours
            }
            Metric::SessionPace | Metric::WeeklyPace | Metric::AutomaticPace => {
                MetricKind::SignedPercent
            }
            Metric::Balance | Metric::CostToday => MetricKind::Usd,
        }
    }

    /// Used/remaining only applies to percent windows.
    pub fn supports_direction(self) -> bool {
        self.kind() == MetricKind::Percent
    }

    pub fn range(self) -> (f64, f64) {
        match self.kind() {
            MetricKind::Percent => (0.0, 100.0),
            MetricKind::SignedPercent => (-100.0, 100.0),
            MetricKind::Hours => (0.0, 8760.0),
            MetricKind::Usd => (0.0, 1_000_000.0),
        }
    }

    pub fn step(self) -> f64 {
        if self.kind() == MetricKind::Hours {
            0.5
        } else {
            1.0
        }
    }

    pub fn unit(self) -> &'static str {
        match self.kind() {
            MetricKind::Percent | MetricKind::SignedPercent => "%",
            MetricKind::Hours => "h",
            MetricKind::Usd => "USD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Used,
    Remaining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Comparison {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl Comparison {
    pub const ALL: [Comparison; 4] = [
        Comparison::GreaterThan,
        Comparison::GreaterThanOrEqual,
        Comparison::LessThan,
        Comparison::LessThanOrEqual,
    ];
    pub fn symbol(self) -> &'static str {
        match self {
            Comparison::GreaterThan => ">",
            Comparison::GreaterThanOrEqual => ">=",
            Comparison::LessThan => "<",
            Comparison::LessThanOrEqual => "<=",
        }
    }
    fn evaluate(self, value: f64, threshold: f64) -> bool {
        match self {
            Comparison::GreaterThan => value > threshold,
            Comparison::GreaterThanOrEqual => value >= threshold,
            Comparison::LessThan => value < threshold,
            Comparison::LessThanOrEqual => value <= threshold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Combinator {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Predicate {
    pub metric: Metric,
    pub direction: Direction,
    pub comparison: Comparison,
    pub threshold: f64,
}

impl Predicate {
    fn normalized(mut self) -> Self {
        let (lo, hi) = self.metric.range();
        self.threshold = if self.threshold.is_finite() {
            self.threshold.clamp(lo, hi)
        } else {
            lo
        };
        if !self.metric.supports_direction() {
            self.direction = Direction::Used;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clause {
    /// `None` for the first clause (ignored there).
    pub combinator: Option<Combinator>,
    pub predicate: Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conditional {
    pub id: String,
    pub name: String,
    pub clauses: Vec<Clause>,
    pub then_token: Token,
    pub else_token: Token,
}

impl Conditional {
    pub fn new(id: String) -> Self {
        Self {
            id,
            name: String::new(),
            clauses: vec![Clause {
                combinator: None,
                predicate: Predicate {
                    metric: Metric::Session,
                    direction: Direction::Used,
                    comparison: Comparison::GreaterThan,
                    threshold: 50.0,
                },
            }],
            then_token: Token::Percent {
                window: Window::Session,
            },
            else_token: Token::Hidden,
        }
    }

    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("Conditional {}", self.id)
        } else {
            self.name.clone()
        }
    }

    fn normalize(&mut self) {
        self.clauses.truncate(MAX_CLAUSES);
        if self.clauses.is_empty() {
            self.clauses = Conditional::new(String::new()).clauses;
        }
        for clause in &mut self.clauses {
            clause.predicate = clause.predicate.clone().normalized();
        }
        self.clauses[0].combinator = None;
        // Branches cannot nest conditionals.
        for branch in [&mut self.then_token, &mut self.else_token] {
            if matches!(branch, Token::Conditional { .. }) {
                *branch = Token::Hidden;
            }
        }
    }

    /// Unknown data makes a predicate false instead of comparing a fabricated 0.
    pub fn evaluate(&self, input: &RenderInput) -> bool {
        let Some(first) = self.clauses.first() else {
            return false;
        };
        let mut result = test(&first.predicate, input);
        for clause in self.clauses.iter().skip(1) {
            let value = test(&clause.predicate, input);
            match clause.combinator {
                Some(Combinator::Or) => result = result || value,
                _ => result = result && value,
            }
        }
        result
    }
}

fn test(predicate: &Predicate, input: &RenderInput) -> bool {
    metric_value(predicate, input)
        .is_some_and(|value| predicate.comparison.evaluate(value, predicate.threshold))
}

fn metric_value(predicate: &Predicate, input: &RenderInput) -> Option<f64> {
    let window = |w: Window| input.entry.and_then(|e| w.pick(e));
    let percent = |w: Window| {
        window(w).map(|q| {
            let remaining = q.remaining.clamp(0, 100) as f64;
            match predicate.direction {
                Direction::Used => 100.0 - remaining,
                Direction::Remaining => remaining,
            }
        })
    };
    let hours = |w: Window| {
        window(w)
            .and_then(|q| reset_minutes(&q.resets_at, input.now))
            .map(|m| m as f64 / 60.0)
    };
    let pace = |w: Window| window(w).map(|q| q.pace_delta).filter(|v| v.is_finite());
    match predicate.metric {
        Metric::Session => percent(Window::Session),
        Metric::Weekly => percent(Window::Weekly),
        Metric::ScopedWeekly => percent(Window::ScopedWeekly),
        Metric::Automatic => percent(Window::Automatic),
        Metric::SessionResetsIn => hours(Window::Session),
        Metric::WeeklyResetsIn => hours(Window::Weekly),
        Metric::AutomaticResetsIn => hours(Window::Automatic),
        Metric::SessionPace => pace(Window::Session),
        Metric::WeeklyPace => pace(Window::Weekly),
        Metric::AutomaticPace => pace(Window::Automatic),
        Metric::Balance => input
            .entry
            .and_then(|e| e.credits)
            .filter(|v| v.is_finite()),
        Metric::CostToday => input.cost.map(|c| c.today).filter(|v| v.is_finite()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarLayout {
    pub version: u32,
    pub lines: Vec<Vec<Token>>,
    pub title_mode: TitleMode,
    pub display_mode: DisplayMode,
    pub conditionals: Vec<Conditional>,
}

impl Default for MenuBarLayout {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            lines: vec![vec![Token::Icon]],
            title_mode: TitleMode::Static,
            display_mode: DisplayMode::Percent,
            conditionals: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Default,
    IconAndPercent,
    PercentAndReset,
    Stacked,
    Bars,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Default,
        Preset::IconAndPercent,
        Preset::PercentAndReset,
        Preset::Stacked,
        Preset::Bars,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Preset::Default => "Icon only (default)",
            Preset::IconAndPercent => "Icon and percent",
            Preset::PercentAndReset => "Percent and reset",
            Preset::Stacked => "Session / weekly",
            Preset::Bars => "Bars",
        }
    }
}

impl MenuBarLayout {
    /// Drops leading empty lines, keeps at most [`MAX_LINES`], and falls back
    /// to the default when nothing is left. Also normalizes conditionals.
    pub fn normalize(&mut self) {
        self.version = SCHEMA_VERSION;
        let first = self.lines.iter().position(|l| !l.is_empty());
        match first {
            Some(i) => {
                self.lines.drain(..i);
                self.lines.truncate(MAX_LINES);
            }
            None => self.lines = MenuBarLayout::default().lines,
        }
        for conditional in &mut self.conditionals {
            conditional.normalize();
        }
        let mut seen = std::collections::HashSet::new();
        self.conditionals.retain(|c| seen.insert(c.id.clone()));
    }

    pub fn from_preset(preset: Preset) -> Self {
        let pct = |window| Token::Percent { window };
        let (lines, title_mode) = match preset {
            Preset::Default => return Self::default(),
            Preset::IconAndPercent => (
                vec![vec![Token::Icon, pct(Window::Automatic)]],
                TitleMode::Layout,
            ),
            Preset::PercentAndReset => (
                vec![vec![
                    Token::Icon,
                    pct(Window::Automatic),
                    Token::Separator { glyph: Glyph::Dot },
                    Token::ResetCountdown {
                        window: Window::Automatic,
                    },
                ]],
                TitleMode::Layout,
            ),
            Preset::Stacked => (
                vec![vec![pct(Window::Session)], vec![pct(Window::Weekly)]],
                TitleMode::Layout,
            ),
            Preset::Bars => (
                vec![
                    vec![Token::ProgressBar {
                        window: Window::Session,
                    }],
                    vec![Token::ProgressBar {
                        window: Window::Weekly,
                    }],
                ],
                TitleMode::Static,
            ),
        };
        let mut layout = Self {
            lines,
            title_mode,
            ..Self::default()
        };
        layout.normalize();
        layout
    }

    pub fn conditional(&self, id: &str) -> Option<&Conditional> {
        self.conditionals.iter().find(|c| c.id == id)
    }

    /// Palette tokens including one entry per library conditional.
    pub fn palette(&self) -> Vec<Token> {
        let mut out = Token::palette();
        out.extend(
            self.conditionals
                .iter()
                .map(|c| Token::Conditional { id: c.id.clone() }),
        );
        out
    }

    /// Human label for a token in this layout (conditionals use their name).
    pub fn token_label(&self, token: &Token) -> String {
        match token {
            Token::Conditional { id } => match self.conditional(id) {
                Some(c) => format!("If: {}", c.display_name()),
                None => format!("If: missing ({id})"),
            },
            other => other.label(),
        }
    }
}

// ---------- rendering ----------

pub struct RenderInput<'a> {
    pub entry: Option<&'a Entry>,
    pub cost: Option<&'a CostEntry>,
    pub quota_used: bool,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Icon,
    Text(String),
    /// Remaining percent of the window, `None` when unknown (track only).
    Bar(Option<i64>),
    Space,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rendered {
    pub lines: Vec<Vec<Piece>>,
}

impl Rendered {
    pub fn has_icon(&self) -> bool {
        self.pieces().any(|p| matches!(p, Piece::Icon))
    }

    pub fn bars(&self) -> Vec<Option<i64>> {
        self.pieces()
            .filter_map(|p| match p {
                Piece::Bar(v) => Some(*v),
                _ => None,
            })
            .take(2)
            .collect()
    }

    fn pieces(&self) -> impl Iterator<Item = &Piece> {
        self.lines.iter().flatten()
    }

    /// Text pieces per line joined with the glue the editor shows; lines are
    /// joined by a space so the single-line SNI title stays readable.
    pub fn title_text(&self) -> String {
        let lines: Vec<String> = self
            .lines
            .iter()
            .map(|line| {
                let mut out = String::new();
                for piece in line {
                    match piece {
                        Piece::Text(t) => {
                            if !out.is_empty() && !out.ends_with(' ') {
                                out.push(' ');
                            }
                            out.push_str(t);
                        }
                        Piece::Space => out.push(' '),
                        Piece::Icon | Piece::Bar(_) => {}
                    }
                }
                out.trim().to_string()
            })
            .filter(|l| !l.is_empty())
            .collect();
        lines.join(" ")
    }
}

pub fn render(layout: &MenuBarLayout, input: &RenderInput) -> Rendered {
    let lines = layout
        .lines
        .iter()
        .map(|line| {
            line.iter()
                .filter_map(|token| render_token(layout, token, input, true))
                .collect()
        })
        .collect();
    Rendered { lines }
}

fn render_token(
    layout: &MenuBarLayout,
    token: &Token,
    input: &RenderInput,
    allow_conditional: bool,
) -> Option<Piece> {
    let text = |s: String| Some(Piece::Text(s));
    let entry = input.entry;
    match token {
        Token::Icon => Some(Piece::Icon),
        Token::ProviderName => text(
            entry
                .map(|e| crate::model::provider_name(&e.provider))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| UNKNOWN.into()),
        ),
        Token::Percent { window } => text(percent_text(entry, *window, input.quota_used)),
        Token::Pace { window } => text(pace_text(entry, *window)),
        Token::ProgressBar { window } => Some(Piece::Bar(
            entry.and_then(|e| window.pick(e)).map(|q| q.remaining),
        )),
        Token::ResetCountdown { window } => text(countdown_text(entry, *window, input.now)),
        Token::ResetAbsolute { window } => text(absolute_text(entry, *window)),
        Token::Balance => text(usd_text(entry.and_then(|e| e.credits))),
        Token::CostToday => text(usd_text(input.cost.map(|c| c.today))),
        Token::Display => text(match layout.display_mode {
            DisplayMode::Percent => percent_text(entry, Window::Automatic, input.quota_used),
            DisplayMode::Pace => pace_text(entry, Window::Automatic),
            DisplayMode::Both => format!(
                "{} {}",
                percent_text(entry, Window::Automatic, input.quota_used),
                pace_text(entry, Window::Automatic)
            ),
            DisplayMode::ResetTime => countdown_text(entry, Window::Automatic, input.now),
        }),
        Token::Separator { glyph } => text(glyph.glyph().to_string()),
        Token::Space => Some(Piece::Space),
        Token::Hidden => None,
        Token::Conditional { id } => {
            // Dangling id or nested conditional: visible placeholder, never silent.
            let Some(conditional) = layout.conditional(id).filter(|_| allow_conditional) else {
                return text(UNKNOWN.into());
            };
            let branch = if conditional.evaluate(input) {
                &conditional.then_token
            } else {
                &conditional.else_token
            };
            render_token(layout, branch, input, false)
        }
    }
}

fn percent_text(entry: Option<&Entry>, window: Window, quota_used: bool) -> String {
    match entry.and_then(|e| window.pick(e)) {
        Some(q) => {
            let remaining = q.remaining.clamp(0, 100);
            format!(
                "{}%",
                if quota_used {
                    100 - remaining
                } else {
                    remaining
                }
            )
        }
        None => UNKNOWN.into(),
    }
}

fn pace_text(entry: Option<&Entry>, window: Window) -> String {
    let Some(delta) = entry
        .and_then(|e| window.pick(e))
        .map(|q| q.pace_delta)
        .filter(|v| v.is_finite())
    else {
        return UNKNOWN.into();
    };
    let value = delta.abs().round() as i64;
    if value == 0 {
        "0%".into()
    } else {
        format!("{}{value}%", if delta >= 0.0 { "+" } else { "-" })
    }
}

fn usd_text(value: Option<f64>) -> String {
    match value.filter(|v| v.is_finite()) {
        Some(v) => format!("${v:.2}"),
        None => UNKNOWN.into(),
    }
}

fn parse_time(resets_at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(resets_at)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(resets_at, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|d| d.and_utc())
        })
}

/// Whole minutes until reset (ceil), `None` when the time is unknown. Past
/// resets clamp to 0.
fn reset_minutes(resets_at: &str, now: DateTime<Utc>) -> Option<i64> {
    let ts = parse_time(resets_at)?;
    Some((((ts - now).num_seconds() as f64 / 60.0).ceil() as i64).max(0))
}

fn countdown_text(entry: Option<&Entry>, window: Window, now: DateTime<Utc>) -> String {
    let Some(minutes) = entry
        .and_then(|e| window.pick(e))
        .and_then(|q| reset_minutes(&q.resets_at, now))
    else {
        return UNKNOWN.into();
    };
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{}d {}h", minutes / 1440, (minutes % 1440) / 60)
    }
}

fn absolute_text(entry: Option<&Entry>, window: Window) -> String {
    match entry
        .and_then(|e| window.pick(e))
        .and_then(|q| parse_time(&q.resets_at))
    {
        Some(ts) => ts
            .with_timezone(&chrono::Local)
            .format("%a %H:%M")
            .to_string(),
        None => UNKNOWN.into(),
    }
}

// ---------- editing ----------

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutEdit {
    Preset(Preset),
    SetTitleMode(TitleMode),
    SetDisplayMode(DisplayMode),
    AddToken {
        line: usize,
        token: Token,
    },
    RemoveToken {
        line: usize,
        index: usize,
    },
    MoveToken {
        line: usize,
        index: usize,
        delta: i32,
    },
    AddConditional,
    RemoveConditional(String),
    SetConditionalName(String, String),
    SetMetric(String, usize, Metric),
    SetDirection(String, usize, Direction),
    SetComparison(String, usize, Comparison),
    StepThreshold(String, usize, i32),
    SetCombinator(String, usize, Combinator),
    AddClause(String),
    RemoveClause(String, usize),
    SetBranch {
        id: String,
        then: bool,
        token: Token,
    },
}

fn next_conditional_id(layout: &MenuBarLayout) -> String {
    (1..)
        .map(|n| format!("c{n}"))
        .find(|id| layout.conditional(id).is_none())
        .unwrap_or_default()
}

pub fn apply(layout: &mut MenuBarLayout, edit: LayoutEdit) {
    match edit {
        LayoutEdit::Preset(p) => {
            // Presets replace the arrangement but keep the conditional library.
            let conditionals = std::mem::take(&mut layout.conditionals);
            let display_mode = layout.display_mode;
            *layout = MenuBarLayout::from_preset(p);
            layout.conditionals = conditionals;
            layout.display_mode = display_mode;
        }
        LayoutEdit::SetTitleMode(m) => layout.title_mode = m,
        LayoutEdit::SetDisplayMode(m) => layout.display_mode = m,
        LayoutEdit::AddToken { line, token } => {
            while layout.lines.len() <= line.min(MAX_LINES - 1) {
                layout.lines.push(Vec::new());
            }
            layout.lines[line.min(MAX_LINES - 1)].push(token);
        }
        LayoutEdit::RemoveToken { line, index } => {
            if let Some(l) = layout.lines.get_mut(line) {
                if index < l.len() {
                    l.remove(index);
                }
            }
        }
        LayoutEdit::MoveToken { line, index, delta } => {
            if let Some(l) = layout.lines.get_mut(line) {
                let target = index as i64 + delta as i64;
                if index < l.len() && target >= 0 && (target as usize) < l.len() {
                    l.swap(index, target as usize);
                }
            }
        }
        LayoutEdit::AddConditional => {
            let id = next_conditional_id(layout);
            layout.conditionals.push(Conditional::new(id));
        }
        LayoutEdit::RemoveConditional(id) => {
            layout.conditionals.retain(|c| c.id != id);
            // Placed references would dangle: remove them.
            for line in &mut layout.lines {
                line.retain(|t| !matches!(t, Token::Conditional { id: i } if *i == id));
            }
        }
        LayoutEdit::SetConditionalName(id, name) => {
            if let Some(c) = cond_mut(layout, &id) {
                c.name = name;
            }
        }
        LayoutEdit::SetMetric(id, i, metric) => {
            if let Some(p) = predicate_mut(layout, &id, i) {
                p.metric = metric;
                let (lo, hi) = metric.range();
                p.threshold = p.threshold.clamp(lo, hi);
            }
        }
        LayoutEdit::SetDirection(id, i, d) => {
            if let Some(p) = predicate_mut(layout, &id, i) {
                p.direction = d;
            }
        }
        LayoutEdit::SetComparison(id, i, cmp) => {
            if let Some(p) = predicate_mut(layout, &id, i) {
                p.comparison = cmp;
            }
        }
        LayoutEdit::StepThreshold(id, i, dir) => {
            if let Some(p) = predicate_mut(layout, &id, i) {
                p.threshold += p.metric.step() * dir as f64;
            }
        }
        LayoutEdit::SetCombinator(id, i, comb) => {
            if i > 0 {
                if let Some(clause) = cond_mut(layout, &id).and_then(|c| c.clauses.get_mut(i)) {
                    clause.combinator = Some(comb);
                }
            }
        }
        LayoutEdit::AddClause(id) => {
            if let Some(c) = cond_mut(layout, &id) {
                if c.clauses.len() < MAX_CLAUSES {
                    let predicate = c.clauses[c.clauses.len() - 1].predicate.clone();
                    c.clauses.push(Clause {
                        combinator: Some(Combinator::And),
                        predicate,
                    });
                }
            }
        }
        LayoutEdit::RemoveClause(id, i) => {
            if let Some(c) = cond_mut(layout, &id) {
                if c.clauses.len() > 1 && i < c.clauses.len() {
                    c.clauses.remove(i);
                }
            }
        }
        LayoutEdit::SetBranch { id, then, token } => {
            if let Some(c) = cond_mut(layout, &id) {
                if then {
                    c.then_token = token;
                } else {
                    c.else_token = token;
                }
            }
        }
    }
    layout.normalize();
}

fn cond_mut<'a>(layout: &'a mut MenuBarLayout, id: &str) -> Option<&'a mut Conditional> {
    layout.conditionals.iter_mut().find(|c| c.id == id)
}

fn predicate_mut<'a>(
    layout: &'a mut MenuBarLayout,
    id: &str,
    index: usize,
) -> Option<&'a mut Predicate> {
    cond_mut(layout, id)
        .and_then(|c| c.clauses.get_mut(index))
        .map(|c| &mut c.predicate)
}

// ---------- persistence ----------

pub fn path() -> PathBuf {
    crate::settings::Settings::path().with_file_name("menubar-layout.json")
}

pub fn load() -> MenuBarLayout {
    load_from(&path())
}

/// Missing, unparseable or newer-version files yield the default layout.
/// [`save_to`] refuses to overwrite the latter two so nothing is lost.
pub fn load_from(path: &Path) -> MenuBarLayout {
    let Ok(bytes) = std::fs::read(path) else {
        return MenuBarLayout::default();
    };
    match serde_json::from_slice::<MenuBarLayout>(&bytes) {
        Ok(mut layout) if layout.version <= SCHEMA_VERSION => {
            layout.normalize();
            layout
        }
        _ => MenuBarLayout::default(),
    }
}

pub fn save(layout: &MenuBarLayout) -> Result<(), String> {
    save_to(layout, &path())
}

pub fn save_to(layout: &MenuBarLayout, path: &Path) -> Result<(), String> {
    if path.exists() {
        let existing = std::fs::read(path)
            .map_err(|_| "Cannot read menubar-layout.json. Check permissions.".to_string())?;
        match serde_json::from_slice::<MenuBarLayout>(&existing) {
            Ok(old) if old.version > SCHEMA_VERSION => {
                return Err(
                    "menubar-layout.json was written by a newer CodexBar; not overwriting it."
                        .into(),
                )
            }
            Err(_) => {
                return Err(
                    "Invalid menubar-layout.json. Fix or remove it before saving the layout."
                        .into(),
                )
            }
            Ok(_) => {}
        }
    }
    let mut layout = layout.clone();
    layout.normalize();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&layout).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn window(key: &str, remaining: i64, delta: f64, resets_at: &str, scoped: bool) -> QuotaWindow {
        QuotaWindow {
            key: key.into(),
            label: key.into(),
            minutes: 300,
            remaining,
            resets_at: resets_at.into(),
            reset_text: String::new(),
            pace: String::new(),
            pace_expected: f64::NAN,
            pace_delta: delta,
            scoped,
        }
    }

    fn entry() -> Entry {
        Entry {
            provider: "codex".into(),
            account_label: String::new(),
            plan: String::new(),
            status: String::new(),
            status_level: String::new(),
            error: String::new(),
            updated_at: String::new(),
            credits: Some(12.5),
            windows: vec![
                window("primary", 80, 4.4, "2026-11-09T14:05:00Z", false),
                window("secondary", 35, -12.0, "2026-11-12T12:00:00Z", false),
            ],
            details: vec![],
            extra_usage_text: String::new(),
            reset_credits_available: -1,
            reset_credits_next_expiry: String::new(),
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 9, 12, 0, 0).unwrap()
    }

    fn input<'a>(e: Option<&'a Entry>, cost: Option<&'a CostEntry>) -> RenderInput<'a> {
        RenderInput {
            entry: e,
            cost,
            quota_used: false,
            now: now(),
        }
    }

    fn params(style: &str) -> crate::tray::TrayParams<'_> {
        crate::tray::TrayParams {
            tray_style: style,
            stale: false,
            quota_used: false,
            warning_colors: true,
            notify_threshold: 10,
        }
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codexbar-mbl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn texts(layout: &MenuBarLayout, e: Option<&Entry>) -> Vec<String> {
        render(layout, &input(e, None))
            .lines
            .iter()
            .flatten()
            .filter_map(|p| match p {
                Piece::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn serde_round_trip_with_conditionals() {
        let mut layout = MenuBarLayout::from_preset(Preset::PercentAndReset);
        apply(&mut layout, LayoutEdit::AddConditional);
        apply(
            &mut layout,
            LayoutEdit::AddToken {
                line: 1,
                token: Token::Conditional { id: "c1".into() },
            },
        );
        apply(&mut layout, LayoutEdit::AddClause("c1".into()));
        apply(&mut layout, LayoutEdit::SetDisplayMode(DisplayMode::Both));
        let json = serde_json::to_string_pretty(&layout).unwrap();
        let back: MenuBarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, layout);
        assert!(json.contains("\"version\": 1"));
        assert!(json.contains("\"type\": \"percent\""));
    }

    #[test]
    fn file_round_trip_atomic_and_guards() {
        let dir = tmp_dir("file");
        let file = dir.join("menubar-layout.json");
        assert_eq!(load_from(&file), MenuBarLayout::default());
        let layout = MenuBarLayout::from_preset(Preset::Stacked);
        save_to(&layout, &file).unwrap();
        assert_eq!(load_from(&file), layout);
        assert!(!dir.join("menubar-layout.json.tmp").exists());
        // Newer schema and garbage are never overwritten.
        std::fs::write(&file, r#"{"version":99,"lines":[]}"#).unwrap();
        assert!(save_to(&layout, &file).is_err());
        assert_eq!(load_from(&file), MenuBarLayout::default());
        std::fs::write(&file, "not json").unwrap();
        assert!(save_to(&layout, &file).is_err());
        assert!(std::fs::read_to_string(&file).unwrap().contains("not json"));
    }

    #[test]
    fn renders_token_text_for_sample_entry() {
        let e = entry();
        let layout = MenuBarLayout {
            lines: vec![vec![
                Token::Percent {
                    window: Window::Session,
                },
                Token::Pace {
                    window: Window::Weekly,
                },
                Token::ResetCountdown {
                    window: Window::Session,
                },
                Token::ResetCountdown {
                    window: Window::Weekly,
                },
                Token::Balance,
                Token::ProviderName,
            ]],
            ..MenuBarLayout::default()
        };
        let t = texts(&layout, Some(&e));
        assert_eq!(t[0], "80%");
        assert_eq!(t[1], "-12%");
        assert_eq!(t[2], "2h 5m");
        assert_eq!(t[3], "3d 0h");
        assert_eq!(t[4], "$12.50");
        assert!(!t[5].is_empty() && t[5] != UNKNOWN);
        // quota "used" flips percent.
        let used = RenderInput {
            quota_used: true,
            ..input(Some(&e), None)
        };
        let r = render(&layout, &used);
        assert_eq!(r.lines[0][0], Piece::Text("20%".into()));
    }

    #[test]
    fn unknown_values_render_as_dash_never_zero() {
        let layout = MenuBarLayout {
            lines: vec![vec![
                Token::Percent {
                    window: Window::Session,
                },
                Token::Pace {
                    window: Window::Session,
                },
                Token::ResetCountdown {
                    window: Window::Session,
                },
                Token::Balance,
                Token::CostToday,
                Token::Percent {
                    window: Window::ScopedWeekly,
                },
                Token::Conditional {
                    id: "missing".into(),
                },
            ]],
            ..MenuBarLayout::default()
        };
        // No entry at all.
        assert!(texts(&layout, None).iter().all(|t| t == UNKNOWN));
        // Entry with unknown pace, unparseable reset, no credits, NaN cost.
        let mut e = entry();
        e.credits = None;
        e.windows[0].pace_delta = f64::NAN;
        e.windows[0].resets_at = String::new();
        let cost = CostEntry {
            provider: "codex".into(),
            today: f64::NAN,
            month: 0.0,
            tokens: 0,
            provenance: String::new(),
            error: String::new(),
            coverage: String::new(),
            daily: vec![],
        };
        let r = render(&layout, &input(Some(&e), Some(&cost)));
        let t: Vec<_> = r.lines[0]
            .iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(t[0], "80%");
        for unknown in &t[1..] {
            assert_eq!(unknown, UNKNOWN);
        }
        // Unknown bar stays unknown, not 0.
        assert_eq!(
            render(
                &MenuBarLayout {
                    lines: vec![vec![Token::ProgressBar {
                        window: Window::ScopedWeekly
                    }]],
                    ..Default::default()
                },
                &input(Some(&e), None)
            )
            .bars(),
            vec![None]
        );
    }

    #[test]
    fn conditionals_branch_and_unknown_is_false() {
        let mut layout = MenuBarLayout::default();
        apply(&mut layout, LayoutEdit::AddConditional);
        // c1: session used > 50 ? percent(session) : hidden. Used = 20 -> hidden.
        layout.lines = vec![vec![Token::Conditional { id: "c1".into() }]];
        let e = entry();
        assert!(texts(&layout, Some(&e)).is_empty());
        apply(
            &mut layout,
            LayoutEdit::SetDirection("c1".into(), 0, Direction::Remaining),
        );
        assert_eq!(texts(&layout, Some(&e)), vec!["80%"]);
        // Unknown data: predicate false (else branch), not compared as 0.
        apply(
            &mut layout,
            LayoutEdit::SetComparison("c1".into(), 0, Comparison::LessThan),
        );
        assert!(texts(&layout, None).is_empty());
        // OR clause.
        apply(&mut layout, LayoutEdit::AddClause("c1".into()));
        apply(
            &mut layout,
            LayoutEdit::SetCombinator("c1".into(), 1, Combinator::Or),
        );
        apply(
            &mut layout,
            LayoutEdit::SetMetric("c1".into(), 1, Metric::Weekly),
        );
        apply(
            &mut layout,
            LayoutEdit::SetDirection("c1".into(), 1, Direction::Remaining),
        );
        apply(
            &mut layout,
            LayoutEdit::SetComparison("c1".into(), 1, Comparison::LessThan),
        );
        // session remaining < 50 false, weekly remaining 35 < 50 true.
        assert_eq!(texts(&layout, Some(&e)), vec!["80%"]);
    }

    #[test]
    fn editing_add_move_remove_normalizes() {
        let mut layout = MenuBarLayout::default();
        apply(
            &mut layout,
            LayoutEdit::AddToken {
                line: 0,
                token: Token::Space,
            },
        );
        apply(
            &mut layout,
            LayoutEdit::MoveToken {
                line: 0,
                index: 1,
                delta: -1,
            },
        );
        assert_eq!(layout.lines[0], vec![Token::Space, Token::Icon]);
        apply(&mut layout, LayoutEdit::RemoveToken { line: 0, index: 0 });
        apply(&mut layout, LayoutEdit::RemoveToken { line: 0, index: 0 });
        assert_eq!(
            layout,
            MenuBarLayout::default(),
            "empty falls back to default"
        );
        // Line 2 only is promoted; third line impossible.
        apply(
            &mut layout,
            LayoutEdit::AddToken {
                line: 5,
                token: Token::Space,
            },
        );
        assert_eq!(layout.lines.len(), 2);
        layout.lines[0].clear();
        layout.normalize();
        assert_eq!(layout.lines, vec![vec![Token::Space]]);
    }

    #[test]
    fn default_layout_reproduces_current_tray() {
        use crate::tray;
        let layout = MenuBarLayout::default();
        assert_eq!(layout.title_mode, TitleMode::Static);
        let e = entry();
        let rendered = render(&layout, &input(Some(&e), None));
        assert_eq!(rendered.lines, vec![vec![Piece::Icon]]);
        for (style, windows) in [("meters", true), ("icon", true), ("meters", false)] {
            let mut ent = e.clone();
            if !windows {
                ent.windows.clear();
            }
            let view = tray::tray_view(&layout, Some(&ent), None, &params(style), now());
            let old_icon = tray::use_icon_name(style, ent.windows.is_empty());
            assert_eq!(matches!(view.icon, tray::TrayIcon::Named), old_icon);
            assert_eq!(view.title, tray::TRAY_TITLE);
            assert_eq!(view.description, "");
            if let tray::TrayIcon::Pixmap(rgba) = view.icon {
                let w: Vec<_> = ent.windows.iter().take(2).cloned().collect();
                assert_eq!(rgba, tray::meters_pixmap_with(&w, false, false, true, 10));
            }
        }
        // No entry at all behaves like "no windows": themed icon.
        let view = tray::tray_view(&layout, None, None, &params("meters"), now());
        assert!(matches!(view.icon, tray::TrayIcon::Named));
        assert_eq!(view.title, tray::TRAY_TITLE);
    }

    #[test]
    fn layout_title_and_bars_reach_the_tray() {
        use crate::tray;
        let e = entry();
        let layout = MenuBarLayout::from_preset(Preset::IconAndPercent);
        let view = tray::tray_view(&layout, Some(&e), None, &params("meters"), now());
        assert_eq!(view.title, "80%");
        assert_eq!(view.description, "80%");
        let bars = MenuBarLayout::from_preset(Preset::Bars);
        let view = tray::tray_view(&bars, Some(&e), None, &params("meters"), now());
        assert!(matches!(view.icon, tray::TrayIcon::Pixmap(_)));
        assert_eq!(view.title, tray::TRAY_TITLE);
    }
}
