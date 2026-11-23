use super::Dir;
use crate::store::{Op, StoreError};

#[test]
fn apply_rolls_back_every_op_when_one_fails() {
    let d = Dir::new();
    let mut s = d.store("k");
    s.put("ns", b"old", b"1").unwrap();
    let ops = vec![
        Op::Put {
            ns: "ns",
            key: b"new".to_vec(),
            value: b"2".to_vec(),
        },
        Op::Delete {
            ns: "ns",
            key: b"old".to_vec(),
        },
        Op::InsertItem {
            chat_ref: b"c".to_vec(),
            value: b"x".to_vec(),
        },
        Op::PutFile {
            id: 1,
            value: b"f".to_vec(),
        },
        Op::Put {
            ns: "bad\0ns",
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        },
    ];
    assert!(matches!(s.apply(ops), Err(StoreError::Invalid(_))));
    assert_eq!(s.get("ns", b"new").unwrap(), None);
    assert_eq!(s.get("ns", b"old").unwrap().as_deref(), Some(&b"1"[..]));
    assert!(s.page(b"c", None, 10).unwrap().is_empty());
    assert_eq!(s.get_file(1).unwrap(), None);
}

#[test]
fn apply_commits_all_ops_and_returns_inserted_item_ids() {
    let d = Dir::new();
    let mut s = d.store("k");
    let ids = s
        .apply(vec![
            Op::Put {
                ns: "ns",
                key: b"a".to_vec(),
                value: b"1".to_vec(),
            },
            Op::InsertItem {
                chat_ref: b"c".to_vec(),
                value: b"one".to_vec(),
            },
            Op::InsertItem {
                chat_ref: b"c".to_vec(),
                value: b"two".to_vec(),
            },
        ])
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(s.get("ns", b"a").unwrap().as_deref(), Some(&b"1"[..]));
    assert_eq!(s.page(b"c", None, 10).unwrap().len(), 2);
}

#[test]
fn updating_a_missing_item_fails() {
    let d = Dir::new();
    let mut s = d.store("k");
    assert!(matches!(
        s.update_item(b"c", 99, b"x"),
        Err(StoreError::Invalid(_))
    ));
}
