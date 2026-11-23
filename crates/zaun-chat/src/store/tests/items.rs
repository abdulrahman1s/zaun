use super::Dir;

#[test]
fn page_returns_the_newest_items_oldest_first_and_honours_the_before_cursor() {
    let d = Dir::new();
    let mut s = d.store("k");
    let ids: Vec<i64> = (0..10)
        .map(|i| s.insert_item(b"c", format!("m{i}").as_bytes()).unwrap())
        .collect();
    s.insert_item(b"other", b"x").unwrap();

    let last = s.page(b"c", None, 3).unwrap();
    assert_eq!(last.iter().map(|r| r.0).collect::<Vec<_>>(), ids[7..]);
    assert_eq!(last[0].1, b"m7");

    let older = s.page(b"c", Some(ids[7]), 3).unwrap();
    assert_eq!(older.iter().map(|r| r.0).collect::<Vec<_>>(), ids[4..7]);

    assert_eq!(s.page(b"c", Some(ids[0]), 5).unwrap(), vec![]);
}

#[test]
fn update_and_delete_item_change_only_that_row() {
    let d = Dir::new();
    let mut s = d.store("k");
    let a = s.insert_item(b"c", b"a").unwrap();
    let b = s.insert_item(b"c", b"b").unwrap();
    s.update_item(b"c", a, b"a2").unwrap();
    s.delete_item(b"c", b).unwrap();
    assert_eq!(s.page(b"c", None, 10).unwrap(), vec![(a, b"a2".to_vec())]);
}

#[test]
fn ids_are_never_reused_after_delete() {
    let d = Dir::new();
    let mut s = d.store("k");
    let (a, b) = (s.next_id("groups").unwrap(), s.next_id("groups").unwrap());
    assert_eq!((a, b), (1, 2));
    assert_eq!(s.next_id("friends").unwrap(), 1);

    let i1 = s.insert_item(b"c", b"x").unwrap();
    s.delete_item(b"c", i1).unwrap();
    assert!(s.insert_item(b"c", b"y").unwrap() > i1);
}
