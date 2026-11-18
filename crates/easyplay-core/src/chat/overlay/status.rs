//! Whether the overlay can be shown, published for the hotkey and for Settings
//! (`SnapshotHub::overlay`): `easyplay overlay` answers with the reason instead of OK.
/// The overlay as the service last saw it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayStatus {
    /// Not wanted now (no game, Chat or the setting off): the hotkey says which.
    #[default]
    Idle,
    /// Started; the hotkey opens and closes it.
    Running,
    /// This build has no overlay, or EasyPlay runs without a Wayland display.
    Missing,
    /// The desktop has no layer shell (GNOME): it cannot be shown here.
    Unsupported,
    /// Nothing on this computer can draw a transparent surface.
    NoGpu,
    /// It stopped five times in a row, or speaks another protocol: given up until the next
    /// game or a change of the setting.
    Failing,
}
