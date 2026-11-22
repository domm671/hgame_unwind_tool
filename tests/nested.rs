use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn zip(dir: &Path, archive: &Path, entries: &[&str]) {
    let output = Command::new("7z")
        .current_dir(dir)
        .args(["a", "-tzip"])
        .arg(archive)
        .args(entries)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn test_root() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("unzip-tool-test-{}-{unique}", std::process::id()))
}

#[test]
fn extracts_all_branches_and_keeps_the_original_on_failure() {
    let root = test_root();
    let payload = root.join("payload");
    let layers = root.join("layers");
    let branch = layers.join("branch");
    let destination = root.join("destination");
    fs::create_dir_all(&payload).unwrap();
    fs::create_dir_all(&branch).unwrap();
    fs::create_dir_all(&destination).unwrap();

    fs::write(payload.join("first.txt"), "first").unwrap();
    fs::write(payload.join("second.txt"), "second").unwrap();
    zip(&payload, &branch.join("bundle.zip"), &["first.txt"]);
    zip(&payload, &layers.join("renamed.png"), &["second.txt"]);
    fs::write(layers.join("cover.jpg"), b"\xff\xd8\xff\xd9").unwrap();
    fs::write(layers.join("notes.txt"), "archive notes").unwrap();
    fs::write(destination.join("notes.txt"), "existing notes").unwrap();
    fs::write(destination.join("unrelated.zip"), "unrelated").unwrap();

    let archive = destination.join("bundle.zip");
    let entries = ["branch", "renamed.png", "cover.jpg", "notes.txt"];
    zip(&layers, &archive, &entries);
    let result = Command::new(env!("CARGO_BIN_EXE_unzip_tool"))
        .current_dir(&destination)
        .arg("bundle.zip")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");

    let extracted = destination.join("bundle.zip.extracted");
    assert_eq!(
        fs::read(extracted.join("branch/first.txt")).unwrap(),
        b"first"
    );
    assert_eq!(fs::read(extracted.join("second.txt")).unwrap(), b"second");
    assert_eq!(
        fs::read(extracted.join("cover.jpg")).unwrap(),
        b"\xff\xd8\xff\xd9"
    );
    assert_eq!(
        fs::read(extracted.join("notes.txt")).unwrap(),
        b"archive notes"
    );
    assert_eq!(
        fs::read(destination.join("notes.txt")).unwrap(),
        b"existing notes"
    );
    assert_eq!(
        fs::read(destination.join("unrelated.zip")).unwrap(),
        b"unrelated"
    );
    assert!(!extracted.join("branch/bundle.zip").exists());
    assert!(!extracted.join("renamed.png").exists());
    assert!(!archive.exists());
    assert!(!destination.join(".unzip-tool-work").exists());

    fs::write(branch.join("broken.7z"), "broken").unwrap();
    zip(&layers, &archive, &entries);
    let original = fs::read(&archive).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_unzip_tool"))
        .arg(&archive)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success(), "{result:?}");
    assert_eq!(fs::read(archive).unwrap(), original);
    assert!(!destination.join("bundle.zip.extracted.1").exists());
    assert!(!destination.join(".unzip-tool-work").exists());

    fs::remove_dir_all(root).unwrap();
}

// A stored RAR5 archive with one 32-byte file split across four volumes.
const RAR_VOLUMES: [&[u8]; 4] = [
    b"Rar!\x1a\x07\x01\x00S*4E\x03\x01\x00\x01\xfc\x9cA\x8c\x18\x02\x12\x08\x04  \x1c\xb6\xdch\x00\x00\x0bpayload.txtABCDEFGH\x8f\x82=B\x03\x05\x00\x01",
    b"Rar!\x1a\x07\x01\x00\xedU4\xd7\x04\x01\x00\x03\x01\x02\xc1\xe7<\x18\x02\x1a\x08\x04  kA\xe7D\x00\x00\x0bpayload.txtIJKLMNOP\x8f\x82=B\x03\x05\x00\x01",
    b"Rar!\x1a\x07\x01\x00W\x04=N\x04\x01\x00\x03\x02?\x923\x11\x18\x02\x1a\x08\x04  :x\xc5\x0b\x00\x00\x0bpayload.txtQRSTUVWX\x8f\x82=B\x03\x05\x00\x01",
    b"Rar!\x1a\x07\x01\x00\xc14:9\x04\x01\x00\x03\x03`\x115\xf4\x18\x02\x0a\x08\x04  \xa3\xf85\xf5\x00\x00\x0bpayload.txtYZ012345\x19\xb2:5\x03\x05\x00\x00",
];

#[test]
fn removes_only_the_rar_volumes_used_by_a_successful_extraction() {
    let root = test_root();
    fs::create_dir(&root).unwrap();
    let names = [
        "C484111.par1.rar",
        "C484111.part2.rar",
        "C484111.par3.rar",
        "C484111.par4.rar",
    ];
    let unused = root.join("C484111.part5.rar");
    fs::write(&unused, b"unused").unwrap();
    let extract = || {
        Command::new(env!("CARGO_BIN_EXE_unzip_tool"))
            .arg(root.join(names[0]))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    for index in [0, 1, 3] {
        fs::write(root.join(names[index]), RAR_VOLUMES[index]).unwrap();
    }
    let result = extract();
    assert!(!result.status.success(), "{result:?}");
    for index in [0, 1, 3] {
        assert_eq!(
            fs::read(root.join(names[index])).unwrap(),
            RAR_VOLUMES[index]
        );
    }
    assert!(!root.join("payload.txt").exists());

    fs::write(root.join(names[2]), RAR_VOLUMES[2]).unwrap();
    let result = extract();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        fs::read(root.join("payload.txt")).unwrap(),
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345"
    );
    assert!(names.iter().all(|name| !root.join(name).exists()));
    assert_eq!(fs::read(unused).unwrap(), b"unused");
    assert!(!root.join(".unzip-tool-work").exists());

    fs::remove_dir_all(root).unwrap();
}
