//! The game menu (GAME_LIBRARY §10; the design's `gameMenu`).

texts! {
    /// Accessible name of the menu.
    NAME = "menu.name";
    /// Item.
    PLAY = "menu.play";
    /// Item.
    STOP = "menu.stop";
    /// Item.
    OPEN_PAGE = "menu.open_page";
    /// Item.
    FAVORITE = "menu.favorite";
    /// Item.
    UNFAVORITE = "menu.unfavorite";
    /// Submenu.
    TAGS = "menu.tags";
    /// Item at the end of the Tags submenu: the tag editor (new tags are made there).
    NEW_TAG = "menu.new_tag";
    /// Submenu.
    PROTECTION = "menu.protection";
    /// Item at the end of the Protection submenu.
    EDIT_PROTECTION = "menu.edit_protection";
    /// Item.
    OPEN_FOLDER = "menu.open_folder";
    /// Item.
    OPEN_SAVES = "menu.open_saves";
    /// Item.
    ADD_SHORTCUT = "menu.add_shortcut";
    /// Item.
    REMOVE_SHORTCUT = "menu.remove_shortcut";
    /// Item.
    RUN_OTHER = "menu.run_other";
    /// Item.
    ARTWORK = "menu.artwork";
    /// Item.
    HIDE = "menu.hide";
    /// Item.
    UNHIDE = "menu.unhide";
    /// Item.
    COPY_COMMAND = "menu.copy_command";
    /// Developer Mode: the launch inspector for the next launch.
    INSPECT = "menu.inspect";
    /// Item.
    REMOVE = "menu.remove";
    /// Shortcut shown beside Play / Stop.
    KEY_PLAY = "menu.key_play";
    /// Shortcut shown beside "Open game page".
    KEY_OPEN = "menu.key_open";
    /// Shortcut shown beside Favourite.
    KEY_FAVORITE = "menu.key_favorite";
    /// Shortcut shown beside Hide.
    KEY_HIDE = "menu.key_hide";
    /// Shortcut shown beside Remove.
    KEY_REMOVE = "menu.key_remove";
    /// Shortcut shown beside the inspector.
    KEY_INSPECT = "menu.key_inspect";
}

/// The command line that starts a game ("easyplay launch <id>").
pub fn launch_command(id: &str) -> String {
    rust_i18n::t!("menu.launch_command", id = id).into_owned()
}
