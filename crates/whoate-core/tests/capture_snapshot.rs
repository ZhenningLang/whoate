use whoate_core::snapshot::{Proc, Snapshot};

#[test]
fn unknown_resources_survive_replay_without_becoming_zero() {
    let proc = Proc::identity(42, 1, 501, Some(1_700_000_000_000_000), "node".into());
    let snap = Snapshot::historical("2026-10-14T19:40:08+08:00".into(), vec![proc]);
    let json = serde_json::to_value(&snap).unwrap();
    assert!(json["host"].is_null());
    assert!(json["procs"][0]["footprint_bytes"].is_null());
    assert!(json["procs"][0]["cpu_time_ns"].is_null());
    assert!(json["procs"][0]["cpu_pct_sampled"].is_null());
    assert_eq!(json["system_procs"]["status"], "missing");
    assert!(json["system_procs"]["as_of"].is_null());
    let replay: Snapshot = serde_json::from_value(json).unwrap();
    assert_eq!(replay.procs[0].start_time, Some(1_700_000_000_000_000));
    assert_eq!(replay.procs[0].pid, 42);
}

#[test]
fn sampler_mock_replays_full_contract_and_local_timestamp() {
    use whoate_core::{
        sample::{SampleResult, Sampler, sysctl},
        snapshot::{Freshness, Host, Pressure, SystemProcs, TmuxPane},
    };
    let mut proc = Proc::identity(8, 7, 501, Some(1), "node".into());
    proc.exe_path = Some("/usr/bin/node".into());
    proc.argv = vec!["node".into(), "--port=80".into()];
    proc.cwd = Some("/project".into());
    proc.tmux_pane = Some("%1".into());
    proc.tmux = Some("socket,1,0".into());
    proc.footprint_bytes = Some(123);
    proc.cpu_time_ns = Some(456);
    proc.responsible_pid = Some(9);
    proc.listen_ports = Some(vec![80]);
    proc.established_count = Some(2);
    proc.cpu_pct_sampled = Some(25.5);
    let mut snap = Snapshot::historical(sysctl::timestamp(sysctl::unix_now()).unwrap(), vec![proc]);
    snap.host = Some(Host {
        mem_total_bytes: 1024,
        mem_used_bytes: 768,
        compressed_bytes: 64,
        wired_bytes: 256,
        swap_used_bytes: 32,
        pressure: Pressure::Warn,
    });
    snap.tmux_panes.push(TmuxPane {
        pane_id: "%1".into(),
        target: "a:0.0".into(),
        window_name: Some("a".into()),
        pane_pid: 7,
        cwd: Some("/project".into()),
    });
    snap.system_procs = SystemProcs {
        status: Freshness::Stale,
        as_of: Some(snap.taken_at.clone()),
    };
    struct Mock(Snapshot);
    impl Sampler for Mock {
        fn sample(&mut self) -> SampleResult<Snapshot> {
            Ok(self.0.clone())
        }
    }
    let sampled = Mock(snap.clone()).sample().unwrap();
    let json = serde_json::to_string(&sampled).unwrap();
    assert_eq!(serde_json::from_str::<Snapshot>(&json).unwrap(), snap);
    let time = time::OffsetDateTime::parse(
        &snap.taken_at,
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!(
        time.offset(),
        sysctl::local_time(time.unix_timestamp()).unwrap().offset()
    );
    let value = serde_json::to_value(sampled).unwrap();
    for field in [
        "pid",
        "ppid",
        "uid",
        "start_time",
        "comm",
        "exe_path",
        "argv",
        "cwd",
        "tmux_pane",
        "footprint_bytes",
        "cpu_time_ns",
        "responsible_pid",
        "listen_ports",
        "established_count",
        "cpu_pct_sampled",
    ] {
        assert!(value["procs"][0].get(field).is_some());
    }
}

#[test]
fn native_taken_at_preserves_subsecond_cpu_sampling_clock() {
    let first = whoate_core::sample::sysctl::now_timestamp().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let second = whoate_core::sample::sysctl::now_timestamp().unwrap();
    let parse = |text: &str| {
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).unwrap()
    };
    assert!(parse(&second) > parse(&first));
    assert!(first.contains('.'));
}
