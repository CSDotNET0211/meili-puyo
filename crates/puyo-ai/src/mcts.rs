//! Chance-PUCT baseline. The search tree owns public information only; the
//! seeded [`GameState`](crate::game::GameState) and its hidden queue never enter
//! this module.

use puyo_core::game::{
    Colour, DEATH_X, DEATH_Y, LegalPlacements, MAX_LEGAL_ACTIONS, Pair, Placement, PublicState,
    legal_placements_compact, place_pair,
};
use puyo_core::{Board, Cell, HEIGHT};

#[derive(Debug, Clone, Copy)]
pub struct SearchState {
    pub board: Board,
    pub pieces: [Pair; 3],
    pub estimated_unseen_colour_counts: [u16; 4],
    pub placements: u32,
    pub maximum_chain: u8,
    pub dead: bool,
}

impl From<&PublicState> for SearchState {
    fn from(state: &PublicState) -> Self {
        Self {
            board: state.board,
            pieces: state.pieces,
            estimated_unseen_colour_counts: state.estimated_unseen_colour_counts,
            placements: state.placements,
            maximum_chain: state.maximum_chain,
            dead: state.dead,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingChance {
    board: Board,
    known_next: [Pair; 2],
    estimated_unseen_colour_counts: [u16; 4],
    placements: u32,
    maximum_chain: u8,
}

impl SearchState {
    fn play(&self, placement: Placement) -> Option<PendingChance> {
        // Search edges are created exclusively by `legal_placements`; avoid
        // enumerating every legal move a second time on each tree descent.
        if self.dead {
            return None;
        }
        let mut board = self.board;
        place_pair(&mut board, self.pieces[0], placement)?;
        let chains = board.simulate_chain_count();
        Some(PendingChance {
            board,
            known_next: [self.pieces[1], self.pieces[2]],
            estimated_unseen_colour_counts: self.estimated_unseen_colour_counts,
            placements: self.placements + 1,
            maximum_chain: self.maximum_chain.max(chains),
        })
    }
}

impl PendingChance {
    fn reveal(&self, pair: Pair) -> SearchState {
        let mut counts = self.estimated_unseen_colour_counts;
        if counts.iter().map(|&count| u32::from(count)).sum::<u32>() < 2 {
            counts = [64; 4];
        }
        counts[pair.axis as usize] = counts[pair.axis as usize].saturating_sub(1);
        counts[pair.child as usize] = counts[pair.child as usize].saturating_sub(1);
        let dead = self.board.get(DEATH_X, DEATH_Y) != Cell::Empty;
        SearchState {
            board: self.board,
            pieces: [self.known_next[0], self.known_next[1], pair],
            estimated_unseen_colour_counts: counts,
            placements: self.placements,
            maximum_chain: self.maximum_chain,
            dead,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchConfig {
    pub simulations: u32,
    pub max_depth: u8,
    pub c_puct: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            simulations: 800,
            max_depth: 8,
            c_puct: 1.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootAction {
    pub placement: Placement,
    pub visits: u32,
    pub prior: f32,
    pub mean_value: f32,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub actions: Vec<RootAction>,
    pub root_value: f32,
    pub root_evaluation: f32,
    pub nodes: usize,
}

impl SearchResult {
    pub fn best(&self) -> Option<RootAction> {
        self.actions
            .iter()
            .copied()
            .max_by_key(|action| action.visits)
    }
}

#[derive(Debug)]
struct Node {
    visits: u32,
    value_sum: f32,
    kind: NodeKind,
}

#[derive(Debug)]
enum NodeKind {
    Decision {
        state: SearchState,
        expanded: bool,
        evaluation: f32,
        edges: Vec<DecisionEdge>,
    },
    Chance {
        pending: PendingChance,
        edges: Vec<ChanceEdge>,
    },
}

#[derive(Debug)]
struct DecisionEdge {
    placement: Placement,
    prior: f32,
    visits: u32,
    value_sum: f32,
    child: Option<usize>,
}

#[derive(Debug)]
struct ChanceEdge {
    pair: Pair,
    probability: f32,
    visits: u32,
    value_sum: f32,
    child: Option<usize>,
}

pub struct Evaluation {
    pub value: f32,
    pub policy_logits: Vec<f32>,
}

pub trait Evaluator {
    fn evaluate(&mut self, state: &SearchState, placements: &[Placement]) -> Evaluation;
}

pub struct BaselineEvaluator;

impl Evaluator for BaselineEvaluator {
    fn evaluate(&mut self, state: &SearchState, placements: &[Placement]) -> Evaluation {
        Evaluation {
            value: baseline_value(state),
            policy_logits: vec![0.0; placements.len()],
        }
    }
}

pub fn search(state: SearchState, config: SearchConfig) -> SearchResult {
    search_with_evaluator(state, config, &mut BaselineEvaluator)
}

pub fn search_with_evaluator<E: Evaluator>(
    state: SearchState,
    config: SearchConfig,
    evaluator: &mut E,
) -> SearchResult {
    // A simulation usually creates at most a decision and a chance node.
    // Reserving the arena keeps node handles stable and avoids Vec growth
    // copies in the hottest search loop.
    let arena_capacity = (config.simulations as usize)
        .saturating_mul(2)
        .saturating_add(1);
    let mut nodes = Vec::with_capacity(arena_capacity);
    nodes.push(Node {
        visits: 0,
        value_sum: 0.0,
        kind: NodeKind::Decision {
            state,
            expanded: false,
            evaluation: 0.0,
            edges: Vec::new(),
        },
    });
    let mut tree = Tree {
        nodes,
        config,
        evaluator,
    };
    tree.expand_decision(0);
    for _ in 0..config.simulations {
        tree.simulate(0, 0);
    }

    let root = &tree.nodes[0];
    let NodeKind::Decision {
        edges, evaluation, ..
    } = &root.kind
    else {
        unreachable!();
    };
    SearchResult {
        actions: edges
            .iter()
            .map(|edge| RootAction {
                placement: edge.placement,
                visits: edge.visits,
                prior: edge.prior,
                mean_value: edge.value_sum / edge.visits.max(1) as f32,
            })
            .collect(),
        root_value: root.value_sum / root.visits.max(1) as f32,
        root_evaluation: *evaluation,
        nodes: tree.nodes.len(),
    }
}

struct Tree<'a, E> {
    nodes: Vec<Node>,
    config: SearchConfig,
    evaluator: &'a mut E,
}

impl<E: Evaluator> Tree<'_, E> {
    fn expand_decision(&mut self, index: usize) -> f32 {
        let needs_expansion = match &self.nodes[index].kind {
            NodeKind::Decision { expanded, .. } => !expanded,
            _ => unreachable!(),
        };
        if needs_expansion {
            let state = match &self.nodes[index].kind {
                NodeKind::Decision { state, .. } => *state,
                _ => unreachable!(),
            };
            let placements = if state.dead {
                LegalPlacements::empty()
            } else {
                legal_placements_compact(&state.board)
            };
            let result = self.evaluator.evaluate(&state, placements.as_slice());
            assert_eq!(
                result.policy_logits.len(),
                placements.len(),
                "evaluator must return one logit per legal placement"
            );
            let mut priors = [0.0f32; MAX_LEGAL_ACTIONS];
            let priors = &mut priors[..placements.len()];
            softmax_into(&result.policy_logits, priors);
            let edges = placements
                .as_slice()
                .iter()
                .copied()
                .zip(priors.iter().copied())
                .map(|(placement, prior)| DecisionEdge {
                    placement,
                    prior,
                    visits: 0,
                    value_sum: 0.0,
                    child: None,
                })
                .collect();
            let NodeKind::Decision {
                expanded,
                evaluation,
                edges: destination,
                ..
            } = &mut self.nodes[index].kind
            else {
                unreachable!();
            };
            *destination = edges;
            *evaluation = result.value.clamp(-1.0, 1.0);
            *expanded = true;
        }
        match &self.nodes[index].kind {
            NodeKind::Decision { evaluation, .. } => *evaluation,
            _ => unreachable!(),
        }
    }

    fn simulate(&mut self, index: usize, depth: u8) -> f32 {
        let is_decision = matches!(self.nodes[index].kind, NodeKind::Decision { .. });
        let value = if is_decision {
            self.simulate_decision(index, depth)
        } else {
            self.simulate_chance(index, depth)
        };
        self.nodes[index].visits += 1;
        self.nodes[index].value_sum += value;
        value
    }

    fn simulate_decision(&mut self, index: usize, depth: u8) -> f32 {
        let (was_expanded, terminal) = match &self.nodes[index].kind {
            NodeKind::Decision {
                state,
                expanded,
                edges,
                ..
            } => (*expanded, state.dead || (*expanded && edges.is_empty())),
            _ => unreachable!(),
        };
        if terminal || depth >= self.config.max_depth || !was_expanded {
            return self.expand_decision(index);
        }

        let edge_index = self.select_decision(index);
        let (placement, child) = match &self.nodes[index].kind {
            NodeKind::Decision { edges, .. } => {
                (edges[edge_index].placement, edges[edge_index].child)
            }
            _ => unreachable!(),
        };
        let child = if let Some(child) = child {
            child
        } else {
            let pending = match &self.nodes[index].kind {
                NodeKind::Decision { state, .. } => state
                    .play(placement)
                    .expect("tree contains only legal placements"),
                _ => unreachable!(),
            };
            let child = self.nodes.len();
            self.nodes.push(Node {
                visits: 0,
                value_sum: 0.0,
                kind: NodeKind::Chance {
                    edges: chance_distribution(pending.estimated_unseen_colour_counts),
                    pending,
                },
            });
            match &mut self.nodes[index].kind {
                NodeKind::Decision { edges, .. } => edges[edge_index].child = Some(child),
                _ => unreachable!(),
            }
            child
        };
        let value = self.simulate(child, depth);
        match &mut self.nodes[index].kind {
            NodeKind::Decision { edges, .. } => {
                edges[edge_index].visits += 1;
                edges[edge_index].value_sum += value;
            }
            _ => unreachable!(),
        }
        value
    }

    fn simulate_chance(&mut self, index: usize, depth: u8) -> f32 {
        let edge_index = self.select_chance(index);
        let (pair, child) = match &self.nodes[index].kind {
            NodeKind::Chance { edges, .. } => (edges[edge_index].pair, edges[edge_index].child),
            _ => unreachable!(),
        };
        let child = if let Some(child) = child {
            child
        } else {
            let state = match &self.nodes[index].kind {
                NodeKind::Chance { pending, .. } => pending.reveal(pair),
                _ => unreachable!(),
            };
            let child = self.nodes.len();
            self.nodes.push(Node {
                visits: 0,
                value_sum: 0.0,
                kind: NodeKind::Decision {
                    state,
                    expanded: false,
                    evaluation: 0.0,
                    edges: Vec::new(),
                },
            });
            match &mut self.nodes[index].kind {
                NodeKind::Chance { edges, .. } => edges[edge_index].child = Some(child),
                _ => unreachable!(),
            }
            child
        };
        let value = self.simulate(child, depth + 1);
        match &mut self.nodes[index].kind {
            NodeKind::Chance { edges, .. } => {
                edges[edge_index].visits += 1;
                edges[edge_index].value_sum += value;
            }
            _ => unreachable!(),
        }
        value
    }

    fn select_decision(&self, index: usize) -> usize {
        let NodeKind::Decision { edges, .. } = &self.nodes[index].kind else {
            unreachable!();
        };
        let parent = self.nodes[index].visits.max(1) as f32;
        edges
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                puct_score(left, parent, self.config.c_puct).total_cmp(&puct_score(
                    right,
                    parent,
                    self.config.c_puct,
                ))
            })
            .map(|(index, _)| index)
            .expect("expanded non-terminal decision has actions")
    }

    fn select_chance(&self, index: usize) -> usize {
        let NodeKind::Chance { edges, .. } = &self.nodes[index].kind else {
            unreachable!();
        };
        // Deterministic weighted fair scheduling converges to the chance
        // probabilities without depending on the hidden game seed.
        edges
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                ((left.visits + 1) as f32 / left.probability)
                    .total_cmp(&((right.visits + 1) as f32 / right.probability))
            })
            .map(|(index, _)| index)
            .expect("chance distribution is non-empty")
    }
}

fn puct_score(edge: &DecisionEdge, parent_visits: f32, c_puct: f32) -> f32 {
    let q = edge.value_sum / edge.visits.max(1) as f32;
    q + c_puct * edge.prior * parent_visits.sqrt() / (1 + edge.visits) as f32
}

/// Softmax into a caller-provided buffer. Returns the number of entries
/// written, which always equals `logits.len()`.
fn softmax_into(logits: &[f32], out: &mut [f32]) {
    debug_assert_eq!(logits.len(), out.len());
    if logits.is_empty() {
        return;
    }
    let maximum = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for (logit, value) in logits.iter().zip(out.iter_mut()) {
        *value = (*logit - maximum).exp();
        sum += *value;
    }
    if !sum.is_finite() || sum <= 0.0 {
        let uniform = 1.0 / out.len() as f32;
        out.fill(uniform);
    } else {
        for value in out.iter_mut() {
            *value /= sum;
        }
    }
}

fn chance_distribution(counts: [u16; 4]) -> Vec<ChanceEdge> {
    let total: u32 = counts.iter().map(|&count| u32::from(count)).sum();
    let effective = if total >= 2 { counts } else { [1; 4] };
    let total: f32 = effective.iter().map(|&count| f32::from(count)).sum();
    let mut result = Vec::with_capacity(16);
    for axis in Colour::ALL {
        if effective[axis as usize] == 0 {
            continue;
        }
        for child in Colour::ALL {
            let remaining = effective[child as usize] - u16::from(axis == child);
            if remaining == 0 {
                continue;
            }
            result.push(ChanceEdge {
                pair: Pair { axis, child },
                probability: f32::from(effective[axis as usize]) / total * f32::from(remaining)
                    / (total - 1.0),
                visits: 0,
                value_sum: 0.0,
                child: None,
            });
        }
    }
    result.sort_by(|left, right| right.probability.total_cmp(&left.probability));
    result
}

fn baseline_value(state: &SearchState) -> f32 {
    if state.dead {
        return -1.0;
    }
    let heights = state.board.column_heights();
    let maximum_height = f32::from(*heights.iter().max().unwrap_or(&0));
    let occupied: u16 = heights.iter().map(|&height| u16::from(height)).sum();
    let chain = f32::from(state.maximum_chain.min(19)) / 19.0;
    let headroom = 1.0 - maximum_height / HEIGHT as f32;
    let emptiness = 1.0 - f32::from(occupied) / (HEIGHT * 6) as f32;
    0.55 * chain + 0.30 * headroom + 0.15 * emptiness
}

#[cfg(test)]
mod tests {
    use super::*;
    use puyo_core::game::GameState;

    struct BiasedEvaluator {
        calls: usize,
    }

    impl Evaluator for BiasedEvaluator {
        fn evaluate(&mut self, _state: &SearchState, placements: &[Placement]) -> Evaluation {
            self.calls += 1;
            let mut logits = vec![0.0; placements.len()];
            if let Some(first) = logits.first_mut() {
                *first = 4.0;
            }
            Evaluation {
                value: 0.25,
                policy_logits: logits,
            }
        }
    }

    #[test]
    fn injected_evaluator_controls_policy_and_is_cached_per_node() {
        let state = SearchState::from(&GameState::new(0).public_state());
        let mut evaluator = BiasedEvaluator { calls: 0 };
        let result = search_with_evaluator(
            state,
            SearchConfig {
                simulations: 0,
                ..SearchConfig::default()
            },
            &mut evaluator,
        );
        assert_eq!(evaluator.calls, 1);
        assert!(result.actions[0].prior > 0.7);
        assert!(
            (result
                .actions
                .iter()
                .map(|action| action.prior)
                .sum::<f32>()
                - 1.0)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn chance_distribution_is_normalized_and_order_sensitive() {
        let edges = chance_distribution([4, 3, 2, 1]);
        let sum: f32 = edges.iter().map(|edge| edge.probability).sum();
        assert!((sum - 1.0).abs() < 1e-6);
        let rg = edges
            .iter()
            .find(|edge| {
                edge.pair
                    == Pair {
                        axis: Colour::Red,
                        child: Colour::Green,
                    }
            })
            .unwrap();
        let gr = edges
            .iter()
            .find(|edge| {
                edge.pair
                    == Pair {
                        axis: Colour::Green,
                        child: Colour::Red,
                    }
            })
            .unwrap();
        assert!((rg.probability - gr.probability).abs() < 1e-6);
    }

    #[test]
    fn puct_returns_a_normalized_root_visit_target() {
        let game = GameState::new(7);
        let result = search(
            SearchState::from(&game.public_state()),
            SearchConfig {
                simulations: 256,
                max_depth: 4,
                ..SearchConfig::default()
            },
        );
        assert_eq!(result.actions.len(), 22);
        assert_eq!(
            result
                .actions
                .iter()
                .map(|action| action.visits)
                .sum::<u32>(),
            256
        );
        assert!(result.best().unwrap().visits > 0);
        assert!(result.nodes > 22);
    }

    #[test]
    fn search_has_no_hidden_seed_after_public_state_is_built() {
        use std::collections::HashMap;

        let mut seen = HashMap::new();
        let mut collision = None;
        for seed in 0..20_000 {
            let public = GameState::new(seed).public_state();
            if let Some(previous) = seen.insert(public.pieces, public.clone()) {
                collision = Some((previous, public));
                break;
            }
        }
        let (left, right) = collision.expect("six-colour public windows must collide");
        assert_eq!(left.pieces, right.pieces);
        assert_eq!(
            left.estimated_unseen_colour_counts,
            right.estimated_unseen_colour_counts
        );
        let config = SearchConfig {
            simulations: 128,
            max_depth: 3,
            ..SearchConfig::default()
        };
        let left = search(SearchState::from(&left), config);
        let right = search(SearchState::from(&right), config);
        assert_eq!(left.actions, right.actions);
        assert_eq!(left.nodes, right.nodes);
    }

    #[test]
    fn weighted_chance_scheduler_tracks_probabilities() {
        let pending = SearchState::from(&GameState::new(11).public_state())
            .play(legal_placements_compact(&Board::empty()).as_slice()[0])
            .unwrap();
        let edges = chance_distribution(pending.estimated_unseen_colour_counts);
        let mut node = Node {
            visits: 0,
            value_sum: 0.0,
            kind: NodeKind::Chance { pending, edges },
        };
        for _ in 0..10_000 {
            let mut evaluator = BaselineEvaluator;
            let tree = Tree {
                nodes: vec![node],
                config: SearchConfig::default(),
                evaluator: &mut evaluator,
            };
            let selected = tree.select_chance(0);
            node = tree.nodes.into_iter().next().unwrap();
            let NodeKind::Chance { edges, .. } = &mut node.kind else {
                unreachable!();
            };
            edges[selected].visits += 1;
        }
        let NodeKind::Chance { edges, .. } = node.kind else {
            unreachable!();
        };
        for edge in edges {
            let observed = edge.visits as f32 / 10_000.0;
            assert!((observed - edge.probability).abs() < 0.002);
        }
    }
}
