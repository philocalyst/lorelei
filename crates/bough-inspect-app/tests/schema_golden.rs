//! G1: the schema's round trip and byte stability. One small real app, one canonical description: the file in
//! `tests/golden/rows.json` changes only when the schema or the app's description changes, and a reader turns
//! the bytes back into a `Snapshot` that writes the same bytes.
use bough_guibench_x::rows::Rows;
use bough_inspect::json;
use bough_inspect::session::Session;
use bough_inspect::{SCHEMA_VERSION, Snapshot};
use bough_inspect_app::Lab;

fn small() -> Session<Lab<Rows>> {
    Session::new(Lab::new(Rows::new(20), 200, 160, 1))
}

#[test]
fn the_version_is_pinned() {
    assert_eq!(SCHEMA_VERSION, 1);
    let mut s = small();
    assert_eq!(s.snapshot().schema, 1);
}

#[test]
fn a_description_reads_back_and_writes_the_same_bytes() {
    let mut s = small();
    let text = s.snapshot().stable_json();
    let parsed = json::parse(&text).expect("canonical JSON parses");
    let back = Snapshot::from_json(&parsed).expect("a snapshot reads back");
    assert_eq!(
        back.stable_json(),
        text,
        "write -> read -> write is the identity"
    );
    assert_eq!(back.digest(), s.snapshot().digest());
    assert_eq!(back.nodes.len(), s.snapshot().nodes.len());
    // another schema is refused, not misread
    let mut other = parsed.clone();
    other.put("schema", 2);
    assert!(Snapshot::from_json(&other).is_err());
}

#[test]
fn the_golden_is_byte_stable() {
    let mut s = small();
    let got = s.snapshot().stable_json();
    assert!(
        !got.contains("/Users/") && !got.contains("/home/"),
        "no absolute paths in a committed description"
    );
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/rows.json");
    if std::env::var("BLESS").is_ok() {
        std::fs::write(path, &got).unwrap();
    }
    let want = std::fs::read_to_string(path).expect("the golden is committed");
    assert_eq!(
        got, want,
        "the schema description changed: re-bless it deliberately"
    );
}

#[test]
fn a_round_one_snapshot_recovers_each_added_id() {
    let mut app = small();
    let expected = app.snapshot().stable_json();
    let mut old = json::parse(&expected).unwrap();
    fn remove(value: &mut json::Value, key: &str) {
        let json::Value::Obj(entries) = value else {
            panic!("expected object")
        };
        entries.retain(|(name, _)| name != key);
    }
    let json::Value::Obj(top) = &mut old else {
        panic!("expected snapshot")
    };
    for (key, value) in top {
        if key == "app" {
            remove(value, "generation");
        }
        if key == "nodes" {
            let json::Value::Arr(nodes) = value else {
                panic!("expected nodes")
            };
            for node in nodes {
                remove(node, "id");
                remove(node, "kind_id");
                let json::Value::Obj(fields) = node else {
                    panic!("expected node")
                };
                for (key, value) in fields {
                    if key == "ops" {
                        let json::Value::Arr(ops) = value else {
                            panic!("expected operands")
                        };
                        for op in ops {
                            remove(op, "id");
                        }
                    }
                }
            }
        }
    }
    assert_eq!(Snapshot::from_json(&old).unwrap().stable_json(), expected);
}
