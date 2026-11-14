//! Relative volume and pan moves through a group. Each member keeps its offset to the moved
//! track, even after it was held at the end of its range (-96 dB, +6 dB, hard left or right) and
//! the move came back: the offsets are taken from where the move started, not from where each
//! member was left.

use crate::model::Id;

/// Volume or pan, with its range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Level {
    Volume,
    Pan,
}

impl Level {
    pub(crate) fn clamp(self, v: f32) -> f32 {
        match self {
            Level::Volume => v.clamp(-96.0, 6.0),
            Level::Pan => v.clamp(-1.0, 1.0),
        }
    }
}

/// A run of linked moves of one track's volume or pan, and where it started.
#[derive(Debug, Clone)]
pub(crate) struct LinkedMove {
    lead: Id,
    level: Level,
    /// The moved track's value where the run started, and the value it was last given.
    lead_from: f32,
    lead_now: f32,
    /// Each member it carries: its value where the run started, and the value it was last given.
    peers: Vec<(Id, f32, f32)>,
}

impl LinkedMove {
    /// Where the members go when `lead` moves to `to`. `now` is the lead's value and `peers` the
    /// members' as they are. The run carries on while nothing else has changed them (another
    /// track moved, undo, a member moved alone); otherwise a new one starts from here.
    pub(crate) fn step(run: &mut Option<LinkedMove>, lead: Id, level: Level, now: f32, peers: &[(Id, f32)], to: f32) -> Vec<(Id, f32)> {
        let carries_on = run.as_ref().is_some_and(|m| {
            m.lead == lead
                && m.level == level
                && m.lead_now == now
                && m.peers.len() == peers.len()
                && m.peers.iter().zip(peers).all(|(&(id, _, last), &(pid, v))| id == pid && last == v)
        });
        if !carries_on {
            *run = Some(LinkedMove { lead, level, lead_from: now, lead_now: now, peers: peers.iter().map(|&(id, v)| (id, v, v)).collect() });
        }
        let m = run.as_mut().unwrap();
        let to = level.clamp(to);
        let d = to - m.lead_from;
        m.lead_now = to;
        m.peers
            .iter_mut()
            .map(|(id, from, last)| {
                *last = level.clamp(*from + d);
                (*id, *last)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moves the lead through `path`, giving each step's results back as the members' values.
    fn drag(run: &mut Option<LinkedMove>, level: Level, lead: &mut f32, peers: &mut [(Id, f32)], path: &[f32]) {
        for &to in path {
            let out = LinkedMove::step(run, 1, level, *lead, peers, to);
            for (p, (_, v)) in peers.iter_mut().zip(out) {
                p.1 = v;
            }
            *lead = level.clamp(to);
        }
    }

    #[test]
    fn a_member_held_at_the_bottom_keeps_its_offset_on_the_way_back() {
        let mut run = None;
        let (mut lead, mut peers) = (0.0, vec![(2, -20.0), (3, 3.0)]);
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[-40.0, -96.0, -50.0, 0.0]);
        assert_eq!(peers, vec![(2, -20.0), (3, 3.0)]);
    }

    #[test]
    fn a_member_held_at_the_top_keeps_its_offset_too() {
        let mut run = None;
        let (mut lead, mut peers) = (0.0, vec![(2, 3.0)]);
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[6.0]);
        assert_eq!(peers, vec![(2, 6.0)]);
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[-2.0]);
        assert_eq!(peers, vec![(2, 1.0)]);
    }

    #[test]
    fn pan_keeps_offsets_past_the_hard_ends() {
        let mut run = None;
        let (mut lead, mut peers) = (0.0, vec![(2, -0.5)]);
        drag(&mut run, Level::Pan, &mut lead, &mut peers, &[-1.0, 0.0]);
        assert_eq!(peers, vec![(2, -0.5)]);
    }

    #[test]
    fn a_member_changed_alone_starts_a_new_run_from_where_it_is() {
        let mut run = None;
        let (mut lead, mut peers) = (0.0, vec![(2, -20.0)]);
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[-96.0]);
        peers[0].1 = -30.0;
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[-86.0]);
        assert_eq!(peers, vec![(2, -20.0)], "10 dB up from -30 dB, not from the old offset");
    }

    #[test]
    fn moving_another_track_or_level_starts_a_new_run() {
        let mut run = None;
        let (mut lead, mut peers) = (0.0, vec![(2, -20.0)]);
        drag(&mut run, Level::Volume, &mut lead, &mut peers, &[-96.0]);
        let out = LinkedMove::step(&mut run, 2, Level::Volume, -96.0, &[(1, -96.0)], -90.0);
        assert_eq!(out, vec![(1, -90.0)]);
        let out = LinkedMove::step(&mut run, 1, Level::Volume, -90.0, &[(2, -90.0)], -80.0);
        assert_eq!(out, vec![(2, -80.0)]);
    }
}
