//! Process trees: descendant sets and PID-namespace membership.

use std::collections::{BTreeMap, BTreeSet};

use super::ProcEntry;

/// An immutable process table indexed by PID and parent.
#[derive(Clone, Debug, Default)]
pub struct ProcTree {
    by_pid: BTreeMap<u32, ProcEntry>,
    children: BTreeMap<u32, Vec<u32>>,
}

impl ProcTree {
    /// Build from a snapshot.
    pub fn build(entries: impl IntoIterator<Item = ProcEntry>) -> Self {
        let mut tree = ProcTree::default();
        for e in entries {
            tree.children.entry(e.ppid).or_default().push(e.pid);
            tree.by_pid.insert(e.pid, e);
        }
        for kids in tree.children.values_mut() {
            kids.sort_unstable();
        }
        tree
    }

    /// One process.
    pub fn get(&self, pid: u32) -> Option<&ProcEntry> {
        self.by_pid.get(&pid)
    }

    /// Number of processes.
    pub fn len(&self) -> usize {
        self.by_pid.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.by_pid.is_empty()
    }

    /// Direct children of `pid`.
    pub fn children(&self, pid: u32) -> &[u32] {
        self.children.get(&pid).map(Vec::as_slice).unwrap_or(&[])
    }

    /// All descendants of `pid` (not including `pid`). Robust against cycles in corrupt data.
    pub fn descendants(&self, pid: u32) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let mut stack = vec![pid];
        while let Some(p) = stack.pop() {
            for &c in self.children(p) {
                if c != pid && out.insert(c) {
                    stack.push(c);
                }
            }
        }
        out
    }

    /// Processes whose PID namespace is `ns` — every process of a sandbox, including those
    /// that were reparented (PROCESS_LIFECYCLE §5).
    pub fn in_namespace(&self, ns: u64) -> Vec<&ProcEntry> {
        self.by_pid
            .values()
            .filter(|e| e.pid_ns == Some(ns))
            .collect()
    }

    /// Members of namespace `ns` plus all their descendants (processes in nested PID
    /// namespaces, such as pressure-vessel's container).
    pub fn sandbox_members(&self, ns: u64) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        for e in self.in_namespace(ns) {
            out.insert(e.pid);
            out.extend(self.descendants(e.pid));
        }
        out
    }

    /// Ancestors of `pid`, nearest first, stopping at PID 0/1 or a missing parent.
    pub fn ancestors(&self, pid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = self.get(pid).map(|e| e.ppid);
        while let Some(p) = cur {
            if p == 0 || p == pid || out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.get(p).map(|e| e.ppid);
        }
        out
    }

    /// Iterate over all processes in PID order.
    pub fn iter(&self) -> impl Iterator<Item = &ProcEntry> {
        self.by_pid.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procfs::ProcState;

    fn e(pid: u32, ppid: u32, comm: &str, ns: u64) -> ProcEntry {
        ProcEntry {
            pid,
            ppid,
            comm: comm.into(),
            start_time_ticks: u64::from(pid) * 10,
            pid_ns: Some(ns),
            state: ProcState::Sleeping,
            exe_name: None,
        }
    }

    fn sample() -> ProcTree {
        // host: 1 systemd → 100 supervisor → 101 bwrap → 102 init (ns 9) → 103 umu → 104 game
        // 105 detached child of init (reparented), 200 nested-ns process under 104
        ProcTree::build([
            e(1, 0, "systemd", 1),
            e(100, 1, "zaun", 1),
            e(101, 100, "bwrap", 1),
            e(102, 101, "zaun-init", 9),
            e(103, 102, "umu-run", 9),
            e(104, 103, "game.exe", 9),
            e(105, 102, "launcher.exe", 9),
            e(200, 104, "pv-adverb", 11),
            e(300, 1, "unrelated", 1),
        ])
    }

    #[test]
    fn descendants_and_children() {
        let t = sample();
        assert_eq!(t.children(102), &[103, 105]);
        assert_eq!(
            t.descendants(101).into_iter().collect::<Vec<_>>(),
            vec![102, 103, 104, 105, 200]
        );
        assert!(t.descendants(300).is_empty());
        assert_eq!(t.ancestors(104), vec![103, 102, 101, 100, 1]);
    }

    #[test]
    fn namespace_membership() {
        let t = sample();
        let ns: Vec<u32> = t.in_namespace(9).iter().map(|e| e.pid).collect();
        assert_eq!(ns, vec![102, 103, 104, 105]);
        assert_eq!(
            t.sandbox_members(9).into_iter().collect::<Vec<_>>(),
            vec![102, 103, 104, 105, 200]
        );
    }

    #[test]
    fn cycles_do_not_hang() {
        let t = ProcTree::build([e(5, 6, "a", 1), e(6, 5, "b", 1)]);
        assert_eq!(t.descendants(5).into_iter().collect::<Vec<_>>(), vec![6]);
        assert_eq!(t.ancestors(5), vec![6]);
    }
}
