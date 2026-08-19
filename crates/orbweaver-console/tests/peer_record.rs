//! The per-peer CSIv2 capability record through the binary — the negative
//! control the live measurement needs.
//!
//! Both project fixtures advertise nothing (measured, `run_checks.sh`'s
//! identity group), so a page that said "bridge only" for every peer would
//! pass against them while measuring nothing. This test fabricates the IOR
//! neither fixture produces — one whose `TAG_CSI_SEC_MECH_LIST` accepts an
//! asserted identity, in both byte orders — and checks the *other* record
//! comes out of the same command line the harness runs on the live IORs. It
//! prints the command and the record lines under `--nocapture`, which is
//! where the commit that landed it took them from.
//!
//! 두 픽스처 모두 아무것도 광고하지 않으므로, 모든 피어에 "bridge only"라고 쓰는
//! 페이지는 아무것도 재지 않고도 통과한다. 그래서 반대 레코드를 내는 IOR을
//! 여기서 조작해 같은 명령줄로 통과시킨다 — 음성 대조군.

use std::path::PathBuf;
use std::process::Command;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::csiv2::{TAG_CSI_SEC_MECH_LIST, TAG_NULL_TAG, options};
use orbweaver_giop::{IiopProfile, Ior, TaggedComponent, Version};

const ECHO_IDL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spikes/echo.idl");
const ECHO: &str = "IDL:spike/Echo:1.0";

fn mechanism_list(endian: Endian) -> TaggedComponent {
    let mut e = Encoder::encapsulation(endian);
    e.put_bool(false);
    e.put_u32(1);
    e.put_u16(0);
    e.put_u32(TAG_NULL_TAG);
    e.put_octet_seq(&[]);
    e.put_u16(0);
    e.put_u16(0);
    e.put_octet_seq(&[]);
    e.put_octet_seq(&[]);
    e.put_u16(options::IDENTITY_ASSERTION);
    e.put_u16(0);
    e.put_u32(0);
    e.put_u32(0);
    e.put_u32(2);
    TaggedComponent { tag: TAG_CSI_SEC_MECH_LIST, data: e.finish().expect("encodes") }
}

fn ior_file(name: &str, components: Vec<TaggedComponent>) -> PathBuf {
    let ior = Ior {
        type_id: ECHO.to_owned(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port: 1,
            object_key: b"fabricated".to_vec(),
            components,
        }],
    };
    let path =
        std::env::temp_dir().join(format!("orbweaver-peer-record-{}-{name}", std::process::id()));
    std::fs::write(&path, ior.to_stringified().expect("stringifies")).expect("writes");
    path
}

fn catalog_text(ior: &PathBuf) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_orbweaver-console"));
    cmd.arg("catalog").arg(ECHO_IDL).arg("--ior").arg(ior).arg("--text");
    println!(
        "$ orbweaver-console catalog spikes/echo.idl --ior {} --text",
        ior.file_name().unwrap().to_string_lossy()
    );
    let out = cmd.output().expect("the console runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).expect("utf-8");
    for line in text.lines().filter(|l| l.contains("peer") || l.contains("identity:")) {
        println!("{line}");
    }
    text
}

/// The negative control: an IOR that advertises identity assertion produces
/// the *target* record, in both byte orders, through the binary.
#[test]
fn a_fabricated_identity_asserting_ior_reads_as_enforced_by_the_target() {
    for (name, endian) in [("be.ior", Endian::Big), ("le.ior", Endian::Little)] {
        let path = ior_file(name, vec![mechanism_list(endian)]);
        let text = catalog_text(&path);
        assert!(text.contains(" enforced-by=target "), "{name}: {text}");
        assert!(text.contains("identity: enforced by the target"), "{name}: {text}");
        assert!(
            text.contains("0 where the bridge is the only enforcement point"),
            "{name}: {text}"
        );
        assert!(!text.contains("fabricated"), "the object key is not on the page");
        let _ = std::fs::remove_file(path);
    }
}

/// The baseline the fixtures produce, fabricated here so the test needs no
/// fixture: no components, "bridge only". The live half — the two real IORs
/// through this same command — is the harness's identity group.
#[test]
fn an_ior_advertising_nothing_reads_as_bridge_only() {
    let path = ior_file("bare.ior", Vec::new());
    let text = catalog_text(&path);
    assert!(text.contains(" enforced-by=bridge only "), "{text}");
    assert!(
        text.contains("identity: not enforced by the target — the bridge is the only enforcement point (no CSIv2 mechanism list in the IOR)"),
        "{text}"
    );
    assert!(text.contains("1 where the bridge is the only enforcement point"), "{text}");
    let _ = std::fs::remove_file(path);
}
