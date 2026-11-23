use super::Dir;
use crate::store::{Store, StoreError};

#[test]
fn reopen_with_the_right_key_reads_everything_back() {
    let d = Dir::new();
    let (item, other);
    {
        let mut s = d.store("k1");
        s.put("agent", b"conn-1", b"state").unwrap();
        item = s.insert_item(b"direct:1", b"hello").unwrap();
        other = s.next_id("friends").unwrap();
        s.put_file(7, b"file record").unwrap();
    }
    let mut s = d.store("k1");
    assert_eq!(
        s.get("agent", b"conn-1").unwrap().as_deref(),
        Some(&b"state"[..])
    );
    assert_eq!(
        s.page(b"direct:1", None, 10).unwrap(),
        vec![(item, b"hello".to_vec())]
    );
    assert_eq!(s.next_id("friends").unwrap(), other + 1);
    assert_eq!(s.get_file(7).unwrap().as_deref(), Some(&b"file record"[..]));
    assert_eq!(s.get_file(8).unwrap(), None);
}

#[test]
fn wrong_key_is_its_own_error() {
    let d = Dir::new();
    drop(d.store("right"));
    assert!(matches!(
        Store::open(&d.db(), "wrong"),
        Err(StoreError::WrongKey)
    ));
    drop(d.store("right"));
}

#[test]
fn a_file_that_is_not_a_database_is_corrupt() {
    let d = Dir::new();
    std::fs::write(d.db(), vec![7u8; 4096]).unwrap();
    assert!(matches!(
        Store::open(&d.db(), "k"),
        Err(StoreError::Corrupt(_))
    ));
}

#[test]
fn keys_with_prefix_lists_one_namespace_and_filters_on_the_real_key() {
    let d = Dir::new();
    let mut s = d.store("k");
    for k in [&b"rcv/1"[..], b"rcv/2", b"snd/1"] {
        s.put("queues", k, b"v").unwrap();
    }
    s.put("other", b"rcv/9", b"v").unwrap();
    let mut keys = s.keys_with_prefix("queues", b"rcv/").unwrap();
    keys.sort();
    assert_eq!(keys, vec![b"rcv/1".to_vec(), b"rcv/2".to_vec()]);
    s.delete("queues", b"rcv/1").unwrap();
    assert_eq!(s.keys_with_prefix("queues", b"").unwrap().len(), 2);
}
