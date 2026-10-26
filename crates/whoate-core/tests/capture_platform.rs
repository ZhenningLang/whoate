use whoate_core::sample::sysctl::parse_procargs;

mod common;

struct WindowReader {
    identities: std::collections::VecDeque<Option<whoate_core::snapshot::Proc>>,
    denied: bool,
    reads: usize,
}
impl whoate_core::sample::libproc::ProcessReader for WindowReader {
    fn identity(&mut self, _: i32) -> Option<whoate_core::snapshot::Proc> {
        self.identities.pop_front().unwrap()
    }
    fn footprint(&mut self, _: i32) -> whoate_core::sample::SampleResult<(u64, u64)> {
        self.reads += 1;
        if self.denied {
            Err(whoate_core::sample::SampleError::os(
                "rusage_v4",
                libc::EPERM,
            ))
        } else {
            Ok((4096, 8192))
        }
    }
    fn exe_path(&mut self, _: i32) -> whoate_core::sample::SampleResult<String> {
        Ok("/usr/bin/node".into())
    }
    fn cwd(&mut self, _: i32) -> whoate_core::sample::SampleResult<String> {
        Ok("/project".into())
    }
    fn args(
        &mut self,
        _: i32,
    ) -> whoate_core::sample::SampleResult<whoate_core::sample::sysctl::ProcArgs> {
        Ok(whoate_core::sample::sysctl::ProcArgs {
            exe_path: "/usr/bin/node".into(),
            argv: vec!["node".into()],
            tmux: None,
            tmux_pane: None,
            codex_thread_id: None,
            launched_app: None,
        })
    }
    fn responsible_pid(&mut self, _: i32) -> Option<i32> {
        Some(123)
    }
}

#[test]
fn permission_denial_keeps_identity_and_unknown_resources_in_real_enrichment_path() {
    use whoate_core::{sample::libproc, snapshot::Proc};
    let mut proc = Proc::identity(42, 1, 501, Some(100), "node".into());
    let mut reader = WindowReader {
        identities: [Some(proc.clone()), Some(proc.clone())].into(),
        denied: true,
        reads: 0,
    };
    let errors = libproc::enrich_proc(&mut proc, 501, &mut reader);
    assert_eq!((proc.pid, proc.uid, proc.start_time), (42, 501, Some(100)));
    assert_eq!((proc.footprint_bytes, proc.cpu_time_ns), (None, None));
    assert_eq!(proc.exe_path.as_deref(), Some("/usr/bin/node"));
    assert_eq!(reader.reads, 1);
    assert!(
        errors
            .iter()
            .any(|error| error.stage == "rusage_v4"
                && error.code == format!("errno={}", libc::EPERM))
    );
}

#[test]
fn before_after_reuse_and_exit_never_attach_old_process_resources() {
    use whoate_core::{sample::libproc, snapshot::Proc};
    let base = Proc::identity(42, 1, 501, Some(100), "node".into());
    for after in [
        None,
        Some(Proc::identity(42, 1, 501, Some(101), "node".into())),
        Some(Proc::identity(42, 1, 502, Some(100), "node".into())),
    ] {
        let mut proc = base.clone();
        let mut reader = WindowReader {
            identities: [Some(base.clone()), after].into(),
            denied: false,
            reads: 0,
        };
        let errors = libproc::enrich_proc(&mut proc, 501, &mut reader);
        assert_eq!(reader.reads, 1);
        assert_eq!(
            proc, base,
            "resource/metadata candidate must be discarded on identity change"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.code == "changed_during_read")
        );
    }
    for before in [
        None,
        Some(Proc::identity(42, 1, 501, Some(101), "node".into())),
    ] {
        let mut proc = base.clone();
        let mut reader = WindowReader {
            identities: [before].into(),
            denied: false,
            reads: 0,
        };
        assert!(!libproc::enrich_proc(&mut proc, 501, &mut reader).is_empty());
        assert_eq!(reader.reads, 0);
        assert_eq!(proc, base);
    }
    let mut proc = base.clone();
    let mut reader = WindowReader {
        identities: [Some(base.clone()), Some(base)].into(),
        denied: false,
        reads: 0,
    };
    assert!(libproc::enrich_proc(&mut proc, 501, &mut reader).is_empty());
    assert_eq!(
        (proc.footprint_bytes, proc.cpu_time_ns),
        (Some(4096), Some(8192))
    );
}

#[test]
fn procargs_keeps_argv_positions_and_only_tmux_environment() {
    let mut bytes = 4_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/usr/bin/node\0\0node\0\0--type=renderer\0--extension-process\0PRIVATE_SENTINEL=secret\0TMUX_PANE=%123\0TMUX=/private/server,10,0\0\0");
    let args = parse_procargs(&bytes).unwrap();
    assert_eq!(args.exe_path, "/usr/bin/node");
    assert_eq!(
        args.argv,
        ["node", "", "--type=renderer", "--extension-process"]
    );
    assert_eq!(args.tmux_pane.as_deref(), Some("%123"));
    assert_eq!(args.tmux.as_deref(), Some("/private/server,10,0"));
    assert!(!format!("{args:?}").contains("PRIVATE_SENTINEL"));
}

#[test]
fn malformed_procargs_never_panics_or_discloses_input() {
    let cases = [
        vec![],
        vec![1],
        (-1_i32).to_ne_bytes().to_vec(),
        i32::MAX.to_ne_bytes().to_vec(),
        b"\x01\0\0\0exe\0\0missing-nul".to_vec(),
        b"\x01\0\0\0exe\0\0\xff\0".to_vec(),
        b"\x02\0\0\0exe\0\0a\0".to_vec(),
    ];
    for bytes in cases {
        assert!(parse_procargs(&bytes).is_err());
    }
    // No fuzz dependency is approved: deterministic malformed-byte property corpus.
    let mut state = 1_u32;
    for size in 0..1024 {
        let bytes: Vec<u8> = (0..size)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        let _ = parse_procargs(&bytes);
    }
}

#[test]
fn macos_identity_and_v4_resources_are_real_and_missing_pid_is_error() {
    use whoate_core::sample::libproc;
    let pid = std::process::id() as i32;
    let list = libproc::identities().unwrap();
    let proc = list.iter().find(|proc| proc.pid == pid).unwrap();
    assert_eq!(proc.uid, unsafe { libc::getuid() });
    assert_eq!(proc.ppid, unsafe { libc::getppid() });
    assert!(proc.start_time.unwrap() > 1_700_000_000_000_000);
    assert!(list.iter().any(|proc| proc.uid == 0));
    assert!(libproc::footprint(pid).unwrap().0 > 0);
    assert!(libproc::footprint(i32::MAX).is_err());
    let mut reused = proc.clone();
    reused.start_time = Some(proc.start_time.unwrap() + 1);
    assert!(!proc.same_identity(&reused));
    reused = proc.clone();
    reused.uid += 1;
    assert!(!proc.same_identity(&reused));
}

#[test]
fn child_metadata_preserves_cwd_exe_tmux_but_not_other_environment() {
    use std::{
        process::{Command, Stdio},
        sync::atomic::AtomicBool,
    };
    use whoate_core::sample::{
        MacSampler, SampleError, SampleResult, Sampler,
        cache::{SlowSource, SystemCache},
    };
    struct NoSlow;
    impl SlowSource for NoSlow {
        fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
            Err(SampleError::new("top", "fixture_no_slow"))
        }
    }
    let dir = common::Temp::new();
    let child = Command::new("python3")
        .args(["-c", "import sys; sys.stdin.read()"])
        .env("TMUX_PANE", "%999999")
        .env("TMUX", "isolated-socket,1,0")
        .env("PRIVATE_ENV", "ENV_POISON_UNSEEN_76492")
        .current_dir(&dir.0)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    struct OwnChild(std::process::Child);
    impl Drop for OwnChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = OwnChild(child);
    let noncandidate = OwnChild(
        Command::new("/bin/cat")
            .env_remove("TMUX_PANE")
            .env_remove("TMUX")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut sampler = MacSampler::with_source_at(dir.0.join("cache/system.json"), NoSlow).unwrap();
    let snap = sampler.sample().unwrap();
    let proc = snap
        .procs
        .iter()
        .find(|proc| proc.pid == child.0.id() as i32)
        .unwrap();
    assert_eq!(proc.ppid, std::process::id() as i32);
    assert_eq!(proc.tmux_pane.as_deref(), Some("%999999"));
    assert_eq!(proc.tmux.as_deref(), Some("isolated-socket,1,0"));
    assert_eq!(
        std::path::Path::new(proc.cwd.as_deref().unwrap())
            .canonicalize()
            .unwrap(),
        dir.0.canonicalize().unwrap()
    );
    assert!(
        proc.exe_path
            .as_deref()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .starts_with("python")
    );
    assert!(proc.footprint_bytes.unwrap() > 0);
    let unqueried = snap
        .procs
        .iter()
        .find(|proc| proc.pid == noncandidate.0.id() as i32)
        .unwrap();
    assert!(
        unqueried.listen_ports.is_none(),
        "a real noncandidate must not be queried (including an empty result)"
    );
    assert!(unqueried.established_count.is_none());
    assert!(
        !serde_json::to_string(&snap)
            .unwrap()
            .contains("ENV_POISON_UNSEEN_76492")
    );
}
