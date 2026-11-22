//! G1: the schema's round trip and byte stability. One small real app, one canonical description: the file in
//! `tests/golden/rows.json` changes only when the schema or the app's description changes, and a reader turns
//! the bytes back into a `Snapshot` that writes the same bytes.
use bough_guibench_dataflow::rows::Rows;
use bough_inspect::json;
use bough_inspect::record::{Command, LOG_SCHEMA_VERSION, Log, Replay, record_step, replay};
use bough_inspect::schema::PropValue;
use bough_inspect::session::Session;
use bough_inspect::{SCHEMA_VERSION, Snapshot};
use bough_inspect_app::Lab;

fn mount() -> Lab<Rows> {
    Lab::new(Rows::new(20), 200, 160, 1)
}

fn small() -> Session<Lab<Rows>> {
    Session::new(mount())
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
    let live = s.snapshot().json_string();
    let live_back = Snapshot::from_json(&json::parse(&live).unwrap()).unwrap();
    assert_eq!(live_back.json_string(), live);
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
fn equivalent_mounts_with_different_revisions_have_the_same_contract() {
    let clean = small().snapshot().clone();
    let mut changed = small();
    changed.set("scroll", &PropValue::Int(8 * 32)).unwrap();
    changed.set("scroll", &PropValue::Int(0)).unwrap();
    let changed = changed.snapshot();
    assert_ne!(clean.app.world, changed.app.world);
    assert_ne!(clean.app.revision, changed.app.revision);
    assert_eq!(clean.app.generation, changed.app.generation);
    assert_eq!(clean.stable_json(), changed.stable_json());
    assert_eq!(clean.digest(), changed.digest());

    // A mount on the earlier typed core advances the structure revision one fewer time.
    let mut earlier = changed.clone();
    earlier.app.shape = earlier.app.shape.wrapping_sub(1);
    assert_ne!(earlier.json_string(), changed.json_string());
    assert_eq!(earlier.stable_json(), changed.stable_json());
    assert_eq!(earlier.digest(), changed.digest());
}

#[test]
fn replay_ignores_diagnostics_and_compares_description_pixels_and_refusal() {
    let mut app = small();
    let mut log = Log::new(&mut app);
    log.steps.push(record_step(
        &mut app,
        Command::Set {
            field: "absent".into(),
            value: PropValue::Int(1),
        },
    ));
    assert_eq!(log.schema, LOG_SCHEMA_VERSION);
    log.steps[0].damage ^= 1;
    log.steps[0].revision ^= 1;
    log.steps[0].refused.as_mut().unwrap().1 = "different diagnostic text".into();
    let log = Log::from_json(&log.to_json()).unwrap();
    assert_eq!(replay(&log, mount), Replay::Match { steps: 1 });

    for field in ["description", "pixels", "refusal"] {
        let mut changed = log.clone();
        let step = &mut changed.steps[0];
        match field {
            "description" => step.digest ^= 1,
            "pixels" => step.pixels ^= 1,
            "refusal" => step.refused = None,
            _ => unreachable!(),
        }
        assert!(matches!(replay(&changed, mount), Replay::Mismatch { what, .. } if what == field));
    }
}

#[test]
fn a_version_one_recording_uses_its_original_description_digest() {
    let mut app = small();
    let mut log = Log::new(&mut app);
    let mut step = record_step(&mut app, Command::Settle);
    // Reconstruct the historical projection at today's revision counts.
    let json::Value::Obj(mut members) = app.snapshot().to_json() else {
        panic!("expected snapshot object");
    };
    members.retain(|(key, _)| key != "damage" && key != "counters");
    for (key, value) in &mut members {
        if let json::Value::Obj(fields) = value {
            if key == "app" {
                fields.retain(|(field, _)| field != "world");
            } else if key == "ax" {
                fields.retain(|(field, _)| field != "projected");
            }
        }
    }
    let legacy = json::to_string(&json::Value::Obj(members));
    let digest = legacy.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    });
    assert_ne!(step.digest, digest);
    step.digest = digest;
    log.schema = 1;
    log.steps.push(step);
    let log = Log::from_json(&log.to_json()).unwrap();
    assert_eq!(replay(&log, mount), Replay::Match { steps: 1 });
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
