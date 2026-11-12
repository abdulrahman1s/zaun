//! Spacing, radii, strokes, sizes and layout constants (UI_DESIGN_SYSTEM §4). Every literal
//! size in widgets and screens comes from here.

/// Spacing scale (8 px grid).
pub mod space {
    /// 2 px.
    pub const S0: f32 = 2.0;
    /// 4 px.
    pub const S1: f32 = 4.0;
    /// 8 px.
    pub const S2: f32 = 8.0;
    /// 12 px.
    pub const S3: f32 = 12.0;
    /// 16 px.
    pub const S4: f32 = 16.0;
    /// 20 px.
    pub const S5: f32 = 20.0;
    /// 24 px.
    pub const S6: f32 = 24.0;
    /// 32 px.
    pub const S8: f32 = 32.0;
    /// 40 px.
    pub const S10: f32 = 40.0;
    /// 48 px.
    pub const S12: f32 = 48.0;
    /// 64 px.
    pub const S16: f32 = 64.0;
}

/// Corner radii.
pub mod radius {
    /// 4 px.
    pub const XS: u8 = 4;
    /// 6 px.
    pub const SM: u8 = 6;
    /// 10 px: cards and fields.
    pub const MD: u8 = 10;
    /// 14 px: sheets.
    pub const LG: u8 = 14;
    /// 20 px.
    pub const XL: u8 = 20;
    /// Full pill (clamped to half the height by the tessellator).
    pub const PILL: u8 = 255;
}

/// Stroke widths.
pub mod stroke {
    /// Borders.
    pub const BORDER: f32 = 1.0;
    /// Focus ring and selected card.
    pub const FOCUS: f32 = 2.0;
    /// Focus ring in the high-contrast theme.
    pub const FOCUS_HC: f32 = 3.0;
    /// Gap between an element and its focus ring.
    pub const FOCUS_GAP: f32 = 2.0;
}

/// Control heights.
pub mod control {
    /// Small: 28 px.
    pub const SM: f32 = 28.0;
    /// Medium: 36 px.
    pub const MD: f32 = 36.0;
    /// Large: 44 px.
    pub const LG: f32 = 44.0;
    /// Extra large: 56 px (Play pill on the game page).
    pub const XL: f32 = 56.0;
    /// Toggle track size.
    pub const TOGGLE_W: f32 = 36.0;
    /// Toggle track height.
    pub const TOGGLE_H: f32 = 20.0;
    /// Chip heights.
    pub const CHIP_SM: f32 = 24.0;
    /// Chip height, regular.
    pub const CHIP_MD: f32 = 28.0;
    /// Menu item height.
    pub const MENU_ITEM: f32 = 32.0;
    /// Minimum Play pill width on the game page.
    pub const PILL_MIN_W: f32 = 120.0;
    /// Minimum pointer target (ACCESSIBILITY §3).
    pub const MIN_TARGET: f32 = 32.0;
    /// Progress bar thickness.
    pub const PROGRESS: f32 = 4.0;
}

/// Icon sizes.
pub mod icon {
    /// Inline with text.
    pub const INLINE: f32 = 16.0;
    /// Buttons and rows.
    pub const BUTTON: f32 = 20.0;
    /// Navigation.
    pub const NAV: f32 = 24.0;
    /// Empty states.
    pub const EMPTY: f32 = 48.0;
}

/// Layout constants (UI_DESIGN_SYSTEM §4, USER_EXPERIENCE §3).
pub mod layout {
    /// Page margin.
    pub const PAGE_MARGIN: f32 = 24.0;
    /// Page margin in the compact layout.
    pub const PAGE_MARGIN_COMPACT: f32 = 16.0;
    /// Gap between cards.
    pub const CARD_GAP: f32 = 16.0;
    /// Gap between cards in the compact layout.
    pub const CARD_GAP_COMPACT: f32 = 12.0;
    /// Sidebar item height.
    pub const SIDEBAR_ITEM: f32 = 36.0;
    /// Sidebar width, regular layout.
    pub const SIDEBAR_W: f32 = 220.0;
    /// Sidebar width, wide layout.
    pub const SIDEBAR_W_WIDE: f32 = 240.0;
    /// Icon rail width (collapsed sidebar).
    pub const RAIL_W: f32 = 56.0;
    /// Toolbar height.
    pub const TOOLBAR_H: f32 = 56.0;
    /// List row height.
    pub const ROW_H: f32 = 44.0;
    /// List row height, compact.
    pub const ROW_H_COMPACT: f32 = 36.0;
    /// Sheet maximum width.
    pub const SHEET_MAX_W: f32 = 720.0;
    /// Dialog maximum width.
    pub const DIALOG_MAX_W: f32 = 480.0;
    /// Sheet padding.
    pub const SHEET_PAD: f32 = 24.0;
    /// Banner height.
    pub const BANNER_H: f32 = 44.0;
    /// Toast maximum width.
    pub const TOAST_MAX_W: f32 = 420.0;
    /// Maximum toasts on screen.
    pub const TOAST_MAX: usize = 3;
    /// Explanatory text line length cap, in characters (≈ 7 px per character of `body`).
    pub const TEXT_MAX_W: f32 = 480.0;
    /// Below this window width the layout is compact.
    pub const COMPACT_BELOW: f32 = 720.0;
    /// At or above this window width the layout is wide.
    pub const WIDE_FROM: f32 = 1200.0;
    /// Below this window width the sidebar collapses automatically.
    pub const SIDEBAR_AUTO_COLLAPSE_BELOW: f32 = 900.0;
    /// Card width, compact density.
    pub const CARD_MIN_COMPACT: f32 = 132.0;
    /// Card width, comfortable density.
    pub const CARD_MIN_COMFORTABLE: f32 = 168.0;
    /// Card width, large density.
    pub const CARD_MIN_LARGE: f32 = 220.0;
    /// Cover aspect ratio (height / width).
    pub const COVER_ASPECT: f32 = 1.5;
    /// Scrim disc behind the card's shield.
    pub const SCRIM_DISC: f32 = 28.0;
    /// Section spacing.
    pub const SECTION_GAP: f32 = 24.0;
    /// Minimum window size.
    pub const MIN_WINDOW: [f32; 2] = [640.0, 480.0];
    /// Default window size.
    pub const DEFAULT_WINDOW: [f32; 2] = [1180.0, 760.0];
}

/// Alpha values used for overlays and dims (fractions of 1).
pub mod alpha {
    /// Cover dim when a game is unavailable.
    pub const UNAVAILABLE_DIM: f32 = 0.40;
    /// Bottom gradient on cards, maximum opacity.
    pub const CARD_GRADIENT: f32 = 0.70;
    /// Fraction of the card covered by the gradient.
    pub const CARD_GRADIENT_SPAN: f32 = 0.40;
    /// Disabled controls.
    pub const DISABLED: f32 = 0.40;
    /// Banner fill.
    pub const BANNER_FILL: f32 = 0.12;
    /// Scrim behind badges on covers.
    pub const SCRIM: f32 = 0.45;
    /// Hover lightness change for buttons.
    pub const HOVER_LIGHTEN: f32 = 0.06;
}

/// Timing values that are not motion (tooltips, toasts, debounce), in seconds.
pub mod timing {
    /// Tooltip delay on hover.
    pub const TOOLTIP_DELAY: f32 = 0.5;
    /// Toast lifetime without an action.
    pub const TOAST: f32 = 5.0;
    /// Toast lifetime with an action.
    pub const TOAST_WITH_ACTION: f32 = 8.0;
    /// Window geometry save debounce.
    pub const GEOMETRY_DEBOUNCE: f32 = 1.0;
    /// Minimum interval between announcements.
    pub const ANNOUNCE_INTERVAL: f32 = 0.5;
    /// Skeleton appears only after this delay.
    pub const SKELETON_DELAY: f32 = 0.15;
    /// Skeleton shimmer period.
    pub const SHIMMER_PERIOD: f32 = 1.2;
}
