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

/// Corner radii. The radius grows with the size of the object it rounds: keycaps and checks,
/// rows, controls, covers and menus, sheets.
pub mod radius {
    /// 4 px: keycaps, tiny marks.
    pub const XS: u8 = 4;
    /// 6 px: thumbnails, inline icon buttons.
    pub const SM: u8 = 6;
    /// 8 px: rows (sidebar, menu and list rows), icon buttons, segments.
    pub const ROW: u8 = 8;
    /// 10 px: controls (buttons, fields) and panels.
    pub const MD: u8 = 10;
    /// 12 px: covers, menus, popovers, toasts.
    pub const POP: u8 = 12;
    /// 16 px: sheets, dialogs, the spotlight cover.
    pub const LG: u8 = 16;
    /// 20 px.
    pub const XL: u8 = 20;
    /// Full pill (clamped to half the height by the tessellator).
    pub const PILL: u8 = 255;
}

/// Stroke widths.
pub mod stroke {
    /// Borders and hairlines.
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
    /// Inset of a segmented control's thumb from its track.
    pub const SEGMENT_INSET: f32 = 3.0;
    /// Count badge on a toolbar button (active filters).
    pub const BADGE: f32 = 18.0;
}

/// Icon sizes.
pub mod icon {
    /// Small marks inside captions (the shield on a card's status line).
    pub const CAPTION: f32 = 14.0;
    /// Inline with text.
    pub const INLINE: f32 = 16.0;
    /// Rows: sidebar items.
    pub const ROW: f32 = 18.0;
    /// Buttons.
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
    /// Page margin in the wide layout.
    pub const PAGE_MARGIN_WIDE: f32 = 32.0;
    /// Page margin in the compact layout.
    pub const PAGE_MARGIN_COMPACT: f32 = 16.0;
    /// Gap between cards.
    pub const CARD_GAP: f32 = 20.0;
    /// Gap between cards in the compact layout.
    pub const CARD_GAP_COMPACT: f32 = 12.0;
    /// Space between a card's caption and the next row of covers.
    pub const CARD_ROW_GAP: f32 = 12.0;
    /// Space between a cover and its title.
    pub const CAPTION_GAP: f32 = 10.0;
    /// Space between a card's title and its status line.
    pub const CAPTION_LINE_GAP: f32 = 2.0;
    /// Sidebar item height.
    pub const SIDEBAR_ITEM: f32 = 32.0;
    /// Space between the sidebar's edge and its items.
    pub const SIDEBAR_PAD: f32 = 10.0;
    /// Sidebar width, regular layout.
    pub const SIDEBAR_W: f32 = 220.0;
    /// Sidebar width, wide layout.
    pub const SIDEBAR_W_WIDE: f32 = 240.0;
    /// Icon rail width (collapsed sidebar).
    pub const RAIL_W: f32 = 56.0;
    /// Height of the top band: the brand row of the sidebar and a route's toolbar share it.
    pub const TOOLBAR_H: f32 = 60.0;
    /// Row height in sheets and pickers.
    pub const ROW_H: f32 = 44.0;
    /// Library list row height.
    pub const LIST_ROW_H: f32 = 48.0;
    /// List header row height.
    pub const ROW_H_COMPACT: f32 = 36.0;
    /// Thumbnail in a library list row.
    pub const LIST_THUMB: f32 = 32.0;
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
    /// How much wider than the density's minimum a card may grow in a collection too small
    /// to fill a row: a sparse shelf shows larger covers instead of stretching them.
    pub const CARD_SPARSE_SCALE: f32 = 1.38;
    /// Cover aspect ratio (height / width).
    pub const COVER_ASPECT: f32 = 1.5;
    /// Widest cover of the spotlight (a collection of exactly one game).
    pub const SPOTLIGHT_COVER_MAX: f32 = 320.0;
    /// Narrowest cover of the spotlight.
    pub const SPOTLIGHT_COVER_MIN: f32 = 150.0;
    /// Widest text column of the spotlight.
    pub const SPOTLIGHT_TEXT_MAX: f32 = 460.0;
    /// Scrim disc behind a control on artwork (favourite, more).
    pub const SCRIM_DISC: f32 = 30.0;
    /// Inset of the controls on a cover from its edges.
    pub const COVER_INSET: f32 = 8.0;
    /// Section spacing.
    pub const SECTION_GAP: f32 = 24.0;
    /// Height of a group header in the grid.
    pub const GROUP_HEADER_H: f32 = 44.0;
    /// The brand mark in the sidebar.
    pub const BRAND_MARK: f32 = 22.0;
    /// Widest the game page is set: its hero, tabs and sheet share one column, centred in
    /// a wider window.
    pub const PAGE_MAX_W: f32 = 960.0;
    /// Widest column of controls in a game page tab.
    pub const PAGE_TEXT_MAX_W: f32 = 880.0;
    /// Cover on the game page.
    pub const HERO_COVER: f32 = 132.0;
    /// Cover on the game page in the compact layout.
    pub const HERO_COVER_COMPACT: f32 = 96.0;
    /// Height of a link row with one line.
    pub const LINK_ROW_H: f32 = 48.0;
    /// From a link row's left edge to its title: the icon and the gap after it.
    pub const LINK_ROW_LEAD: f32 = 36.0;
    /// Width of a link row's title column: values start on one axis down a list.
    pub const LINK_ROW_TITLE_W: f32 = 148.0;
    /// How far a link row's plate reaches past its content on both sides, so that the
    /// content keeps the page's axis (as the plate of a library list row does).
    pub const LINK_ROW_OVERHANG: f32 = 8.0;
    /// Below this width a link row's value moves under its title.
    pub const LINK_ROW_STACK_BELOW: f32 = 440.0;
    /// Line of a point beneath a link row's value (an access statement).
    pub const POINT_H: f32 = 26.0;
    /// Narrowest column of points; two columns need room for two of these.
    pub const POINT_MIN_W: f32 = 264.0;
    /// A bar of the week strip.
    pub const SPARK_BAR_W: f32 = 6.0;
    /// Space between the bars of the week strip.
    pub const SPARK_GAP: f32 = 3.0;
    /// Height of the week strip.
    pub const SPARK_H: f32 = 18.0;
    /// Minimum window size.
    pub const MIN_WINDOW: [f32; 2] = [640.0, 480.0];
    /// Default window size.
    pub const DEFAULT_WINDOW: [f32; 2] = [1180.0, 760.0];
}

/// Alpha values used for overlays and dims (fractions of 1).
pub mod alpha {
    /// Cover dim when a game is unavailable.
    pub const UNAVAILABLE_DIM: f32 = 0.45;
    /// Disabled controls.
    pub const DISABLED: f32 = 0.40;
    /// Banner fill.
    pub const BANNER_FILL: f32 = 0.12;
    /// Scrim behind controls on covers: keeps a white glyph at 3:1 over any artwork.
    pub const SCRIM: f32 = 0.55;
    /// Hover lightness change for buttons.
    pub const HOVER_LIGHTEN: f32 = 0.06;
    /// The edge of a cover at rest (a hairline of `on_art`).
    pub const COVER_EDGE: f32 = 0.10;
    /// The edge of a cover under the pointer.
    pub const COVER_EDGE_HOT: f32 = 0.28;
}

/// Motion distances and scales (UI_DESIGN_SYSTEM §5). All are skipped under reduced motion.
pub mod travel {
    /// Rise of content that appears: sheets, dialogs, a route, a collection.
    pub const RISE: f32 = 8.0;
    /// Rise of a toast.
    pub const TOAST_RISE: f32 = 12.0;
    /// Scale a menu or dialog grows from.
    pub const POP_FROM: f32 = 0.96;
    /// Scale of a cover while it is pressed.
    pub const PRESS: f32 = 0.98;
    /// Scale a control on a cover grows from when it is revealed.
    pub const REVEAL_FROM: f32 = 0.86;
    /// Extra scale at the peak of the favourite star's pop.
    pub const POP_PEAK: f32 = 0.35;
    /// Delay between rows when the library first appears, in seconds.
    pub const STAGGER: f32 = 0.035;
    /// Rows that take part in that stagger.
    pub const STAGGER_ROWS: usize = 5;
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
