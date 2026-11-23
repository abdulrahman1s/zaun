//! Relay checks (codec: a_relay_that_does_not_answer_is_disabled_…, when_no_relay_answers_nothing_is_disabled, …).

use super::Call;
use super::relays_fake::{DEAD, Fake, LIVE, THIRD, started};
use crate::engine::clock::Clock;

fn disabled_calls(t: &super::T) -> Vec<Vec<String>> {
    t.e.agent
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::SetDisabledRelays(d) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

fn servers(f: &Fake) -> Vec<String> {
    let mut s: Vec<String> = f.asked().into_iter().map(|(s, _)| s).collect();
    s.sort();
    s
}

#[test]
fn a_check_tests_only_the_users_own_relays_after_the_start() {
    let (_, fake) = started(&[LIVE, DEAD]);
    assert_eq!(servers(&fake), vec![LIVE.to_owned(), DEAD.to_owned()]);
    let (_, none) = started(&[]);
    assert!(none.asked().is_empty(), "the presets are never contacted");
}

#[test]
fn a_lone_relay_is_never_checked_because_it_could_not_be_disabled() {
    let (_, fake) = started(&[LIVE]);
    assert!(fake.asked().is_empty());
}

#[test]
fn a_relay_that_does_not_answer_is_disabled_and_the_presets_are_left_alone() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, DEAD, false);
    assert!(disabled_calls(&t).is_empty(), "waits for every answer");
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(disabled_calls(&t), vec![vec![DEAD.to_owned()]]);
}

#[test]
fn a_relay_disabled_by_a_check_is_enabled_again_when_it_answers() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    t.relay_time(181);
    assert_eq!(fake.asked().len(), 4, "the disabled one is tested again");
    t.relay_answers(&fake, LIVE, true);
    t.relay_answers(&fake, DEAD, true);
    assert_eq!(disabled_calls(&t).last(), Some(&Vec::new()));
}

#[test]
fn a_relay_still_silent_stays_disabled_without_telling_the_agent_again() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    t.relay_time(181);
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(disabled_calls(&t).len(), 1);
}

#[test]
fn when_no_relay_answers_nothing_is_disabled() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, false);
    assert!(disabled_calls(&t).is_empty());
}

#[test]
fn a_check_never_leaves_every_relay_disabled() {
    let (mut t, fake) = started(&[LIVE, DEAD, THIRD]);
    t.relay_answers(&fake, LIVE, false);
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, THIRD, true);
    let all = disabled_calls(&t);
    assert_eq!(all.len(), 1);
    assert!(all[0].len() < 3, "{all:?}");
}

#[test]
fn a_check_that_never_hears_from_a_relay_counts_it_failed_at_its_deadline() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, LIVE, true);
    t.relay_time(30);
    assert!(disabled_calls(&t).is_empty(), "not yet");
    t.relay_time(61);
    assert_eq!(disabled_calls(&t), vec![vec![DEAD.to_owned()]]);
}

#[test]
fn an_answer_of_an_older_check_is_ignored() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    let old = fake.run();
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    t.relay_time(181);
    assert_eq!(fake.run(), old + 1, "a new check started");
    t.relay_answers_run(old, DEAD, None);
    t.relay_answers_run(old, LIVE, None);
    assert_eq!(
        disabled_calls(&t).len(),
        1,
        "the old answers enabled nothing"
    );
    t.relay_answers(&fake, DEAD, true);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(disabled_calls(&t).last(), Some(&Vec::new()));
}

#[test]
fn the_timer_wakes_the_loop_for_the_deadline_and_for_the_next_check_of_a_disabled_relay() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    let soon = t.e.sleep_for();
    assert!(soon <= std::time::Duration::from_secs(5), "{soon:?}");
    assert!(
        t.e.next_relay_wake().is_some(),
        "the running check's deadline"
    );
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    let wake = t.e.next_relay_wake().unwrap();
    assert_eq!(wake - t.clock.wall_ms(), 180_000);
}

#[test]
fn nothing_wakes_the_loop_while_no_relay_is_disabled_and_no_check_runs() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    t.relay_answers(&fake, DEAD, true);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(t.e.next_relay_wake(), None);
}

#[test]
fn a_relay_that_is_not_the_users_does_not_hide_offline() {
    use crate::engine::agent_port::AgentEvent;
    use zaun_chat_wire::Event;
    let (mut t, _) =
        started(&["smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.1:5223"]);
    let up = |s: &str| AgentEvent::Up { server: s.into() };
    let down = |s: &str| AgentEvent::Down { server: s.into() };
    t.agent(up("127.0.0.1"));
    // A peer's relay that was up and then closed for being idle stays "up" in the books.
    t.agent(up("peer.example"));
    assert_eq!(t.agent(down("127.0.0.1")), vec![Event::Offline(true)]);
    assert_eq!(t.agent(up("127.0.0.1")), vec![Event::Offline(false)]);
}
