//! The grid under both canvases, and the manager's answer when nobody names a place: the footprint
//! a card takes, the packer that fills free cells, and the arrangers that tidy a whole scope.

use goofi_node::Uid;
use std::collections::HashMap;

use crate::{machine::Machine, subpatch::Dir, Graph};

/// One cell in flow px: the slot unit, which a hand's drag and the packer both snap to.
pub const GRID: f64 = 24.0;
/// A node card as `app.css` draws it (`--node-*`): width, header, slot unit, open viewer.
const NODE_W: f64 = 233.0;
const HEADER: f64 = 36.0;
const SLOT: f64 = 24.0;
const VIEWER: f64 = 144.0;
/// A state card: `CARD_W` and `FALLBACK_H` in `machine/layout.ts`.
const CARD_W: f64 = 200.0;
const CARD_H: f64 = 44.0;
/// Cells kept clear around a packed card.
const GAP: i64 = 2;

/// A card's place and size in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

fn cells(px: f64) -> i64 {
    (px / GRID).ceil() as i64
}

fn rect_of(pos: [f64; 2], (w, h): (i64, i64)) -> Rect {
    Rect { x: (pos[0] / GRID).round() as i64, y: (pos[1] / GRID).round() as i64, w, h }
}

fn px(x: i64, y: i64) -> [f64; 2] {
    [x as f64 * GRID, y as f64 * GRID]
}

/// A node's footprint in cells, from what the document says of it: its slots, and which output
/// viewers are open (the default is open, collapsed on a node with three or more outputs).
pub fn node_size(g: &Graph, uid: Uid) -> (i64, i64) {
    let ins = g.slots(uid, Dir::In).len().max(1);
    let outs = g.slots(uid, Dir::Out);
    let open = |slot: &str| {
        let collapsed = g.viewers(uid).and_then(|v| v.get(slot)?.get("collapsed")?.as_bool());
        !collapsed.unwrap_or(outs.len() >= 3)
    };
    let out_h: f64 = outs.iter().map(|(k, _, _)| if open(k) { SLOT + VIEWER } else { SLOT }).sum();
    (cells(NODE_W), cells(HEADER + out_h.max(ins as f64 * SLOT)))
}

fn state_size() -> (i64, i64) {
    (cells(CARD_W), cells(CARD_H))
}

fn clear_of(taken: &[Rect], r: Rect) -> bool {
    r.x >= 0
        && r.y >= 0
        && !taken.iter().any(|t| {
            t.x < r.x + r.w + GAP && r.x < t.x + t.w + GAP && t.y < r.y + r.h + GAP && r.y < t.y + t.h + GAP
        })
}

/// The clear place right of `from` in its own row, else down that column, else further right: a
/// chain of adds runs left to right, and a fan-out stacks under the first.
fn pack_right(taken: &[Rect], (w, h): (i64, i64), from: Option<Rect>) -> [f64; 2] {
    let (x0, y0) = from.map_or((0, 0), |f| (f.x + f.w + GAP, f.y));
    for x in x0.. {
        for y in y0..y0 + 400 {
            if clear_of(taken, Rect { x, y, w, h }) {
                return px(x, y);
            }
        }
    }
    unreachable!("the columns never run out")
}

/// The clear place nearest the centre of what is there, so a machine grows outward.
fn pack_around(taken: &[Rect], (w, h): (i64, i64)) -> [f64; 2] {
    if taken.is_empty() {
        return px(0, 0);
    }
    let n = taken.len() as f64;
    let cx = taken.iter().map(|t| t.x as f64 + t.w as f64 / 2.0).sum::<f64>() / n;
    let cy = taken.iter().map(|t| t.y as f64 + t.h as f64 / 2.0).sum::<f64>() / n;
    let (x0, y0) = (cx.round() as i64, cy.round() as i64);
    let mut best: Option<(f64, i64, i64)> = None;
    for x in (x0 - 40).max(0)..=x0 + 40 {
        for y in (y0 - 40).max(0)..=y0 + 40 {
            if !clear_of(taken, Rect { x, y, w, h }) {
                continue;
            }
            let d = (x as f64 + w as f64 / 2.0 - cx).hypot(y as f64 + h as f64 / 2.0 - cy);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, x, y));
            }
        }
    }
    best.map_or(px(x0, y0), |(_, x, y)| px(x, y))
}

/// The member of `scope` that holds `uid`: itself, or the facade of the sub-patch it is in.
fn member_in(g: &Graph, scope: Option<Uid>, uid: Uid) -> Option<Uid> {
    let mut at = uid;
    loop {
        let parent = g.scope_of(at);
        if parent == scope {
            return Some(at);
        }
        at = parent?;
    }
}

/// Where a node of a scope goes when its caller names no place: right of the member added before
/// it, which is what it is most often wired from. `uid` is the node as born, so its footprint is
/// read, not guessed.
pub fn place_node(g: &Graph, scope: Option<Uid>, uid: Uid) -> [f64; 2] {
    let taken: Vec<Rect> = g
        .all_uids()
        .into_iter()
        .filter(|u| *u != uid && g.scope_of(*u) == scope)
        .filter_map(|u| Some(rect_of(g.pos(u)?, node_size(g, u))))
        .collect();
    pack_right(&taken, node_size(g, uid), taken.last().copied())
}

/// Where a new state's card goes: the clear cell nearest the machine's other cards.
pub fn place_state(m: &Machine) -> [f64; 2] {
    let taken: Vec<Rect> = m.states.values().map(|s| rect_of(s.pos, state_size())).collect();
    pack_around(&taken, state_size())
}

/// The edges `items` make among themselves, by index; a back edge of a cycle is left out so the
/// rest layers as a flow. `succ(i)` names what i feeds.
fn forward_edges(n: usize, succ: &dyn Fn(usize) -> Vec<usize>) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); n];
    let mut state = vec![0u8; n]; // 0 unseen, 1 on the stack, 2 done
    fn visit(i: usize, succ: &dyn Fn(usize) -> Vec<usize>, state: &mut [u8], preds: &mut [Vec<usize>]) {
        state[i] = 1;
        for j in succ(i) {
            match state[j] {
                1 => {}
                0 => {
                    preds[j].push(i);
                    visit(j, succ, state, preds);
                }
                _ => preds[j].push(i),
            }
        }
        state[i] = 2;
    }
    for i in 0..n {
        if state[i] == 0 {
            visit(i, succ, &mut state, &mut preds);
        }
    }
    preds
}

/// Layers by depth: a column per dataflow depth, each ordered by where its sources landed, so a
/// cable runs to the right and seldom crosses. Sizes in cells; the answer in flow px per index.
fn layer(sizes: &[(i64, i64)], preds: &[Vec<usize>]) -> Vec<[f64; 2]> {
    let n = sizes.len();
    let mut depth = vec![usize::MAX; n];
    fn depth_of(i: usize, preds: &[Vec<usize>], depth: &mut [usize]) -> usize {
        if depth[i] != usize::MAX {
            return depth[i];
        }
        depth[i] = 0;
        let d = preds[i].iter().map(|&p| depth_of(p, preds, depth) + 1).max().unwrap_or(0);
        depth[i] = d;
        d
    }
    for i in 0..n {
        depth_of(i, preds, &mut depth);
    }
    let mut out = vec![[0.0, 0.0]; n];
    let mut row = vec![0i64; n];
    let mut x = 0i64;
    let layers = depth.iter().copied().max().map_or(0, |d| d + 1);
    for d in 0..layers {
        let mut members: Vec<usize> = (0..n).filter(|&i| depth[i] == d).collect();
        let want = |i: usize, row: &[i64]| -> Option<i64> {
            let ps = &preds[i];
            (!ps.is_empty()).then(|| ps.iter().map(|&p| row[p]).sum::<i64>() / ps.len() as i64)
        };
        members.sort_by_key(|&i| (want(i, &row).unwrap_or(i64::MAX), i));
        let mut cursor = 0i64;
        for i in members.iter().copied() {
            row[i] = want(i, &row).unwrap_or(0).max(cursor);
            out[i] = px(x, row[i]);
            cursor = row[i] + sizes[i].1 + GAP;
        }
        x += members.iter().map(|&i| sizes[i].0).max().unwrap_or(0) + GAP;
    }
    out
}

/// Every member of `scope` moved into dataflow layers: `(uid, pos)` for each, in member order.
pub fn arrange_scope(g: &Graph, scope: Option<Uid>) -> Vec<(Uid, [f64; 2])> {
    let members: Vec<Uid> = g.all_uids().into_iter().filter(|u| g.scope_of(*u) == scope).collect();
    let index: HashMap<Uid, usize> = members.iter().enumerate().map(|(i, u)| (*u, i)).collect();
    let mut succ = vec![Vec::new(); members.len()];
    for l in g.links_view() {
        let (Some(a), Some(b)) = (member_in(g, scope, l.node_out), member_in(g, scope, l.node_in)) else { continue };
        if a != b && !succ[index[&a]].contains(&index[&b]) {
            succ[index[&a]].push(index[&b]);
        }
    }
    let sizes: Vec<(i64, i64)> = members.iter().map(|u| node_size(g, *u)).collect();
    let preds = forward_edges(members.len(), &|i| succ[i].clone());
    members.into_iter().zip(layer(&sizes, &preds)).collect()
}

/// Every state on a ring, in the order a playhead reaches them from its start, the ring sized so
/// the cards clear each other; one state sits at the origin. The machine as it would then be.
pub fn arrange_machine(m: &Machine) -> Machine {
    let names: Vec<&String> = m.states.keys().collect();
    let n = names.len();
    let mut next = m.clone();
    if n == 0 {
        return next;
    }
    let index: HashMap<&String, usize> = names.iter().enumerate().map(|(i, s)| (*s, i)).collect();
    let mut succ = vec![Vec::new(); n];
    for t in m.transitions.values() {
        if let (Some(&a), Some(&b)) = (index.get(&t.from), index.get(&t.to)) {
            if a != b && !succ[a].contains(&b) {
                succ[a].push(b);
            }
        }
    }
    // Reachability order from the first playhead's start, then from whatever is left.
    let first = m.playheads.values().next().and_then(|p| index.get(&p.start).copied()).unwrap_or(0);
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    fn walk(i: usize, succ: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        seen[i] = true;
        order.push(i);
        for &j in &succ[i] {
            if !seen[j] {
                walk(j, succ, seen, order);
            }
        }
    }
    walk(first, &succ, &mut seen, &mut order);
    for i in 0..n {
        if !seen[i] {
            walk(i, &succ, &mut seen, &mut order);
        }
    }
    let (w, h) = state_size();
    if n == 1 {
        next.states[names[0]].pos = px(0, 0);
        return next;
    }
    let step = (w.max(h) + GAP) as f64;
    let rx = (n as f64 * step * 1.15 / std::f64::consts::TAU).max(step);
    let ry = (rx * 0.72).max((h + GAP) as f64);
    let cells: Vec<(i64, i64)> = (0..n)
        .map(|k| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * k as f64 / n as f64;
            ((rx + a.cos() * rx).round() as i64, (ry + a.sin() * ry).round() as i64)
        })
        .collect();
    let min_x = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    for (k, &i) in order.iter().enumerate() {
        next.states[names[i]].pos = px(cells[k].0 - min_x, cells[k].1 - min_y);
    }
    next
}
