//! `<GAME>` on the command line and the instance socket (CONFIGURATION §6): a game ID, a
//! unique ID prefix of at least 6 characters, or a name matching exactly one game
//! case-insensitively. Ambiguity is an error that lists the candidates.

use crate::error::{Error, ErrorCode};
use crate::ids::GameId;

/// Shortest ID prefix accepted.
pub const MIN_PREFIX: usize = 6;
/// Candidates listed in an ambiguity error.
const MAX_LISTED: usize = 10;

/// Resolve `arg` among `games` (`(id, name)` pairs).
pub fn resolve_game<'a>(
    games: impl IntoIterator<Item = (GameId, &'a str)>,
    arg: &str,
) -> Result<GameId, Error> {
    let arg = arg.trim();
    let games: Vec<(GameId, &str)> = games.into_iter().collect();
    if let Ok(id) = arg.parse::<GameId>()
        && games.iter().any(|(g, _)| *g == id)
    {
        return Ok(id);
    }
    let lower = arg.to_lowercase();
    let by_prefix =
        |(g, _): &&(GameId, &str)| arg.len() >= MIN_PREFIX && g.to_string().starts_with(&lower);
    let by_name = |(_, n): &&(GameId, &str)| n.to_lowercase() == lower;
    // An exact name wins over a prefix that happens to match too.
    let named: Vec<&(GameId, &str)> = games.iter().filter(by_name).collect();
    let matches: Vec<&(GameId, &str)> = if named.len() == 1 {
        named
    } else {
        games
            .iter()
            .filter(|g| by_name(g) || by_prefix(g))
            .collect()
    };
    match matches.as_slice() {
        [(id, _)] => Ok(*id),
        [] => Err(Error::new(ErrorCode::LibInvalidField)
            .with("field", "game")
            .with("detail", format!("no game matches {arg:?}"))),
        many => {
            let mut list: Vec<String> = many
                .iter()
                .take(MAX_LISTED)
                .map(|(g, n)| format!("{} {n}", g.short()))
                .collect();
            if many.len() > MAX_LISTED {
                list.push(format!("… {} more", many.len() - MAX_LISTED));
            }
            Err(Error::new(ErrorCode::LibInvalidField)
                .with("field", "game")
                .with(
                    "detail",
                    format!("{arg:?} matches {} games: {}", many.len(), list.join(", ")),
                ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_prefixes_and_names() {
        let a: GameId = "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap();
        let b: GameId = "0192f0c2-aaaa-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap();
        let c: GameId = "11112222-aaaa-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap();
        let games = [(a, "Hollow Knight"), (b, "Celeste"), (c, "celeste")];
        let r = |s: &str| resolve_game(games.iter().copied(), s);
        assert_eq!(r(&a.to_string()).unwrap(), a);
        assert_eq!(r("hollow knight").unwrap(), a);
        assert_eq!(r("0192f0c2-7b").unwrap(), a);
        assert_eq!(r("111122").unwrap(), c);
        // Too short a prefix.
        assert!(r("0192").is_err());
        let e = r("0192f0c2").unwrap_err();
        let d = e.context_value("detail").unwrap();
        assert!(d.contains("matches 2 games"), "{d}");
        assert!(d.contains("Hollow Knight") && d.contains("Celeste"), "{d}");
        let e = r("CELESTE").unwrap_err();
        assert!(e.context_value("detail").unwrap().contains("matches 2"));
        assert!(r("nothing").is_err());
    }
}
