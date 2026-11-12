//! `UiBridge { cmd_tx, event_rx, hub }`: `send(Command)` and snapshot accessors. The UI never
//! blocks on it (`try_recv` only). See MODULES §8 and ARCHITECTURE §5.1.
