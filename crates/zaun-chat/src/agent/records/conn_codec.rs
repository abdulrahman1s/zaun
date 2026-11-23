//! The stored form of a connection record: a format byte, then every field; fields that can pass 255 bytes are `Large`.
use crate::crypto::PrivateKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

use super::conn::{Chain, ConnRecord, NewLink};

/// A new record's first byte; a new layout gets a new one. Format 1 had no `request`, format 2 no
/// `client_data`, format 3 no `switch` and `sync`.
const FORMAT: u8 = 4;

fn big(b: &[u8]) -> Large {
    Large(b.to_vec())
}

impl Encode for NewLink {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (
            &self.corr_id,
            big(&self.fixed),
            big(&self.user),
            &self.link_key,
        )
            .encode(out)
    }
}

impl Decode for NewLink {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (corr_id, Large(fixed), Large(user), link_key) =
            <(Vec<u8>, Large, Large, Vec<u8>)>::decode(d)?;
        Ok(Self {
            corr_id,
            fixed,
            user,
            link_key,
        })
    }
}

impl Encode for Chain {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.snd_id, &self.snd_hash, self.rcv_id, &self.rcv_hash).encode(out)
    }
}

impl Decode for Chain {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (snd_id, snd_hash, rcv_id, rcv_hash) = <_>::decode(d)?;
        Ok(Self {
            snd_id,
            snd_hash,
            rcv_id,
            rcv_hash,
        })
    }
}

impl Encode for ConnRecord {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (FORMAT, self.id, self.status, self.agent_version, self.pq).encode(out)?;
        (&self.rcv, &self.snd, &self.x3dh, &self.chain, self.last_id).encode(out)?;
        let link = self.link.as_ref().map(|l| big(l.as_bytes()));
        let (info, e2e) = (
            self.conn_info.as_deref().map(big),
            self.e2e_reply.as_deref().map(big),
        );
        (info, link, &self.lkey, e2e).encode(out)?;
        (&self.peer_reply, &self.new_link, &self.request).encode(out)?;
        self.client_data
            .as_ref()
            .map(|d| big(d.as_bytes()))
            .encode(out)?;
        (&self.switch, &self.sync).encode(out)
    }
}

impl Decode for ConnRecord {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let format = u8::decode(d)?;
        if !(1..=FORMAT).contains(&format) {
            return Err(DecodeError::BadTag("connection record", format));
        }
        let (id, status, agent_version, pq) = <_>::decode(d)?;
        let (rcv, snd, x3dh, chain, last_id) = <_>::decode(d)?;
        let (conn_info, link, lkey, e2e_reply) = <(
            Option<Large>,
            Option<Large>,
            Option<PrivateKey>,
            Option<Large>,
        )>::decode(d)?;
        let link = link
            .map(|l| String::from_utf8(l.0).map_err(|_| DecodeError::Utf8))
            .transpose()?;
        let (conn_info, e2e_reply) = (conn_info.map(|l| l.0), e2e_reply.map(|l| l.0));
        let (peer_reply, new_link) = <_>::decode(d)?;
        let request = if format >= 2 {
            Option::decode(d)?
        } else {
            None
        };
        let client_data = if format >= 3 {
            Option::<Large>::decode(d)?
                .map(|l| String::from_utf8(l.0).map_err(|_| DecodeError::Utf8))
                .transpose()?
        } else {
            None
        };
        let (switch, sync) = if format >= 4 {
            <_>::decode(d)?
        } else {
            (None, None)
        };
        Ok(Self {
            id,
            status,
            agent_version,
            pq,
            rcv,
            snd,
            x3dh,
            chain,
            last_id,
            conn_info,
            link,
            lkey,
            e2e_reply,
            peer_reply,
            new_link,
            request,
            client_data,
            switch,
            sync,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::SmpQueueInfo;
    use crate::agent::client::{AgentMsgId, ConnId};
    use crate::agent::integrity::{MsgErrorType, MsgIntegrity};
    use crate::agent::records::{ConnStatus, ContactRequest, PendingSend, SendKind, Unacked};
    use crate::crypto::{Alg, KeyPair};
    use crate::encoding::Version;

    #[test]
    fn a_connection_record_with_every_field_set_reads_back_the_same() {
        let mut rec = ConnRecord::new(ConnId(7), ConnStatus::Joined, Version(8), true);
        rec.chain = Chain {
            snd_id: 3,
            snd_hash: vec![1; 32],
            rcv_id: 2,
            rcv_hash: vec![2; 32],
        };
        rec.last_id = 9;
        rec.conn_info = Some(vec![5; 1000]);
        rec.link = Some("https://example.test/i#abc/def".into());
        rec.lkey = Some(KeyPair::generate(Alg::X25519).unwrap().private);
        rec.e2e_reply = Some(vec![6; 2400]);
        rec.new_link = Some(NewLink {
            corr_id: vec![1; 24],
            fixed: vec![2; 2032],
            user: vec![3; 13808],
            link_key: vec![4; 32],
        });
        rec.client_data = Some(r#"{"groupLinkId":"abc"}"#.into());
        rec.request = Some(ContactRequest {
            queue: SmpQueueInfo::parse(
                "smp://AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=@fake.test/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA#/?v=1-4&dh=MCowBQYDK2VuAyEAQWIOFJ7ZTMqTFfTsHuyRYNDwszkpHQfRxObZXAWAIRU%3D&q=c",
            )
            .unwrap(),
            agent_version: Version(8),
            ratchet_key_id: Some(vec![8; 16]),
            body: Some(vec![9; 15904]),
        });
        assert_eq!(
            ConnRecord::from_bytes(&rec.to_bytes().unwrap()).unwrap(),
            rec
        );
    }

    #[test]
    fn a_record_stored_before_requests_existed_still_reads() {
        let rec = ConnRecord::new(ConnId(3), ConnStatus::Active, Version(8), false);
        let mut bytes = rec.to_bytes().unwrap();
        bytes[0] = 1; // format 1 had none of `request`, `client_data`, `switch`, `sync`
        bytes.truncate(bytes.len() - 4); // the absent `Option`s are one '0' byte each
        assert_eq!(ConnRecord::from_bytes(&bytes).unwrap(), rec);
    }

    #[test]
    fn a_full_size_pending_send_and_unacked_message_read_back_the_same() {
        let p = PendingSend {
            id: AgentMsgId(4),
            kind: SendKind::Confirmation,
            body: vec![9; 16100],
            created_at: 1_700_000_000_000,
        };
        assert_eq!(PendingSend::from_bytes(&p.to_bytes().unwrap()).unwrap(), p);
        let u = Unacked {
            msg_id: AgentMsgId(5),
            smp_msg_id: vec![1; 24],
            ext_snd_id: 3,
            hash: vec![2; 32],
            body: vec![7; 15000],
            integrity: MsgIntegrity::Error(MsgErrorType::Skipped { from: 2, to: 4 }),
            broker_ts: 1_700_000_000,
            attempts: 2,
            first_at: 1_700_000_000_000,
        };
        assert_eq!(Unacked::from_bytes(&u.to_bytes().unwrap()).unwrap(), u);
    }
}
