use crate::model::{CostEntry, Entry};
use crate::settings::Settings;
use crate::spend::SpendModel;
use iced::window;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum TrayCommand {
    QuickView,
    Usage,
    Settings,
    Refresh,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IpcCommand {
    Show(String),
    Refresh,
    RefreshCostsIfStale,
    Quit,
    ReloadSettings,
}

/// Snapshot-friendly shared state read by tray + IPC threads.
#[derive(Clone, Default)]
pub struct Shared {
    pub entries: Vec<Entry>,
    pub spending: Vec<CostEntry>,
    pub summary: String,
    pub busy: bool,
    pub cost_busy: bool,
    pub stale: bool,
    pub error: String,
    pub cost_error: String,
    pub updated: String,
    pub quota_used: bool,
    pub settings: Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    QuickView,
    Usage,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPane {
    General,
    Providers,
    Menu,
    MenuBar,
    Notifications,
    Spend,
    Hooks,
    Plugins,
    Advanced,
    Debug,
    About,
}

pub struct App {
    pub settings: Settings,
    pub entries: Vec<Entry>,
    pub prev_entries: Vec<Entry>,
    pub spending: Vec<CostEntry>,
    pub spend: SpendModel,
    pub catalog: Vec<(String, String)>, // (id, displayName)
    pub provider_meta: Vec<crate::cli::ProviderCatalogEntry>,
    /// Unsaved API key / cookie typed in provider detail; cleared after every write.
    pub credential_draft: String,
    pub config_status: Result<String, String>,
    pub busy: bool,
    pub cost_busy: bool,
    pub stale: bool,
    pub error: String,
    pub cost_error: String,
    pub updated: String,
    pub windows: BTreeMap<window::Id, Page>,
    pub selected: i32, // -1 overview, else entry index (popover)
    pub settings_pane: SettingsPane,
    pub settings_search: String,
    pub selected_provider: Option<String>,
    pub spend_period_days: i64,
    pub spend_tab: String,
    pub pending_page: Option<Page>,
    /// Last successful cost load / last cost request (Qt `m_costUpdated` / `m_costRequested`).
    pub cost_updated: Option<std::time::Instant>,
    pub cost_requested: Option<std::time::Instant>,
    pub show_about: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    RefreshCosts,
    UsageLoaded(Result<Vec<Entry>, String>),
    CostLoaded(Result<Vec<CostEntry>, String>),
    CatalogLoaded(Vec<crate::cli::ProviderCatalogEntry>),
    SetProviderEnabled(String, bool),
    CredentialDraftChanged(String),
    SaveCredential { provider: String, cookie: bool },
    ClearCredential(String),
    ConfigDone(Result<String, String>),
    Tick,
    ShowPage(Page),
    WindowOpened(window::Id, Page),
    WindowClosed(window::Id),
    SelectEntry(i32),
    TrayCmd(TrayCommand),
    IpcCmd(IpcCommand),
    OpenSettingsPane(SettingsPane),
    SettingsSearchChanged(String),
    SelectProvider(String),
    SettingsChanged(Box<Settings>),
    SaveSettings,
    SetAutostart(bool),
    AutostartResult(Result<bool, String>),
    SetSpendPeriod(i64),
    SetSpendTab(String),
    AccountAction { provider: String, login: bool },
    OpenUrl(String),
    CopySummary,
    Quit,
    None,
}
