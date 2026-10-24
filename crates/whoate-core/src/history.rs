//! 1800 one-second interval points per owner; no synthetic samples across gaps.
use crate::{
    model::{HistoryPoint, Owner, OwnerKind},
    snapshot::{Proc, Snapshot},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug)]
struct Point {
    sample: HistoryPoint,
    delta: Option<i128>,
}
#[derive(Default)]
struct Series {
    anchor: Option<(i32, u32, Option<u64>)>,
    points: VecDeque<Point>,
}
#[derive(Default)]
pub struct History {
    owners: BTreeMap<String, Series>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trend {
    pub growth_bytes: i128,
    pub monotonic: bool,
}
impl History {
    pub fn retain(&mut self, ids: &BTreeSet<String>) {
        self.owners.retain(|id, _| ids.contains(id));
    }
    /// PID-keyed roots use their process identity; app/session IDs are logical.
    /// Use record_snapshot_at for panes so pane-root reuse can also be detected.
    pub fn record_at(&mut self, owner: &Owner, procs: &[Proc], time_s: i64) {
        let pid = match owner.kind {
            OwnerKind::Orphan | OwnerKind::Service | OwnerKind::System => owner
                .id
                .rsplit_once(':')
                .and_then(|(_, pid)| pid.parse().ok()),
            OwnerKind::AgentSession
                if owner.agent.as_ref().is_some_and(|a| a.session_id.is_none()) =>
            {
                owner
                    .id
                    .rsplit_once(":pid:")
                    .and_then(|(_, pid)| pid.parse().ok())
            }
            _ => None,
        };
        self.record_with_anchor(owner, procs, time_s, pid);
    }
    /// Pane IDs can be reused: the snapshot supplies the actual pane root identity.
    pub fn record_snapshot_at(&mut self, owner: &Owner, snap: &Snapshot, time_s: i64) {
        if owner.kind == OwnerKind::TmuxPane {
            let pid = owner
                .id
                .strip_prefix("pane:")
                .and_then(|id| snap.tmux_panes.iter().find(|p| p.pane_id == id))
                .map(|p| p.pane_pid);
            self.record_with_anchor(owner, &snap.procs, time_s, pid);
        } else {
            self.record_at(owner, &snap.procs, time_s);
        }
    }
    fn record_with_anchor(&mut self, owner: &Owner, procs: &[Proc], time_s: i64, pid: Option<i32>) {
        let anchor = pid
            .and_then(|pid| procs.iter().find(|p| p.pid == pid))
            .map(|p| (p.pid, p.uid, p.start_time));
        let s = self.owners.entry(owner.id.clone()).or_default();
        // Members are measurements, not owner identity. Child spawn/exit must
        // remain visible in the owner's aggregate curve rather than erase it.
        if s.anchor != anchor || anchor.is_some_and(|(_, _, start)| start.is_none()) {
            s.points.clear();
        }
        s.anchor = anchor;
        // With nothing readable there is no measurement: a gap, never a fabricated zero.
        // Partly readable owners still get a point over the readable part (feedback 1).
        if owner.unknown_mem_count > 0 && owner.unknown_mem_count >= owner.pids.len() {
            return;
        }
        let unknown = (owner.unknown_mem_count > 0).then_some(owner.unknown_mem_count);
        if s.points
            .back()
            .is_some_and(|p| p.sample.timestamp_s > time_s)
        {
            s.points.clear();
        }
        if s.points
            .back()
            .is_some_and(|p| p.sample.timestamp_s == time_s)
        {
            s.points.pop_back();
        }
        // A change in how many processes are unreadable changes what mem_bytes
        // covers: no delta there, so window() never sees a fake rise or fall.
        let delta = s
            .points
            .back()
            .filter(|p| p.sample.timestamp_s + 1 == time_s && p.sample.unknown_mem_count == unknown)
            .map(|p| owner.mem_bytes as i128 - p.sample.mem_bytes as i128);
        // Each retained point carries its preceding one-second interval delta.
        // This measures a real 1800-second window without retaining an 1801st point.
        s.points.push_back(Point {
            sample: HistoryPoint {
                timestamp_s: time_s,
                mem_bytes: owner.mem_bytes,
                cpu_pct: owner.cpu_pct,
                unknown_mem_count: unknown,
                cpu_pct_readable: owner.cpu_pct_readable,
                unknown_cpu_count: owner.unknown_cpu_count,
            },
            delta,
        });
        if s.points.len() > 1800 {
            s.points.pop_front();
        }
    }
    pub fn series(&self, id: &str) -> Vec<HistoryPoint> {
        self.owners
            .get(id)
            .map(|s| s.points.iter().map(|p| p.sample.clone()).collect())
            .unwrap_or_default()
    }
    /// A stored window is usable only when its last point is the current sample.
    pub fn ends_at(&self, id: &str, time_s: i64) -> bool {
        self.owners
            .get(id)
            .and_then(|s| s.points.back())
            .is_some_and(|p| p.sample.timestamp_s == time_s)
    }
    pub fn window(&self, id: &str, seconds: usize) -> Option<Trend> {
        let points = &self.owners.get(id)?.points;
        if seconds == 0 || points.len() < seconds {
            return None;
        }
        let mut growth = 0;
        let mut monotonic = true;
        let mut last = None;
        for p in points.iter().skip(points.len() - seconds) {
            if last.is_some_and(|last| last + 1 != p.sample.timestamp_s) {
                return None;
            }
            let d = p.delta?;
            growth += d;
            monotonic &= d >= 0;
            last = Some(p.sample.timestamp_s);
        }
        Some(Trend {
            growth_bytes: growth,
            monotonic,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{model::*, snapshot::*};
    #[test]
    fn phase02_history_pane_anchor_and_missing_measurement_contract() {
        let root = Proc::identity(999001, 1, 501, Some(1), "zsh".into());
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![root]);
        snap.tmux_panes.push(TmuxPane {
            pane_title: None,
            pane_id: "%one".into(),
            target: "demo:0.0".into(),
            window_name: None,
            pane_pid: 999001,
            cwd: None,
        });
        let mut owner = Owner::new("pane:%one".into(), OwnerKind::TmuxPane, "demo".into());
        owner.pids = vec![999001];
        owner.mem_bytes = 10;
        let mut h = super::History::default();
        h.record_snapshot_at(&owner, &snap, 0);
        snap.procs
            .push(Proc::identity(999002, 999001, 501, Some(2), "git".into()));
        owner.pids.push(999002);
        owner.mem_bytes = 20;
        h.record_snapshot_at(&owner, &snap, 1);
        assert_eq!(h.series(&owner.id).len(), 2);
        // Phase 10 feedback 1: only a fully unreadable owner leaves the gap now
        // (partly readable owners are recorded; see tests/phase10_history.rs).
        owner.unknown_mem_count = 2;
        h.record_snapshot_at(&owner, &snap, 2);
        assert_eq!(h.series(&owner.id).len(), 2);
        owner.unknown_mem_count = 0;
        h.record_snapshot_at(&owner, &snap, 3);
        assert!(h.window(&owner.id, 2).is_none());
        snap.procs[0].start_time = Some(4);
        h.record_snapshot_at(&owner, &snap, 4);
        assert_eq!(h.series(&owner.id).len(), 1);
    }
    #[test]
    fn phase02_history_member_churn_preserves_owner_window() {
        let mut h = super::History::default();
        let root = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        let child = Proc::identity(999002, 999001, 501, Some(2), "git".into());
        let mut o = Owner::new("service:999001".into(), OwnerKind::Service, "demo".into());
        for t in 0..=1800 {
            let mut ps = vec![root.clone()];
            if t % 2 == 0 {
                ps.push(child.clone());
            }
            o.pids = ps.iter().map(|p| p.pid).collect();
            o.mem_bytes = t * 1024 * 1024;
            h.record_at(&o, &ps, t as i64);
        }
        assert_eq!(h.series(&o.id).len(), 1800);
        assert_eq!(h.window(&o.id, 1800).unwrap().growth_bytes, 1887436800);
        let mut reused = root;
        reused.start_time = Some(3);
        o.pids = vec![999001];
        h.record_at(&o, &[reused], 1801);
        assert_eq!(h.series(&o.id).len(), 1);
    }
    #[test]
    fn phase02_history_same_second_replaces_instead_of_adding_point() {
        let mut h = super::History::default();
        let p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        let mut o = Owner::new("service:999001".into(), OwnerKind::Service, "demo".into());
        o.pids = vec![999001];
        o.mem_bytes = 10;
        h.record_at(&o, std::slice::from_ref(&p), 100);
        o.mem_bytes = 20;
        h.record_at(&o, &[p], 100);
        assert_eq!(h.series(&o.id).len(), 1);
        assert_eq!(h.series(&o.id)[0].mem_bytes, 20);
    }
    #[test]
    fn phase02_history_capacity_real_window_gaps_and_identity() {
        let mut h = super::History::default();
        let mut p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        p.footprint_bytes = Some(0);
        let mut o = Owner::new("service:999001".into(), OwnerKind::Service, "demo".into());
        o.pids = vec![999001];
        for t in 0..=1800 {
            o.mem_bytes = t * 1024 * 1024;
            h.record_at(&o, &[p.clone()], t as i64);
        }
        assert_eq!(h.series(&o.id).len(), 1800);
        let trend = h.window(&o.id, 1800).unwrap();
        assert_eq!(trend.growth_bytes, 1800 * 1024 * 1024);
        assert!(trend.monotonic);
        h.record_at(&o, &[p.clone()], 1800);
        assert_eq!(h.series(&o.id).len(), 1800);
        h.record_at(&o, &[p.clone()], 1802);
        assert!(h.window(&o.id, 1800).is_none());
        p.start_time = Some(2);
        h.record_at(&o, &[p], 1803);
        assert_eq!(h.series(&o.id).len(), 1);
    }
}
