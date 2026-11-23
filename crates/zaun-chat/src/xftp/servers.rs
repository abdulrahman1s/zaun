//! The preset XFTP servers of simplexmq 7.1.4 (FileTransfer/Client/Presets.hs) and simplex-chat's Flux operator (Operators/Presets.hs).
use super::address::XftpServer;

pub const SIMPLEX: [&str; 6] = [
    "xftp://da1aH3nOT-9G8lV7bWamhxpDYdJ1xmW7j3JpGaDR5Ug=@xftp1.simplex.im,blisztokwh6alt5oem45f2eksm2xdowxszmhjesntcyolmhutoon7sid.onion",
    "xftp://5vog2Imy1ExJB_7zDZrkV1KDWi96jYFyy9CL6fndBVw=@xftp2.simplex.im,jk6jybnel5hin2am5omd7g3hyvb4tfkyl7ea4vx5ldi7gpvz5ogqtcqd.onion",
    "xftp://PYa32DdYNFWi0uZZOprWQoQpIk5qyjRJ3EF7bVpbsn8=@xftp3.simplex.im,pqhchr4bpkzef5wra4hnmzvppn5j5mjrpzkh5ixx6mhylnxxd6lmwnad.onion",
    "xftp://k_GgQl40UZVV0Y4BX9ZTyMVqX5ZewcLW0waQIl7AYDE=@xftp4.simplex.im,q2lrjgiyo2efyweksavwxh3dlsrxetaxjztde2x5nateugrtrwzvmdid.onion",
    "xftp://-bIo6o8wuVc4wpZkZD3tH-rCeYaeER_0lz1ffQcSJDs=@xftp5.simplex.im,2kikdbdhlrluburwt6q6rnoxvezcm2ifx2hj463jlunkeftlzm7hf3qd.onion",
    "xftp://6nSvtY9pJn6PXWTAIMNl95E1Kk1vD7FM2TeOA64CFLg=@xftp6.simplex.im,tg3fmu6houq7vfqlsipm2qqzhtpgxesnuydqozx3khojxydhs53sbbyd.onion",
];

pub const FLUX: [&str; 6] = [
    "xftp://92Sctlc09vHl_nAqF2min88zKyjdYJ9mgxRCJns5K2U=@xftp1.simplexonflux.com,apl3pumq3emwqtrztykyyoomdx4dg6ysql5zek2bi3rgznz7ai3odkid.onion",
    "xftp://YBXy4f5zU1CEhnbbCzVWTNVNsaETcAGmYqGNxHntiE8=@xftp2.simplexonflux.com,c5jjecisncnngysah3cz2mppediutfelco4asx65mi75d44njvua3xid.onion",
    "xftp://ARQO74ZSvv2OrulRF3CdgwPz_AMy27r0phtLSq5b664=@xftp3.simplexonflux.com,dc4mohiubvbnsdfqqn7xhlhpqs5u4tjzp7xpz6v6corwvzvqjtaqqiqd.onion",
    "xftp://ub2jmAa9U0uQCy90O-fSUNaYCj6sdhl49Jh3VpNXP58=@xftp4.simplexonflux.com,4qq5pzier3i4yhpuhcrhfbl6j25udc4czoyascrj4yswhodhfwev3nyd.onion",
    "xftp://Rh19D5e4Eez37DEE9hAlXDB3gZa1BdFYJTPgJWPO9OI=@xftp5.simplexonflux.com,q7itltdn32hjmgcqwhow4tay5ijetng3ur32bolssw32fvc5jrwvozad.onion",
    "xftp://0AznwoyfX8Od9T_acp1QeeKtxUi676IBIiQjXVwbdyU=@xftp6.simplexonflux.com,upvzf23ou6nrmaf3qgnhd6cn3d74tvivlmz3p7wdfwq6fhthjrjiiqid.onion",
];

/// All twelve, SimpleX first. A user's default is three random SimpleX and three random Flux servers (native-chat-p0 section 8).
#[must_use]
pub fn preset_servers() -> Vec<XftpServer> {
    SIMPLEX
        .iter()
        .chain(FLUX.iter())
        .filter_map(|s| s.parse().ok())
        .collect()
}
