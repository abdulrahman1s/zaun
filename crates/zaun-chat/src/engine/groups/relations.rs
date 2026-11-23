//! Who is connected to whom, as the host keeps it while introducing (Types/MemberRelations.hs `MemberRelation`, Store/Groups.hs `setMemberVectorNewRelations`): forwarding goes to members introduced to an author but not connected to it yet.

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::MemberRelation;

impl<A: AgentPort> Engine<A> {
    /// The members of `group` that `member` was introduced to (or to whom it was) and that
    /// are not connected to it yet: those an admin forwards its messages to.
    pub(crate) fn not_yet_connected_to(&self, group: i64, member: i64) -> Vec<i64> {
        let Some(m) = self.state.members.get(&(group, member)) else {
            return Vec::new();
        };
        m.relations
            .iter()
            .filter(|&(_, r)| *r == MemberRelation::Introduced)
            .map(|(&other, _)| other)
            .filter(|o| {
                self.state
                    .members
                    .get(&(group, *o))
                    .is_some_and(|o| !o.status.gone())
            })
            .collect()
    }

    /// Both members' vectors, as 7.1.4 writes the pair (`IDSubjectIntroduced`/`IDReferencedIntroduced`).
    pub(crate) fn relate(
        &mut self,
        group: i64,
        a: i64,
        b: i64,
        relation: MemberRelation,
    ) -> Result<()> {
        for (x, y) in [(a, b), (b, a)] {
            let mut m = self.member(group, x)?;
            m.relations.insert(y, relation);
            self.save_member(m)?;
        }
        Ok(())
    }
}
