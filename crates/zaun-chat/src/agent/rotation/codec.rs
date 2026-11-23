//! The stored form of a switch, inside its connection record (format 4).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

use super::state::{Moving, Step, Switch};
use crate::agent::client::AgentMsgId;
use crate::agent::records::SndQueue;

impl Encode for Step {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let tag = match self {
            Self::Securing => b'S',
            Self::Confirming => b'C',
            Self::Qkey => b'K',
            Self::WaitingQuse => b'W',
            Self::Qtest => b'Q',
            Self::Testing { body } => return (b'T', Large(body.clone())).encode(out),
            Self::Ready => b'R',
        };
        tag.encode(out)
    }
}

impl Decode for Step {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match u8::decode(d)? {
            b'S' => Self::Securing,
            b'C' => Self::Confirming,
            b'K' => Self::Qkey,
            b'W' => Self::WaitingQuse,
            b'Q' => Self::Qtest,
            b'T' => Self::Testing {
                body: Large::decode(d)?.0,
            },
            b'R' => Self::Ready,
            t => return Err(DecodeError::BadTag("switch step", t)),
        })
    }
}

impl Encode for Switch {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Moving(m) => {
                (b'M', &m.new, m.mode, &m.step).encode(out)?;
                (m.keep_after, m.kept, m.started_at).encode(out)
            }
            Self::EndingOld {
                old,
                body,
                last,
                started_at,
            } => (b'E', old, Large(body.clone()), *last, *started_at).encode(out),
        }
    }
}

impl Decode for Switch {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match u8::decode(d)? {
            b'M' => {
                let (new, mode, step) = <_>::decode(d)?;
                let (keep_after, kept, started_at) = <_>::decode(d)?;
                Ok(Self::Moving(Moving {
                    new,
                    mode,
                    step,
                    keep_after,
                    kept,
                    started_at,
                }))
            }
            b'E' => {
                let (old, Large(body), last, started_at) =
                    <(SndQueue, Large, AgentMsgId, i64)>::decode(d)?;
                Ok(Self::EndingOld {
                    old,
                    body,
                    last,
                    started_at,
                })
            }
            t => Err(DecodeError::BadTag("switch", t)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::QueueMode;
    use crate::crypto::{Alg, KeyPair};
    use crate::encoding::Version;

    fn queue() -> SndQueue {
        let key = || KeyPair::generate(Alg::X25519).unwrap();
        SndQueue {
            server: "smp://AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=@fake.test"
                .parse()
                .unwrap(),
            snd_id: vec![7; 24],
            auth: key().private,
            e2e_priv: key().private,
            peer_dh: key().public,
            client_version: Version(4),
            secured: true,
        }
    }

    #[test]
    fn a_stored_switch_reads_back_the_same_at_every_step() {
        let steps = [
            Step::Securing,
            Step::Confirming,
            Step::Qkey,
            Step::WaitingQuse,
            Step::Qtest,
            Step::Testing {
                body: vec![5; 16000],
            },
            Step::Ready,
        ];
        for step in steps {
            let sw = Switch::Moving(Moving {
                new: queue(),
                mode: Some(QueueMode::Messaging),
                step,
                keep_after: Some(AgentMsgId(3)),
                kept: Some(AgentMsgId(9)),
                started_at: 1_700_000_000_000,
            });
            assert_eq!(Switch::from_bytes(&sw.to_bytes().unwrap()).unwrap(), sw);
        }
        let end = Switch::EndingOld {
            old: queue(),
            body: vec![3; 16000],
            last: AgentMsgId(12),
            started_at: 1_700_000_000_000,
        };
        assert_eq!(Switch::from_bytes(&end.to_bytes().unwrap()).unwrap(), end);
    }
}
