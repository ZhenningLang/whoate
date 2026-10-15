use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};
use whoate_core::{sample::ports, snapshot::Proc};

#[test]
fn ipv4_ipv6_listen_and_established_are_read_from_own_child() {
    let code = "import socket,sys; a=socket.socket(); a.bind(('127.0.0.1',0)); a.listen(); b=socket.socket(socket.AF_INET6); b.bind(('::1',0)); b.listen(); c=socket.create_connection(a.getsockname()); d,_=a.accept(); e=a.dup(); f=c.dup(); print(a.getsockname()[1],b.getsockname()[1],flush=True); sys.stdin.read()";
    let child = Command::new("python3")
        .args(["-c", code])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    struct OwnChild(std::process::Child);
    impl Drop for OwnChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnChild(child);
    let mut line = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let expected: Vec<u16> = line
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(expected.len(), 2);
    let (listening, connections) = ports::sockets(child.0.id() as i32).unwrap();
    assert!(listening.contains(&expected[0]));
    assert!(listening.contains(&expected[1]));
    assert_eq!(listening.len(), 2);
    assert_eq!(connections, 2);
    assert!(ports::sockets(i32::MAX).is_err());
}

#[test]
fn refused_socket_query_is_unknown_not_an_empty_success() {
    use whoate_core::sample::SampleError;
    let mut proc = Proc::identity(42, 1, 501, Some(100), "node".into());
    proc.listen_ports = Some(vec![8080]);
    proc.established_count = Some(2);
    let error = ports::apply_socket_result(&mut proc, Err(SampleError::os("sockets", libc::EPERM)))
        .unwrap_err();
    assert_eq!(error.code, format!("errno={}", libc::EPERM));
    assert_eq!(
        (proc.listen_ports.as_ref(), proc.established_count),
        (None, None)
    );
    ports::apply_socket_result(&mut proc, Ok((vec![], 0))).unwrap();
    assert_eq!(
        (proc.listen_ports.as_ref(), proc.established_count),
        (Some(&vec![]), Some(0))
    );
}

#[test]
fn only_orphan_terminal_or_runtime_candidates_scan_fds() {
    let mut proc = Proc::identity(2, 1, 501, Some(1), "node".into());
    assert!(ports::is_candidate(&proc));
    proc.ppid = 100;
    assert!(!ports::is_candidate(&proc));
    proc.ppid = 1;
    proc.comm = "unrelated".into();
    assert!(!ports::is_candidate(&proc));
    proc.tmux_pane = Some("%1".into());
    assert!(ports::is_candidate(&proc));
    proc.uid = 0;
    assert!(!ports::is_candidate(&proc));
    assert!(proc.listen_ports.is_none());
    assert!(proc.established_count.is_none());
}

#[test]
fn orphan_descendant_listeners_are_candidates_but_live_panes_apps_and_cycles_are_not() {
    use whoate_core::snapshot::TmuxPane;
    let mut root = Proc::identity(10, 1, 501, Some(1), "npm".into());
    let mut node = Proc::identity(11, 10, 501, Some(1), "node".into());
    node.tmux_pane = Some("%99".into());
    let panes = vec![TmuxPane {
        pane_id: "%1".into(),
        target: "a:0.0".into(),
        window_name: None,
        pane_pid: 10,
        cwd: None,
    }];
    assert!(ports::candidate_pids(&[root.clone(), node.clone()], &[], 501).contains(&11));
    assert!(!ports::candidate_pids(&[root.clone(), node.clone()], &panes, 501).contains(&11));
    root.exe_path = Some("/Applications/App.app/Contents/MacOS/App".into());
    assert!(!ports::candidate_pids(&[root.clone(), node.clone()], &[], 501).contains(&11));
    root.exe_path = None;
    root.ppid = 11;
    assert!(!ports::candidate_pids(&[root, node], &[], 501).contains(&11));
}
