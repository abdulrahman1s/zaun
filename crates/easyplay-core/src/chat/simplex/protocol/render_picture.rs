//! Pictures (ADR-0038): the user's and a group's travel in their profiles as `data:` URIs.
use super::*;
use crate::chat::picture;
use crate::model::Picture;

impl Codec {
    /// `/_profile` with `p`'s name and line about them. The kept profile takes them at once:
    /// a picture made on the job pool meanwhile is rendered after this and must carry them,
    /// not the ones the engine last answered with.
    pub(super) fn render_set_profile(&mut self, p: &ChatProfile) -> (RequestKind, String) {
        let v = self.profile_json(p);
        self.profile.name = p.name.trim().to_owned();
        self.profile.about = p.about.trim().to_owned();
        (
            RequestKind::SetProfile,
            format!("/_profile {} {v}", self.uid()),
        )
    }

    /// `/_profile` with the picture set or removed, the rest of the profile as last sent (the
    /// name and line of a `SetProfile` not answered yet included). The kept profile takes
    /// the picture at once, so a profile change sent before the answer does not put the old
    /// one back.
    pub(super) fn render_set_picture(&mut self, p: Option<&Picture>) -> (RequestKind, String) {
        let mut v = self.profile_json(&self.profile);
        set_image(&mut v, p);
        if self.user_profile.is_object() {
            set_image(&mut self.user_profile, p);
        }
        (
            RequestKind::SetProfile,
            format!("/_profile {} {v}", self.uid()),
        )
    }

    /// `/_group_profile` with the group's profile from the last list and the picture set or
    /// removed. The engine refuses it unless the user owns the group.
    pub(super) fn render_set_group_picture(
        &mut self,
        group: i64,
        p: Option<&Picture>,
        out: &mut Output,
    ) -> Option<(RequestKind, String)> {
        let Some(mut v) = self.group_profiles.get(&group).cloned() else {
            let detail = format!("group {group} is not in the list");
            out.events.push(failed(
                RequestKind::Group,
                ErrorCode::ChtRequestFailed,
                detail,
            ));
            return None;
        };
        set_image(&mut v, p);
        Some((RequestKind::Group, format!("/_group_profile #{group} {v}")))
    }
}

impl Codec {
    /// A friend's new profile (picture or name) reaches their member entries only when the
    /// members are read again (the list does not carry members).
    pub(super) fn reread_members(&mut self, out: &mut Output) {
        let mut groups: Vec<i64> = self.members_read.iter().copied().collect();
        groups.sort_unstable();
        for g in groups {
            self.send(out, Pending::Members(g), format!("/_members #{g}"));
        }
    }
}

fn set_image(v: &mut Value, p: Option<&Picture>) {
    match p {
        Some(p) => v["image"] = picture::data_uri(p).into(),
        None => {
            if let Some(m) = v.as_object_mut() {
                m.remove("image");
            }
        }
    }
}

/// The picture at `path` in an engine profile, if it passes the checks (a bad one is
/// dropped, never the profile).
pub(super) fn picture_at(v: &Value, path: &[&str]) -> Option<Picture> {
    str_at(v, path).and_then(picture::from_data_uri)
}
