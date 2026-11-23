//! SetPicture: the user's picture changed, the profile sent to every friend (Commands.hs
//! `APIUpdateProfile`, `updateProfile_`).

use zaun_chat_wire::model::Picture;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::checked_picture;
use crate::engine::records::UserRecord;

impl<A: AgentPort> Engine<A> {
    /// The rest of the profile stays as it was last set.
    pub(crate) fn set_picture(&mut self, picture: Option<Picture>) -> Result<()> {
        if let Some(p) = &picture {
            checked_picture(p).map_err(|why| fail(Code::RequestFailed, why))?;
        }
        let user = UserRecord {
            picture,
            ..self.state.user.clone()
        };
        self.update_profile(user)
    }
}
