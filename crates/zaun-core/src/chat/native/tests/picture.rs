use zaun_chat_wire::model as wm;

use crate::chat::engine::{ChatEvent, Request};
use crate::chat::native::from_wire::event;
use crate::chat::native::to_wire::request;
use crate::model::Picture;
use zaun_chat_wire::{Event, Request as WireRequest};

pub(super) fn png_bytes() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(8, 8)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn profile(p: wm::Picture) -> Event {
    Event::Ready(wm::ChatProfile {
        name: "Ada".into(),
        picture: Some(p),
        ..wm::ChatProfile::default()
    })
}

fn picture_of(e: ChatEvent) -> Option<Picture> {
    let ChatEvent::Ready(p) = e else {
        panic!("not Ready: {e:?}")
    };
    assert_eq!(p.name, "Ada");
    p.picture
}

#[test]
fn a_good_picture_from_the_helper_reaches_the_profile() {
    let bytes = png_bytes();
    let got = picture_of(event(profile(wm::Picture {
        bytes: bytes.clone(),
        mime: "image/png".into(),
    })));
    assert_eq!(got.unwrap().bytes(), bytes);
}

#[test]
fn a_bad_picture_from_the_helper_is_dropped_and_the_profile_stays() {
    let not_a_picture = wm::Picture {
        bytes: b"not a picture".to_vec(),
        mime: "image/png".into(),
    };
    assert!(picture_of(event(profile(not_a_picture))).is_none());
    let wrong_type = wm::Picture {
        bytes: png_bytes(),
        mime: "image/svg+xml".into(),
    };
    assert!(picture_of(event(profile(wrong_type))).is_none());
    let too_big = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(1025, 4)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        wm::Picture {
            bytes: out.into_inner(),
            mime: "image/png".into(),
        }
    };
    assert!(picture_of(event(profile(too_big))).is_none());
}

#[test]
fn a_picture_sent_to_the_helper_carries_its_type() {
    let png = Picture::new(png_bytes());
    let WireRequest::SetPicture(Some(w)) = request(Request::SetPicture(Some(png))) else {
        panic!("not SetPicture");
    };
    assert_eq!(w.mime, "image/png");
    let jpeg = Picture::new(vec![0xff, 0xd8, 0xff, 0]);
    let WireRequest::SetPicture(Some(w)) = request(Request::SetPicture(Some(jpeg))) else {
        panic!("not SetPicture");
    };
    assert_eq!(w.mime, "image/jpeg");
}
