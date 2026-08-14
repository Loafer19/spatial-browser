// Switchable UI chrome themes: canvas background, page-corner rounding,
// focus ring, and the matching palette for the F1 help page
// (hotkeys::open_help) — kept as one struct so nothing (dimensions
// included, not just colors) can drift out of sync between what's
// rendered and what the help page describes. Cycled at runtime via
// Ctrl+Shift+Space (hotkeys::cycle_theme); page *content* is whatever
// CEF rendered and has no theme applied to it.

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub canvas_background: wgpu::Color,
    pub corner_radius: f32,
    pub focus_border_width: f32,
    pub focus_border_color: [f32; 4],
    // CSS-ready rgb() strings (not hex — an unescaped `#` in a `data:`
    // URL starts a fragment, silently truncating the rest of the
    // document) for the F1 help page.
    pub help_bg: &'static str,
    pub help_fg: &'static str,
    pub help_heading: &'static str,
    pub help_card_bg: &'static str,
    pub help_card_border: &'static str,
    pub help_key_bg: &'static str,
    pub help_key_fg: &'static str,
}

// This machine's actual active Omarchy theme (Tokyo Night), colors read
// from /usr/share/omarchy/themes/tokyo-night/colors.toml. Sharp corners,
// not rounded — Omarchy/Quattro's own chrome doesn't round them either.
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
    help_bg: "rgb(26,27,38)",
    help_fg: "rgb(169,177,214)",
    help_heading: "rgb(192,202,245)",
    help_card_bg: "rgb(36,40,59)",
    help_card_border: "rgb(65,72,104)",
    help_key_bg: "rgb(122,162,247)",
    help_key_fg: "rgb(19,20,28)",
};

// Terminal look: near-black background, sharp corners (a character grid
// doesn't round), a thin border (terminal box-drawing rules are 1px),
// and a bright orange accent instead of Tokyo Night's blue.
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
    help_bg: "rgb(5,5,5)",
    help_fg: "rgb(220,220,220)",
    help_heading: "rgb(255,153,51)",
    help_card_bg: "rgb(18,18,18)",
    help_card_border: "rgb(255,153,51)",
    help_key_bg: "rgb(255,153,51)",
    help_key_fg: "rgb(0,0,0)",
};

pub const ALL: &[Theme] = &[TOKYO_NIGHT, ANSI_TERMINAL];
