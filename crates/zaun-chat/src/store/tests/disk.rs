use super::Dir;

fn disk_bytes(d: &Dir) -> Vec<u8> {
    let mut all = Vec::new();
    for e in std::fs::read_dir(&d.0).unwrap() {
        all.extend(std::fs::read(e.unwrap().path()).unwrap());
    }
    all
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn values_on_disk_contain_no_plaintext() {
    let d = Dir::new();
    {
        let mut s = d.store("k");
        s.put("agent", b"KEYMARKER-key", b"VALMARKER-kv").unwrap();
        s.insert_item(b"CHATMARKER-ref", b"ITEMMARKER-text")
            .unwrap();
        s.put_file(1, b"FILEMARKER-rec").unwrap();
        s.next_id("friends").unwrap();
        // Still in the WAL, then checkpointed on close: check both states.
        let live = disk_bytes(&d);
        assert!(!contains(&live, b"MARKER"));
    }
    let closed = disk_bytes(&d);
    assert!(
        contains(&closed, b"agent"),
        "namespace tags are the documented clear part"
    );
    assert!(!contains(&closed, b"MARKER"));
}

#[test]
fn padding_hides_small_size_differences() {
    let d = Dir::new();
    let mut s = d.store("k");
    let len = |s: &mut crate::store::Store, id: i64, v: &[u8]| {
        s.put_file(id, v).unwrap();
        s.db_value_len(id)
    };
    let sizes: Vec<usize> = [0usize, 1, 20, 40, 59]
        .iter()
        .map(|n| len(&mut s, 1, &vec![b'a'; *n]))
        .collect();
    assert!(sizes.windows(2).all(|w| w[0] == w[1]), "{sizes:?}");
    assert!(len(&mut s, 1, &[b'a'; 61]) > sizes[0]);
}

impl crate::store::Store {
    fn db_value_len(&mut self, id: i64) -> usize {
        let (n,): (i64,) = self
            .db
            .run(async |c| {
                Ok(
                    ormlite::query_as("SELECT length(v) FROM files WHERE id = ?1")
                        .bind(id)
                        .fetch_one(c)
                        .await?,
                )
            })
            .unwrap();
        n as usize
    }
}
