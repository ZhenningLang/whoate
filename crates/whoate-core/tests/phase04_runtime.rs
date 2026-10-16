mod common;
use common::Temp;
use std::{collections::VecDeque, fs, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().unwrap())
    }
}
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}

#[test]
fn phase04_runtime_gui_arguments_are_not_executed_scripts() {
    let home = Temp::new();
    let app = home.0.join("Visual Studio Code.app/Contents");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.microsoft.VSCode</string><key>CFBundleName</key><string>Visual Studio Code</string></dict></plist>"#).unwrap();
    for entry in ["vite", "tsserver.js"] {
        let mut p = Proc::identity(940001, 1, 501, Some(1), "Code".into());
        let exe = app.join("MacOS/Electron").to_string_lossy().into_owned();
        p.exe_path = Some(exe.clone());
        p.argv = vec![exe, entry.into()];
        p.footprint_bytes = Some(2_000_000_000);
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let r = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time,
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        )
        .sample_once()
        .unwrap();
        assert_eq!(r.owners[0].id, "app:com.microsoft.VSCode");
        assert_eq!(r.owners[0].mem_bytes, 2_000_000_000);
        assert!(
            !r.owners[0]
                .insights
                .iter()
                .any(|id| id.starts_with("vite.") || id.starts_with("typescript.")),
            "GUI argument misidentified: {entry}"
        );
        assert!(
            !r.owners[0]
                .parts
                .iter()
                .any(|p| p.name == "TypeScript language server"),
            "GUI file is not a language server"
        );
    }
}

#[test]
fn phase04_runtime_only_plugin_or_native_node_executes_the_entry() {
    let home = Temp::new();
    let contents = home.0.join("Visual Studio Code.app/Contents");
    fs::create_dir_all(&contents).unwrap();
    fs::write(contents.join("Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.microsoft.VSCode</string><key>CFBundleName</key><string>Visual Studio Code</string></dict></plist>"#).unwrap();
    for (role, entry, expected) in [
        ("Code Helper (Plugin)", "/public/vite.js", true),
        ("Code Helper (Renderer)", "/public/vite.js", false),
        ("Code Helper (GPU)", "/public/vite.js", false),
        ("Code Helper", "/public/vite.js", false),
        ("node", "/public/vite.js", true),
        ("node", "/public/ordinary.js", false),
    ] {
        let main_exe = contents
            .join("MacOS/Electron")
            .to_string_lossy()
            .into_owned();
        let mut main = Proc::identity(940101, 1, 501, Some(1), "Code".into());
        main.exe_path = Some(main_exe.clone());
        main.argv = vec![main_exe];
        main.footprint_bytes = Some(10);
        let mut child = Proc::identity(940102, 940101, 501, Some(2), role.into());
        let exe = if role == "node" {
            "/public/node".to_owned()
        } else {
            contents
                .join(format!("Frameworks/{role}.app/Contents/MacOS/{role}"))
                .to_string_lossy()
                .into_owned()
        };
        child.exe_path = Some(exe.clone());
        child.argv = vec![exe, "--max-old-space-size=3072".into(), entry.into()];
        child.footprint_bytes = Some(2_000_000_000);
        let snapshot = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![main, child]);
        let r = Engine::with_sampler(
            Replay(VecDeque::from([snapshot.clone(), snapshot])),
            Time,
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        )
        .sample_once()
        .unwrap();
        let owner = r.owners.iter().find(|o| o.pids.contains(&940101)).unwrap();
        assert_eq!(owner.id, "app:com.microsoft.VSCode");
        assert_eq!(owner.mem_bytes, 2_000_000_010);
        assert_eq!(owner.pids, vec![940101, 940102]);
        assert_eq!(
            owner.insights.iter().any(|id| id.starts_with("vite.")),
            expected,
            "role={role}, entry={entry}"
        );
    }
}
