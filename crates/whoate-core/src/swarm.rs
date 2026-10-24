//! Horizontal views only: members keep their original owners and resource totals.
use crate::{
    model::*,
    snapshot::{Proc, Snapshot},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::OnceLock,
};

pub fn logical_service(proc: &Proc) -> Option<String> {
    let exe = proc
        .exe_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name()?.to_str())
        .unwrap_or(&proc.comm);
    let args = &proc.argv;
    let interpreter = matches!(exe, "node" | "bun" | "deno");
    // npm's installed executable is npm-cli.js with an env-node shebang. The
    // kernel exe_path therefore names node even when npm sets its process title.
    let wrapper = if matches!(exe, "npm" | "npx") {
        Some((exe, 1))
    } else if interpreter && matches!(proc.comm.as_str(), "npm" | "npx") {
        Some((proc.comm.as_str(), 1))
    } else if interpreter {
        match args.get(1).and_then(|p| Path::new(p).file_name()?.to_str()) {
            Some("npm-cli.js") => Some(("npm", 2)),
            Some("npx-cli.js") => Some(("npx", 2)),
            _ => None,
        }
    } else {
        None
    };
    let entry = if let Some((wrapper, offset)) = wrapper {
        let start = if wrapper == "npm" {
            if args.get(offset).map(String::as_str) != Some("exec") {
                return None;
            }
            offset + 1
        } else {
            offset
        };
        args.iter().skip(start).find(|a| !a.starts_with('-'))?
    } else if interpreter {
        let entry = args.get(1)?;
        let tail = entry.rsplit_once("/node_modules/")?.1;
        let mut items = tail.split('/');
        let first = items.next()?;
        if first == ".bin" {
            return Some(items.next()?.into());
        }
        if first.starts_with('@') {
            return Some(format!("{first}/{}", items.next()?));
        }
        return Some(first.into());
    } else {
        return None;
    };
    if entry.contains('/') && !entry.starts_with('@') {
        return Path::new(entry).file_name()?.to_str().map(str::to_owned);
    }
    let name = entry
        .rsplit_once('@')
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, _)| name)
        .unwrap_or(entry);
    Some(name.to_owned())
}
fn variable(arg: &str) -> bool {
    static UUID: OnceLock<regex::Regex> = OnceLock::new();
    arg.bytes().all(|b| b.is_ascii_digit())
        || arg.starts_with("/tmp/")
        || arg.starts_with("/private/tmp/")
        || arg.starts_with("/var/folders/")
        || arg.starts_with("/private/var/folders/")
        || UUID
            .get_or_init(|| {
                regex::Regex::new(
                    r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
                )
                .expect("literal UUID pattern")
            })
            .is_match(arg)
        || arg.parse::<std::net::SocketAddr>().is_ok()
}
pub fn signature(proc: &Proc) -> String {
    if let Some(service) = logical_service(proc) {
        return format!("service:{service}");
    }
    let mut parts = vec![proc.comm.clone()];
    for arg in proc.argv.iter().skip(1).take(3) {
        if variable(arg) {
            continue;
        }
        if let Some((key, value)) = arg.split_once('=')
            && variable(value)
        {
            parts.push(key.into());
            continue;
        }
        parts.push(arg.clone());
    }
    // JSON avoids ambiguous joins: ["a b","c"] must differ from ["a","b c"].
    serde_json::to_string(&parts).expect("strings serialize")
}
pub fn timestamp_us(snap: &Snapshot) -> Option<u64> {
    let t = time::OffsetDateTime::parse(
        &snap.taken_at,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()?;
    u64::try_from(t.unix_timestamp_nanos() / 1000).ok()
}
fn build(signature: &str, members: &[&Proc], shape: Shape, owners: &BTreeMap<i32, &str>) -> Swarm {
    let mut distribution: BTreeMap<String, usize> = BTreeMap::new();
    let mut parents = BTreeSet::new();
    let mut pids = BTreeSet::new();
    for p in members {
        pids.insert(p.pid);
        parents.insert(p.ppid);
        if let Some(id) = owners.get(&p.pid) {
            *distribution.entry((*id).into()).or_default() += 1;
        }
    }
    let starts: Vec<_> = members.iter().filter_map(|p| p.start_time).collect();
    let span = if starts.len() == members.len() {
        starts
            .iter()
            .max()
            .zip(starts.iter().min())
            .map(|(max, min)| (max - min) as f64 / 1e6)
    } else {
        None
    };
    let service = signature.strip_prefix("service:");
    let unknown_cpu = members
        .iter()
        .filter(|p| p.cpu_pct_sampled.is_none())
        .count();
    let readable_cpu: f64 = members.iter().filter_map(|p| p.cpu_pct_sampled).sum();
    Swarm {
        signature: signature.into(),
        label: service.unwrap_or(&members[0].comm).into(),
        count: pids.len(),
        mem_bytes: members.iter().filter_map(|p| p.footprint_bytes).sum(),
        unknown_mem_count: members
            .iter()
            .filter(|p| p.footprint_bytes.is_none())
            .count(),
        cpu_pct: (unknown_cpu == 0).then_some(readable_cpu),
        // R12: the readable part when some members are unreadable (Owner has the same pair).
        cpu_pct_readable: (unknown_cpu > 0 && unknown_cpu < members.len()).then_some(readable_cpu),
        unknown_cpu_count: (unknown_cpu > 0).then_some(unknown_cpu),
        shape,
        owners: distribution
            .into_iter()
            .map(|(id, count)| SwarmOwner { id, count })
            .collect(),
        parents: parents.into_iter().collect(),
        started_span_s: span,
        pids: pids.into_iter().collect(),
        evidence: service
            .map(|_| {
                vec![Evidence::new(
                    "logical_service",
                    "npm/npx wrapper and node package entry identify the same service",
                    Confidence::Inferred,
                )]
            })
            .unwrap_or_default(),
        verdict: Verdict::normal(),
    }
}
pub fn detect(snap: &Snapshot, owners: &[Owner]) -> Vec<Swarm> {
    let confirmed_panes: BTreeMap<_, _> = owners
        .iter()
        .filter_map(|o| {
            o.origin
                .as_ref()
                .filter(|origin| origin.pane_alive == Some(true))
                .and_then(|origin| origin.tmux_pane.as_ref())
                .map(|pane| (o.id.as_str(), pane))
        })
        .collect();
    let allocation: BTreeMap<_, _> = owners
        .iter()
        .flat_map(|o| o.pids.iter().map(move |p| (*p, o.id.as_str())))
        .collect();
    let mut signatures: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
    for p in &snap.procs {
        signatures.entry(signature(p)).or_default().push(p);
    }
    let now = timestamp_us(snap);
    let mut output = vec![];
    for (sig, mut members) in signatures {
        members.sort_by_key(|p| p.pid);
        members.dedup_by_key(|p| p.pid);
        let mut clusters: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        for p in &members {
            if !now
                .zip(p.start_time)
                .is_some_and(|(n, s)| s <= n && n - s <= 600_000_000)
            {
                continue;
            }
            clusters
                .entry(format!("parent:{}", p.ppid))
                .or_default()
                .push(p);
            if let Some(pane) = p.tmux_pane.as_ref().or_else(|| {
                allocation
                    .get(&p.pid)
                    .and_then(|id| confirmed_panes.get(id).copied())
            }) {
                clusters.entry(format!("pane:{pane}")).or_default().push(p);
            }
        }
        let mut clusters: Vec<_> = clusters.into_iter().collect();
        clusters.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        let mut used = BTreeSet::new();
        for (_, cluster) in clusters {
            let fresh: Vec<_> = cluster
                .into_iter()
                .filter(|p| !used.contains(&p.pid))
                .collect();
            if fresh.len() >= 20 {
                for p in &fresh {
                    used.insert(p.pid);
                }
                output.push(build(&sig, &fresh, Shape::Burst, &allocation));
            }
        }
        let rest: Vec<_> = members
            .into_iter()
            .filter(|p| !used.contains(&p.pid))
            .collect();
        let count = rest
            .iter()
            .filter_map(|p| allocation.get(&p.pid))
            .collect::<BTreeSet<_>>()
            .len();
        if rest.len() >= 10
            && count >= 5
            && rest.iter().filter_map(|p| p.footprint_bytes).sum::<u64>() >= 1024 * 1024 * 1024
        {
            output.push(build(&sig, &rest, Shape::Spread, &allocation));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use crate::{model::*, snapshot::*};
    pub fn scenario(n: usize, owner_count: usize, bytes: u64) -> (Snapshot, Vec<Owner>) {
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]);
        let now = 1791978000_u64 * 1_000_000;
        let mut owners: Vec<_> = (0..owner_count)
            .map(|i| Owner::new(format!("pane:{i}"), OwnerKind::TmuxPane, "demo".into()))
            .collect();
        for i in 0..n {
            let mut p = Proc::identity(
                990000 + i as i32,
                970000 + i as i32,
                501,
                Some(now),
                "sleep".into(),
            );
            p.argv = vec!["sleep".into(), "600".into()];
            p.footprint_bytes = Some(bytes);
            owners[i % owner_count].pids.push(p.pid);
            snap.procs.push(p);
        }
        (snap, owners)
    }
    #[test]
    fn phase02_swarm_burst_and_spread_independent_boundaries() {
        let (mut snap, owners) = scenario(20, 1, 1);
        for p in &mut snap.procs {
            p.ppid = 900001;
        }
        assert_eq!(super::detect(&snap, &owners).len(), 1);
        assert_eq!(super::detect(&snap, &owners)[0].shape, Shape::Burst);
        snap.procs.pop();
        assert!(super::detect(&snap, &owners).is_empty());
        let (snap, owners) = scenario(10, 5, 107374183);
        let groups = super::detect(&snap, &owners);
        assert_eq!(groups[0].shape, Shape::Spread);
        assert_eq!(groups[0].count, 10);
        assert_eq!(groups[0].owners.len(), 5);
        assert_eq!(groups[0].parents.len(), 10);
        let (snap, owners) = scenario(10, 4, 107374183);
        assert!(super::detect(&snap, &owners).is_empty());
        let (snap, owners) = scenario(10, 5, 107374182);
        assert!(super::detect(&snap, &owners).is_empty());
    }
}
