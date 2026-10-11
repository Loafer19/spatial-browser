// settings.json — user preferences.

use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Which built-in host/filter subscriptions are enabled.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterLists {
    /// Compiled-in Peter Lowe ad-server hosts (`blocked_domains.txt`).
    pub peter_lowe: bool,
    /// EasyList (network + cosmetic) — wired in a later phase.
    pub easylist: bool,
    /// EasyPrivacy — wired in a later phase.
    pub easyprivacy: bool,
}

impl Default for FilterLists {
    fn default() -> Self {
        Self {
            peter_lowe: true,
            easylist: true,
            easyprivacy: true,
        }
    }
}

/// Environment variable that overrides `AppSettings::gpu` at startup.
pub const GPU_ENV: &str = "SPATIAL_BROWSER_GPU";
/// Environment variable that overrides `AppSettings::osr` at startup.
pub const OSR_ENV: &str = "SPATIAL_BROWSER_OSR";

/// Which GPU wgpu asks for. Low = the display-driving iGPU Chromium also
/// uses, so shared-texture OSR imports work on hybrid laptops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuPower {
    #[default]
    Low,
    High,
}

impl GpuPower {
    pub const ALL: [GpuPower; 2] = [GpuPower::Low, GpuPower::High];

    /// Settings / action id.
    pub fn id(self) -> &'static str {
        match self {
            GpuPower::Low => "low",
            GpuPower::High => "high",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }

    /// `SPATIAL_BROWSER_GPU` value, with its historical aliases.
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value {
            "high" | "discrete" | "dgpu" => Some(GpuPower::High),
            "low" | "integrated" | "igpu" => Some(GpuPower::Low),
            _ => None,
        }
    }
}

/// How CEF hands page pixels to the compositor. Auto = shared GPU texture
/// unless the compositor runs on the discrete GPU (cross-device DMA-BUF
/// import fails), then CPU paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OsrMode {
    #[default]
    Auto,
    Gpu,
    Cpu,
}

impl OsrMode {
    pub const ALL: [OsrMode; 3] = [OsrMode::Auto, OsrMode::Gpu, OsrMode::Cpu];

    pub fn id(self) -> &'static str {
        match self {
            OsrMode::Auto => "auto",
            OsrMode::Gpu => "gpu",
            OsrMode::Cpu => "cpu",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }

    /// `SPATIAL_BROWSER_OSR` value, with its historical aliases.
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value {
            "cpu" | "software" => Some(OsrMode::Cpu),
            "gpu" | "shared" | "accelerated" => Some(OsrMode::Gpu),
            "auto" => Some(OsrMode::Auto),
            _ => None,
        }
    }

    /// Shared-texture OSR with the compositor on `gpu`.
    pub fn shared_texture(self, gpu: GpuPower) -> bool {
        match self {
            OsrMode::Gpu => true,
            OsrMode::Cpu => false,
            OsrMode::Auto => gpu != GpuPower::High,
        }
    }
}

// A value this build doesn't know (hand edit, newer version) falls back to
// the default instead of failing the whole settings.json.
macro_rules! lenient_deserialize {
    ($ty:ty) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = serde_json::Value::deserialize(d)?;
                Ok(value.as_str().and_then(<$ty>::from_id).unwrap_or_default())
            }
        }
    };
}
lenient_deserialize!(GpuPower);
lenient_deserialize!(OsrMode);

/// Frame rate caps offered in Settings.
pub const FRAME_RATES: &[u32] = &[60, 90, 120];

// Container-level default: new fields don't break older settings.json.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Master content-filtering switch. Off → no list / layer applies.
    pub ad_block_enabled: bool,
    /// Apply network (request-cancel) rules from enabled lists.
    pub filter_network_enabled: bool,
    /// Apply cosmetic hiding (CSS inject) — no-op until P2.
    pub filter_cosmetic_enabled: bool,
    /// Apply scriptlets (`+js`) — no-op until P3; default Off.
    pub filter_scriptlets_enabled: bool,
    pub filter_lists: FilterLists,
    /// Strips utm_*/fbclid/gclid/etc. from navigations.
    pub clean_urls_enabled: bool,
    pub default_search_engine: String,
    pub reader_theme: usize,
    pub custom_blocked_hosts: Vec<String>,
    pub target_fps: u32,
    /// Compositor GPU; `SPATIAL_BROWSER_GPU` wins at startup. Restart to apply.
    pub gpu: GpuPower,
    /// Page paint path; `SPATIAL_BROWSER_OSR` wins at startup. Restart to apply.
    pub osr: OsrMode,
    /// Last Settings UI tab: see `SETTINGS_TABS`.
    pub settings_tab: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ad_block_enabled: true,
            filter_network_enabled: true,
            filter_cosmetic_enabled: true,
            filter_scriptlets_enabled: false,
            filter_lists: FilterLists::default(),
            clean_urls_enabled: true,
            default_search_engine: "https://www.google.com/search?q=".to_string(),
            reader_theme: 0,
            custom_blocked_hosts: Vec::new(),
            target_fps: 60,
            gpu: GpuPower::default(),
            osr: OsrMode::default(),
            settings_tab: "general".to_string(),
        }
    }
}

/// Settings page categories: `(id, label)`.
pub const SETTINGS_TABS: &[(&str, &str)] = &[
    ("general", "General"),
    ("blocking", "Blocking"),
    ("appearance", "Appearance"),
    ("performance", "Performance"),
];

impl AppSettings {
    pub fn normalize_tab(tab: &str) -> &'static str {
        SETTINGS_TABS
            .iter()
            .find(|(id, _)| *id == tab)
            .map_or("general", |(id, _)| id)
    }
}

/// Longest custom search URL accepted.
pub const MAX_SEARCH_URL: usize = 2048;

/// A search URL template the omnibox can append a query to: http(s), no
/// whitespace or control characters, bounded length. Returns it trimmed.
pub fn valid_search_engine(url: &str) -> Option<&str> {
    let url = url.trim_matches(' ');
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_ok = rest
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|h| !h.is_empty());
    let clean = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    (host_ok && clean && url.len() <= MAX_SEARCH_URL).then_some(url)
}

// ---- startup-only options -----------------------------------------------------

/// GPU / OSR as resolved once at startup (environment over settings.json).
/// Both decide device and paint path for the whole process, so a changed
/// setting only takes effect after a restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Startup {
    pub gpu: GpuPower,
    pub osr: OsrMode,
    /// Valid `SPATIAL_BROWSER_GPU` / `_OSR` values seen at startup.
    pub gpu_env: Option<GpuPower>,
    pub osr_env: Option<OsrMode>,
}

impl Startup {
    /// `env` reads a variable (injected for tests). An unknown value is
    /// logged and ignored, so the saved setting applies.
    pub fn resolve(saved: &AppSettings, env: impl Fn(&str) -> Option<String>) -> Self {
        let gpu_env = env(GPU_ENV).and_then(|v| {
            let parsed = GpuPower::from_env_value(&v);
            if parsed.is_none() {
                log::warn!("unknown {GPU_ENV}={v:?}, using the saved setting");
            }
            parsed
        });
        let osr_env = env(OSR_ENV).and_then(|v| {
            let parsed = OsrMode::from_env_value(&v);
            if parsed.is_none() {
                log::warn!("unknown {OSR_ENV}={v:?}, using the saved setting");
            }
            parsed
        });
        Self {
            gpu: gpu_env.unwrap_or(saved.gpu),
            osr: osr_env.unwrap_or(saved.osr),
            gpu_env,
            osr_env,
        }
    }

    /// What a restart with today's environment would run with.
    pub fn next(&self, saved: &AppSettings) -> (GpuPower, OsrMode) {
        (
            self.gpu_env.unwrap_or(saved.gpu),
            self.osr_env.unwrap_or(saved.osr),
        )
    }

    /// A saved startup option differs from what this process runs with.
    pub fn restart_pending(&self, saved: &AppSettings) -> bool {
        self.next(saved) != (self.gpu, self.osr)
    }

    /// Shared-texture OSR for this process (only with `accelerated_osr`).
    pub fn shared_texture(&self) -> bool {
        cfg!(feature = "accelerated_osr") && self.osr.shared_texture(self.gpu)
    }
}

static STARTUP: OnceLock<Startup> = OnceLock::new();

/// Fix the startup options from `saved` and the process environment. Call
/// once, before the GPU or any browser is created; later calls are no-ops.
pub fn init_startup(saved: &AppSettings) -> Startup {
    *STARTUP.get_or_init(|| Startup::resolve(saved, |name| std::env::var(name).ok()))
}

/// The startup options (defaults + environment if `init_startup` never ran).
pub fn startup() -> Startup {
    *STARTUP.get_or_init(|| Startup::resolve(&AppSettings::default(), |n| std::env::var(n).ok()))
}

fn path() -> PathBuf {
    crate::persistence::config_path("settings.json")
}

pub fn load() -> AppSettings {
    super::store::load_json(&path(), "settings").unwrap_or_default()
}

pub fn save(settings: &AppSettings) {
    super::store::save_json(&path(), "settings", settings, super::store::Backup::Keep);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    /// settings.json as written by v0.9.0 (before gpu / osr / performance).
    #[test]
    fn previous_version_settings_still_load() {
        let old = r#"{
            "ad_block_enabled": false,
            "filter_network_enabled": true,
            "filter_cosmetic_enabled": false,
            "filter_scriptlets_enabled": true,
            "filter_lists": {"peter_lowe": false, "easylist": true, "easyprivacy": false},
            "clean_urls_enabled": false,
            "default_search_engine": "https://duckduckgo.com/?q=",
            "reader_theme": 2,
            "custom_blocked_hosts": ["ads.example.test"],
            "target_fps": 120,
            "settings_tab": "blocking"
        }"#;
        let s: AppSettings = serde_json::from_str(old).expect("old settings parse");
        assert!(!s.ad_block_enabled && !s.filter_cosmetic_enabled && s.filter_scriptlets_enabled);
        assert!(!s.filter_lists.peter_lowe && !s.filter_lists.easyprivacy);
        assert!(!s.clean_urls_enabled);
        assert_eq!(s.default_search_engine, "https://duckduckgo.com/?q=");
        assert_eq!(s.reader_theme, 2);
        assert_eq!(s.custom_blocked_hosts, ["ads.example.test"]);
        assert_eq!(s.target_fps, 120);
        assert_eq!(s.settings_tab, "blocking");
        assert_eq!((s.gpu, s.osr), (GpuPower::Low, OsrMode::Auto));
        // Missing keys and an empty object are fine too.
        let s: AppSettings = serde_json::from_str("{}").expect("empty");
        assert!(s.ad_block_enabled);
    }

    #[test]
    fn new_fields_round_trip_and_unknown_values_fall_back() {
        let s = AppSettings {
            gpu: GpuPower::High,
            osr: OsrMode::Cpu,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"gpu\":\"high\"") && json.contains("\"osr\":\"cpu\""));
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!((back.gpu, back.osr), (GpuPower::High, OsrMode::Cpu));
        // A value from a newer build must not reset every other setting.
        let s: AppSettings =
            serde_json::from_str(r#"{"gpu":"quantum","osr":7,"target_fps":90}"#).unwrap();
        assert_eq!(
            (s.gpu, s.osr, s.target_fps),
            (GpuPower::Low, OsrMode::Auto, 90)
        );
    }

    #[test]
    fn environment_overrides_saved_startup_options() {
        let saved = AppSettings {
            gpu: GpuPower::High,
            osr: OsrMode::Cpu,
            ..Default::default()
        };

        let none = Startup::resolve(&saved, env(&[]));
        assert_eq!((none.gpu, none.osr), (GpuPower::High, OsrMode::Cpu));
        assert_eq!((none.gpu_env, none.osr_env), (None, None));

        let both = Startup::resolve(&saved, env(&[(GPU_ENV, "igpu"), (OSR_ENV, "shared")]));
        assert_eq!((both.gpu, both.osr), (GpuPower::Low, OsrMode::Gpu));
        assert_eq!(both.gpu_env, Some(GpuPower::Low));
        assert_eq!(both.osr_env, Some(OsrMode::Gpu));

        // Unknown env values are ignored, not a silent default.
        let bad = Startup::resolve(&saved, env(&[(GPU_ENV, "turbo"), (OSR_ENV, "")]));
        assert_eq!((bad.gpu, bad.osr), (GpuPower::High, OsrMode::Cpu));
        assert_eq!((bad.gpu_env, bad.osr_env), (None, None));
    }

    #[test]
    fn restart_pending_only_for_options_the_environment_does_not_pin() {
        let saved = AppSettings::default();
        let started = Startup::resolve(&saved, env(&[(GPU_ENV, "high")]));
        let mut changed = saved.clone();
        assert!(!started.restart_pending(&changed));
        // GPU is pinned by the environment: saving another value changes nothing.
        changed.gpu = GpuPower::High;
        assert!(!started.restart_pending(&changed));
        changed.osr = OsrMode::Cpu;
        assert!(started.restart_pending(&changed));
        changed.osr = OsrMode::Auto;
        assert!(!started.restart_pending(&changed));
    }

    #[test]
    fn auto_osr_follows_the_gpu_choice() {
        // Same rule the env vars had: dGPU without forced OSR → CPU paint.
        assert!(OsrMode::Auto.shared_texture(GpuPower::Low));
        assert!(!OsrMode::Auto.shared_texture(GpuPower::High));
        assert!(OsrMode::Gpu.shared_texture(GpuPower::High));
        assert!(!OsrMode::Cpu.shared_texture(GpuPower::Low));
    }

    #[test]
    fn search_engine_validation() {
        assert_eq!(
            valid_search_engine("  https://kagi.com/search?q= "),
            Some("https://kagi.com/search?q=")
        );
        assert!(valid_search_engine("http://127.0.0.1:8080/?q=").is_some());
        for bad in [
            "",
            "kagi.com/search?q=",
            "javascript:alert(1)//",
            "https://",
            "https:///path",
            "https://a.test/?q=x y",
            "https://a.test/\n",
            "data:text/html,x",
        ] {
            assert_eq!(valid_search_engine(bad), None, "{bad:?}");
        }
        let long = format!("https://a.test/?q={}", "x".repeat(MAX_SEARCH_URL));
        assert_eq!(valid_search_engine(&long), None);
    }

    #[test]
    fn tabs_normalize() {
        assert_eq!(AppSettings::normalize_tab("performance"), "performance");
        assert_eq!(AppSettings::normalize_tab("blocking"), "blocking");
        assert_eq!(AppSettings::normalize_tab("<x>"), "general");
    }
}
