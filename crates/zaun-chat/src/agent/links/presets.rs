//! Short links of preset servers carry only the host: restoring and shortening them (Agent/Protocol.hs `restoreShortLink`, `shortenShortLink`, AGPL-3.0-only).
use super::short_link::ConnShortLink;
use crate::smp::SmpServer;

fn find_preset<'a>(presets: &'a [SmpServer], srv: &SmpServer) -> Option<&'a SmpServer> {
    presets
        .iter()
        .find(|p| p.hosts.first() == srv.hosts.first())
}

/// `restoreShortLink`: fills in the port, key hash and extra hosts of a preset server written by host only.
#[must_use]
pub fn restore_short_link(presets: &[SmpServer], link: ConnShortLink) -> ConnShortLink {
    let full = link
        .server()
        .is_only_host()
        .then(|| find_preset(presets, link.server()).cloned())
        .flatten();
    match full {
        Some(srv) => link.with_server(srv),
        None => link,
    }
}

/// `isPresetServer`.
fn is_preset(presets: &[SmpServer], srv: &SmpServer) -> bool {
    find_preset(presets, srv).is_some_and(|p| {
        srv.hosts.iter().all(|h| p.hosts.contains(h))
            && (srv.port == p.port || (p.port.is_none() && matches!(srv.port, Some(443 | 5223))))
            && srv.key_hash == p.key_hash
    })
}

/// `shortenShortLink`: the preset servers passed are the built-in ones, not the user's own.
#[must_use]
pub fn shorten_short_link(presets: &[SmpServer], link: ConnShortLink) -> ConnShortLink {
    if !is_preset(presets, link.server()) {
        return link;
    }
    let short = SmpServer::only_host(&link.server().hosts[0]);
    link.with_server(short)
}
