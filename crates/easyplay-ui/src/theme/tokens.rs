//! Spacing, radii, strokes, sizes and layout constants (UI_DESIGN_SYSTEM §4). Every literal
//! size in widgets and screens comes from here. Lengths are logical points.

/// Spacing scale.
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
    /// 24 px.
    pub const S6: f32 = 24.0;
    /// 32 px.
    pub const S8: f32 = 32.0;
    /// 40 px.
    pub const S10: f32 = 40.0;
    /// 64 px.
    pub const S16: f32 = 64.0;
}

/// Corner radii. Play and toasts are pills; a cover's radius follows its width.
pub mod radius {
    /// 1 px: a bar of the play record.
    pub const BAR: u8 = 1;
    /// 2 px: the top corners of a chart bar.
    pub const CHART: u8 = 2;
    /// 4 px: keycaps, checkboxes, links.
    pub const XS: u8 = 4;
    /// 6 px: menu items, the segmented thumb, column headers, tooltips.
    pub const SM: u8 = 6;
    /// 7 px: the words of the filter line, the back link.
    pub const WORD: u8 = 7;
    /// 8 px: buttons, fields, rows.
    pub const ROW: u8 = 8;
    /// 8 px: controls (buttons, fields, selects, the segmented track).
    pub const MD: u8 = 8;
    /// 10 px: menus, notes, the log view, the drop area.
    pub const POP: u8 = 10;
    /// 14 px: dialogs.
    pub const LG: u8 = 14;
    /// Full pill (clamped to half the height by the tessellator).
    pub const PILL: u8 = 255;

    /// The radius of a cover `width` wide: 2.6 % of it (4 at 164, 7 at 280).
    pub fn cover(width: f32) -> u8 {
        (width * 0.026).round().clamp(1.0, 64.0) as u8
    }
}

/// Stroke widths.
pub mod stroke {
    /// Hairlines and outlines.
    pub const BORDER: f32 = 1.0;
    /// The accent line inside a focused field, a dashed outline, an unchecked box.
    pub const FIELD_FOCUS: f32 = 1.5;
    /// Focus ring, selection ring, the tab underline.
    pub const FOCUS: f32 = 2.0;
    /// Focus ring in the high-contrast theme.
    pub const FOCUS_HC: f32 = 3.0;
    /// Gap between an element and its focus ring.
    pub const FOCUS_GAP: f32 = 2.0;
    /// Gap between a cover and its focus or selection ring.
    pub const COVER_RING_GAP: f32 = 3.0;
    /// The ring of a chosen radio.
    pub const RADIO_ON: f32 = 4.5;
    /// The spinner's arc.
    pub const SPINNER: f32 = 2.0;
}

/// Control sizes.
pub mod control {
    /// Buttons, the search field, text fields, top-bar destinations.
    pub const BUTTON: f32 = 30.0;
    /// Horizontal padding of a button.
    pub const BUTTON_PAD: f32 = 10.0;
    /// Gap between a button's icon and its label.
    pub const BUTTON_GAP: f32 = 7.0;
    /// A small button inside a row (a tool on the Overview).
    pub const BUTTON_SM: f32 = 26.0;
    /// The smallest text button (the chart's "Show as table").
    pub const BUTTON_XS: f32 = 22.0;
    /// The Play pill.
    pub const PLAY: f32 = 44.0;
    /// Minimum width of the Play pill.
    pub const PLAY_MIN_W: f32 = 112.0;
    /// Padding of the Play pill, left (before its glyph) and right.
    pub const PLAY_PAD: [f32; 2] = [19.0, 24.0];
    /// Gap between the Play pill's glyph and its label.
    pub const PLAY_GAP: f32 = 9.0;
    /// The compact Play in the pinned tab bar.
    pub const PLAY_SM: f32 = 26.0;
    /// Padding of the compact Play, left and right.
    pub const PLAY_SM_PAD: [f32; 2] = [9.0, 12.0];
    /// Round buttons beside Play (favourite, more).
    pub const ROUND: f32 = 44.0;
    /// Segmented control, selects, the words of the filter line, menu items.
    pub const SM: f32 = 28.0;
    /// Padding of a segmented control's track around its segments.
    pub const SEGMENT_INSET: f32 = 2.0;
    /// Horizontal padding of a segment's label.
    pub const SEGMENT_PAD: f32 = 10.0;
    /// Switch track.
    pub const TOGGLE_W: f32 = 32.0;
    /// Switch track height.
    pub const TOGGLE_H: f32 = 20.0;
    /// Switch knob.
    pub const TOGGLE_KNOB: f32 = 14.0;
    /// How far the knob travels.
    pub const TOGGLE_TRAVEL: f32 = 12.0;
    /// Checkbox and radio.
    pub const CHECK: f32 = 15.0;
    /// Accent swatch.
    pub const SWATCH: f32 = 22.0;
    /// Slider track length.
    pub const SLIDER_W: f32 = 150.0;
    /// Slider track thickness.
    pub const SLIDER_TRACK: f32 = 4.0;
    /// Slider thumb.
    pub const SLIDER_THUMB: f32 = 14.0;
    /// Menu item height.
    pub const MENU_ITEM: f32 = 28.0;
    /// A row to pick in a dialog.
    pub const PICK: f32 = 38.0;
    /// Minimum height of a setting row.
    pub const SETTING_ROW: f32 = 38.0;
    /// A keycap.
    pub const KBD: f32 = 17.0;
    /// A tag's dot.
    pub const DOT: f32 = 7.0;
    /// Play on a cover.
    pub const DISC: f32 = 30.0;
    /// Play on a cover of a small library.
    pub const DISC_BIG: f32 = 40.0;
    /// Inset of Play and badges from a cover's corner.
    pub const DISC_INSET: f32 = 7.0;
    /// Inset of Play on a cover of a small library.
    pub const DISC_BIG_INSET: f32 = 10.0;
    /// A badge on a cover (favourite, selected).
    pub const BADGE: f32 = 20.0;
    /// The stop button of the running game in the top bar.
    pub const RUN_STOP: f32 = 24.0;
    /// The spinner.
    pub const SPINNER: f32 = 14.0;
    /// A toast.
    pub const TOAST: f32 = 38.0;
    /// A toast's action.
    pub const TOAST_ACTION: f32 = 26.0;
    /// Minimum pointer target (ACCESSIBILITY §3).
    pub const MIN_TARGET: f32 = 24.0;
    /// Progress bar thickness.
    pub const PROGRESS: f32 = 4.0;
    /// What stands at the end of the tab bar (the compact Play) is centred in the bar less
    /// this much at its foot, so it sits half of it above the bar's middle.
    pub const TAB_TRAIL_LIFT: f32 = 3.0;
    /// How far a tab's focus ring is drawn inside the tab.
    pub const TAB_RING_INSET: f32 = 3.0;
    /// The slider's height (its track and thumb are centred in it).
    pub const SLIDER_H: f32 = 20.0;
    /// The round remove button of a filter that is on.
    pub const WORD_X: f32 = 18.0;
    /// Space after that remove button, to the word's end.
    pub const WORD_X_END: f32 = 3.0;
    /// The small select under a setting's label.
    pub const SELECT_INLINE: f32 = 22.0;
    /// Padding of a select: left (before its value) and right (after its caret).
    pub const SELECT_PAD: [f32; 2] = [10.0, 7.0];
    /// Padding of the small select under a label: left and right.
    pub const SELECT_INLINE_PAD: [f32; 2] = [8.0, 5.0];
    /// Padding of a multi-line field: across, and above and below.
    pub const TEXTAREA_PAD: [f32; 2] = [10.0, 8.0];
    /// Line height of a multi-line field.
    pub const TEXTAREA_LINE: f32 = 19.0;
    /// Padding of the search field: before the magnifier, after the keycap.
    pub const SEARCH_PAD: [f32; 2] = [9.0, 8.0];
    /// A row to pick in a dialog: padding across, and above and below.
    pub const PICK_PAD: [f32; 2] = [10.0, 5.0];
    /// A row to pick in a dialog: gap between the mark and the content.
    pub const PICK_GAP: f32 = 10.0;
    /// The check inside a checked box.
    pub const CHECK_GLYPH: f32 = 11.0;
    /// The running game's "live" mark: bar width, gap and height.
    pub const LIVE_BAR: [f32; 3] = [2.0, 2.0, 9.0];
    /// The lowest point of a "live" bar, as a fraction of its height.
    pub const LIVE_LOW: f32 = 0.35;
    /// Gap between the glyph and the label of the compact Play.
    pub const PLAY_SM_GAP: f32 = 6.0;
    /// The ring that keeps Play on a cover apart from a light cover.
    pub const DISC_RING: f32 = 1.5;
    /// The soft shadow of Play on a cover: offset down and blur.
    pub const DISC_SHADE: [u8; 2] = [2, 8];
}

/// Icon sizes.
pub mod icon {
    /// Inside text: carets, the shield beside the protection level, a row's arrow.
    pub const SMALL: f32 = 13.0;
    /// The default.
    pub const INLINE: f32 = 16.0;
    /// A badge's glyph, a column header's arrow.
    pub const TINY: f32 = 11.0;
    /// The sort arrow of a list column header.
    pub const HEAD: f32 = 12.0;
}

/// Layout constants (UI_DESIGN_SYSTEM §4, USER_EXPERIENCE §3).
pub mod layout {
    /// Page gutter.
    pub const GUTTER: f32 = 28.0;
    /// Page gutter when the page area is narrower than [`NARROW_BELOW`].
    pub const GUTTER_NARROW: f32 = 16.0;
    /// Height of the top bar.
    pub const TOP_BAR: f32 = 52.0;
    /// Right padding of the top bar.
    pub const TOP_BAR_END: f32 = 18.0;
    /// Gap between the top bar's parts.
    pub const TOP_BAR_GAP: f32 = 6.0;
    /// The mark in the top bar.
    pub const MARK: f32 = 20.0;
    /// Space after the mark.
    pub const MARK_GAP: f32 = 12.0;
    /// Gap between the top bar's destinations.
    pub const NAV_GAP: f32 = 4.0;
    /// Horizontal padding of a destination.
    pub const NAV_PAD: f32 = 9.0;
    /// The search field at rest.
    pub const SEARCH_W: f32 = 208.0;
    /// The search field while focused.
    pub const SEARCH_W_FOCUSED: f32 = 280.0;
    /// The search field in a narrow window, at rest and focused.
    pub const SEARCH_W_NARROW: [f32; 2] = [132.0, 170.0];
    /// Below this window width the search field narrows and buttons drop their labels.
    pub const LABELS_FROM: f32 = 720.0;
    /// Widest the running game's pill in the top bar.
    pub const RUN_PILL_MAX_W: f32 = 300.0;

    /// Height of the filter line.
    pub const FILTER_LINE: f32 = 34.0;
    /// Space under the filter line.
    pub const FILTER_LINE_GAP: f32 = 8.0;
    /// Horizontal padding of a word of the filter line; the line starts this far left of
    /// the gutter so its first word's text stands on the page's axis.
    pub const WORD_PAD: f32 = 8.0;
    /// Gap between a word and what follows it inside the word (count, caret).
    pub const WORD_GAP: f32 = 5.0;
    /// Gap between words.
    pub const WORD_SPACING: f32 = 2.0;
    /// The hairline between scopes and filters: height and the margin on each side.
    pub const FILTER_RULE: [f32; 2] = [14.0, 8.0];
    /// Padding of a scrolling page: top, bottom (its sides are the gutter).
    pub const PAGE_PAD: [f32; 2] = [6.0, 72.0];

    /// Cover width, compact.
    pub const COVER_COMPACT: f32 = 132.0;
    /// Cover width, comfortable.
    pub const COVER_COMFORTABLE: f32 = 164.0;
    /// Cover width, large.
    pub const COVER_LARGE: f32 = 200.0;
    /// Cover width in a library of one game.
    pub const COVER_ONE: f32 = 280.0;
    /// Cover width in a library of up to [`BIG_UP_TO`] games.
    pub const COVER_FEW: f32 = 240.0;
    /// Cover width in a library of up to [`SPARSE_UP_TO`] games.
    pub const COVER_SPARSE: f32 = 208.0;
    /// Up to this many games the wall is a small display in the middle of the window.
    pub const BIG_UP_TO: usize = 3;
    /// Up to this many games covers keep a fixed width and the wall offers "Add game".
    pub const SPARSE_UP_TO: usize = 6;
    /// Where the middle of a small display's covers sits, as a fraction of the viewport.
    pub const BIG_CENTRE: f32 = 0.46;
    /// Gap between covers, across.
    pub const WALL_GAP_X: f32 = 16.0;
    /// Gap between rows of covers.
    pub const WALL_GAP_Y: f32 = 36.0;
    /// Gap between covers of a small library, across.
    pub const SPARSE_GAP_X: f32 = 20.0;
    /// Gap between rows of covers of a small library.
    pub const SPARSE_GAP_Y: f32 = 60.0;
    /// Space between a cover and its caption.
    pub const CAPTION_GAP: f32 = 9.0;
    /// Line height of a caption.
    pub const CAPTION_LINE: f32 = 16.0;
    /// Space between a caption's title and its meta on one line.
    pub const CAPTION_META_GAP: f32 = 8.0;
    /// A caption that would end closer than this to the page's right edge hangs from its
    /// cover's right side instead.
    pub const CAPTION_EDGE: f32 = 20.0;
    /// Cover aspect ratio (height / width).
    pub const COVER_ASPECT: f32 = 1.5;
    /// Under this width a cover shows initials instead of its poster.
    pub const POSTER_MIN_W: f32 = 72.0;
    /// How far a cover rises under the pointer.
    pub const COVER_LIFT: f32 = 3.0;
    /// Space between groups of the wall.
    pub const GROUP_GAP: f32 = 44.0;
    /// Space between a group's heading and its covers.
    pub const GROUP_HEAD_GAP: f32 = 12.0;
    /// Space between a group's heading and its count.
    pub const GROUP_COUNT_GAP: f32 = 6.0;
    /// A small display's covers: the room under their middle taken by the caption.
    pub const BIG_LESS: f32 = 30.0;
    /// A small display: the least space above its row.
    pub const BIG_TOP_MIN: f32 = 8.0;

    /// Library list: header height.
    pub const LIST_HEAD: f32 = 30.0;
    /// Library list: space under the header.
    pub const LIST_HEAD_GAP: f32 = 4.0;
    /// Library list: row height. Also rows of the statistics lists.
    pub const LIST_ROW_H: f32 = 44.0;
    /// Library list: the cover in a row.
    pub const LIST_THUMB: f32 = 24.0;
    /// Library list: gap between columns.
    pub const LIST_COL_GAP: f32 = 16.0;
    /// Library list: widths of the Protection, Last played, Time played and play columns.
    pub const LIST_COLS: [f32; 4] = [110.0, 130.0, 96.0, 30.0];
    /// How far a row's plate reaches past its content on both sides, so that the content
    /// keeps the page's axis.
    pub const ROW_OVERHANG: f32 = 8.0;
    /// A column header's button.
    pub const TH_H: f32 = 24.0;

    /// Empty states: the column.
    pub const BLANK_W: f32 = 440.0;
    /// Empty states: the least height the column is centred in.
    pub const BLANK_MIN_H: f32 = 520.0;
    /// Loading: the cover-shaped blocks on the wall.
    pub const SKELETON_COVERS: usize = 14;
    /// Space between a caption's star and the title.
    pub const CAPTION_STAR_GAP: f32 = 5.0;
    /// Library list: a header button's padding across, and the gap before its arrow.
    pub const TH_PAD: [f32; 2] = [6.0, 4.0];

    /// Game page: the cover column.
    pub const GAME_COVER: f32 = 264.0;
    /// Game page: the cover column in a page narrower than [`MEDIUM_BELOW`].
    pub const GAME_COVER_MEDIUM: f32 = 180.0;
    /// Game page: gap between the columns.
    pub const GAME_GAP: f32 = 44.0;
    /// Game page: gap between the columns in a medium page.
    pub const GAME_GAP_MEDIUM: f32 = 32.0;
    /// Game page: space above the columns.
    pub const GAME_TOP: f32 = 10.0;
    /// Game page: widest the right column's content.
    pub const GAME_COL_MAX: f32 = 720.0;
    /// Game page: left padding inside the scrolling column (room for focus rings).
    pub const GAME_COL_PAD: f32 = 8.0;
    /// Game page: space under the scrolling column's content.
    pub const GAME_BOTTOM: f32 = 80.0;
    /// Game page: gap between tabs, and when the column is narrower than
    /// [`TABS_TIGHT_BELOW`].
    pub const TAB_GAP: [f32; 2] = [22.0, 14.0];
    /// Game page: column width under which the tabs close up.
    pub const TABS_TIGHT_BELOW: f32 = 560.0;
    /// Game page: column width under which two columns of rows become one.
    pub const TWO_COL_BELOW: f32 = 730.0;
    /// Height of a tab.
    pub const TAB_H: f32 = 36.0;
    /// The play record: bars.
    pub const RECORD_BARS: usize = 30;
    /// The play record: height of the bars.
    pub const RECORD_BARS_H: f32 = 30.0;
    /// The play record: gap between bars.
    pub const RECORD_BAR_GAP: f32 = 2.0;
    /// The play record: a day without play.
    pub const RECORD_STUB: f32 = 2.0;

    /// Forms and reading: widest column.
    pub const COL_MAX: f32 = 620.0;
    /// Statistics: widest column.
    pub const STATS_MAX: f32 = 860.0;
    /// Welcome: the column.
    pub const WELCOME_W: f32 = 460.0;
    /// Page-area width under which side-by-side lists stack.
    pub const STACK_BELOW: f32 = 1000.0;
    /// Page-area width under which the game page's cover column narrows.
    pub const MEDIUM_BELOW: f32 = 860.0;
    /// Page-area width under which the cover column is not shown and gutters shrink.
    pub const NARROW_BELOW: f32 = 620.0;

    /// A fact or door row: the label column.
    pub const FACT_LABEL_W: f32 = 128.0;
    /// A fact or door row: the label column in a narrow page.
    pub const FACT_LABEL_W_NARROW: f32 = 96.0;
    /// A fact or door row: gap between columns.
    pub const FACT_GAP: f32 = 16.0;
    /// A fact or door row: gap between columns in a narrow page.
    pub const FACT_GAP_NARROW: f32 = 12.0;
    /// A fact's padding above and below.
    pub const FACT_PAD_Y: f32 = 5.0;
    /// A door's padding all round.
    pub const DOOR_PAD: f32 = 8.0;
    /// Space above a section heading, and below it.
    pub const HEADING_GAP: [f32; 2] = [32.0, 8.0];
    /// Gap between a setting row's label and its control.
    pub const SETTING_GAP: f32 = 20.0;
    /// Widest a row's help text, in points: the design's `56ch`, 56 advances of Inter's
    /// zero at 12 pt.
    pub const HELP_MAX_W: f32 = 424.0;
    /// Help text is set as CSS `text-wrap: pretty` sets it: a last line of one word takes
    /// words from the line above, the measure narrowing in these steps…
    pub const PRETTY_STEP: f32 = 4.0;
    /// …down to this share of the full measure at most.
    pub const PRETTY_MIN: f32 = 0.75;
    /// Gaps of two columns of rows: between rows of groups, between columns.
    pub const TWO_COL_GAP: [f32; 2] = [30.0, 48.0];

    /// A table row.
    pub const TABLE_ROW: f32 = 34.0;
    /// A table header row.
    pub const TABLE_HEAD: f32 = 28.0;
    /// The bar chart: plot height.
    pub const CHART_H: f32 = 132.0;
    /// The bar chart: widest bar.
    pub const CHART_BAR_MAX: f32 = 44.0;
    /// The bar chart: gap between bars, and when there are more than twelve.
    pub const CHART_GAP: [f32; 2] = [10.0, 3.0];
    /// The bar chart: space above it.
    pub const CHART_TOP: f32 = 32.0;
    /// The bar chart: the line above the plot (tallest value, "Show as table").
    pub const CHART_HEAD: f32 = 20.0;
    /// The bar chart: space between that line and the plot.
    pub const CHART_HEAD_GAP: f32 = 4.0;
    /// The bar chart: space between the plot and the labels under it.
    pub const CHART_LABEL_GAP: f32 = 7.0;
    /// The bar chart: a row of its table.
    pub const CHART_TABLE_ROW: f32 = 30.0;
    /// The bar chart: past this many bars the gap closes up and labels thin out.
    pub const CHART_DENSE_FROM: usize = 12;
    /// The bar chart: a dense chart labels every this many bars, counting from the newest.
    pub const CHART_LABEL_EVERY: usize = 5;
    /// Statistics: the plot of the busiest days of the week (kept from the earlier page).
    pub const WEEKDAY_CHART_H: f32 = 66.0;

    /// Statistics: space above the headline.
    pub const STATS_TOP: f32 = 18.0;
    /// Space between a headline and the sentence under it.
    pub const LEDE_GAP: f32 = 10.0;
    /// Statistics: space between the chart and the two lists.
    pub const STATS_LISTS_TOP: f32 = 36.0;
    /// Statistics: the rank column of "Most played".
    pub const RANK_W: f32 = 14.0;
    /// Statistics: the day column of "Recent sessions".
    pub const SESSION_DAY_W: f32 = 76.0;
    /// Statistics: gap between the columns of a list row.
    pub const STATS_COL_GAP: f32 = 12.0;
    /// Statistics: "Most played" shows this many games…
    pub const TOP_SHOWN: usize = 5;
    /// …and at most this many after "Show all".
    pub const TOP_MAX: usize = 20;
    /// Statistics: sessions in "Recent sessions".
    pub const RECENT_SHOWN: usize = 8;
    /// Space above a row of quiet buttons under a list ("Show all 9", "Check again").
    pub const LIST_ACTIONS_GAP: f32 = 6.0;

    /// Settings: space between the top bar and the tab bar.
    pub const SETTINGS_TABS_TOP: f32 = 4.0;
    /// Settings: space between the tab bar and the page.
    pub const SETTINGS_TOP: f32 = 22.0;
    /// Settings: gap between accent swatches.
    pub const SWATCH_GAP: f32 = 10.0;
    /// Settings: the interface size's value ("100 %").
    pub const SLIDER_VALUE_W: f32 = 46.0;
    /// Settings: the field of a program location.
    pub const PATH_FIELD_W: f32 = 280.0;
    /// Settings: the From, Size and action columns of the installed layers.
    pub const LAYER_COLS: [f32; 3] = [80.0, 70.0, 70.0];
    /// Settings: the shares of the Layer and In use columns.
    pub const LAYER_SHARES: [f32; 2] = [1.5, 1.0];
    /// Settings: shares of the name and value fields of a new variable.
    pub const ENV_SHARES: [f32; 2] = [1.0, 1.4];
    /// Settings: space between About's headline and its sentence, and before the version.
    pub const ABOUT_GAP: f32 = 12.0;

    /// Welcome: padding around the column, above and below, and at the sides.
    pub const WELCOME_PAD: [f32; 2] = [32.0, 24.0];
    /// Welcome: the mark.
    pub const WELCOME_MARK: f32 = 32.0;
    /// Welcome: space under the mark.
    pub const WELCOME_MARK_GAP: f32 = 28.0;
    /// Welcome: space above a step's body.
    pub const WELCOME_BODY_GAP: f32 = 26.0;
    /// Welcome: least height of a step's body.
    pub const WELCOME_BODY_MIN: f32 = 210.0;
    /// Welcome: space above the foot.
    pub const WELCOME_FOOT_GAP: f32 = 28.0;
    /// Welcome: padding above and below a line of the first step.
    pub const WELCOME_POINT_PAD: f32 = 10.0;

    /// System check: gap between a check's glyph and its name.
    pub const CHECK_GLYPH_GAP: f32 = 10.0;
    /// System check: how far the glyph sits below the row's top.
    pub const CHECK_GLYPH_DROP: f32 = 1.0;
    /// System check: least gap between a check's name and its result.
    pub const CHECK_RESULT_GAP: f32 = 16.0;
    /// System check: the most of a row's width a check's result may take.
    pub const CHECK_RESULT_SHARE: f32 = 0.6;
    /// A command line: padding at its left, and around the rest.
    pub const CODE_PAD: [f32; 2] = [10.0, 4.0];
    /// A command line: space above it.
    pub const CODE_GAP: f32 = 6.0;

    /// Empty states: padding above and below the column.
    pub const BLANK_PAD: f32 = 40.0;
    /// Empty states: space above the actions.
    pub const BLANK_ACTIONS_GAP: f32 = 24.0;
    /// Damaged library: space above the choices.
    pub const PICKS_GAP: f32 = 18.0;

    /// Menu: least and greatest width.
    pub const MENU_W: [f32; 2] = [180.0, 300.0];
    /// Menu: padding.
    pub const MENU_PAD: f32 = 5.0;
    /// Menu: greatest height.
    pub const MENU_MAX_H: f32 = 560.0;
    /// Menu: gap to its anchor.
    pub const MENU_GAP: f32 = 5.0;
    /// Dialog width.
    pub const DIALOG_MAX_W: f32 = 440.0;
    /// Wide dialog width (Add game, Artwork, Report).
    pub const SHEET_MAX_W: f32 = 560.0;
    /// Dialog padding.
    pub const SHEET_PAD: f32 = 22.0;
    /// Space kept between a dialog and the window's edge.
    pub const DIALOG_MARGIN: f32 = 20.0;
    /// Toast: distance from the window's bottom edge.
    pub const TOAST_BOTTOM: f32 = 24.0;
    /// Tooltip: widest.
    pub const TIP_MAX_W: f32 = 300.0;
    /// Tooltip: gap to its anchor.
    pub const TIP_GAP: f32 = 7.0;
    /// The log view: least and greatest height.
    pub const LOG_H: [f32; 2] = [200.0, 400.0];
    /// A multi-line field: least height.
    pub const TEXTAREA_MIN_H: f32 = 76.0;
    /// The cover preview in a dialog (Set up this game, Artwork).
    pub const DIALOG_COVER: f32 = 112.0;

    /// Minimum window size.
    pub const MIN_WINDOW: [f32; 2] = [640.0, 480.0];
    /// Default window size.
    pub const DEFAULT_WINDOW: [f32; 2] = [1180.0, 760.0];

    /// Toasts on screen: one at a time.
    pub const TOAST_MAX: usize = 1;
    /// Explanatory text line length cap.
    pub const TEXT_MAX_W: f32 = 480.0;
}

/// Alpha values used for overlays and dims (fractions of 1).
pub mod alpha {
    /// A setting row whose condition is unmet.
    pub const DISABLED: f32 = 0.38;
    /// Play while the game is starting.
    pub const BUSY: f32 = 0.8;
    /// The mark on a poster.
    pub const POSTER_MARK: f32 = 0.5;
    /// The low point of a loading block's pulse.
    pub const PULSE_LOW: f32 = 0.45;
    /// A keycap inside a tooltip.
    pub const TIP_KBD: f32 = 0.65;
    /// A danger button under the pointer is this much brighter (CSS `brightness(1.08)`).
    pub const DANGER_HOVER_BRIGHTNESS: f32 = 1.08;
}

/// The poster (UI_DESIGN_SYSTEM §7.2). All are fractions of the cover's width.
pub mod poster {
    /// A line fills this much of the width.
    pub const FILL: f32 = 0.86;
    /// Taken off every fitted size, so a line never touches the right inset.
    pub const EASE: f32 = 0.985;
    /// No line is set larger than this.
    pub const MAX_SIZE: f32 = 0.58;
    /// Line height, as a fraction of the line's size.
    pub const LINE: f32 = 0.9;
    /// The lines' left edge, and the mark's inset from the top left.
    pub const INSET_X: f32 = 0.07;
    /// The block's inset from the bottom edge.
    pub const INSET_BOTTOM: f32 = 0.06;
    /// The room the lines may fill, top to bottom.
    pub const ROOM: f32 = 1.18;
    /// How far a line with letters that hang below it is raised, as a fraction of its size…
    pub const HANG: f32 = 0.12;
    /// …less this much.
    pub const HANG_LESS: f32 = 0.02;
    /// The mark.
    pub const MARK: f32 = 0.09;
    /// Initials, on covers too small for a poster.
    pub const INITIALS: f32 = 0.44;
    /// Tracking of the lines, in em.
    pub const TRACKING: f32 = -0.01;
    /// Most lines a poster has.
    pub const MAX_LINES: usize = 6;
}

/// Motion distances and scales (UI_DESIGN_SYSTEM §5). All are skipped under reduced motion.
pub mod travel {
    /// Rise of content that appears: a tab's body, a settings section, a dialog's step, a
    /// cover newly shown.
    pub const RISE: f32 = 8.0;
    /// Rise of a page's title, actions and tabs when the page arrives.
    pub const PAGE_RISE: f32 = 12.0;
    /// Rise of a toast and of a dialog.
    pub const TOAST_RISE: f32 = 10.0;
    /// Drop of a menu as it opens.
    pub const MENU_DROP: f32 = 4.0;
    /// Scale a menu, a dialog or a cover newly shown grows from.
    pub const POP_FROM: f32 = 0.97;
    /// Scale of a button while it is pressed.
    pub const PRESS: f32 = 0.97;
    /// Scale of a cover while it is pressed.
    pub const COVER_PRESS: f32 = 0.985;
    /// How far a pressed cover stays risen.
    pub const COVER_PRESS_LIFT: f32 = 1.0;
    /// Scale Play on a cover grows from when it is revealed.
    pub const REVEAL_FROM: f32 = 0.86;
    /// Scale the favourite star grows from.
    pub const STAR_FROM: f32 = 0.7;
    /// Delay between covers newly shown, in seconds.
    pub const STAGGER: f32 = 0.018;
    /// Covers that take part in that stagger.
    pub const STAGGER_CAP: usize = 12;
    /// Delay between bars of a chart as they grow, in seconds.
    pub const BAR_STAGGER: f32 = 0.010;
    /// Bars that take part in that stagger.
    pub const BAR_STAGGER_CAP: usize = 30;
    /// Delays of a page's rising parts after the page itself starts to arrive, in seconds:
    /// title, actions, tabs.
    pub const PAGE_RISE_AT: [f32; 3] = [0.0, 0.050, 0.100];
    /// Delay before a page arriving fades in, in seconds.
    pub const PAGE_IN_AT: f32 = 0.080;
    /// Only covers within this distance of the viewport animate.
    pub const REFLOW_MARGIN: f32 = 100.0;
    /// How long the wall's reflow takes while typing in the search field, in seconds.
    pub const REFLOW_TYPING: f32 = 0.200;
    /// Delay between columns of loading blocks, in seconds.
    pub const PULSE_STAGGER: f32 = 0.090;
    /// Columns of loading blocks that pulse in turn.
    pub const PULSE_COLUMNS: usize = 7;
}

/// Timing values that are not motion (tooltips, toasts, debounce), in seconds.
pub mod timing {
    /// Tooltip delay on hover.
    pub const TOOLTIP_DELAY: f32 = 0.5;
    /// Toast lifetime without an action.
    pub const TOAST: f32 = 3.2;
    /// Toast lifetime with an action.
    pub const TOAST_WITH_ACTION: f32 = 6.0;
    /// "Saved" appears this long after typing stops…
    pub const SAVED_AFTER: f32 = 0.5;
    /// …and stays this long.
    pub const SAVED_FOR: f32 = 1.6;
    /// Window geometry save debounce.
    pub const GEOMETRY_DEBOUNCE: f32 = 1.0;
    /// Minimum interval between announcements.
    pub const ANNOUNCE_INTERVAL: f32 = 0.5;
    /// Skeleton appears only after this delay.
    pub const SKELETON_DELAY: f32 = 0.15;
    /// Period of a loading block's pulse.
    pub const SHIMMER_PERIOD: f32 = 1.4;
    /// One turn of the spinner; under reduced motion, [`SPIN_REDUCED`].
    pub const SPIN: f32 = 0.7;
    /// One turn of the spinner under reduced motion.
    pub const SPIN_REDUCED: f32 = 1.6;
    /// Half a beat of the running game's bars.
    pub const LIVE: f32 = 0.9;
}

/// Measures of what floats above a page (menus, dialogs, toasts, tooltips) and of the page
/// furniture that is not a row: the note, the empty state, the drop area and the detail
/// block (CSS "9 · OVERLAYS", `.note`, `.blank`, `.drop`, `.pre`, `.keys`).
pub mod overlay {
    /// Menu item padding: before the content, after it.
    pub const MENU_ITEM_PAD: [f32; 2] = [8.0, 10.0];
    /// Gap between a menu item's parts (tick, label, shortcut).
    pub const MENU_ITEM_GAP: f32 = 8.0;
    /// The column of a menu item's tick, and the tick's glyph.
    pub const MENU_TICK: f32 = 14.0;
    /// A menu's section heading: padding above, at the sides, below.
    pub const MENU_HEAD_PAD: [f32; 3] = [7.0, 8.0, 3.0];
    /// A menu's separator: margin above and below, at the sides.
    pub const MENU_RULE: [f32; 2] = [5.0, 4.0];
    /// Greatest height of a menu, as a fraction of the window's (then at most
    /// `layout::MENU_MAX_H`).
    pub const MENU_MAX_H_FRACTION: f32 = 0.76;
    /// Gap between a menu and the pointer position it opens at.
    pub const MENU_POINTER_GAP: f32 = 2.0;

    /// Room kept at the end of a dialog's title for the close button.
    pub const DIALOG_TITLE_END: f32 = 28.0;
    /// Space between a dialog's title and its text.
    pub const DIALOG_TEXT_GAP: f32 = 8.0;
    /// Space between a dialog's text and its body.
    pub const DIALOG_BODY_GAP: f32 = 16.0;
    /// Padding of a dialog's scrolling body: at the sides (it reaches this far past the
    /// text column, so focus rings are not cut), above and below.
    pub const DIALOG_BODY_PAD: [f32; 2] = [8.0, 2.0];
    /// Space above a dialog's actions.
    pub const DIALOG_ACTIONS_GAP: f32 = 22.0;
    /// Gap between a dialog's actions.
    pub const DIALOG_ACTION_GAP: f32 = 8.0;
    /// Inset of a dialog's close button from its top right corner.
    pub const DIALOG_CLOSE_INSET: f32 = 14.0;
    /// Gap between the cover preview and the column beside it in a dialog.
    pub const DIALOG_COVER_GAP: f32 = 20.0;

    /// A toast's padding: before the text, after the action.
    pub const TOAST_PAD: [f32; 2] = [16.0, 6.0];
    /// Gap between a toast's text and its action.
    pub const TOAST_GAP: f32 = 6.0;
    /// Room after a toast's text.
    pub const TOAST_TEXT_END: f32 = 10.0;
    /// Padding of a toast's action, at the sides.
    pub const TOAST_ACTION_PAD: f32 = 10.0;

    /// A tooltip's padding: at the sides, above and below.
    pub const TIP_PAD: [f32; 2] = [8.0, 4.0];
    /// Gap between a tooltip's text and its shortcut.
    pub const TIP_KBD_GAP: f32 = 8.0;

    /// A note's padding: left, top, right, bottom.
    pub const NOTE_PAD: [f32; 4] = [12.0, 10.0, 10.0, 10.0];
    /// Gap between a note's glyph and its text.
    pub const NOTE_GAP: f32 = 10.0;
    /// How far a note's glyph sits below the top of its text.
    pub const NOTE_GLYPH_DROP: f32 = 2.0;
    /// Line height of a note's text.
    pub const NOTE_LINE: f32 = 19.0;
    /// Gap between a note's title and the error code after it.
    pub const NOTE_CODE_GAP: f32 = 6.0;
    /// A note's actions: space above them, how far they reach past the text on the left
    /// (so their labels stand on its axis), how far below the text they reach, gap between.
    pub const NOTE_ACTIONS: [f32; 4] = [6.0, 9.0, 4.0, 2.0];
    /// Space above a note on a page.
    pub const NOTE_TOP: f32 = 20.0;

    /// An empty state: space between the headline and the sentence.
    pub const BLANK_TEXT_GAP: f32 = 10.0;
    /// An empty state: space above the actions.
    pub const BLANK_ACTIONS_GAP: f32 = 24.0;
    /// An empty state: space above the fine print.
    pub const BLANK_FINE_GAP: f32 = 14.0;
    /// An empty state: padding above and below the column.
    pub const BLANK_PAD: f32 = 40.0;
    /// Line height of fine print (12 pt).
    pub const FINE_LINE: f32 = 17.0;

    /// The drop area of "Add a game": padding at the sides, above and below.
    pub const DROP_PAD: [f32; 2] = [16.0, 28.0];
    /// The drop area: gap between its lines.
    pub const DROP_GAP: f32 = 4.0;
    /// The drop area: space above its button.
    pub const DROP_BUTTON_GAP: f32 = 12.0;
    /// Dash and gap of the drop area's outline.
    pub const DROP_DASH: [f32; 2] = [4.0, 3.0];

    /// The detail block (`.pre`): space above it, padding at the sides and above and below.
    pub const PRE: [f32; 3] = [8.0, 10.0, 8.0];
    /// Line height of the detail block.
    pub const PRE_LINE: f32 = 18.0;
    /// Gap between keycaps, and around "or" between alternatives.
    pub const KEYS_GAP: f32 = 4.0;
    /// A row of the keyboard shortcuts dialog.
    pub const KEYS_ROW: f32 = 32.0;
    /// How long "Add a game" waits, busy, after *Continue* for the setup (or another answer)
    /// before it gives way, in seconds.
    pub const ADD_WAIT: f32 = 5.0;
}

/// The game page's own measures (easyplay-redesign.html, CSS §7), beyond the columns in
/// [`layout`].
pub mod game {
    /// The back link: height.
    pub const BACK_H: f32 = 26.0;
    /// The back link: padding before its caret and after its word.
    pub const BACK_PAD: [f32; 2] = [3.0, 8.0];
    /// The back link: gap between the caret and the word.
    pub const BACK_GAP: f32 = 2.0;
    /// The back link: how far its plate hangs left of the column's axis.
    pub const BACK_HANG: f32 = 6.0;
    /// Space above the title.
    pub const TITLE_TOP: f32 = 10.0;
    /// Space above the meta row (the protection level and the tags).
    pub const META_TOP: f32 = 8.0;
    /// Space above the record's sentence in a narrow page.
    pub const SENTENCE_TOP: f32 = 6.0;
    /// Space above the actions (Play, favourite, more).
    pub const ACTIONS_TOP: f32 = 18.0;
    /// Gap between the actions.
    pub const ACTIONS_GAP: f32 = 8.0;
    /// Space above each note.
    pub const NOTE_TOP: f32 = 20.0;
    /// Space above the tabs.
    pub const TABS_TOP: f32 = 30.0;
    /// Space above a tab's body.
    pub const PANEL_TOP: f32 = 22.0;
    /// The pinned tab bar is pinned once the actions are this close under the column's top.
    pub const PIN_SLACK: f32 = 4.0;
    /// A tab's focus ring: inset and radius.
    pub const TAB_RING_INSET: f32 = 3.0;

    /// The play record: space under the cover.
    pub const RECORD_TOP: f32 = 20.0;
    /// The play record: space above its sentence.
    pub const RECORD_LINE_TOP: f32 = 5.0;
    /// The play record: space above its bars.
    pub const RECORD_BARS_TOP: f32 = 18.0;
    /// The play record: space above the line under its bars.
    pub const RECORD_FOOT_TOP: f32 = 6.0;
    /// The play record: corner of its plate (focus ring).
    pub const RECORD_RADIUS: u8 = 6;

    /// A note: padding left, and on the other sides.
    pub const NOTE_PAD: [f32; 2] = [12.0, 10.0];
    /// A note: gap between its glyph and its text.
    pub const NOTE_GAP: f32 = 10.0;
    /// A note: line height of its text.
    pub const NOTE_LINE: f32 = 19.0;
    /// A note: its glyph sits this far below the text's top.
    pub const NOTE_GLYPH_TOP: f32 = 2.0;
    /// A note: the code after its title, size and gap before it.
    pub const NOTE_CODE: [f32; 2] = [11.0, 6.0];
    /// A note: its actions, space above, how far they hang left (so their text keeps the
    /// axis), and how far they reach into the padding under them.
    pub const NOTE_ACTS: [f32; 3] = [6.0, 9.0, 4.0];
    /// A note: gap between its actions.
    pub const NOTE_ACTS_GAP: f32 = 2.0;

    /// The Overview's small boxed buttons (other programs): gap after each.
    pub const TOOL_GAP: f32 = 6.0;
    /// The Overview's small boxed buttons: the height of the line they sit on (a 26 button
    /// with 4 off above and below, on its text's baseline).
    pub const TOOL_LINE: f32 = 20.0;
    /// The reach lists inside a door: space above them.
    pub const REACH_TOP: f32 = 5.0;
    /// Protection: space above "Custom, based on …".
    pub const CUSTOM_TOP: f32 = 10.0;
    /// Protection: space above the reach lists.
    pub const REACH_LISTS_TOP: f32 = 12.0;
    /// Protection: space above the two columns of switches.
    pub const SWITCHES_TOP: f32 = 26.0;
    /// "+ Add folder…", "+ Add program…": space above, and how far they hang left.
    pub const ROW_BUTTONS: [f32; 2] = [4.0, 10.0];
    /// The small picker under a label: how far it hangs left, and space above it.
    pub const PICKER_HANG: [f32; 2] = [8.0, 1.0];

    /// Fine print: line height.
    pub const FINE_LINE: f32 = 17.0;
    /// Fine print under a field: space above.
    pub const FINE_TOP: f32 = 6.0;
    /// Environment variables: the value field's share of the row against the name's 1.
    pub const ENV_VALUE_SHARE: f32 = 1.4;

    /// Activity: space under the range control.
    pub const RANGE_GAP: f32 = 22.0;
    /// Activity: space above the sentence under the headline.
    pub const LEDE_GAP: f32 = 10.0;
    /// Activity: the history's fixed columns (started, duration, how it ended).
    pub const HISTORY_COLS: [f32; 3] = [70.0, 100.0, 130.0];
    /// Activity of a game never played: least height of the blank.
    pub const BLANK_MIN_H: f32 = 200.0;

    /// Logs: gap of the bar, and space under it.
    pub const LOG_BAR: [f32; 2] = [8.0, 10.0];
    /// Logs: width of the filter field.
    pub const LOG_FILTER_W: f32 = 180.0;
    /// Logs: padding of the log view, across and above and below.
    pub const LOG_PAD: [f32; 2] = [12.0, 10.0];
    /// Logs: line height of the log view.
    pub const LOG_LINE: f32 = 19.0;
    /// Logs: space above the fine print.
    pub const LOG_NOTE_TOP: f32 = 8.0;
}

/// The developer views and the quick-launch window (not drawn in the design; built from its
/// parts): table columns and the window's own measures.
pub mod debug {
    /// A table's index column ("#").
    pub const INDEX_COL: f32 = 28.0;
    /// A table's column of short words (a mount's kind, a descriptor's kind, an event's kind).
    pub const KIND_COL: f32 = 64.0;
    /// A table's column of time offsets ("+2.580 s").
    pub const OFFSET_COL: f32 = 72.0;
    /// The mount table's mode column ("ro", "rw").
    pub const MODE_COL: f32 = 32.0;
    /// The environment table's origin column.
    pub const ORIGIN_COL: f32 = 80.0;
    /// The process table: PID, role, state, CPU time and memory columns.
    pub const PROCESS_COLS: [f32; 5] = [52.0, 72.0, 60.0, 64.0, 64.0];
    /// The mount filter field's width.
    pub const FILTER_W: f32 = 180.0;
    /// Space between a section's controls and the table under them.
    pub const CONTROLS_GAP: f32 = 8.0;
    /// Rows of an access statement that is open: how far they stand in from its text.
    pub const DETAIL_INDENT: f32 = 12.0;

    /// The quick-launch window: padding around its content.
    pub const QUICK_PAD: f32 = 16.0;
    /// The quick-launch window: the poster's width (2:3).
    pub const QUICK_COVER_W: f32 = 48.0;
    /// The quick-launch window: gap between the poster and the text beside it.
    pub const QUICK_COVER_GAP: f32 = 14.0;
    /// The quick-launch window: space between the title and the phase, and above the bar.
    pub const QUICK_LINE_GAP: f32 = 4.0;
}
