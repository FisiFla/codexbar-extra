use crate::model::{CostEntry, Entry};
use crate::settings::Settings;
use crate::spend::SpendModel;
use iced::window;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub enum TrayCommand {
    QuickView,
    Usage,
    Settings,
    Sessions,
    ShareStats,
    OpenAiLive,
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
    pub menubar_layout: crate::menubar_layout::MenuBarLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    QuickView,
    Usage,
    Settings,
    Sessions,
    ShareStats,
    OpenAiLive,
    /// Transient always-on-top overlays (no chrome).
    QuotaAlert,
    Confetti,
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
    pub menubar_layout: crate::menubar_layout::MenuBarLayout,
    pub entries: Vec<Entry>,
    pub prev_entries: Vec<Entry>,
    pub spending: Vec<CostEntry>,
    pub spend: SpendModel,
    pub catalog: Vec<(String, String)>, // (id, displayName)
    pub provider_meta: Vec<crate::cli::ProviderCatalogEntry>,
    /// Unsaved API key / cookie typed in provider detail; cleared after every write.
    pub credential_draft: String,
    pub config_status: Result<String, String>,
    /// Hooks section of the shared config; `None` until the Hooks pane first loads it.
    pub hooks: Option<Result<crate::cli::HooksInfo, String>>,
    pub busy: bool,
    pub cost_busy: bool,
    pub stale: bool,
    pub error: String,
    pub cost_error: String,
    pub updated: String,
    /// Unix time of the last successful refresh; drives the Mac-style relative "Updated" line.
    pub updated_epoch: Option<i64>,
    pub windows: BTreeMap<window::Id, Page>,
    pub selected: i32, // -1 overview, else entry index (popover)
    pub settings_pane: SettingsPane,
    pub settings_search: String,
    /// Settings toolbar back/forward stacks: (pane, selected provider), like the Mac window.
    pub settings_back: Vec<(SettingsPane, Option<String>)>,
    pub settings_forward: Vec<(SettingsPane, Option<String>)>,
    pub settings_sort_alpha: bool,
    pub selected_provider: Option<String>,
    pub spend_period_days: i64,
    pub spend_tab: String,
    pub spend_ui: crate::ui::spend_view::SpendUi,
    pub pending_page: Option<Page>,
    /// Last successful cost load / last cost request (Qt `m_costUpdated` / `m_costRequested`).
    pub cost_updated: Option<std::time::Instant>,
    pub cost_requested: Option<std::time::Instant>,
    pub show_about: bool,
    pub sessions: Vec<crate::sessions::Session>,
    pub sessions_error: String,
    pub sessions_busy: bool,
    pub sessions_loaded: bool,
    /// Per-day model breakdowns from the same `cost` call (chart sections).
    pub cost_payloads: Vec<crate::cli::CostPayload>,
    /// Collapsed chart sections in the popover (`<provider>:<chart>`); charts default open.
    /// Menu sections flipped from their default: the Cost block starts open, chart
    /// sections start collapsed like the Mac submenus (a key here means "toggled").
    pub toggled_charts: BTreeSet<String>,
    /// Hovered bar: (chart key, bar index).
    pub chart_hover: Option<(String, usize)>,
    /// Selected series per chart key (utilization / burndown pickers).
    pub chart_series: BTreeMap<String, usize>,
    /// Local plan-utilization history (`usage-history.json`) and where it is persisted.
    pub history: crate::history::History,
    pub history_path: Option<std::path::PathBuf>,
    /// Cached provider storage footprints; refreshed rarely off the UI thread.
    pub storage: BTreeMap<String, crate::storage::Footprint>,
    pub storage_loaded: bool,
    /// Share Stats window: dark card toggle (session only, not persisted) and last status line.
    pub share_dark: bool,
    pub share_status: String,
    /// `linux-extras.json` toggles, the live overlays and the last "Focus" status line.
    pub extras: crate::extras::Extras,
    pub alert: Option<crate::ui::overlay_view::AlertState>,
    pub confetti: Option<crate::ui::overlay_view::ConfettiState>,
    pub focus_status: String,
    /// Idle inhibitor for the Stay Awake extra; dropped (released) with the app.
    pub stay_awake: crate::stay_awake::StayAwake,
    /// OpenAI Live Usage window state.
    pub live: crate::openai_live::LiveState,
    /// Local session/tool scan (Usage & Spend): raw merged facts, their aggregate for the current
    /// period/time zone, and the browser state. Nothing here is persisted except the cache file.
    pub scan: Option<std::sync::Arc<crate::scan::ScanSnapshot>>,
    pub scan_view: crate::scan::aggregate::View,
    pub scan_busy: bool,
    pub scan_loaded: bool,
    pub scan_zone: crate::scan::aggregate::Zone,
    pub session_query: String,
    pub session_sort: SessionSort,
    pub selected_session: Option<(crate::scan::Provider, String)>,
    pub tool_filter: String,
    pub tools_expanded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSort {
    Recent,
    Cost,
    Tokens,
    Tools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareAction {
    Save,
    CopyImage,
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    RefreshCosts,
    UsageLoaded(Result<Vec<Entry>, String>),
    CostLoaded(Result<(Vec<CostEntry>, Vec<crate::cli::CostPayload>), String>),
    ToggleChart(String),
    ChartHover {
        key: String,
        index: Option<usize>,
    },
    ChartSeries {
        key: String,
        index: usize,
    },
    HistorySaved(Result<(), String>),
    ScanStorage,
    StorageScanned(BTreeMap<String, crate::storage::Footprint>),
    CatalogLoaded(Vec<crate::cli::ProviderCatalogEntry>),
    SetProviderEnabled(String, bool),
    CredentialDraftChanged(String),
    SaveCredential {
        provider: String,
        cookie: bool,
    },
    ClearCredential(String),
    ConfigDone(Result<String, String>),
    HooksLoaded(Result<crate::cli::HooksInfo, String>),
    SetHooksEnabled(bool),
    RefreshHooks,
    Tick,
    ShowPage(Page),
    WindowOpened(window::Id, Page),
    WindowClosed(window::Id),
    SelectEntry(i32),
    TrayCmd(TrayCommand),
    IpcCmd(IpcCommand),
    OpenSettingsPane(SettingsPane),
    SettingsBack,
    SettingsForward,
    SettingsSearchChanged(String),
    ToggleProviderSort,
    SelectProvider(String),
    SettingsChanged(Box<Settings>),
    LayoutEdit(crate::menubar_layout::LayoutEdit),
    SaveSettings,
    SetAutostart(bool),
    AutostartResult(Result<bool, String>),
    SetSpendPeriod(i64),
    SetSpendTab(String),
    Spend(crate::ui::spend_view::SpendMsg),
    ScanLoaded(std::sync::Arc<crate::scan::ScanSnapshot>),
    SetScanZone(crate::scan::aggregate::Zone),
    SessionQuery(String),
    SessionSort(SessionSort),
    SelectSession(Option<(crate::scan::Provider, String)>),
    ToolFilter(String),
    ToggleToolsExpanded,
    AccountAction {
        provider: String,
        login: bool,
    },
    OpenUrl(String),
    CopySummary,
    RefreshSessions,
    FocusSession(i64, Vec<String>),
    FocusDone(Result<String, String>),
    ExtrasChanged(Box<crate::extras::Extras>),
    DismissOverlays,
    OverlayFrame,
    Redrawn(window::Id),
    ForceOverlays,
    SessionsLoaded(Result<Vec<crate::sessions::Session>, String>),
    OpenFolder(String),
    CopyText(String),
    ShareSetPeriod(i64),
    ShareSetDark(bool),
    ShareSave,
    ShareCopyImage,
    ShareCopyText,
    ShareCaptured(ShareAction, Result<Vec<u8>, String>),
    ShareDone(Result<String, String>),
    OpenAiLiveRefresh,
    OpenAiLiveRange(i64),
    OpenAiLiveFilter(Option<String>),
    OpenAiLiveOpenSettings,
    OpenAiLiveLoaded(
        u64,
        Result<crate::openai_live::Snapshot, crate::openai_live::LiveError>,
    ),
    Quit,
    None,
}
