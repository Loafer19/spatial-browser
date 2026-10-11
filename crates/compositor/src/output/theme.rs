// UI chrome themes (canvas, corners, focus ring, help palette). Ctrl+Shift+Space cycles.

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub canvas_background: wgpu::Color,
    pub corner_radius: f32,
    pub focus_border_width: f32,
    pub focus_border_color: [f32; 4],
    // Always-on, thinner/dimmer than the focus ring — keeps an unfocused
    // page's edge visible even when its content happens to match
    // canvas_background (a same-theme dark page on a dark canvas would
    // otherwise have no visible boundary at all).
    pub unfocused_border_width: f32,
    pub unfocused_border_color: [f32; 4],
    // Multiplied into an unfocused page's rendered color (1.0 = no
    // dimming) — makes the focused page pop by contrast, on top of the
    // border above.
    pub unfocused_dim: f32,
    // What dimming, the loading placeholder and the load-bar track fade
    // toward: black on dark themes (plain darkening); a light neutral on
    // light themes, so unfocused pages wash out instead of turning gray.
    pub fade_color: [f32; 4],
    // Dot grid drawn on the bare canvas (see background.wgsl) — a
    // spatial reference that pans/zooms with everything else, not a
    // static wallpaper. Spacing/radius are world-space units.
    pub background_dot_color: [f32; 4],
    pub background_grid_spacing: f32,
    pub background_dot_radius: f32,
    // CSS-ready rgb() strings for the built-in pages (rgb() dates from
    // raw data: URLs, where `#` cut the document; harmless to keep).
    pub help_bg: &'static str,
    pub help_fg: &'static str,
    pub help_heading: &'static str,
    pub help_card_bg: &'static str,
    pub help_card_border: &'static str,
    pub help_key_bg: &'static str,
    pub help_key_fg: &'static str,
    // Error / destructive text in generated UI pages.
    pub help_danger: &'static str,
    // Completed / OK state (downloads, saved banners).
    pub help_success: &'static str,
    // CSS font stacks: UI text, and URLs / keys / code.
    pub help_font: &'static str,
    pub help_mono: &'static str,
    // Card / button rounding in built-in pages (px); independent of tile corners.
    pub help_radius: f32,
    // CSS `color-scheme` for built-in pages (form controls, scrollbars).
    pub is_dark: bool,
}

impl Theme {
    /// CSS `border-radius` matching canvas page-tile rounding.
    pub fn css_radius(&self) -> String {
        format!("{}px", self.corner_radius.max(0.0))
    }

    /// Slightly tighter radius for buttons / inputs inside cards.
    pub fn css_radius_inner(&self) -> String {
        format!("{}px", (self.corner_radius * 0.75).max(0.0))
    }
}

/// Proportional UI text; system-ui resolves through fontconfig on Linux.
const UI_SANS: &str =
    "Inter, \"Inter Variable\", \"Noto Sans\", \"Segoe UI\", Roboto, system-ui, sans-serif";
/// URLs, shortcuts, code — and all text in terminal-style themes.
const UI_MONO: &str =
    "\"JetBrainsMono Nerd Font\", \"JetBrains Mono\", ui-monospace, \"Cascadia Code\", monospace";

// Tokyo Night (Omarchy colors.toml); sharp corners like Omarchy chrome.
pub const TOKYO_NIGHT: Theme = Theme {
    name: "Tokyo Night",
    canvas_background: wgpu::Color {
        r: 0.102,
        g: 0.106,
        b: 0.149,
        a: 1.0,
    },
    corner_radius: 0.0,
    focus_border_width: 4.0,
    focus_border_color: [0.478, 0.635, 0.969, 1.0], // accent #7aa2f7
    unfocused_border_width: 2.0,
    unfocused_border_color: [0.4, 0.42, 0.52, 1.0], // dim blue-gray #666b85
    unfocused_dim: 0.72,
    fade_color: [0.0, 0.0, 0.0, 1.0],
    background_dot_color: [1.0, 1.0, 1.0, 0.06],
    background_grid_spacing: 48.0,
    background_dot_radius: 1.6,
    help_bg: "rgb(26,27,38)",
    help_fg: "rgb(169,177,214)",
    help_heading: "rgb(192,202,245)",
    help_card_bg: "rgb(36,40,59)",
    help_card_border: "rgb(65,72,104)",
    help_key_bg: "rgb(122,162,247)",
    help_key_fg: "rgb(19,20,28)",
    help_danger: "rgb(247,118,142)",
    help_success: "rgb(158,206,106)",
    help_font: UI_SANS,
    help_mono: UI_MONO,
    help_radius: 8.0,
    is_dark: true,
};

// Terminal look: near-black, sharp corners, thin border, orange accent.
pub const ANSI_TERMINAL: Theme = Theme {
    name: "ANSI Terminal",
    canvas_background: wgpu::Color {
        r: 0.02,
        g: 0.02,
        b: 0.02,
        a: 1.0,
    },
    corner_radius: 0.0,
    focus_border_width: 1.5,
    focus_border_color: [1.0, 0.6, 0.2, 1.0],
    unfocused_border_width: 1.0,
    unfocused_border_color: [0.35, 0.35, 0.35, 1.0],
    unfocused_dim: 0.7,
    fade_color: [0.0, 0.0, 0.0, 1.0],
    background_dot_color: [1.0, 0.6, 0.2, 0.08],
    background_grid_spacing: 40.0,
    background_dot_radius: 1.2,
    help_bg: "rgb(5,5,5)",
    help_fg: "rgb(220,220,220)",
    help_heading: "rgb(255,153,51)",
    help_card_bg: "rgb(18,18,18)",
    help_card_border: "rgb(255,153,51)",
    help_key_bg: "rgb(255,153,51)",
    help_key_fg: "rgb(0,0,0)",
    help_danger: "rgb(255,85,85)",
    help_success: "rgb(80,250,123)",
    help_font: UI_MONO,
    help_mono: UI_MONO,
    help_radius: 0.0,
    is_dark: true,
};

// Catppuccin Latte (catppuccin.com/palette): the light theme. Canvas is
// crust so base-colored pages and panels sit a step above it.
pub const CATPPUCCIN_LATTE: Theme = Theme {
    name: "Catppuccin Latte",
    canvas_background: wgpu::Color {
        r: 0.863,
        g: 0.878,
        b: 0.910,
        a: 1.0,
    }, // crust #dce0e8
    corner_radius: 10.0,
    focus_border_width: 3.0,
    focus_border_color: [0.118, 0.400, 0.961, 1.0], // blue #1e66f5
    unfocused_border_width: 1.5,
    unfocused_border_color: [0.737, 0.753, 0.800, 1.0], // surface1 #bcc0cc
    unfocused_dim: 0.8,
    fade_color: [0.937, 0.945, 0.961, 1.0], // base #eff1f5
    background_dot_color: [0.298, 0.310, 0.412, 0.16], // text #4c4f69
    background_grid_spacing: 48.0,
    background_dot_radius: 1.6,
    help_bg: "rgb(239,241,245)",
    help_fg: "rgb(76,79,105)",
    help_heading: "rgb(53,55,77)",
    help_card_bg: "rgb(230,233,239)",
    help_card_border: "rgb(204,208,218)",
    help_key_bg: "rgb(30,102,245)",
    help_key_fg: "rgb(239,241,245)",
    help_danger: "rgb(210,15,57)",
    help_success: "rgb(64,160,43)",
    help_font: UI_SANS,
    help_mono: UI_MONO,
    help_radius: 10.0,
    is_dark: false,
};

pub const ALL: &[Theme] = &[TOKYO_NIGHT, ANSI_TERMINAL, CATPPUCCIN_LATTE];

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(s: &str) -> [f32; 3] {
        let inner = s
            .strip_prefix("rgb(")
            .and_then(|t| t.strip_suffix(')'))
            .unwrap_or_else(|| panic!("not rgb(): {s}"));
        let v: Vec<f32> = inner
            .split(',')
            .map(|p| p.trim().parse::<f32>().expect("number") / 255.0)
            .collect();
        assert_eq!(v.len(), 3, "{s}");
        [v[0], v[1], v[2]]
    }

    /// WCAG relative luminance.
    fn luminance([r, g, b]: [f32; 3]) -> f32 {
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn names_are_unique_and_page_colors_parse() {
        for (i, theme) in ALL.iter().enumerate() {
            assert!(ALL[..i].iter().all(|t| t.name != theme.name));
            for c in [
                theme.help_bg,
                theme.help_fg,
                theme.help_heading,
                theme.help_card_bg,
                theme.help_card_border,
                theme.help_key_bg,
                theme.help_key_fg,
                theme.help_danger,
                theme.help_success,
            ] {
                rgb(c);
            }
        }
    }

    #[test]
    fn page_text_is_legible_in_every_theme() {
        for theme in ALL {
            let bg = rgb(theme.help_bg);
            let card = rgb(theme.help_card_bg);
            for (what, fg, base, min) in [
                ("fg on bg", theme.help_fg, bg, 4.5),
                ("fg on card", theme.help_fg, card, 4.5),
                ("heading on bg", theme.help_heading, bg, 4.5),
                ("danger on bg", theme.help_danger, bg, 4.0),
                // Semibold button / kbd labels; Latte's base-on-blue is 4.3.
                ("key", theme.help_key_fg, rgb(theme.help_key_bg), 4.0),
            ] {
                let ratio = contrast(rgb(fg), base);
                assert!(ratio >= min, "{}: {what} = {ratio:.2}", theme.name);
            }
        }
    }

    #[test]
    fn is_dark_matches_the_palette() {
        for theme in ALL {
            let dark = luminance(rgb(theme.help_bg)) < 0.2;
            assert_eq!(dark, theme.is_dark, "{}", theme.name);
            // Dark themes fade to black, i.e. dimming is plain darkening.
            if theme.is_dark {
                assert_eq!(theme.fade_color[..3], [0.0, 0.0, 0.0], "{}", theme.name);
            }
        }
    }
}
