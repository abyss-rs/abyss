use std::path::PathBuf;

use super::{Location, LocationCodec, StoragePath};

#[test]
fn local_paths_remain_local() {
    assert_eq!(
        LocationCodec::parse("../media").unwrap(),
        Location::Local(PathBuf::from("../media"))
    );
}

#[cfg(feature = "s3")]
#[test]
fn remote_uri_round_trips() {
    let location = LocationCodec::parse("s3://backup/videos/Season%2001").unwrap();
    let Location::Remote(remote) = &location else {
        panic!("expected remote")
    };
    assert_eq!(remote.connection, "backup");
    assert_eq!(
        remote.path,
        StoragePath::Remote("videos/Season 01".to_owned())
    );
    assert_eq!(
        LocationCodec::format(&location),
        "s3://backup/videos/Season%2001"
    );
}

#[cfg(feature = "s3")]
#[test]
fn traversal_is_rejected() {
    assert!(LocationCodec::parse("s3://backup/a/../b").is_err());
}

#[cfg(feature = "kubernetes")]
#[test]
fn kubernetes_raw_name_round_trips() {
    let location = Location::Remote(super::RemoteLocation {
        scheme: "kube".to_owned(),
        connection: "cluster".to_owned(),
        path: StoragePath::Kubernetes(vec![
            b"namespace".to_vec(),
            b"claim".to_vec(),
            vec![b'n', 0xff],
        ]),
    });
    let encoded = LocationCodec::format(&location);
    assert_eq!(encoded, "kube://cluster/namespace/claim/n%FF");
    assert_eq!(LocationCodec::parse(&encoded).unwrap(), location);
}

#[cfg(windows)]
#[test]
fn windows_rejects_unrepresentable_remote_names_without_mangling() {
    let root = Location::Local(PathBuf::from(r"C:\transfer"));
    assert!(root.child_transfer(&[b'n', 0xff]).is_err());
    assert!(root.child_transfer(b"CON.txt").is_err());
    assert!(root.child_transfer(b"trailing.").is_err());
    assert!(root.child_transfer(b"valid name.txt").is_ok());
}
